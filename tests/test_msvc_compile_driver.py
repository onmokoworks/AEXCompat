import json
import subprocess
from pathlib import Path

import _msvc_compile
import pytest
from _msvc_compile import compile_driver


def test_compile_driver_uses_plain_msvc_by_default(monkeypatch):
    monkeypatch.delenv("AEXCOMPAT_COMPILE_CACHE", raising=False)
    assert compile_driver() == "cl.exe"


def test_compile_driver_wraps_only_the_explicit_sccache_mode(monkeypatch):
    monkeypatch.setenv("AEXCOMPAT_COMPILE_CACHE", "sccache")
    monkeypatch.setattr(_msvc_compile.shutil, "which", lambda name: f"/fake/{name}")
    assert compile_driver() == "sccache cl.exe"

    monkeypatch.setenv("AEXCOMPAT_COMPILE_CACHE", "ccache")
    assert compile_driver() == "cl.exe"


def test_compile_driver_refuses_an_absent_sccache(monkeypatch):
    monkeypatch.setenv("AEXCOMPAT_COMPILE_CACHE", "sccache")
    monkeypatch.setattr(_msvc_compile.shutil, "which", lambda name: None)
    with pytest.raises(RuntimeError, match="sccache is not on PATH"):
        compile_driver()


@pytest.fixture
def exported_environment(monkeypatch, tmp_path):
    installation = tmp_path / 'VS'
    compiler = installation / 'VC/Tools/MSVC/14.50/bin/Hostx64/x64/cl.exe'
    compiler.parent.mkdir(parents=True)
    compiler.write_bytes(b'fixture compiler')
    linker = compiler.with_name('link.exe')
    linker.write_bytes(b'fixture linker')
    state = {
        'installation': str(installation), 'compiler': str(compiler),
        'linker': str(linker),
        'include': 'sdk-include;msvc-include', 'lib': 'sdk-lib;msvc-lib',
        'libpath': 'framework-lib',
    }
    for name in ('include', 'lib', 'libpath'):
        monkeypatch.setenv(name.upper(), state[name])
    monkeypatch.setenv('AEXCOMPAT_MSVC_ENV_STATE', json.dumps(state))
    monkeypatch.setattr(_msvc_compile.shutil, 'which', lambda name: state.get({'cl.exe': 'compiler', 'link.exe': 'linker'}.get(name)))
    return installation, state


def test_exported_x64_environment_avoids_reinitialization(exported_environment):
    installation, _ = exported_environment
    assert _msvc_compile.msvc_setup_prefix(installation) == ''


def test_unrelated_path_additions_keep_the_same_resolved_compiler(monkeypatch, exported_environment):
    installation, _ = exported_environment
    monkeypatch.setenv('PATH', 'unrelated-tool-directory')
    assert _msvc_compile.msvc_setup_prefix(installation) == ''


@pytest.mark.parametrize('field', ['installation', 'compiler', 'linker', 'include', 'lib', 'libpath'])
def test_non_string_snapshot_field_reinitializes(monkeypatch, exported_environment, field):
    installation, state = exported_environment
    state[field] = 5
    monkeypatch.setenv('AEXCOMPAT_MSVC_ENV_STATE', json.dumps(state))
    assert _msvc_compile.msvc_setup_prefix(installation).startswith('@call ')


@pytest.mark.parametrize('name', ['INCLUDE', 'LIB', 'LIBPATH'])
def test_changed_environment_reinitializes(monkeypatch, exported_environment, name):
    installation, _ = exported_environment
    monkeypatch.setenv(name, 'changed')
    assert _msvc_compile.msvc_setup_prefix(installation) == (
        f'@call "{installation}\\VC\\Auxiliary\\Build\\vcvars64.bat" >nul\n'
    )


@pytest.mark.parametrize('state', [None, '', '{', 'null', '[]', '{}', '{"compiler":5}'])
def test_missing_or_malformed_snapshot_reinitializes(monkeypatch, exported_environment, state):
    installation, _ = exported_environment
    if state is None:
        monkeypatch.delenv('AEXCOMPAT_MSVC_ENV_STATE')
    else:
        monkeypatch.setenv('AEXCOMPAT_MSVC_ENV_STATE', state)
    assert _msvc_compile.msvc_setup_prefix(installation).startswith('@call ')


@pytest.mark.parametrize('change', ['latest_vs', 'resolved_cl', 'x86', 'foreign', 'missing_cl', 'deleted_cl', 'empty_include', 'empty_lib'])
def test_stale_or_non_x64_compiler_reinitializes(monkeypatch, exported_environment, change):
    installation, state = exported_environment
    if change == 'latest_vs':
        installation = installation.parent / 'new-VS'
    elif change == 'resolved_cl':
        monkeypatch.setattr(_msvc_compile.shutil, 'which', lambda name: 'different-cl.exe')
    elif change == 'missing_cl':
        monkeypatch.setattr(_msvc_compile.shutil, 'which', lambda name: None)
    elif change == 'deleted_cl':
        Path(state['compiler']).unlink()
    elif change in ('x86', 'foreign'):
        compiler = Path(state['compiler'])
        compiler = compiler.parent.parent / 'x86/cl.exe' if change == 'x86' else installation.parent / 'cl.exe'
        compiler.parent.mkdir(parents=True, exist_ok=True)
        compiler.write_bytes(b'other fixture compiler')
        state['compiler'] = str(compiler)
        monkeypatch.setattr(_msvc_compile.shutil, 'which', lambda name: state['compiler'])
    else:
        name = change.removeprefix('empty_')
        state[name] = ''
        monkeypatch.setenv(name.upper(), '')
    monkeypatch.setenv('AEXCOMPAT_MSVC_ENV_STATE', json.dumps(state))
    assert _msvc_compile.msvc_setup_prefix(installation).startswith('@call ')


@pytest.mark.parametrize('change', ['foreign_path', 'missing', 'deleted', 'foreign_snapshot'])
def test_changed_linker_reinitializes(monkeypatch, exported_environment, tmp_path, change):
    installation, state = exported_environment
    original = state['linker']
    foreign = tmp_path / 'foreign-link.exe'
    foreign.write_bytes(b'foreign fixture linker')
    if change == 'deleted':
        Path(original).unlink()
    elif change == 'foreign_snapshot':
        state['linker'] = str(foreign)
    else:
        resolved = str(foreign) if change == 'foreign_path' else None
        monkeypatch.setattr(_msvc_compile.shutil, 'which',
                            lambda name: state['compiler'] if name == 'cl.exe' else resolved)
    monkeypatch.setenv('AEXCOMPAT_MSVC_ENV_STATE', json.dumps(state))
    assert _msvc_compile.msvc_setup_prefix(installation).startswith('@call ')


def test_reused_environment_still_runs_and_propagates_compile_failure(monkeypatch, exported_environment):
    from test_pf_adv_time_suite1 import run_in_vs_environment

    installation, _ = exported_environment
    failure = subprocess.CalledProcessError(2, 'fixture compiler')
    calls = []

    def run(argv, **kwargs):
        calls.append((argv, kwargs))
        if str(argv[0]).endswith('vswhere.exe'):
            return subprocess.CompletedProcess(argv, 0, stdout=str(installation))
        # Inspect generated output, not repository source: the original command
        # must still execute, without the redundant environment initialization.
        assert Path(argv[0]).read_text(encoding='ascii') == 'cl.exe /c invalid.cpp\n'
        assert kwargs == {'cwd': installation, 'check': True, 'timeout': 17}
        raise failure

    monkeypatch.setattr(subprocess, 'run', run)
    with pytest.raises(subprocess.CalledProcessError) as raised:
        run_in_vs_environment('cl.exe /c invalid.cpp', cwd=installation, timeout=17)
    assert raised.value is failure
    assert len(calls) == 2

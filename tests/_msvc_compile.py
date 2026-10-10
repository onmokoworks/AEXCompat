import json
import ntpath
import os
import shutil
from pathlib import Path


def require_sccache() -> None:
    """Fail with a readable message when the requested cache wrapper is absent.

    Without this, cmd reports ``'sccache' is not recognized`` (exit 9009) from
    inside a vcvars batch and the test looks like a compiler failure.
    """
    if shutil.which("sccache") is None:
        raise RuntimeError(
            "AEXCOMPAT_COMPILE_CACHE=sccache but sccache is not on PATH; "
            "install it or unset the variable to compile without a cache"
        )


def compile_driver() -> str:
    """Return the configured MSVC compile-only driver for batch commands."""
    if os.environ.get("AEXCOMPAT_COMPILE_CACHE") == "sccache":
        require_sccache()
        return "sccache cl.exe"
    return "cl.exe"


def _windows_path(path) -> str:
    return ntpath.normcase(ntpath.normpath(os.path.abspath(str(path))))


def _exported_environment_matches(installation) -> bool:
    # This is an optional build optimization, not compiler admission. Any
    # absent/changed state keeps the existing vcvars64 initialization path.
    try:
        state = json.loads(os.environ.get('AEXCOMPAT_MSVC_ENV_STATE', ''))
        fields = {'installation', 'compiler', 'linker', 'include', 'lib', 'libpath'}
        if not isinstance(state, dict) or set(state) != fields:
            return False
        if not all(isinstance(value, str) for value in state.values()):
            return False
        if _windows_path(state['installation']) != _windows_path(installation):
            return False
        compiler = shutil.which('cl.exe')
        if not compiler or not Path(compiler).is_file():
            return False
        if _windows_path(compiler) != _windows_path(state['compiler']):
            return False
        relative = ntpath.relpath(_windows_path(compiler), _windows_path(installation)).split('\\')
        if len(relative) != 8 or relative[:3] != ['vc', 'tools', 'msvc']:
            return False
        if relative[4:] != ['bin', 'hostx64', 'x64', 'cl.exe']:
            return False
        linker = shutil.which('link.exe')
        if not linker or not Path(linker).is_file():
            return False
        if (_windows_path(linker) != _windows_path(state['linker'])
                or _windows_path(linker) != _windows_path(Path(compiler).with_name('link.exe'))):
            return False
        if not state['include'] or not state['lib']:
            return False
        return all(os.environ.get(name.upper(), '') == state[name]
                   for name in ('include', 'lib', 'libpath'))
    except (ValueError, TypeError, OSError):
        return False


def msvc_setup_prefix(installation) -> str:
    """Retain latest-VS selection; reuse only its unchanged exported x64 env."""
    if _exported_environment_matches(installation):
        return ''
    return f'@call "{installation}\\VC\\Auxiliary\\Build\\vcvars64.bat" >nul\n'

import importlib.util
import json
from pathlib import Path
import subprocess

import pytest

ROOT = Path(__file__).resolve().parents[1]


@pytest.fixture
def builder():
    spec = importlib.util.spec_from_file_location('archive_builder', ROOT / 'tools/build-broker-test-archive.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def full_metadata(workspace):
    return {
        'version': 1, 'workspace_root': str(workspace),
        'workspace_members': ['fixture'],
        'packages': [{'id': 'fixture'}, {'id': 'dependency'}],
        'resolve': {'nodes': [{'id': 'fixture'}, {'id': 'dependency'}]},
    }


def test_warm_cache_keeps_archive_contract_and_avoids_network(builder, monkeypatch, tmp_path):
    manifest = tmp_path / 'Cargo.toml'
    calls = []

    def run(argv, **kwargs):
        calls.append((argv, kwargs))
        if argv[1] == 'metadata':
            return subprocess.CompletedProcess(argv, 0, json.dumps(full_metadata(tmp_path)), '')
        return subprocess.CompletedProcess(argv, 0)

    monkeypatch.setattr(builder.subprocess, 'run', run)
    builder.build_archive(manifest, 'custom-archive.tar.zst')
    assert len(calls) == 2
    metadata_command, metadata_options = calls[0]
    assert metadata_command == ['cargo', 'metadata', '--manifest-path', str(manifest), '--locked', '--offline', '--format-version', '1']
    assert metadata_options['cwd'] == tmp_path
    assert metadata_options['capture_output'] is True
    assert metadata_options['check'] is False
    assert calls[1] == (
        ['cargo', 'nextest', 'archive', '--workspace', '--locked', '--archive-file', 'custom-archive.tar.zst', '--offline'],
        {'cwd': tmp_path, 'check': True},
    )


@pytest.mark.parametrize('preflight_code', [1, 101])
def test_cache_miss_keeps_exact_original_online_archive(builder, monkeypatch, tmp_path, preflight_code):
    calls = []

    def run(argv, **kwargs):
        calls.append((argv, kwargs))
        return subprocess.CompletedProcess(argv, preflight_code if argv[1] == 'metadata' else 0, '', 'cache missing')

    monkeypatch.setattr(builder.subprocess, 'run', run)
    builder.build_archive(tmp_path / 'Cargo.toml')
    assert calls[-1] == (
        ['cargo', 'nextest', 'archive', '--workspace', '--locked', '--archive-file', 'target/nextest-archive.tar.zst'],
        {'cwd': tmp_path, 'check': True},
    )
    assert len(calls) == 2


@pytest.mark.parametrize(('reason', 'kind'), [
    ('', 'other'),
    ('error: no matching package named `object` found\nregistry: crates-io', 'missing_index_entry'),
    ('error: failed to download `object v1.2.3`\n--offline was specified', 'missing_crate_archive'),
    ('error: unable to update repository in offline mode', 'missing_git_checkout'),
    ("error: can't checkout git dependency in offline mode", 'missing_git_checkout'),
    ('the lock file needs to be updated but --locked was passed', 'locked_input_change'),
    ('failed to parse manifest at C:/private/Cargo.toml', 'manifest_error'),
    ('failed to load manifest for workspace member', 'manifest_error'),
    ('unrecognized error: é' * 4096, 'other'),
], ids=['empty', 'index', 'archive', 'git_update', 'git_checkout', 'lock', 'parse', 'load', 'unknown_unicode'])
def test_cache_miss_kind_does_not_leak_diagnostic_values(builder, monkeypatch, tmp_path, capsys, reason, kind):
    reason += '\nprivate C:/Users/secret/project https://user:token@example.invalid/private'
    monkeypatch.setattr(builder.subprocess, 'run', lambda argv, **kwargs: subprocess.CompletedProcess(argv, 101, '', reason))
    assert builder.cached_dependencies_ready(tmp_path / 'Cargo.toml') is False
    output = capsys.readouterr()
    assert output.out == f'offline_dependency_cache=unavailable metadata_exit=101\noffline_dependency_cache_kind={kind}\n'
    assert output.err == ''


@pytest.mark.parametrize('warm', [True, False])
def test_actual_archive_failure_is_not_hidden_or_retried(builder, monkeypatch, tmp_path, warm):
    calls = []
    failure = subprocess.CalledProcessError(101, 'archive failure')

    def run(argv, **kwargs):
        calls.append(argv)
        if argv[1] == 'metadata':
            return subprocess.CompletedProcess(argv, 0 if warm else 101, json.dumps(full_metadata(tmp_path)), '')
        raise failure

    monkeypatch.setattr(builder.subprocess, 'run', run)
    with pytest.raises(subprocess.CalledProcessError) as raised:
        builder.build_archive(tmp_path / 'Cargo.toml')
    assert raised.value is failure
    assert len(calls) == 2


@pytest.mark.parametrize('damage', ['invalid_json', 'version', 'bool_version', 'root', 'relative_root', 'no_deps', 'no_packages', 'duplicate_package', 'bad_package_id', 'unknown_node', 'missing_member', 'duplicate_member'])
def test_bad_success_metadata_cannot_admit_an_offline_archive(builder, monkeypatch, tmp_path, damage):
    metadata = full_metadata(tmp_path)
    if damage == 'version':
        metadata['version'] = 2
    elif damage == 'bool_version':
        metadata['version'] = True
    elif damage == 'root':
        metadata['workspace_root'] = str(tmp_path / 'other-workspace')
    elif damage == 'relative_root':
        metadata['workspace_root'] = 'relative-workspace'
    elif damage == 'no_deps':
        metadata['resolve'] = None
    elif damage == 'no_packages':
        metadata['packages'] = []
    elif damage == 'duplicate_package':
        metadata['packages'].append(metadata['packages'][0])
    elif damage == 'bad_package_id':
        metadata['packages'][0]['id'] = 17
    elif damage == 'unknown_node':
        metadata['resolve']['nodes'].append({'id': 'unknown'})
    elif damage == 'missing_member':
        metadata['workspace_members'] = ['missing']
    elif damage == 'duplicate_member':
        metadata['workspace_members'].append('fixture')
    calls = []

    def run(argv, **kwargs):
        calls.append(argv)
        return subprocess.CompletedProcess(argv, 0, '{' if damage == 'invalid_json' else json.dumps(metadata), '')

    monkeypatch.setattr(builder.subprocess, 'run', run)
    with pytest.raises(RuntimeError):
        builder.build_archive(tmp_path / 'Cargo.toml')
    assert len(calls) == 1


def test_main_preserves_actual_archive_exit_code(builder, monkeypatch):
    monkeypatch.setattr(builder.sys, 'argv', ['build-archive'])
    failure = subprocess.CalledProcessError(37, 'archive failure')
    monkeypatch.setattr(builder, 'build_archive', lambda **kwargs: (_ for _ in ()).throw(failure))
    assert builder.main() == 37


@pytest.mark.parametrize('nested', [False, True])
def test_duplicate_metadata_keys_stop_before_archive(builder, monkeypatch, tmp_path, nested):
    text = json.dumps(full_metadata(tmp_path))
    text = text.replace('"id": "fixture"', '"id": "fixture", "id": "fixture"', 1) if nested else text.replace('"version": 1', '"version": 1, "version": 1', 1)
    calls = []

    def run(argv, **kwargs):
        calls.append(argv)
        return subprocess.CompletedProcess(argv, 0, text, '')

    monkeypatch.setattr(builder.subprocess, 'run', run)
    with pytest.raises(RuntimeError, match='not valid JSON'):
        builder.build_archive(tmp_path / 'Cargo.toml')
    assert len(calls) == 1

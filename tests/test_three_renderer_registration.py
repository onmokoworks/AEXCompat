"""Behavioral installer tests; fixture bytes are never executed."""
import importlib.util
import json
from pathlib import Path

import pytest

spec = importlib.util.spec_from_file_location(
    'three_registration', Path(__file__).resolve().parents[1]/'tools/register_three_renderer.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def runtime(tmp_path):
    root = tmp_path/'renderer'
    for relative in ('node_modules/electron/dist/electron.exe',
                     'out/main/index.js', 'out/renderer/index.html'):
        path = root/relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b'fixture only')
    plugin = tmp_path/'selected.aex'
    plugin.write_bytes(b'fixture only')
    return root, plugin, tmp_path/'registration/three-v1.json'


def test_registration_publishes_complete_record_and_cleans_temporary(tmp_path):
    root, plugin, output = runtime(tmp_path)
    actual = module.register(root, [plugin], output)
    assert json.loads(output.read_text(encoding='utf-8')) == actual
    assert actual['plugin_paths'] == [str(plugin.resolve())]
    assert actual['runtime_root'] == str(root.resolve())
    assert list(output.parent.iterdir()) == [output]


def test_existing_registration_is_preserved_byte_for_byte(tmp_path):
    root, plugin, output = runtime(tmp_path)
    output.parent.mkdir()
    output.write_bytes(b'prior configuration including unknown fields')
    with pytest.raises(FileExistsError):
        module.register(root, [plugin], output)
    assert output.read_bytes() == b'prior configuration including unknown fields'
    assert list(output.parent.iterdir()) == [output]


def test_missing_runtime_does_not_publish(tmp_path):
    root, plugin, output = runtime(tmp_path)
    (root/'out/main/index.js').unlink()
    with pytest.raises(FileNotFoundError):
        module.register(root, [plugin], output)
    assert not output.exists()


def test_duplicate_plugin_rejected_without_publication(tmp_path):
    root, plugin, output = runtime(tmp_path)
    with pytest.raises(ValueError, match='duplicate'):
        module.register(root, [plugin, plugin], output)
    assert not output.exists()


def test_rebuilt_plugin_can_be_registered_without_hash_approval(tmp_path):
    root, plugin, output = runtime(tmp_path)
    plugin.write_bytes(b'rebuilt')
    module.register(root, [plugin], output)
    assert json.loads(output.read_text())['plugin_paths'] == [str(plugin.resolve())]


def test_registration_key_uses_path_not_plugin_bytes(tmp_path):
    root, plugin, output = runtime(tmp_path)
    before = module.registration_path(output.parent, plugin)
    plugin.write_bytes(b'new binary')
    assert module.registration_path(output.parent, plugin) == before
    assert module.path_key(r'\\?\C:\Plugins\Example.aex') == module.path_key('c:/plugins/example.aex')
    assert module.path_key(r'\\?\UNC\Server\Share\Example.aex') == module.path_key('//server/share/example.aex')

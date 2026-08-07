from __future__ import annotations

import base64
import hashlib
import importlib.util
import json
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path

import jsonschema
import pytest


ROOT = Path(__file__).resolve().parents[1]


def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


SESSION = load_module("blender_aexcompat_session", ROOT / "tools" / "blender_aexcompat_session.py")
SCHEMA = json.loads((ROOT / "contracts" / "blender" / "aexcompat_blender_session.schema.json").read_text(encoding="utf-8"))
PACKAGE_WRAPPER = ROOT / "blender_addon" / "aexcompat_blender" / "session_wrapper.py"


def request(mode: str = "identity_no_aex") -> dict:
    raw = bytes(range(16))
    return {
        "schema_version": 1,
        "request_kind": "aexcompat_blender_session",
        "mode": mode,
        "frame": {"width": 2, "height": 2, "stride": 8, "channels": "RGBA8", "alpha": "straight", "color_space": "scene_linear", "frame_time": {"seconds": 0.0}},
        "input": {"encoding": "base64-rgba8", "data": base64.b64encode(raw).decode(), "sha256": hashlib.sha256(raw).hexdigest()},
    }


def test_identity_transport_is_explicitly_not_aex_success():
    payload = request()
    payload["plugin"] = {"source_relative_path": "fixtures/not-loaded.aex"}
    result = SESSION.build_response(payload)
    jsonschema.validate(result, SCHEMA)
    assert result["status"] == "identity_only"
    assert result["failure_class"] == "aex_not_loaded"
    assert result["aex_render_performed"] is False
    assert result["host_success"] is False
    assert result["plugin_identity"]["source_relative_path"] == "fixtures/not-loaded.aex"
    assert result["diff"] == {"byte_count": 16, "changed_bytes": 0, "max_abs_delta": 0}


def test_non_identity_mode_stays_fail_closed():
    result = SESSION.build_response(request("aex_render"))
    jsonschema.validate(result, SCHEMA)
    assert result["status"] == "unsupported"
    assert result["failure_class"] == "aex_not_loaded"
    assert "output" not in result


def test_fixture_invert_is_a_real_transport_transform_but_not_aex_success():
    result = SESSION.build_response(request("fixture_invert_no_aex"))
    jsonschema.validate(result, SCHEMA)
    raw = bytes(range(16))
    expected = bytes([255 - value if index % 4 != 3 else value for index, value in enumerate(raw)])
    assert base64.b64decode(result["output"]["data"]) == expected
    assert result["status"] == "fixture_transform"
    assert result["failure_class"] == "aex_not_loaded"
    assert result["aex_render_performed"] is False
    assert result["host_success"] is False
    assert result["diff"]["changed_bytes"] > 0


def test_empty_input_is_a_separate_fail_closed_classification():
    payload = request()
    payload["input"]["data"] = ""
    payload["input"]["sha256"] = hashlib.sha256(b"").hexdigest()
    with pytest.raises(ValueError) as error:
        SESSION.build_response(payload)
    result = SESSION._error_response(error.value)
    jsonschema.validate(result, SCHEMA)
    assert result["failure_class"] == "empty_input"


@pytest.mark.parametrize("field,value", [("stride", 4), ("channels", "BGRA8"), ("alpha", "unknown")])
def test_rejects_invalid_transport(field, value):
    payload = request()
    if field in payload["frame"]:
        payload["frame"][field] = value
    with pytest.raises(ValueError):
        SESSION.build_response(payload)


def test_jsonl_subprocess_roundtrip():
    completed = subprocess.run(
        [sys.executable, str(ROOT / "tools" / "blender_aexcompat_session.py")],
        input=json.dumps(request()) + "\n",
        text=True,
        capture_output=True,
        check=True,
    )
    result = json.loads(completed.stdout)
    jsonschema.validate(result, SCHEMA)
    assert result["status"] == "identity_only"


def test_packaged_wrapper_roundtrip_supports_addon_only_install():
    completed = subprocess.run(
        [sys.executable, str(PACKAGE_WRAPPER)],
        input=json.dumps(request()) + "\n",
        text=True,
        capture_output=True,
        check=True,
    )
    result = json.loads(completed.stdout)
    jsonschema.validate(result, SCHEMA)
    assert result["status"] == "identity_only"


@pytest.mark.parametrize("layout", ["installed_directory", "zip_extract"])
def test_addon_install_layouts_bundle_a_resolvable_wrapper(tmp_path, layout):
    source_package = ROOT / "blender_addon" / "aexcompat_blender"
    if layout == "installed_directory":
        package = tmp_path / "scripts" / "addons" / "aexcompat_blender"
        shutil.copytree(source_package, package)
    else:
        archive = tmp_path / "aexcompat_blender.zip"
        with zipfile.ZipFile(archive, "w") as bundle:
            for source in source_package.iterdir():
                bundle.write(source, Path("aexcompat_blender") / source.name)
        extract_root = tmp_path / "scripts" / "addons"
        with zipfile.ZipFile(archive) as bundle:
            bundle.extractall(extract_root)
        package = extract_root / "aexcompat_blender"

    wrapper = package / "session_wrapper.py"
    assert wrapper.is_file()
    completed = subprocess.run(
        [sys.executable, str(wrapper)],
        input=json.dumps(request()) + "\n",
        text=True,
        capture_output=True,
        check=True,
        cwd=wrapper.parent,
    )
    result = json.loads(completed.stdout)
    jsonschema.validate(result, SCHEMA)
    assert result["status"] == "identity_only"


@pytest.mark.parametrize(
    "value",
    [
        "../outside.aex",
        "..\\outside.aex",
        "effects/../outside.aex",
        "effects\\..\\outside.aex",
        "/tmp/outside.aex",
        "C:\\outside.aex",
        "C:outside.aex",
        "\\\\server\\share\\outside.aex",
        "\x00outside.aex",
    ],
)
def test_plugin_path_rejects_root_and_traversal(value):
    payload = request()
    payload["plugin"] = {"source_relative_path": value}
    with pytest.raises(ValueError):
        SESSION.build_response(payload)


def test_plugin_path_is_normalized_to_relative_posix_form():
    payload = request()
    payload["plugin"] = {"source_relative_path": "effects\\safe.aex"}
    result = SESSION.build_response(payload)
    assert result["plugin_identity"]["source_relative_path"] == "effects/safe.aex"

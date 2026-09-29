from __future__ import annotations

import base64
import hashlib
import importlib.util
import json
import shutil
import subprocess
import sys
import zipfile
import zlib
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
    with pytest.raises(jsonschema.ValidationError):
        jsonschema.validate({**result, "parameter_overrides": [{
            "slot": 1, "kind": "integer", "value": 1,
            "description_sha256": "0" * 64, "description_plugin_sha256": "0" * 64,
            "description_files_unchanged": True, "description_matches_render_plugin": False,
        }]}, SCHEMA)


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


@pytest.fixture
def fake_render_environment(tmp_path, monkeypatch):
    root = tmp_path / "plugins"
    root.mkdir()
    (root / "effect.aex").write_bytes(b"public test AEX identity")
    harness = tmp_path / "harness"
    harness.write_bytes(b"public test harness identity")
    worker = tmp_path / "worker"
    worker.write_bytes(b"public test worker identity")
    monkeypatch.setenv("AEXCOMPAT_PLUGIN_ROOT", str(root))
    monkeypatch.setenv("AEXCOMPAT_HARNESS", str(harness))
    monkeypatch.setenv("AEXCOMPAT_GUEST_WORKER", str(worker))
    monkeypatch.setattr(SESSION.sys, "platform", "darwin")
    return root


def _png_rgba(path):
    content = path.read_bytes()
    assert content[:8] == b"\x89PNG\r\n\x1a\n"
    chunks = []
    offset = 8
    while offset < len(content):
        size = int.from_bytes(content[offset : offset + 4], "big")
        kind = content[offset + 4 : offset + 8]
        data = content[offset + 8 : offset + 8 + size]
        chunks.append((kind, data))
        offset += 12 + size
    width = int.from_bytes(chunks[0][1][:4], "big")
    height = int.from_bytes(chunks[0][1][4:8], "big")
    scanlines = zlib.decompress(b"".join(data for kind, data in chunks if kind == b"IDAT"))
    assert len(scanlines) == height * (1 + width * 4)
    return b"".join(scanlines[row * (1 + width * 4) + 1 : (row + 1) * (1 + width * 4)] for row in range(height))


def _fake_harness(args, *_unused, corruption=None, **_kwargs):
    fixture_path, output_dir = Path(args[4]), Path(args[5])
    fixture_bytes = fixture_path.read_bytes()
    fixture = json.loads(fixture_bytes)
    render_path = fixture["render_path"]
    artifact_render_path = "smartfx" if render_path == "smart" else "classic"
    rgba = _png_rgba(fixture_path.parent / fixture["primary_layer"])
    width, height = len(rgba) // 8, 2
    assert (width, height) == (2, 2)
    plugin_sha = hashlib.sha256(Path(args[3]).read_bytes()).hexdigest()
    fixture_sha = hashlib.sha256(fixture_bytes).hexdigest()
    case_sha = "a" * 64
    identity = {
        "sha256": case_sha, "plugin_sha256": plugin_sha, "fixture_sha256": fixture_sha,
        "case_index": 0, "pixel_format": "argb8", "render_path": render_path,
    }
    to_argb = lambda data: b"".join(data[i + 3 : i + 4] + data[i : i + 3] for i in range(0, len(data), 4))
    input_argb = to_argb(rgba)
    output_rgba = bytes(255 - value if index % 4 != 3 else value for index, value in enumerate(rgba))
    output_argb = to_argb(output_rgba)
    case_dir = output_dir / "cases" / case_sha
    metadata_by_stage = {}
    for stage, raw in (("checkpoints/input", input_argb), ("final", output_argb)):
        artifact = case_dir / stage
        artifact.mkdir(parents=True)
        (artifact / "output.bin").write_bytes(raw)
        if corruption == "oversize" and stage == "final":
            (artifact / "output.bin").write_bytes(raw + b"oversized")
        metadata = {
            "schema": "aexcompat.render_raw", "width": width, "height": height,
            "pixel_format": "argb8", "channel_order": "ARGB", "rowbytes": width * 4,
            "data_size_bytes": len(raw), "data_sha256": hashlib.sha256(raw).hexdigest(),
            "data_file": "output.bin",
            "premultiplication": "straight",
            "comparison_identity": {
                "plugin_sha256": plugin_sha, "fixture_case": identity, "pixel_format": "argb8",
                "render_path": artifact_render_path, "input_sha256": hashlib.sha256(input_argb).hexdigest(),
                "world_sha256": hashlib.sha256(raw).hexdigest(),
                "timing": fixture["timing"],
            },
        }
        if corruption == "checksum" and stage == "final":
            metadata["data_sha256"] = "0" * 64
        if corruption == "timing" and stage == "final":
            metadata["comparison_identity"]["timing"] = {
                **fixture["timing"], "current_time": fixture["timing"]["current_time"] + 1
            }
        (artifact / "output.json").write_text(json.dumps(metadata), encoding="utf-8")
        if corruption == "duplicate" and stage == "final":
            (artifact / "output.json").write_text(
                (artifact / "output.json").read_text(encoding="utf-8")[:-1]
                + ', "data_sha256": "0"}',
                encoding="utf-8",
            )
        metadata_by_stage[stage] = metadata
    if corruption == "identity":
        identity["plugin_sha256"] = "0" * 64
    if corruption == "path":
        identity["sha256"] = "../" * 21 + "."
    if corruption == "mutated_file":
        Path(args[0]).write_bytes(b"changed harness file while rendering")
    report = {
        "schema": "aexcompat.render_fixture_report", "schema_version": 2,
        "complete": True, "fixture_sha256": fixture_sha,
        "cases": [{
            "case_identity": identity, "artifact_directory": "cases/" + identity["sha256"],
            "report": {
                "schema": "aexcompat.render_fixture_report", "schema_version": 1,
                "pixel_format": "argb8", "render_path": render_path,
                "final_artifact": metadata_by_stage["final"],
                "checkpoints": {"input": metadata_by_stage["checkpoints/input"]},
            },
        }],
    }
    return json.dumps(report)


def _fake_description(plugin_sha, *, kind="float", maximum=255.0):
    parameter = {
        "slot": 1, "name": "Echo", "kind": kind, "minimum": 0.0,
        "maximum": maximum, "value": 5.0, "choices": [],
        "color": [255, 0, 0, 0], "components": [0.0, 0.0, 0.0],
        "component_count": 0, "layer_path": None, "enabled": True,
        "visible": True, "supervised": False, "debug_summary": None,
        "custom_ui_events": 0, "control_size": [0, 0],
    }
    return {
        "schema": "aexcompat.macos_aex_description", "schema_version": 1,
        "plugin_identity": {
            "sha256": plugin_sha, "post_setup_sha256": plugin_sha,
            "files_unchanged": True,
        },
        "parameters": [parameter], "defaults": [json.loads(json.dumps(parameter))],
    }


def description_request() -> dict:
    return {
        "schema_version": 1, "request_kind": "aexcompat_blender_session",
        "mode": "describe_aex", "plugin": {"source_relative_path": "effect.aex"},
    }


def test_description_lists_scalar_and_color_names_without_render(fake_render_environment, monkeypatch):
    plugin_sha = hashlib.sha256(b"public test AEX identity").hexdigest()
    description = _fake_description(plugin_sha)
    description["parameters"].append({**description["parameters"][0], "slot": 2, "name": "Color", "kind": "color"})
    calls = []

    def harness(args, *_unused, **_kwargs):
        calls.append(args)
        assert "--describe-aex" in args and "--render-fixture" not in args
        assert _unused[1] is None
        return json.dumps(description).encode()

    monkeypatch.setattr(SESSION, "_invoke_harness", harness)
    result = SESSION.build_response(description_request())
    jsonschema.validate(result, SCHEMA)
    assert result["status"] == "described" and not result["aex_render_performed"]
    assert result["parameter_catalog"] == [{
        "slot": 1, "name": "Echo", "kind": "float", "minimum": 0.0,
        "maximum": 255.0, "value": 5.0,
    }, {"slot": 2, "name": "Color", "kind": "color", "color": [255, 0, 0, 0]}]
    assert result["description_identity"]["description_matches_source"] is True
    assert result["description_identity"]["files_unchanged"] is True
    assert len(calls) == 1
    with pytest.raises(jsonschema.ValidationError):
        jsonschema.validate({**result, "output": {"sha256": plugin_sha, "bytes": 0}}, SCHEMA)


def test_description_hides_unavailable_scalar_records(fake_render_environment, monkeypatch):
    plugin_sha = hashlib.sha256(b"public test AEX identity").hexdigest()
    description = _fake_description(plugin_sha)
    description["parameters"].append({**description["parameters"][0], "slot": 2, "name": "Hidden", "visible": False})
    description["parameters"].append({**description["parameters"][0], "slot": 3, "name": "Disabled", "enabled": False})
    monkeypatch.setattr(SESSION, "_invoke_harness", lambda *_args, **_kwargs: json.dumps(description).encode())
    result = SESSION.build_response(description_request())
    assert [entry["name"] for entry in result["parameter_catalog"]] == ["Echo"]


@pytest.mark.parametrize("bad", ["duplicate_slot", "hidden_duplicate_slot", "out_of_range_default", "nonintegral_integer"])
def test_description_rejects_ambiguous_or_unusable_scalar(fake_render_environment, monkeypatch, bad):
    plugin_sha = hashlib.sha256(b"public test AEX identity").hexdigest()
    description = _fake_description(plugin_sha)
    if bad == "duplicate_slot":
        description["parameters"].append({**description["parameters"][0], "name": "Second"})
    elif bad == "hidden_duplicate_slot":
        description["parameters"].append({**description["parameters"][0], "name": "Hidden", "visible": False})
    elif bad == "out_of_range_default":
        description["parameters"][0]["value"] = 999.0
    else:
        description["parameters"][0]["kind"] = "integer"
        description["parameters"][0]["value"] = 4.5
    monkeypatch.setattr(SESSION, "_invoke_harness", lambda *_args, **_kwargs: json.dumps(description).encode())
    with pytest.raises(SESSION.SessionRequestError) as error:
        SESSION.build_response(description_request())
    assert error.value.failure_class == "parameter_description_error"


def test_description_records_identity_mismatch_without_refusing_load(fake_render_environment, monkeypatch):
    monkeypatch.setattr(
        SESSION, "_invoke_harness",
        lambda *_args, **_kwargs: json.dumps(_fake_description("0" * 64)).encode(),
    )
    result = SESSION.build_response(description_request())
    jsonschema.validate(result, SCHEMA)
    assert result["status"] == "described"
    assert result["description_identity"]["description_matches_source"] is False


@pytest.mark.parametrize("mutate", ["bad_json", "missing_defaults", "duplicate", "exit"])
def test_description_errors_do_not_become_success(fake_render_environment, monkeypatch, mutate):
    plugin_sha = hashlib.sha256(b"public test AEX identity").hexdigest()
    description = _fake_description(plugin_sha)
    if mutate == "missing_defaults":
        del description["defaults"]

    def harness(*_args, **_kwargs):
        if mutate == "exit":
            raise SESSION.SessionRequestError("harness failed", "parameter_description_error")
        if mutate == "bad_json":
            return b"{"
        if mutate == "duplicate":
            return b'{"schema":1,"schema":2}'
        return json.dumps(description).encode()

    monkeypatch.setattr(SESSION, "_invoke_harness", harness)
    with pytest.raises(SESSION.SessionRequestError) as error:
        SESSION.build_response(description_request())
    assert error.value.failure_class == "parameter_description_error"


def test_description_rejects_extra_render_fields_before_load(fake_render_environment, monkeypatch):
    monkeypatch.setattr(SESSION, "_invoke_harness", lambda *_args, **_kwargs: pytest.fail("loaded AEX"))
    payload = description_request()
    payload["frame"] = request()["frame"]
    with pytest.raises(SESSION.SessionRequestError):
        SESSION.build_response(payload)


def test_description_rejects_deadline_before_load(fake_render_environment, monkeypatch):
    monkeypatch.setattr(SESSION, "_invoke_harness", lambda *_args, **_kwargs: pytest.fail("loaded AEX"))
    payload = description_request()
    payload["timeout_ms"] = 1000
    with pytest.raises(SESSION.SessionRequestError):
        SESSION.build_response(payload)


@pytest.mark.skipif(sys.platform != "darwin", reason="macOS behavior")
@pytest.mark.parametrize("behavior,failure_class", [
    ("bad_stdout", "parameter_description_error"),
    ("nonzero_exit", "parameter_description_error"),
    ("oversized_stdout", "parameter_description_error"),
])
def test_description_subprocess_failure_is_explicit(fake_render_environment, behavior, failure_class):
    harness = Path(SESSION.os.environ["AEXCOMPAT_HARNESS"])
    if behavior == "bad_stdout":
        body = "printf '{bad json'\n"
    elif behavior == "nonzero_exit":
        body = "exit 3\n"
    elif behavior == "oversized_stdout":
        body = f"{sys.executable} -c 'import sys; sys.stdout.write(\"x\" * {SESSION.MAX_RENDER_METADATA_BYTES + 1})'\n"
    harness.write_text("#!/bin/sh\n" + body, encoding="utf-8")
    harness.chmod(0o755)
    with pytest.raises(SESSION.SessionRequestError) as error:
        SESSION.build_response(description_request())
    assert error.value.failure_class == failure_class


def test_scalar_override_uses_single_canonical_record_and_smart_fixture(fake_render_environment, monkeypatch):
    plugin_sha = hashlib.sha256(b"public test AEX identity").hexdigest()
    description = _fake_description(plugin_sha)
    observed = []

    def harness(args, *unused, **kwargs):
        if "--describe-aex" in args:
            return json.dumps(description).encode()
        fixture = json.loads(Path(args[4]).read_text())
        observed.append(fixture)
        return _fake_harness(args, *unused, **kwargs)

    monkeypatch.setattr(SESSION, "_invoke_harness", harness)
    payload = request("render_aex")
    payload["plugin"] = {"source_relative_path": "effect.aex"}
    payload["render_path"] = "smart"
    payload["parameter_override"] = {"slot": 1, "value": 80.0}
    result = SESSION.build_response(payload)
    jsonschema.validate(result, SCHEMA)
    assert result["status"] == "rendered"
    assert result["render_path"] == "smart"
    assert observed[0]["checkpoints"] == [{"id": "input", "stage": "smart-input"}]
    assert observed[0]["parameters"] == [{**description["parameters"][0], "value": 80.0}]
    assert result["parameter_override"]["description_matches_render_plugin"] is True
    assert result["parameter_override"]["value"] == 80.0


@pytest.mark.parametrize("disabled_field", ["enabled", "visible"])
def test_legacy_single_override_keeps_hidden_or_disabled_scalar_behavior(fake_render_environment, monkeypatch, disabled_field):
    plugin_sha = hashlib.sha256(b"public test AEX identity").hexdigest()
    description = _fake_description(plugin_sha)
    description["parameters"][0][disabled_field] = False
    observed = []

    def harness(args, *unused, **kwargs):
        if "--describe-aex" in args:
            return json.dumps(description).encode()
        observed.append(json.loads(Path(args[4]).read_text())["parameters"])
        return _fake_harness(args, *unused, **kwargs)

    monkeypatch.setattr(SESSION, "_invoke_harness", harness)
    payload = request("render_aex")
    payload["plugin"] = {"source_relative_path": "effect.aex"}
    payload["parameter_override"] = {"slot": 1, "value": 80.0}
    result = SESSION.build_response(payload)
    assert result["status"] == "rendered" and result["parameter_override"]["value"] == 80.0
    assert observed == [[{**description["parameters"][0], "value": 80.0}]]


@pytest.mark.parametrize("render_path", ["classic", "smart"])
def test_multiple_scalar_overrides_use_one_description_and_one_render(fake_render_environment, monkeypatch, render_path):
    plugin_sha = hashlib.sha256(b"public test AEX identity").hexdigest()
    description = _fake_description(plugin_sha)
    second = {**description["parameters"][0], "slot": 2, "name": "Iterations", "kind": "integer", "value": 3.0, "maximum": 2147483647.0}
    description["parameters"].append(second)
    description["defaults"].append(dict(second))
    calls = []
    observed = []

    def harness(args, *unused, **kwargs):
        calls.append(args)
        if "--describe-aex" in args:
            return json.dumps(description).encode()
        observed.append(json.loads(Path(args[4]).read_text())["parameters"])
        return _fake_harness(args, *unused, **kwargs)

    monkeypatch.setattr(SESSION, "_invoke_harness", harness)
    payload = request("render_aex")
    payload["plugin"] = {"source_relative_path": "effect.aex"}
    payload["render_path"] = render_path
    payload["parameter_overrides"] = [{"slot": 1, "value": 80.0}, {"slot": 2, "value": 16777217}]
    result = SESSION.build_response(payload)
    jsonschema.validate(result, SCHEMA)
    assert result["status"] == "rendered" and "parameter_override" not in result
    assert result["render_path"] == render_path
    assert [record["slot"] for record in result["parameter_overrides"]] == [1, 2]
    assert result["parameter_overrides"][1]["value"] == 16777217
    assert observed == [[{**description["parameters"][0], "value": 80.0}, {**second, "value": 16777217}]]
    assert len(calls) == 2 and "--describe-aex" in calls[0] and "--render-fixture" in calls[1]


@pytest.mark.parametrize("render_path", ["classic", "smart"])
def test_color_and_scalar_overrides_share_one_description_and_frame(fake_render_environment, monkeypatch, render_path):
    plugin_sha = hashlib.sha256(b"public test AEX identity").hexdigest()
    description = _fake_description(plugin_sha)
    color = {**description["parameters"][0], "slot": 2, "name": "Tint", "kind": "color"}
    description["parameters"].append(color)
    description["defaults"].append(dict(color))
    calls = []
    observed = []

    def harness(args, *unused, **kwargs):
        calls.append(args)
        if "--describe-aex" in args:
            return json.dumps(description).encode()
        observed.append(json.loads(Path(args[4]).read_text())["parameters"])
        return _fake_harness(args, *unused, **kwargs)

    monkeypatch.setattr(SESSION, "_invoke_harness", harness)
    payload = request("render_aex")
    payload["plugin"] = {"source_relative_path": "effect.aex"}
    payload["render_path"] = render_path
    payload["parameter_overrides"] = [{"slot": 1, "value": 80.0}, {"slot": 2, "color": [255, 17, 34, 51]}]
    result = SESSION.build_response(payload)
    jsonschema.validate(result, SCHEMA)
    assert observed == [[{**description["parameters"][0], "value": 80.0}, {**color, "color": [255, 17, 34, 51]}]]
    assert len(calls) == 2 and "--describe-aex" in calls[0] and "--render-fixture" in calls[1]
    assert result["parameter_overrides"][1]["color"] == [255, 17, 34, 51]
    assert "value" not in result["parameter_overrides"][1]
    broken = json.loads(json.dumps(result))
    broken["parameter_overrides"][1]["color"] = [255, 17, 34, 256]
    with pytest.raises(jsonschema.ValidationError):
        jsonschema.validate(broken, SCHEMA)


@pytest.mark.parametrize("color", [
    [255, 0, 0], [255, 0, 0, 256], [255, 0, -1, 0], [255, True, 0, 0],
    [255, 0.5, 0, 0], "255,0,0,0",
])
def test_color_override_rejects_invalid_channels_before_description(fake_render_environment, monkeypatch, color):
    monkeypatch.setattr(SESSION, "_invoke_harness", lambda *_args, **_kwargs: pytest.fail("AEX described"))
    payload = request("render_aex")
    payload["plugin"] = {"source_relative_path": "effect.aex"}
    payload["parameter_overrides"] = [{"slot": 1, "color": color}]
    with pytest.raises(SESSION.SessionRequestError):
        SESSION.build_response(payload)


def test_legacy_single_override_rejects_color_before_description(fake_render_environment, monkeypatch):
    monkeypatch.setattr(SESSION, "_invoke_harness", lambda *_args, **_kwargs: pytest.fail("AEX described"))
    payload = request("render_aex")
    payload["plugin"] = {"source_relative_path": "effect.aex"}
    payload["parameter_override"] = {"slot": 1, "color": [255, 1, 2, 3]}
    with pytest.raises(SESSION.SessionRequestError):
        SESSION.build_response(payload)


@pytest.mark.parametrize("invalid", ["scalar_slot", "color_with_value", "hidden", "duplicate", "duplicate_catalog", "both_fields"])
def test_color_override_rejects_wrong_kind_or_slot_before_render(fake_render_environment, monkeypatch, invalid):
    plugin_sha = hashlib.sha256(b"public test AEX identity").hexdigest()
    description = _fake_description(plugin_sha)
    color = {**description["parameters"][0], "slot": 2, "kind": "color"}
    description["parameters"].append(color)
    description["defaults"].append(dict(color))
    overrides = [{"slot": 2, "color": [255, 17, 34, 51]}]
    if invalid == "scalar_slot":
        overrides[0]["slot"] = 1
    elif invalid == "color_with_value":
        overrides[0] = {"slot": 2, "value": 17}
    elif invalid == "hidden":
        color["visible"] = False
    elif invalid == "duplicate":
        overrides.append({"slot": 2, "value": 17})
    elif invalid == "duplicate_catalog":
        description["parameters"].append({**color, "name": "Ambiguous color"})
        description["defaults"].append({**color, "name": "Ambiguous color"})
    else:
        overrides[0]["value"] = 17
    calls = []

    def harness(args, *_unused, **_kwargs):
        calls.append(args)
        assert "--describe-aex" in args
        return json.dumps(description).encode()

    monkeypatch.setattr(SESSION, "_invoke_harness", harness)
    payload = request("render_aex")
    payload["plugin"] = {"source_relative_path": "effect.aex"}
    payload["parameter_overrides"] = overrides
    with pytest.raises(SESSION.SessionRequestError):
        SESSION.build_response(payload)
    assert len(calls) <= 1


@pytest.mark.parametrize("case", [
    "empty", "too_many", "duplicate", "unknown", "hidden", "disabled", "non_scalar", "out_of_range", "bad_integer", "both_forms", "malformed",
])
def test_multiple_scalar_overrides_reject_invalid_selection_before_render(fake_render_environment, monkeypatch, case):
    plugin_sha = hashlib.sha256(b"public test AEX identity").hexdigest()
    description = _fake_description(plugin_sha)
    second = {**description["parameters"][0], "slot": 2, "name": "Second"}
    description["parameters"].append(second)
    description["defaults"].append(dict(second))
    overrides = [{"slot": 1, "value": 5.0}, {"slot": 2, "value": 6.0}]
    if case == "empty":
        overrides = []
    elif case == "too_many":
        overrides = [{"slot": index, "value": 1.0} for index in range(1, 18)]
    elif case == "duplicate":
        overrides[1]["slot"] = 1
    elif case == "unknown":
        overrides[1]["slot"] = 3
    elif case == "hidden":
        description["parameters"][1]["visible"] = False
    elif case == "disabled":
        description["parameters"][1]["enabled"] = False
    elif case == "non_scalar":
        description["parameters"][1]["kind"] = "color"
    elif case == "out_of_range":
        overrides[1]["value"] = 999.0
    elif case == "bad_integer":
        description["parameters"][1]["kind"] = "integer"
        overrides[1]["value"] = 6.5
    elif case == "malformed":
        overrides[1]["value"] = True
    calls = []

    def harness(args, *_unused, **_kwargs):
        calls.append(args)
        assert "--describe-aex" in args
        return json.dumps(description).encode()

    monkeypatch.setattr(SESSION, "_invoke_harness", harness)
    payload = request("render_aex")
    payload["plugin"] = {"source_relative_path": "effect.aex"}
    payload["parameter_overrides"] = overrides
    if case == "both_forms":
        payload["parameter_override"] = {"slot": 1, "value": 5.0}
    with pytest.raises(SESSION.SessionRequestError):
        SESSION.build_response(payload)
    assert len(calls) <= 1


@pytest.mark.parametrize("slot,value,kind", [(2, 4.0, "float"), (1, 256.0, "float"), (1, 4.5, "integer"), (1, 4.0, "color")])
def test_scalar_override_rejects_unknown_out_of_range_or_non_scalar(fake_render_environment, monkeypatch, slot, value, kind):
    plugin_sha = hashlib.sha256(b"public test AEX identity").hexdigest()
    description = _fake_description(plugin_sha, kind=kind)
    monkeypatch.setattr(SESSION, "_invoke_harness", lambda args, *_unused, **_kwargs: json.dumps(description).encode())
    payload = request("render_aex")
    payload["plugin"] = {"source_relative_path": "effect.aex"}
    payload["parameter_override"] = {"slot": slot, "value": value}
    with pytest.raises(SESSION.SessionRequestError) as error:
        SESSION.build_response(payload)
    assert error.value.failure_class == "request_validation_error"


def test_scalar_override_rejects_failed_description_without_render(fake_render_environment, monkeypatch):
    calls = []

    def harness(args, *_unused, **_kwargs):
        calls.append(args)
        raise SESSION.SessionRequestError("description failed", "parameter_description_error")

    monkeypatch.setattr(SESSION, "_invoke_harness", harness)
    payload = request("render_aex")
    payload["plugin"] = {"source_relative_path": "effect.aex"}
    payload["parameter_override"] = {"slot": 1, "value": 80.0}
    with pytest.raises(SESSION.SessionRequestError) as error:
        SESSION.build_response(payload)
    assert error.value.failure_class == "parameter_description_error"
    assert len(calls) == 1 and "--describe-aex" in calls[0]


@pytest.mark.parametrize(
    "corruption",
    ["invalid_json", "duplicate_key", "missing_identity_field", "missing_defaults",
     "missing_post_setup", "extra_record_key", "bad_default", "nonfinite", "huge_bound", "oversize"],
)
def test_scalar_override_rejects_malformed_description_before_render(fake_render_environment, monkeypatch, corruption):
    plugin_sha = hashlib.sha256(b"public test AEX identity").hexdigest()
    description = _fake_description(plugin_sha)
    if corruption == "missing_identity_field":
        del description["plugin_identity"]["files_unchanged"]
    if corruption == "missing_defaults":
        del description["defaults"]
    if corruption == "missing_post_setup":
        del description["plugin_identity"]["post_setup_sha256"]
    if corruption == "extra_record_key":
        description["parameters"][0]["unexpected"] = 1
    if corruption == "bad_default":
        description["defaults"][0]["color"] = [1, 2]
    if corruption == "nonfinite":
        description["parameters"][0]["value"] = float("nan")
    if corruption == "huge_bound":
        description["parameters"][0]["maximum"] = 10**400
    calls = []

    def harness(args, *_unused, **_kwargs):
        calls.append(args)
        assert "--describe-aex" in args
        if corruption == "invalid_json":
            return b"{"
        if corruption == "duplicate_key":
            return b'{"schema":1,"schema":2}'
        if corruption == "oversize":
            raise SESSION.SessionRequestError("too large", "artifact_mismatch")
        return json.dumps(description).encode()

    monkeypatch.setattr(SESSION, "_invoke_harness", harness)
    payload = request("render_aex")
    payload["plugin"] = {"source_relative_path": "effect.aex"}
    payload["parameter_override"] = {"slot": 1, "value": 80.0}
    with pytest.raises(SESSION.SessionRequestError) as error:
        SESSION.build_response(payload)
    assert error.value.failure_class == "parameter_description_error"
    assert len(calls) == 1


def test_scalar_override_records_description_identity_change_without_launch_gate(fake_render_environment, monkeypatch):
    description = _fake_description("0" * 64)
    observed = []

    def harness(args, *unused, **kwargs):
        if "--describe-aex" in args:
            return json.dumps(description).encode()
        observed.append(json.loads(Path(args[4]).read_text())["parameters"])
        return _fake_harness(args, *unused, **kwargs)

    monkeypatch.setattr(SESSION, "_invoke_harness", harness)
    payload = request("render_aex")
    payload["plugin"] = {"source_relative_path": "effect.aex"}
    payload["parameter_override"] = {"slot": 1, "value": 80.0}
    result = SESSION.build_response(payload)
    jsonschema.validate(result, SCHEMA)
    assert result["status"] == "rendered"
    assert result["parameter_override"]["description_matches_render_plugin"] is False
    assert observed == [[{**description["parameters"][0], "value": 80.0}]]


def test_angle_override_changes_only_first_component(fake_render_environment, monkeypatch):
    plugin_sha = hashlib.sha256(b"public test AEX identity").hexdigest()
    description = _fake_description(plugin_sha, kind="angle")
    description["parameters"][0]["component_count"] = 1
    description["parameters"][0]["components"] = [2.0, 7.0, 9.0]
    observed = []

    def harness(args, *unused, **kwargs):
        if "--describe-aex" in args:
            return json.dumps(description).encode()
        observed.append(json.loads(Path(args[4]).read_text())["parameters"])
        return _fake_harness(args, *unused, **kwargs)

    monkeypatch.setattr(SESSION, "_invoke_harness", harness)
    payload = request("render_aex")
    payload["plugin"] = {"source_relative_path": "effect.aex"}
    payload["parameter_override"] = {"slot": 1, "value": 45.0}
    result = SESSION.build_response(payload)
    assert result["status"] == "rendered"
    assert observed[0] == [{**description["parameters"][0], "components": [45.0, 7.0, 9.0]}]


def test_non_aex_mode_rejects_override_without_invoking_harness(monkeypatch):
    monkeypatch.setattr(SESSION, "_invoke_harness", lambda *_args, **_kwargs: pytest.fail("harness was invoked"))
    payload = request("identity_no_aex")
    payload["parameter_override"] = {"slot": 1, "value": 5.0}
    with pytest.raises(SESSION.SessionRequestError):
        SESSION.build_response(payload)


@pytest.mark.skipif(sys.platform != "darwin", reason="macOS behavior")
@pytest.mark.parametrize("behavior", ["invalid_stdout", "nonzero_exit", "oversized_stdout"])
def test_parameter_description_subprocess_failure_does_not_render(fake_render_environment, behavior):
    harness = Path(SESSION.os.environ["AEXCOMPAT_HARNESS"])
    if behavior == "invalid_stdout":
        body = "printf '{bad json'\n"
    elif behavior == "nonzero_exit":
        body = "exit 3\n"
    else:
        body = f"{sys.executable} -c 'import sys; sys.stdout.write(\"x\" * {SESSION.MAX_RENDER_METADATA_BYTES + 1})'\n"
    harness.write_text("#!/bin/sh\n" + body, encoding="utf-8")
    harness.chmod(0o755)
    payload = request("render_aex")
    payload["plugin"] = {"source_relative_path": "effect.aex"}
    payload["parameter_override"] = {"slot": 1, "value": 80.0}
    with pytest.raises(SESSION.SessionRequestError) as error:
        SESSION.build_response(payload)
    assert error.value.failure_class == "parameter_description_error"


def test_real_render_mode_checks_pixels_and_identity(fake_render_environment, monkeypatch):
    monkeypatch.setattr(SESSION, "_invoke_harness", _fake_harness)
    payload = request("render_aex")
    payload["plugin"] = {"source_relative_path": "effect.aex"}
    payload["frame"]["frame_time"] = {"seconds": 1.25}
    result = SESSION.build_response(payload)
    jsonschema.validate(result, SCHEMA)
    original = bytes(range(16))
    expected = bytes(255 - value if index % 4 != 3 else value for index, value in enumerate(original))
    assert base64.b64decode(result["output"]["data"]) == expected
    assert result["status"] == "rendered"
    assert result["aex_render_performed"] and result["host_success"]
    assert result["diff"]["changed_bytes"] == 12
    assert result["plugin_identity"]["sha256"] == hashlib.sha256(b"public test AEX identity").hexdigest()
    assert result["render_identity"]["files_unchanged"] is True


def test_real_render_mode_records_changed_executable_without_hash_gate(fake_render_environment, monkeypatch):
    monkeypatch.setattr(
        SESSION, "_invoke_harness",
        lambda *args, **kwargs: _fake_harness(*args, corruption="mutated_file", **kwargs),
    )
    payload = request("render_aex")
    payload["plugin"] = {"source_relative_path": "effect.aex"}
    result = SESSION.build_response(payload)
    jsonschema.validate(result, SCHEMA)
    assert result["status"] == "rendered"
    assert result["render_identity"]["files_unchanged"] is False
    assert result["render_identity"]["harness_sha256"] != result["render_identity"]["post_run"]["harness_sha256"]


@pytest.mark.parametrize("corruption", ["checksum", "identity", "timing", "duplicate", "path", "oversize"])
def test_real_render_mode_rejects_corrupt_artifact(fake_render_environment, monkeypatch, corruption):
    monkeypatch.setattr(SESSION, "_invoke_harness", lambda *args, **kwargs: _fake_harness(*args, corruption=corruption, **kwargs))
    payload = request("render_aex")
    payload["plugin"] = {"source_relative_path": "effect.aex"}
    with pytest.raises(SESSION.SessionRequestError) as error:
        SESSION.build_response(payload)
    assert error.value.failure_class == "artifact_mismatch"


def test_real_render_mode_classifies_missing_aex(fake_render_environment):
    payload = request("render_aex")
    payload["plugin"] = {"source_relative_path": "missing.aex"}
    with pytest.raises(SESSION.SessionRequestError) as error:
        SESSION.build_response(payload)
    assert error.value.failure_class == "aex_not_loaded"


@pytest.mark.skipif(sys.platform != "darwin", reason="macOS behavior")
def test_real_render_mode_classifies_timeout(fake_render_environment):
    harness = Path(SESSION.os.environ["AEXCOMPAT_HARNESS"])
    harness.write_text("#!/bin/sh\nsleep 3\n", encoding="utf-8")
    harness.chmod(0o755)
    payload = request("render_aex")
    payload["plugin"] = {"source_relative_path": "effect.aex"}
    payload["timeout_ms"] = 1000
    with pytest.raises(SESSION.SessionRequestError) as error:
        SESSION.build_response(payload)
    assert error.value.failure_class == "session_timeout"


@pytest.mark.skipif(sys.platform != "darwin", reason="macOS behavior")
def test_real_render_mode_classifies_harness_failure(fake_render_environment):
    harness = Path(SESSION.os.environ["AEXCOMPAT_HARNESS"])
    harness.write_text("#!/bin/sh\nexit 3\n", encoding="utf-8")
    harness.chmod(0o755)
    payload = request("render_aex")
    payload["plugin"] = {"source_relative_path": "effect.aex"}
    with pytest.raises(SESSION.SessionRequestError) as error:
        SESSION.build_response(payload)
    assert error.value.failure_class == "worker_failure"


@pytest.mark.skipif(sys.platform != "darwin", reason="macOS behavior")
def test_real_render_mode_classifies_unlaunchable_harness(fake_render_environment):
    harness = Path(SESSION.os.environ["AEXCOMPAT_HARNESS"])
    harness.chmod(0o644)
    payload = request("render_aex")
    payload["plugin"] = {"source_relative_path": "effect.aex"}
    with pytest.raises(SESSION.SessionRequestError) as error:
        SESSION.build_response(payload)
    assert error.value.failure_class == "worker_crash"


@pytest.mark.skipif(sys.platform != "darwin", reason="macOS behavior")
def test_real_render_mode_bounds_harness_report(fake_render_environment):
    harness = Path(SESSION.os.environ["AEXCOMPAT_HARNESS"])
    harness.write_text(
        f"#!{sys.executable}\nimport sys\nsys.stdout.buffer.write(b'x' * {SESSION.MAX_RENDER_METADATA_BYTES + 1})\n",
        encoding="utf-8",
    )
    harness.chmod(0o755)
    payload = request("render_aex")
    payload["plugin"] = {"source_relative_path": "effect.aex"}
    with pytest.raises(SESSION.SessionRequestError) as error:
        SESSION.build_response(payload)
    assert error.value.failure_class == "artifact_mismatch"

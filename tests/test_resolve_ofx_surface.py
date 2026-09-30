import hashlib
import json
import shutil
import subprocess
import sys
from pathlib import Path

import jsonschema
import pytest


ROOT = Path(__file__).resolve().parents[1]


def _reject_duplicate_keys(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def _read_contract_and_schema():
    contract = json.loads(
        (ROOT / "contracts" / "resolve_ofx_surface.json").read_text(encoding="utf-8"),
        object_pairs_hook=_reject_duplicate_keys,
    )
    schema = json.loads(
        (ROOT / "contracts" / "resolve_ofx_surface.schema.json").read_text(encoding="utf-8"),
        object_pairs_hook=_reject_duplicate_keys,
    )
    jsonschema.Draft202012Validator.check_schema(schema)
    jsonschema.validate(contract, schema)
    return contract, schema


def test_resolve_ofx_contract_is_machine_checkable_and_fail_closed():
    contract, schema = _read_contract_and_schema()
    assert schema["properties"]["schema_version"]["const"] == 1
    assert contract["schema_version"] == 1
    assert contract["provenance"]["vendored_headers"] is False
    assert contract["package"] == {
        "bundle_suffix": ".ofx.bundle",
        "binary_suffix": ".ofx",
        "windows_architecture": "Win64",
        "macos_architecture": "MacOS/arm64",
    }
    assert contract["control_render"]["state"] == "verified_local_fixture"
    assert contract["control_render"]["render_claim"] == "builtin_rgba8_control"
    assert contract["control_render"]["aex_render_claim"] == (
        "blocked_missing_aex_rendersession"
    )
    assert contract["control_render"]["source_rowbytes"] == 20
    assert contract["control_render"]["output_rowbytes"] == 24
    assert contract["control_render"]["parameter"] == {"name": "strength", "time": 7.0, "value": 0.25}
    assert contract["control_render"]["pixel_diff"] == 12
    assert contract["float_control_render"]["changed_rgb_values"] == 12
    assert contract["float_control_render"]["mismatched_depth_rejected"] is True
    assert contract["aex_render_gate"]["state"] == "blocked"
    assert contract["aex_render_gate"]["success_status"] == "not_claimed"
    assert contract["aex_render_gate"]["blocked_status"] == "kOfxStatErrUnsupported"
    assert len(contract["identity"]["source_sha256"]) == 64
    assert len(contract["identity"]["binary_sha256"]) == 64
    assert contract["identity"]["binary_sha256"] == contract["macos_host_evidence"]["binary_sha256"]
    # Git may expand LF to CRLF in a Windows checkout; this records the
    # canonical source content rather than that checkout's line endings.
    source_bytes = (
        ROOT / "bridges" / "resolve-ofx" / "src" / "resolve_ofx_plugin.cpp"
    ).read_bytes().replace(b"\r\n", b"\n")
    assert hashlib.sha256(source_bytes).hexdigest().upper() == contract["identity"]["source_sha256"]
    assert hashlib.sha256(
        (ROOT / "bridges" / "resolve-ofx" / "fixtures" / "control-input.png").read_bytes()
    ).hexdigest().upper() == contract["macos_host_evidence"]["control_input_sha256"]
    assert contract["host_evidence"]["standard_external_path_entries"] == 0
    assert contract["host_evidence"]["real_resolve_smoke"] == (
        "blocked_external_path_empty"
    )
    assert contract["macos_host_evidence"]["discovery"] == "verified"
    assert contract["macos_host_evidence"]["changed_exported_pixels"] == 1166400


@pytest.mark.skipif(sys.platform not in {"darwin", "win32"}, reason="native OFX targets macOS and Windows")
def test_resolve_ofx_native_lifecycle_and_render_match_contract(tmp_path):
    cmake = shutil.which("cmake")
    if cmake is None:
        pytest.skip("CMake unavailable")
    contract, _ = _read_contract_and_schema()
    source = ROOT / "bridges" / "resolve-ofx"
    build = tmp_path / "build"
    stage = tmp_path / "stage"
    subprocess.run([cmake, "-S", str(source), "-B", str(build), "-DCMAKE_BUILD_TYPE=Release"], check=True, capture_output=True, text=True)
    subprocess.run([cmake, "--build", str(build), "--config", "Release"], check=True, capture_output=True, text=True)
    subprocess.run([cmake, "--install", str(build), "--config", "Release", "--prefix", str(stage)], check=True, capture_output=True, text=True)
    arch = "MacOS" if sys.platform == "darwin" else "Win64"
    plugin = stage / "AEXCompatResolve.ofx.bundle" / "Contents" / arch / "AEXCompatResolve.ofx"
    smoke = next(
        path for path in (build / "resolve_ofx_smoke", build / "Release" / "resolve_ofx_smoke.exe", build / "resolve_ofx_smoke.exe")
        if path.is_file()
    )
    result = subprocess.run([str(smoke), str(plugin)], check=True, capture_output=True, text=True)
    report = json.loads(result.stdout, object_pairs_hook=_reject_duplicate_keys)
    assert report["plugin_identifier"] == contract["identity"]["plugin_identifier"]
    assert report["lifecycle_ok"] is True
    assert report["descriptor_contract_ok"] is True
    assert report["rgba8_contract"] is True
    assert report["rgba_float_contract"] is True
    assert report["instance_data_failure_checks"] is True
    assert report["mismatched_depth_rejected"] is True
    assert report["safety_checks"] is True
    assert report["render_claim"] == contract["control_render"]["render_claim"]
    assert report["aex_render_claim"] == contract["control_render"]["aex_render_claim"]
    assert report["input_sha256"] == contract["control_render"]["input_sha256"]
    assert report["output_sha256"] == contract["control_render"]["output_sha256"]
    assert report["pixel_diff"] == contract["control_render"]["pixel_diff"]
    assert report["float_input_sha256"] == contract["float_control_render"]["input_sha256"]
    assert report["float_output_sha256"] == contract["float_control_render"]["output_sha256"]
    assert report["float_pixel_diff"] == contract["float_control_render"]["changed_rgb_values"]

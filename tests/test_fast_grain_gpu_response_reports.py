"""Mutation coverage for the Fast Grain GPU report oracle, without an AEX."""
import copy
import json
import shutil
import subprocess
import sys
from pathlib import PureWindowsPath

import pytest
from PIL import Image

from test_fast_grain_fixed_diagnostic_card import source_pixels
from test_fast_grain_gpu_response import (
    PIXELS,
    CASES,
    PLUGIN_SHA,
    argb_float_bytes,
    assert_report,
    assert_response,
    float_bytes,
    read_json,
    sha,
    synthetic_outputs,
    unique_object,
    validate_evidence,
)
from test_render_fixture_semantic_response import HEIGHT, WIDTH


IDENTITIES = {
    "plugin": {"sha256": "a" * 64, "size_bytes": 1_568_256},
    "worker": {"sha256": "b" * 64, "size_bytes": 2_336_256},
}


def _parameter(slot, value, kind):
    return {"id": f"param_{slot}", "kind": kind, "slot": slot, "value": value}


def _valid_case():
    source = source_pixels()
    raw = float_bytes(source)
    report = {
        "schema_version": 1,
        "stage": "interactive_image_render",
        "output_transport": "native_raw+rgba8_png_preview",
        "smart_render_selector_dispatched": True,
        "passed": True,
        "output_pixels_valid": True,
        "gpu_render_possible": True,
        "gpu_render_dispatched": True,
        "cuda_context_used": True,
        "guard_bytes_intact": True,
        "suite_leases_balanced": True,
        "handle_lifetimes_balanced": True,
        "world_lifetimes_balanced": True,
        "param_checkouts_balanced": True,
        "parameter_count_contract_ok": True,
        "gpu_fallback_used": False,
        "worker_classification": "ok",
        "render_path": "smartfx",
        "pixel_format": "argb32f",
        "cuda_upload_bytes": PIXELS * 16,
        "cuda_download_bytes": PIXELS * 16,
        "gpu_device_setup_error": 0,
        "gpu_device_setdown_error": 0,
        "gpu_device_setdown_exception_code": 0,
        "cuda_sync_failures": 0,
        "pre_render_error": 0,
        "smart_render_error": 0,
        "smart_render_selector_error": 0,
        "last_seh_exception_code": 0,
        "width": WIDTH,
        "height": HEIGHT,
        "current_time": 0,
        "time_step": 1,
        "total_time": 300,
        "time_scale": 30,
        "output_sha256": sha(argb_float_bytes(raw)),
        "input_sha256": sha(argb_float_bytes(float_bytes(source))),
        "gpu_memory": {
            "lifetimes_balanced": True,
            "allocations_created": 1,
            "allocations_freed": 1,
            "live_allocation_count": 0,
            "live_bytes": 0,
            "invalid_operations": 0,
        },
        "requested_parameters": [
            _parameter(1, 0, "float"),
            _parameter(2, 1.25, "float"),
            _parameter(3, 18, "float"),
            _parameter(4, 24, "float"),
            _parameter(8, {"bytes": 11}, "arbitrary_text"),
            _parameter(9, 1, "integer"),
            _parameter(10, 0, "integer"),
            _parameter(11, 0, "integer"),
            _parameter(13, 8, "integer"),
            _parameter(14, 100, "float"),
        ],
        "worker_diagnostics": {
            "classification": "ok",
            "exit_code": 0,
            "last_completed_stage": "global_setdown",
            "failure_stage": None,
            "first_failure_stage": None,
            "active_stage": None,
            "load_failure": None,
            "kill_reason": None,
            "callback_denials": [],
            "callback_addr_denials": [],
            "callback_denials_truncated": False,
            "callback_addr_denials_truncated": False,
            "missing_suites_truncated": False,
            "suite_acquire_failures_truncated": False,
            "unsupported_suite_calls_truncated": False,
            "suite_timeline_truncated": False,
            "stderr_truncated": False,
            "stage_events": [{"stage": "smart_render_gpu", "state": "end", "errors": {"error": 0}}],
            "suite_timeline": [
                {"name": "AEGP Stream Suite", "version": 10, "action": action,
                 "selector": "SMART_PRE_RENDER", "result": 0}
                for action in ("acquire", "release")],
            "missing_suites": [],
            "suite_acquire_failures": [],
            "unsupported_suite_calls": [],
            "execution_identity": {
                "schema_version": 1,
                "worker": {
                    "sha256": IDENTITIES["worker"]["sha256"],
                    "size_bytes": IDENTITIES["worker"]["size_bytes"],
                    "binding": "broker_authenticated_pinned_stage",
                },
                "plugin_images": [{
                    "basename": "Fast Grain.aex",
                    "plugin_index": 0,
                    "sha256": IDENTITIES["plugin"]["sha256"],
                    "size_bytes": IDENTITIES["plugin"]["size_bytes"],
                    "binding_status": "same_file_identity_matches_loaded_module",
                }],
            },
        },
    }
    return report, source, raw, copy.deepcopy(IDENTITIES)


def _parameter_by_slot(report, slot):
    return next(item for item in report["requested_parameters"]
                if item["slot"] == slot)


def _corrupt(case, fault):
    report, source, raw, identities = case
    if fault == "gpu_possible_false":
        report["gpu_render_possible"] = False
    elif fault == "gpu_dispatch_false":
        report["gpu_render_dispatched"] = False
    elif fault == "cuda_context_false":
        report["cuda_context_used"] = False
    elif fault == "gpu_fallback":
        report["gpu_fallback_used"] = True
    elif fault == "cuda_bytes":
        report["cuda_download_bytes"] -= 16
    elif fault == "raw_bytes":
        damaged = bytearray(raw)
        damaged[0] ^= 1
        raw = bytes(damaged)
    elif fault == "short_raw":
        raw = raw[:-16]
    elif fault == "source_bytes":
        damaged = bytearray(source)
        damaged[0] ^= 1
        source = bytes(damaged)
    elif fault == "native_output_hash":
        report["output_sha256"] = "0" * 64
    elif fault == "native_input_hash":
        report["input_sha256"] = "0" * 64
    elif fault == "current_time":
        report["current_time"] = 1
    elif fault == "time_step":
        report["time_step"] = 0
    elif fault == "total_time":
        report["total_time"] = 299
    elif fault == "time_scale":
        report["time_scale"] = 29
    elif fault == "width":
        report["width"] -= 1
    elif fault == "height":
        report["height"] -= 1
    elif fault == "duplicate_slot":
        duplicate = copy.deepcopy(_parameter_by_slot(report, 1))
        duplicate["id"] = "duplicate_param"
        report["requested_parameters"].append(duplicate)
    elif fault == "duplicate_id":
        _parameter_by_slot(report, 2)["id"] = "param_1"
    elif fault == "missing_parameter":
        report["requested_parameters"] = [
            item for item in report["requested_parameters"] if item["slot"] != 14
        ]
    elif fault == "wrong_parameter_value":
        _parameter_by_slot(report, 1)["value"] = 1
    elif fault == "wrong_parameter_id":
        _parameter_by_slot(report, 13)["id"] = "param_99"
    elif fault == "wrong_parameter_kind":
        _parameter_by_slot(report, 1)["kind"] = "integer"
    elif fault == "boolean_parameter":
        _parameter_by_slot(report, 1)["value"] = False
    elif fault == "boolean_slot":
        _parameter_by_slot(report, 1)["slot"] = True
    elif fault == "wrong_arbitrary_id":
        _parameter_by_slot(report, 8)["id"] = "param_99"
    elif fault == "wrong_arbitrary_parameter":
        _parameter_by_slot(report, 8)["value"] = {"bytes": 10}
    elif fault == "plugin_binding":
        report["worker_diagnostics"]["execution_identity"]["plugin_images"][0][
            "binding_status"
        ] = "observed_without_loaded_module_match"
    elif fault == "plugin_identity":
        report["worker_diagnostics"]["execution_identity"]["plugin_images"][0][
            "sha256"
        ] = "c" * 64
    elif fault == "plugin_basename":
        report["worker_diagnostics"]["execution_identity"]["plugin_images"][0][
            "basename"
        ] = "Other.aex"
    elif fault == "plugin_index":
        report["worker_diagnostics"]["execution_identity"]["plugin_images"][0][
            "plugin_index"
        ] = 1
    elif fault == "worker_binding":
        report["worker_diagnostics"]["execution_identity"]["worker"][
            "binding"
        ] = "unbound"
    elif fault == "worker_identity":
        report["worker_diagnostics"]["execution_identity"]["worker"][
            "sha256"
        ] = "d" * 64
    elif fault == "boolean_identity_schema":
        report["worker_diagnostics"]["execution_identity"]["schema_version"] = True
    elif fault == "unsupported_call":
        report["worker_diagnostics"]["unsupported_suite_calls"].append({})
    elif fault == "missing_suite":
        report["worker_diagnostics"]["missing_suites"].append({})
    elif fault == "suite_acquire_failure":
        report["worker_diagnostics"]["suite_acquire_failures"].append({})
    elif fault == "render_error":
        report["smart_render_error"] = 1
    elif fault == "boolean_error":
        report["smart_render_error"] = False
    elif fault == "suite_lease":
        report["suite_leases_balanced"] = False
    elif fault == "handle_lease":
        report["handle_lifetimes_balanced"] = False
    elif fault == "gpu_memory_lease":
        report["gpu_memory"]["lifetimes_balanced"] = False
    elif fault == "gpu_memory_count":
        report["gpu_memory"]["allocations_freed"] = 0
    elif fault == "gpu_memory_live":
        report["gpu_memory"]["live_bytes"] = 16
    else:
        raise AssertionError(f"unknown mutation: {fault}")
    return report, source, raw, identities


@pytest.mark.parametrize("fault", [
    "gpu_possible_false",
    "gpu_dispatch_false",
    "cuda_context_false",
    "gpu_fallback",
    "cuda_bytes",
    "raw_bytes",
    "short_raw",
    "source_bytes",
    "native_output_hash",
    "native_input_hash",
    "current_time",
    "time_step",
    "total_time",
    "time_scale",
    "width",
    "height",
    "duplicate_slot",
    "duplicate_id",
    "missing_parameter",
    "wrong_parameter_value",
    "wrong_parameter_id",
    "wrong_parameter_kind",
    "boolean_parameter",
    "boolean_slot",
    "wrong_arbitrary_id",
    "wrong_arbitrary_parameter",
    "plugin_binding",
    "plugin_identity",
    "plugin_basename",
    "plugin_index",
    "worker_binding",
    "worker_identity",
    "boolean_identity_schema",
    "unsupported_call",
    "missing_suite",
    "suite_acquire_failure",
    "render_error",
    "boolean_error",
    "suite_lease",
    "handle_lease",
    "gpu_memory_lease",
    "gpu_memory_count",
    "gpu_memory_live",
])
def test_report_oracle_rejects_false_success(fault):
    valid = _valid_case()
    assert_report(valid[0], valid[1], valid[2], 0, valid[3])
    corrupted = _corrupt(copy.deepcopy(valid), fault)
    with pytest.raises(AssertionError):
        assert_report(corrupted[0], corrupted[1], corrupted[2], 0, corrupted[3])


def test_unique_object_rejects_duplicate_json_keys():
    with pytest.raises(AssertionError, match="duplicate JSON key: passed"):
        json.loads('{"passed":true,"passed":false}', object_pairs_hook=unique_object)


def test_optimized_python_cannot_silently_skip_evidence_assertions():
    script = ("import sys; sys.path.insert(0, 'tests'); "
              "from test_fast_grain_gpu_response import require_assertions; require_assertions()")
    from test_render_fixture_semantic_response import ROOT
    result = subprocess.run([sys.executable, "-O", "-c", script], cwd=ROOT,
                            capture_output=True, timeout=30)
    assert result.returncode != 0
    assert b"evidence validation requires Python assertions" in result.stderr


@pytest.mark.parametrize("scope,key,value", [
    ("report", "schema_version", 2),
    ("report", "stage", "inspection"),
    ("report", "output_transport", "rgba8_png"),
    ("report", "smart_render_selector_dispatched", False),
    ("diag", "classification", "worker_exit"),
    ("diag", "exit_code", 1),
    ("diag", "last_completed_stage", "smart_render_gpu"),
    ("diag", "failure_stage", "global_setdown"),
    ("diag", "first_failure_stage", "smart_pre_render"),
    ("diag", "active_stage", "global_setdown"),
    ("diag", "load_failure", {}),
    ("diag", "kill_reason", "deadline"),
    ("diag", "callback_denials", [{}]),
    ("diag", "callback_addr_denials", [{}]),
    ("diag", "callback_denials_truncated", True),
    ("diag", "callback_addr_denials_truncated", True),
    ("diag", "missing_suites_truncated", True),
    ("diag", "suite_acquire_failures_truncated", True),
    ("diag", "unsupported_suite_calls_truncated", True),
    ("diag", "suite_timeline_truncated", True),
    ("diag", "stderr_truncated", True),
    ("diag", "stage_events", []),
    ("diag", "suite_timeline", []),
])
def test_report_oracle_rejects_incomplete_or_contradictory_evidence(scope, key, value):
    report, source, raw, identities = _valid_case()
    assert_report(report, source, raw, 0, identities)
    target = report if scope == "report" else report["worker_diagnostics"]
    target[key] = value
    with pytest.raises(AssertionError):
        assert_report(report, source, raw, 0, identities)


@pytest.fixture(scope="module")
def synthetic_capture(tmp_path_factory):
    directory = tmp_path_factory.mktemp("synthetic-grain-capture")
    outputs = synthetic_outputs()
    metadata = {"schema_version": 1, "identities_before": {
        "plugin": {"sha256": PLUGIN_SHA, "size_bytes": 1_568_256},
        "worker": IDENTITIES["worker"],
        "harness": {"sha256": "c" * 64, "size_bytes": 12345}}, "cases": {}}
    metadata["identities_after"] = copy.deepcopy(metadata["identities_before"])
    for label, alternate, intensity in CASES:
        source = source_pixels(alternate)
        Image.frombytes("RGBA", (WIDTH, HEIGHT), source).save(directory / f"{label}-source.png")
        Image.frombytes("RGBA", (WIDTH, HEIGHT), outputs[label]).save(directory / f"{label}.png")
        raw = float_bytes(outputs[label])
        (directory / f"{label}.rgba32f-le").write_bytes(raw)
        report, _, _, _ = _valid_case()
        report["output_sha256"] = sha(argb_float_bytes(raw))
        report["input_sha256"] = sha(argb_float_bytes(float_bytes(source)))
        report["worker_diagnostics"]["execution_identity"]["plugin_images"][0]["sha256"] = PLUGIN_SHA
        _parameter_by_slot(report, 1)["value"] = intensity
        report["output_png"] = str(PureWindowsPath("C:/capture") / f"{label}.png")
        report["output_raw"] = str(PureWindowsPath(report["output_png"]).with_suffix(".rgba32f-le"))
        (directory / f"{label}.stdout.json").write_text(json.dumps(report), encoding="utf-8")
        (directory / f"{label}.stderr.txt").write_bytes(b"")
        metadata["cases"][label] = {"returncode": 0, "intensity": intensity, "elapsed_seconds": 0.5,
            "command": ["C:/app/aexcompat-harness.exe", "--headless", "--render-experimental-session-param",
                "C:/plugins/Fast Grain.aex", f"C:/capture/{label}-source.png", report["output_png"],
                "argb32f", "smart", "0", "300", "30", "1", str(intensity)]}
    (directory / "capture.json").write_text(json.dumps(metadata), encoding="utf-8")
    (directory / "verification.json").write_text(json.dumps(assert_response(outputs)), encoding="utf-8")
    validate_evidence(directory)
    return directory


@pytest.mark.parametrize("fault", ["capture_schema", "harness_identity", "argv", "source_label", "elapsed_nan",
    "elapsed_negative", "returncode", "stderr", "report_path", "raw_path", "metrics", "png_channels"])
def test_capture_oracle_rejects_mismatched_metadata(synthetic_capture, tmp_path, fault):
    directory = tmp_path / "capture"
    shutil.copytree(synthetic_capture, directory)
    metadata = read_json(directory / "capture.json")
    if fault == "capture_schema":
        metadata["schema_version"] = True
    elif fault == "harness_identity":
        del metadata["identities_before"]["harness"]
        del metadata["identities_after"]["harness"]
    elif fault == "argv":
        metadata["cases"]["a-0"]["command"][2] = "--render-experimental-smart-32-cpu"
    elif fault == "source_label":
        metadata["cases"]["a-0"]["command"][4] = "C:/capture/b-0-source.png"
    elif fault.startswith("elapsed"):
        metadata["cases"]["a-0"]["elapsed_seconds"] = float("nan") if fault.endswith("nan") else -1
    elif fault == "returncode":
        metadata["cases"]["a-0"]["returncode"] = False
    elif fault == "stderr":
        (directory / "a-0.stderr.txt").write_bytes(b"unexpected warning")
    elif fault in ("report_path", "raw_path"):
        report = read_json(directory / "a-0.stdout.json")
        report["output_png" if fault == "report_path" else "output_raw"] = "C:/other/mismatched.png"
        (directory / "a-0.stdout.json").write_text(json.dumps(report), encoding="utf-8")
    elif fault == "metrics":
        metrics = read_json(directory / "verification.json")
        metrics["a"]["changed_pixels"] -= 1
        (directory / "verification.json").write_text(json.dumps(metrics), encoding="utf-8")
    else:
        with Image.open(directory / "b-100.png") as image:
            r, g, b, a = image.convert("RGBA").split()
            Image.merge("RGBA", (b, g, r, a)).save(directory / "b-100.png")
    (directory / "capture.json").write_text(json.dumps(metadata), encoding="utf-8")
    with pytest.raises(AssertionError):
        validate_evidence(directory)

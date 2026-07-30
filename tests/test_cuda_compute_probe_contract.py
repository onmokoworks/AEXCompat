import copy
import json
from pathlib import Path

import pytest
from jsonschema import Draft202012Validator


ROOT = Path(__file__).resolve().parents[1]
SCHEMA_PATH = ROOT / "schemas" / "cuda-compute-probe.schema.json"
WORKER_PATH = (
    ROOT
    / "broker"
    / "crates"
    / "broker"
    / "src"
    / "bin"
    / "cuda_compute_probe_worker.rs"
)


def _validator():
    schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    return Draft202012Validator(schema)


def _device(ordinal=0):
    return {
        "ordinal": ordinal,
        "name": "NVIDIA Fixture",
        "uuid_fingerprint_sha256": "a" * 64,
        "pci_location": {"domain": 0, "bus": 1, "device": 0},
        "compute_capability_major": 8,
        "compute_capability_minor": 9,
        "total_memory_bytes": 12 << 30,
        "stage": "passed",
        "operation": None,
        "api_error": None,
        "jit_log": None,
        "cleanup_failures": [],
    }


def _diagnostic(kind, operation):
    return {
        "kind": kind,
        "operation": operation,
        "api_error": None,
        "device_ordinal": None,
    }


def _aggregate(status="observed"):
    if status == "observed":
        return {
            "status": "observed",
            "driver_version": 13000,
            "devices": [_device()],
            "diagnostics": [],
            "cuda_compute_ready": True,
        }
    return {
        "status": status,
        "driver_version": None,
        "devices": [],
        "diagnostics": [_diagnostic(status, "probe.failure")],
        "cuda_compute_ready": False,
    }


def _report(launch_status="observed"):
    observed = launch_status == "observed"
    return {
        "schema_version": 1,
        "contract": "system_cuda_driver_compute_probe",
        "aggregate_driver_observation": _aggregate() if observed else None,
        "launch": {
            "status": launch_status,
            "exit_code": None if launch_status == "launch_error" else 0,
            "kill_reason": None,
            "stdout_truncated": False,
            "stderr_truncated": False,
            "memory_limit_reached": False,
        },
        "cuda_compute_ready": observed,
        "backend_ready": False,
    }


def _failed_device(stage, operation, api_error=700):
    device = _device()
    device.update(
        {
            "stage": stage,
            "operation": operation,
            "api_error": api_error,
            "jit_log": None,
        }
    )
    return device


def test_schema_accepts_success_and_all_launch_failures():
    validator = _validator()
    validator.validate(_report())
    for status in (
        "nonzero_exit",
        "timeout",
        "crash",
        "output_truncated",
        "malformed_output",
        "launch_error",
    ):
        validator.validate(_report(status))


@pytest.mark.parametrize(
    "stage,operation",
    (
        ("context", "cuCtxCreate_v2.null_handle"),
        ("allocation", "cuMemAlloc_v2.input.null_handle"),
        ("allocation", "cuMemAlloc_v2.output.null_handle"),
        ("module", "cuModuleLoadDataEx.null_handle"),
        ("function", "cuModuleGetFunction.null_handle"),
    ),
)
def test_null_handle_is_structural_failure_with_null_api_error(stage, operation):
    validator = _validator()
    report = _report()
    report["aggregate_driver_observation"] = {
        "status": "failed",
        "driver_version": 13000,
        "devices": [_failed_device(stage, operation, None)],
        "diagnostics": [_diagnostic("device_failure", "device_compute_failed")],
        "cuda_compute_ready": False,
    }
    report["cuda_compute_ready"] = False
    validator.validate(report)

    invalid = copy.deepcopy(report)
    invalid["aggregate_driver_observation"]["devices"][0][
        "operation"
    ] = operation.removesuffix(".null_handle")
    assert list(validator.iter_errors(invalid))


def test_partial_keeps_devices_but_cannot_claim_ready():
    validator = _validator()
    report = _report()
    report["aggregate_driver_observation"] = {
        "status": "partial",
        "driver_version": 13000,
        "devices": [
            _device(0),
            {
                **_failed_device("launch", "cuLaunchKernel", 719),
                "ordinal": 1,
            },
        ],
        "diagnostics": [_diagnostic("partial", "device_compute_partial")],
        "cuda_compute_ready": False,
    }
    report["cuda_compute_ready"] = False
    validator.validate(report)

    wrong_ready = copy.deepcopy(report)
    wrong_ready["cuda_compute_ready"] = True
    assert list(validator.iter_errors(wrong_ready))

    missing_diagnostic = copy.deepcopy(report)
    missing_diagnostic["aggregate_driver_observation"]["diagnostics"] = []
    assert list(validator.iter_errors(missing_diagnostic))


@pytest.mark.parametrize(
    "target,operation,api_error",
    (
        ("module", "cuModuleUnload", 711),
        ("output_memory", "cuMemFree_v2.output", 703),
        ("input_memory", "cuMemFree_v2.input", 702),
        ("context", "cuCtxDestroy_v2", 709),
    ),
)
def test_cleanup_failure_is_structured_and_forces_not_ready(
    target, operation, api_error
):
    validator = _validator()
    report = _report()
    device = _device()
    device.update(
        {
            "stage": "cleanup_failed",
            "operation": operation,
            "api_error": api_error,
            "cleanup_failures": [
                {
                    "target": target,
                    "operation": operation,
                    "api_error": api_error,
                    "symbolic_info": "CUDA_ERROR_UNKNOWN",
                }
            ],
        }
    )
    report["aggregate_driver_observation"] = {
        "status": "failed",
        "driver_version": 13000,
        "devices": [device],
        "diagnostics": [_diagnostic("device_failure", "device_compute_failed")],
        "cuda_compute_ready": False,
    }
    report["cuda_compute_ready"] = False
    validator.validate(report)

    wrong_target = copy.deepcopy(report)
    wrong_target["aggregate_driver_observation"]["devices"][0][
        "cleanup_failures"
    ][0]["target"] = "context" if target != "context" else "module"
    assert list(validator.iter_errors(wrong_target))

    raw_symbolic = copy.deepcopy(report)
    raw_symbolic["aggregate_driver_observation"]["devices"][0][
        "cleanup_failures"
    ][0]["symbolic_info"] = r"C:\Users\name\driver.log"
    assert list(validator.iter_errors(raw_symbolic))

    success_code = copy.deepcopy(report)
    success_code["aggregate_driver_observation"]["devices"][0][
        "cleanup_failures"
    ][0]["api_error"] = 0
    assert list(validator.iter_errors(success_code))


def test_cleanup_failures_are_bounded_and_only_valid_for_cleanup_stage():
    validator = _validator()
    report = _report()
    failure = {
        "target": "module",
        "operation": "cuModuleUnload",
        "api_error": 711,
        "symbolic_info": "CUDA_ERROR_UNKNOWN",
    }
    report["aggregate_driver_observation"]["devices"][0]["cleanup_failures"] = [
        failure
    ]
    assert list(validator.iter_errors(report))

    report["aggregate_driver_observation"]["devices"][0].update(
        {
            "stage": "cleanup_failed",
            "operation": "cuModuleUnload",
            "api_error": 711,
            "cleanup_failures": [failure] * 5,
        }
    )
    report["aggregate_driver_observation"]["status"] = "failed"
    report["aggregate_driver_observation"]["cuda_compute_ready"] = False
    report["aggregate_driver_observation"]["diagnostics"] = [
        _diagnostic("device_failure", "device_compute_failed")
    ]
    report["cuda_compute_ready"] = False
    assert list(validator.iter_errors(report))


def test_failed_rejects_a_successful_device_and_observed_rejects_failure():
    validator = _validator()
    failed = _report()
    failed["aggregate_driver_observation"] = {
        "status": "failed",
        "driver_version": 13000,
        "devices": [_device()],
        "diagnostics": [_diagnostic("device_failure", "device_compute_failed")],
        "cuda_compute_ready": False,
    }
    failed["cuda_compute_ready"] = False
    assert list(validator.iter_errors(failed))

    observed = _report()
    observed["aggregate_driver_observation"]["devices"][0] = _failed_device(
        "launch", "cuLaunchKernel", 719
    )
    observed["aggregate_driver_observation"]["cuda_compute_ready"] = False
    observed["cuda_compute_ready"] = False
    assert list(validator.iter_errors(observed))


@pytest.mark.parametrize(
    "value",
    (
        r"C:\Users\name",
        r"\Users\name",
        r"C:Users\name",
        "vendor/private",
        r"vendor\private",
        "/usr/lib/nvidia",
    ),
)
def test_device_metadata_rejects_path_like_values(value):
    validator = _validator()
    report = _report()
    report["aggregate_driver_observation"]["devices"][0]["name"] = value
    assert list(validator.iter_errors(report))


def test_jit_log_is_hash_only_and_schema_is_closed():
    validator = _validator()
    report = _report()
    device = _failed_device("jit", "cuModuleLoadDataEx", 218)
    device["jit_log"] = {
        "status": "redacted",
        "length_bytes": 42,
        "sha256": "b" * 64,
    }
    report["aggregate_driver_observation"] = {
        "status": "failed",
        "driver_version": 13000,
        "devices": [device],
        "diagnostics": [_diagnostic("device_failure", "device_compute_failed")],
        "cuda_compute_ready": False,
    }
    report["cuda_compute_ready"] = False
    validator.validate(report)

    raw = copy.deepcopy(report)
    raw["aggregate_driver_observation"]["devices"][0]["jit_log"][
        "raw_log"
    ] = r"C:\Users\name\ptx.log"
    assert list(validator.iter_errors(raw))

    extra = copy.deepcopy(report)
    extra["backend_name"] = "cuda"
    assert list(validator.iter_errors(extra))


def test_source_contract_is_system32_driver_only_and_backend_never_ready():
    source = WORKER_PATH.read_text(encoding="utf-8")
    assert 'LoadLibraryExW(name.as_ptr(), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32)' in source
    assert '"nvcuda.dll\\0"' in source
    assert "cudart" not in source.lower()
    assert "backend_ready: false" in (
        ROOT
        / "broker"
        / "crates"
        / "broker"
        / "src"
        / "cuda_compute_probe.rs"
    ).read_text(encoding="utf-8")

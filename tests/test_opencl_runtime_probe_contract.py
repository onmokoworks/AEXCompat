import copy
import json
from pathlib import Path

import pytest
from jsonschema import Draft202012Validator


ROOT = Path(__file__).resolve().parents[1]
SCHEMA_PATH = ROOT / "schemas" / "opencl-runtime-probe.schema.json"
WORKER_PATH = (
    ROOT
    / "broker"
    / "crates"
    / "broker"
    / "src"
    / "bin"
    / "opencl_runtime_probe_worker.rs"
)


def _validator():
    schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    return Draft202012Validator(schema)


def _device():
    return {
        "device_type": 4,
        "vendor_id": 0x10DE,
        "name": "Fixture device",
        "vendor": "Fixture vendor",
        "driver_version": "1.2.3",
        "version": "OpenCL 3.0",
        "profile": "FULL_PROFILE",
        "extensions": ["cl_khr_fp64"],
        "available": True,
        "compiler_available": True,
        "max_compute_units": 16,
        "max_clock_frequency_mhz": 1500,
        "max_work_group_size": 1024,
        "max_work_item_sizes": [1024, 1024, 64],
        "max_mem_alloc_bytes": 1 << 30,
        "global_mem_bytes": 8 << 30,
        "local_mem_bytes": 64 << 10,
        "compute": {
            "stage": "passed",
            "queue_api": "legacy",
            "api_error": None,
            "missing_symbols": [],
            "build_log": None,
        },
    }


def _aggregate(status="observed"):
    if status == "observed":
        return {
            "status": status,
            "platforms": [
                {
                    "name": "Fixture platform",
                    "vendor": "Fixture vendor",
                    "version": "OpenCL 3.0 Fixture",
                    "profile": "FULL_PROFILE",
                    "extensions": ["cl_khr_fp64", "cl_khr_icd"],
                    "icd_suffix": "FIX",
                    "devices": [_device()],
                }
            ],
            "diagnostics": [],
            "compute_ready": True,
        }
    return {
        "status": status,
        "platforms": [],
        "diagnostics": [
            {
                "kind": status,
                "operation": "clGetPlatformIDs",
                "api_error": None,
                "platform_index": None,
                "device_index": None,
            }
        ],
        "compute_ready": False,
    }


def _report(status="observed"):
    observed = status == "observed"
    return {
        "schema_version": 2,
        "contract": "system_opencl_loader_probe",
        "candidate_evidence": {
            "legacy_registry_contract": "opencl_icd_registry_candidates",
            "adapter_binding_contract": "opencl_icd_adapter_driver",
            "pnp_software_key_contract": "windows_pnp_opencl_runtime",
            "individual_icd_binding": "not_attempted",
        },
        "aggregate_loader_observation": _aggregate() if observed else None,
        "launch": {
            "status": status,
            "exit_code": 0 if status != "launch_error" else None,
            "kill_reason": None,
            "stdout_truncated": False,
            "stderr_truncated": False,
            "memory_limit_reached": False,
        },
        "compute_ready": observed,
        "backend_ready": False,
    }


def test_schema_accepts_observed_and_all_launch_failures():
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


@pytest.mark.parametrize("status", ("no_loader", "missing_symbol", "no_platform"))
def test_schema_accepts_fail_closed_aggregate_states(status):
    validator = _validator()
    report = _report()
    report["aggregate_loader_observation"] = _aggregate(status)
    report["compute_ready"] = False
    validator.validate(report)


@pytest.mark.parametrize(
    "stage",
    (
        "context",
        "queue",
        "buffer",
        "build",
        "kernel",
        "enqueue",
        "finish",
        "readback",
        "mismatch",
        "not_attempted",
    ),
)
def test_compute_failure_stages_force_aggregate_not_ready(stage):
    validator = _validator()
    report = _report()
    compute = report["aggregate_loader_observation"]["platforms"][0]["devices"][0][
        "compute"
    ]
    compute["stage"] = stage
    compute["queue_api"] = (
        "with_properties"
        if stage in ("queue", "buffer", "build", "kernel", "enqueue", "finish", "readback", "mismatch")
        else None
    )
    compute["api_error"] = -5 if stage not in ("mismatch", "not_attempted") else None
    compute["missing_symbols"] = ["clCreateContext"] if stage == "not_attempted" else []
    if stage == "build":
        compute["build_log"] = {"status": "redacted", "sha256": "a" * 64}
    report["aggregate_loader_observation"]["compute_ready"] = False
    report["compute_ready"] = False
    validator.validate(report)


def test_partial_compute_success_cannot_claim_ready():
    validator = _validator()
    report = _report()
    failed = copy.deepcopy(_device())
    failed["compute"] = {
        "stage": "readback",
        "queue_api": "legacy",
        "api_error": -5,
        "missing_symbols": [],
        "build_log": None,
    }
    report["aggregate_loader_observation"]["platforms"][0]["devices"].append(failed)
    report["aggregate_loader_observation"]["compute_ready"] = False
    report["compute_ready"] = False
    validator.validate(report)
    report["compute_ready"] = True
    assert list(validator.iter_errors(report))


@pytest.mark.parametrize(
    "build_log",
    (
        {"status": "redacted", "sha256": None},
        {"status": "overflow", "sha256": "a" * 64},
        {"status": "redacted", "sha256": r"C:\Users\name"},
        {"status": "redacted", "sha256": "/private/build.log"},
        {"status": "redacted", "sha256": "raw compiler output"},
    ),
)
def test_build_log_is_hash_only_and_privacy_bounded(build_log):
    validator = _validator()
    report = _report()
    compute = report["aggregate_loader_observation"]["platforms"][0]["devices"][0][
        "compute"
    ]
    compute["stage"] = "build"
    compute["api_error"] = -11
    compute["build_log"] = build_log
    report["aggregate_loader_observation"]["compute_ready"] = False
    report["compute_ready"] = False
    assert list(validator.iter_errors(report))


@pytest.mark.parametrize("status", ("overflow", "unavailable", "malformed"))
def test_build_log_nonraw_failure_evidence_is_accepted(status):
    validator = _validator()
    report = _report()
    compute = report["aggregate_loader_observation"]["platforms"][0]["devices"][0][
        "compute"
    ]
    compute.update(
        {
            "stage": "build",
            "queue_api": "with_properties",
            "api_error": -11,
            "build_log": {"status": status, "sha256": None},
        }
    )
    report["aggregate_loader_observation"]["compute_ready"] = False
    report["compute_ready"] = False
    validator.validate(report)


def test_top_level_and_aggregate_compute_readiness_must_match():
    validator = _validator()
    report = _report()
    report["compute_ready"] = False
    assert list(validator.iter_errors(report))


def test_compute_queue_api_state_invariants():
    validator = _validator()
    report = _report()
    report["aggregate_loader_observation"]["platforms"][0]["devices"][0]["compute"][
        "queue_api"
    ] = None
    assert list(validator.iter_errors(report))

    report = _report()
    compute = report["aggregate_loader_observation"]["platforms"][0]["devices"][0][
        "compute"
    ]
    compute.update(
        {
            "stage": "not_attempted",
            "queue_api": "legacy",
            "missing_symbols": ["clCreateContext"],
        }
    )
    report["aggregate_loader_observation"]["compute_ready"] = False
    report["compute_ready"] = False
    assert list(validator.iter_errors(report))


def test_candidate_evidence_cannot_claim_individual_binding_or_raw_identity():
    validator = _validator()
    report = _report()
    report["candidate_evidence"]["individual_icd_binding"] = "nvidia"
    assert list(validator.iter_errors(report))

    report = _report()
    report["candidate_evidence"]["candidate_path"] = r"C:\Windows\System32\OpenCL.dll"
    assert list(validator.iter_errors(report))


PATH_LIKE_METADATA = (
    r"\Users\name",
    r"C:Users\name",
    r"vendor\private",
    "vendor/private",
    r"C:\Private\platform",
    "C:/Private/platform",
    r"\\server\share",
    "/usr/lib/vendor",
    "C:drive-relative",
)


@pytest.mark.parametrize(
    "field",
    ("name", "vendor", "version", "profile", "extensions", "icd_suffix"),
)
@pytest.mark.parametrize("value", PATH_LIKE_METADATA)
def test_schema_rejects_path_like_platform_metadata(field, value):
    validator = _validator()
    report = _report()
    report["aggregate_loader_observation"]["platforms"][0][field] = (
        [value] if field == "extensions" else value
    )
    assert list(validator.iter_errors(report))


@pytest.mark.parametrize(
    "field",
    ("name", "vendor", "driver_version", "version", "profile", "extensions"),
)
@pytest.mark.parametrize("value", PATH_LIKE_METADATA)
def test_schema_rejects_path_like_device_metadata(field, value):
    validator = _validator()
    report = _report()
    report["aggregate_loader_observation"]["platforms"][0]["devices"][0][field] = (
        [value] if field == "extensions" else value
    )
    assert list(validator.iter_errors(report))


@pytest.mark.parametrize(
    "field,value",
    (
        ("icd_suffix", "bad suffix"),
        ("extensions", ["cl_ok", "bad extension"]),
    ),
)
def test_schema_rejects_other_malformed_platform_fields(field, value):
    validator = _validator()
    report = _report()
    report["aggregate_loader_observation"]["platforms"][0][field] = value
    assert list(validator.iter_errors(report))


@pytest.mark.parametrize(
    "field,value",
    (
        ("max_work_item_sizes", [1] * 9),
        ("max_compute_units", 0),
        ("available", 1),
        ("vendor_id", 0),
        ("extensions", ["bad extension"]),
    ),
)
def test_schema_rejects_malformed_or_unbounded_device_fields(field, value):
    validator = _validator()
    report = _report()
    report["aggregate_loader_observation"]["platforms"][0]["devices"][0][field] = value
    assert list(validator.iter_errors(report))


def test_observed_launch_requires_aggregate_and_failures_forbid_it():
    validator = _validator()
    observed = _report()
    observed["aggregate_loader_observation"] = None
    observed["compute_ready"] = False
    assert list(validator.iter_errors(observed))

    failed = _report("timeout")
    failed["aggregate_loader_observation"] = _aggregate()
    failed["compute_ready"] = True
    assert list(validator.iter_errors(failed))


def test_closed_schema_rejects_raw_paths_and_backend_readiness():
    validator = _validator()
    report = copy.deepcopy(_report())
    report["raw_candidate_identity"] = {"path": r"C:\Private\opencl.dll"}
    assert list(validator.iter_errors(report))

    report = _report()
    report["backend_ready"] = True
    assert list(validator.iter_errors(report))


def test_worker_loads_only_the_system_opencl_loader_surface():
    source = WORKER_PATH.read_text(encoding="utf-8")
    assert 'LoadLibraryExW(name.as_ptr(), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32)' in source
    assert '"OpenCL.dll\\0"' in source
    for symbol in (
        "clGetPlatformIDs",
        "clGetPlatformInfo",
        "clGetDeviceIDs",
        "clGetDeviceInfo",
    ):
        assert symbol in source
    assert "LoadLibraryW(" not in source
    assert "candidate.path" not in source


def test_worker_resolves_only_the_bounded_compute_surface():
    source = WORKER_PATH.read_text(encoding="utf-8")
    for symbol in (
        "clCreateContext",
        "clCreateCommandQueueWithProperties",
        "clCreateCommandQueue",
        "clCreateBuffer",
        "clEnqueueWriteBuffer",
        "clCreateProgramWithSource",
        "clBuildProgram",
        "clGetProgramBuildInfo",
        "clCreateKernel",
        "clSetKernelArg",
        "clEnqueueNDRangeKernel",
        "clFinish",
        "clEnqueueReadBuffer",
        "clReleaseKernel",
        "clReleaseProgram",
        "clReleaseMemObject",
        "clReleaseCommandQueue",
        "clReleaseContext",
    ):
        assert f'b"{symbol}\\0"' in source
    assert "COMPUTE_ELEMENT_COUNT" in source
    assert "input[i] * 3u + 7u" in source
    assert "backend_ready: true" not in source

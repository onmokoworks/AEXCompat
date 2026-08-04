import copy
import json
from pathlib import Path

import jsonschema
import pytest


ROOT = Path(__file__).resolve().parents[1]
SCHEMA = json.loads(
    (ROOT / "schemas/wgpu-dx12-pf-probe.schema.json").read_text(encoding="utf-8")
)
RUNTIME_CARGO = (
    ROOT / "instruments/pf-wgpu-dx12-probe/runtime/Cargo.toml"
).read_text(encoding="utf-8")
RUNTIME = (
    ROOT / "instruments/pf-wgpu-dx12-probe/runtime/src/lib.rs"
).read_text(encoding="utf-8")
AEX = (ROOT / "instruments/pf-wgpu-dx12-probe/pf_wgpu_dx12_probe.cpp").read_text(
    encoding="utf-8"
)
BROKER = (ROOT / "broker/crates/broker/src/wgpu_dx12_pf_probe.rs").read_text(
    encoding="utf-8"
)
BUILD = (ROOT / "tools/build-wgpu-dx12-pf-probe.ps1").read_text(encoding="utf-8")


def artifact(path: str) -> dict:
    return {"path": path, "size": 1, "sha256": "a" * 64}


def valid_report() -> dict:
    values = [value * 3 + 7 for value in range(64)]
    return {
        "schema_version": 1,
        "probe": "aexcompat.pf-wgpu-dx12-compute",
        "backend": "dx12",
        "wgpu_version": "0.19.4",
        "build_manifest_sha256": "b" * 64,
        "artifacts": {
            "aex": artifact(
                "target/pf-wgpu-dx12-probe-build/Release/pf_wgpu_dx12_probe.aex"
            ),
            "runtime": artifact(
                "target/pf-wgpu-dx12-probe-build/Release/"
                "aexcompat_wgpu_dx12_runtime.dll"
            ),
        },
        "sources": {
            "aex_cpp": artifact(
                "instruments/pf-wgpu-dx12-probe/pf_wgpu_dx12_probe.cpp"
            ),
            "runtime_cargo_toml": artifact(
                "instruments/pf-wgpu-dx12-probe/runtime/Cargo.toml"
            ),
            "runtime_rust": artifact(
                "instruments/pf-wgpu-dx12-probe/runtime/src/lib.rs"
            ),
            "cargo_lock": artifact(
                "instruments/pf-wgpu-dx12-probe/runtime/Cargo.lock"
            ),
        },
        "driver_identity": {
            "schema_version": 1,
            "backend": "directx",
            "adapter_luid": "0000000000000001",
            "pci_vendor_id": "10de",
            "pci_device_id": "2782",
            "pci_subsystem_id": "40cb1458",
            "pci_revision_id": "a1",
            "driver_inf": "oem3.inf",
            "driver_catalog_sha256": "c" * 64,
            "driver_version": "32.0.15.9579",
            "os_build": 22631,
        },
        "worker_exit": "ok",
        "worker_exit_code": 0,
        "process_memory_limit_bytes": 1073741824,
        "worker_peak_commit_bytes": 700000000,
        "memory_limit_reached": False,
        "stdout_truncated": False,
        "stderr_truncated": False,
        "lifecycle": {
            "status": "parameters_inspected",
            "global_setup_error": 0,
            "params_setup_error": 0,
            "global_setdown_error": 0,
        },
        "module_audit": {
            "status": "passed",
            "phase_count": 5,
            "unknown_count": 0,
            "plugin_modules": [
                "pf_wgpu_dx12_probe.aex",
                "aexcompat_wgpu_dx12_runtime.dll",
            ],
        },
        "global_setup": {
            "schema_version": 1,
            "stage": "compute_readback_complete",
            "backend": "dx12",
            "wgpu_version": "0.19.4",
            "wgsl_sha256": "d" * 64,
            "element_count": 64,
            "expected_values": values,
            "actual_values": values,
            "expected_sha256": "e" * 64,
            "actual_sha256": "e" * 64,
            "adapter": {
                "name": "fixture",
                "adapter_luid": "0000000000000001",
                "vendor_id": 0x10DE,
                "device_id": 0x2782,
                "device_type": "discretegpu",
                "driver": "",
                "driver_info": "",
                "backend": "dx12",
            },
            "wgpu_compute_ready": True,
            "error": None,
        },
        "global_setdown": {
            "schema_version": 1,
            "stage": "global_setdown",
            "cleanup_complete": True,
            "live_state_after_setdown": False,
            "error": None,
        },
        "wgpu_compute_ready": True,
        "backend_ready": False,
        "passed": True,
    }


def test_wgpu_dx12_report_schema_is_closed_and_fail_closed():
    report = valid_report()
    jsonschema.validate(report, SCHEMA)

    mutations = []
    wrong_backend = copy.deepcopy(report)
    wrong_backend["backend"] = "vulkan"
    mutations.append(wrong_backend)
    wrong_ready = copy.deepcopy(report)
    wrong_ready["backend_ready"] = True
    mutations.append(wrong_ready)
    wrong_count = copy.deepcopy(report)
    wrong_count["global_setup"]["actual_values"].pop()
    mutations.append(wrong_count)
    dirty_cleanup = copy.deepcopy(report)
    dirty_cleanup["global_setdown"]["cleanup_complete"] = False
    mutations.append(dirty_cleanup)
    unknown_module = copy.deepcopy(report)
    unknown_module["module_audit"]["unknown_count"] = 1
    mutations.append(unknown_module)
    extra = copy.deepcopy(report)
    extra["surprise"] = True
    mutations.append(extra)

    for mutation in mutations:
        with pytest.raises(jsonschema.ValidationError):
            jsonschema.validate(mutation, SCHEMA)







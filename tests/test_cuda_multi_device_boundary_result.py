import json
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]


def test_cuda_multi_device_evidence_preserves_runtime_and_failure_isolation():
    evidence = json.loads(
        (ROOT / "analysis" / "CUDA_MULTI_DEVICE_BOUNDARY_RESULT_2026-07-15.json").read_text(encoding="utf-8")
    )
    assert evidence["host"]["observed_device_count"] >= 1
    runtime = evidence["device_0_runtime"]
    assert runtime["gpu_setup_error"] == runtime["gpu_setdown_error"] == 0
    assert runtime["selector_error"] == runtime["render_error"] == 0
    assert runtime["allocations_created"] == runtime["allocations_freed"] == 3
    assert runtime["output_pixels_valid"] is True
    rejected = evidence["invalid_ordinal_runtime"]
    assert rejected["requested_device_index"] >= rejected["observed_device_count"]
    assert rejected["gpu_render_dispatched"] is False
    assert rejected["device_allocations_created"] == 0
    assert rejected["guard_bytes_intact"] is True
    assert rejected["worker_process_crashed"] is False



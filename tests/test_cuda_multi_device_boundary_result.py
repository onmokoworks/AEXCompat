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


def test_cuda_device_enumeration_is_bounded_and_worlds_keep_their_ordinal():
    source = (ROOT / "minihost" / "src" / "gpu_memory_world_transport.cpp").read_text(encoding="utf-8")
    backend = (ROOT / "minihost" / "src" / "gpu_cuda_backend.cpp").read_text(encoding="utf-8")
    for marker in (
        'load_function(state->module, state->device_get_count, "cuDeviceGetCount")',
        "static_assert(kMaxGpuDevices == 16)",
        "active_device_index >= last_discovered_device_count_",
        "state->primary_retain(&state->contexts[index]",
        "state->context_push(state->contexts[active_device_index])",
    ):
        assert marker in backend
    for marker in (
        "g_created_worlds.emplace(*world, index)",
        "gpu_free_device_memory(nullptr, index, pixels)",
        "owned == g_created_worlds.end() ? active_gpu_device_index() : owned->second",
    ):
        assert marker in source

    broker = source_owners.IMAGE_RENDER_SOURCE.read_text(encoding="utf-8")
    assert '"cuda_device_count"' in broker
    assert '"cuda_device_index"' in broker

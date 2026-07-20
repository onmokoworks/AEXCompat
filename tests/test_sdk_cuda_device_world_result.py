import json
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]


def test_cuda_device_world_matches_cpu_public_pixels_and_balances_ownership():
    evidence = json.loads(
        (ROOT / "analysis" / "SDK_CUDA_DEVICE_WORLD_RESULT_2026-07-15.json").read_text(encoding="utf-8")
    )
    assert evidence["fixture"]["source_modified"] is False
    assert evidence["one_pixel_oracle"]["output_rgba8"] == [245, 235, 225, 255]
    image = evidence["image_oracle"]
    assert image["gpu_selector_error"] == image["gpu_render_error"] == 0
    assert image["gpu_cpu_different_rgba8_bytes"] == 0
    assert image["gpu_cpu_max_rgba8_error"] == 0
    ownership = evidence["ownership"]
    assert ownership["device_allocations_created"] == ownership["device_allocations_freed"] == 3
    assert ownership["live_device_allocations"] == ownership["live_device_bytes"] == 0
    assert ownership["cuda_sync_failures"] == 0
    assert ownership["gpu_memory_lifetimes_balanced"] is True
    assert ownership["guard_bytes_intact"] is True
    assert evidence["broker"]["gpu_fallback_used"] is False


def test_cuda_driver_boundary_is_dynamic_bounded_and_channel_explicit():
    source = (ROOT / "minihost" / "src" / "gpu_memory_world_transport.cpp").read_text(encoding="utf-8")
    worker = source_owners.worker_text()
    backend = (ROOT / "minihost" / "src" / "gpu_cuda_backend.cpp").read_text(encoding="utf-8")
    for marker in (
        'LoadLibraryExW(L"nvcuda.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32)',
        'load_function(state->module, state->mem_alloc, "cuMemAlloc_v2")',
        'load_function(state->module, state->copy_host_to_device, "cuMemcpyHtoD_v2")',
        'load_function(state->module, state->copy_device_to_host, "cuMemcpyDtoH_v2")',
        'load_function(state->module, state->host_alloc, "cuMemHostAlloc")',
        'load_function(state->module, state->host_free, "cuMemFreeHost")',
    ):
        assert marker in backend
    for marker in (
        "kMaxGpuAllocationBytes = 256u * 1024u * 1024u",
        "destination[x * 4] = source[x * 4 + 3]",
        "destination[x * 4 + 3] = source[x * 4]",
        "end_cuda_context",
    ):
        assert marker in source
    assert "finish_cuda_render_transport" in worker

    build = (ROOT / "tools" / "build-sdk-invert-cuda.ps1").read_text(encoding="utf-8")
    assert "SDK_Invert_ProcAmp_Kernel.cu" in build
    assert "/DHAS_CUDA=1" in build
    assert "target\\sdk-fixtures" in build

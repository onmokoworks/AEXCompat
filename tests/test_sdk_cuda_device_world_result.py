import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_cuda_device_world_matches_cpu_public_pixels_and_balances_ownership():
    evidence = json.loads(
        (ROOT / "analysis" / "SDK_CUDA_DEVICE_WORLD_RESULT_2026-07-15.json").read_text()
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
    source = (ROOT / "minihost" / "src" / "l2_main.cpp").read_text()
    for marker in (
        'LoadLibraryExW(L"nvcuda.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32)',
        'load_cuda_function(g_cuda.mem_alloc, "cuMemAlloc_v2")',
        'load_cuda_function(g_cuda.copy_host_to_device, "cuMemcpyHtoD_v2")',
        'load_cuda_function(g_cuda.copy_device_to_host, "cuMemcpyDtoH_v2")',
        'load_cuda_function(g_cuda.host_alloc, "cuMemHostAlloc")',
        'load_cuda_function(g_cuda.host_free, "cuMemFreeHost")',
        "kMaxGpuAllocationBytes = 256u * 1024u * 1024u",
        "destination[x * 4] = source[x * 4 + 3]",
        "destination[x * 4 + 3] = source[x * 4]",
        "finish_cuda_render_transport",
        "end_cuda_context",
    ):
        assert marker in source

    build = (ROOT / "tools" / "build-sdk-invert-cuda.ps1").read_text()
    assert "SDK_Invert_ProcAmp_Kernel.cu" in build
    assert "/DHAS_CUDA=1" in build
    assert "target\\sdk-fixtures" in build

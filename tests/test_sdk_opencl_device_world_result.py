import json
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]


def test_opencl_device_world_evidence_records_exact_public_rgba8_conformance():
    evidence = json.loads(
        (ROOT / "analysis" / "SDK_OPENCL_DEVICE_WORLD_RESULT_2026-07-15.json").read_text(encoding="utf-8")
    )
    assert evidence["fixture"]["source_modified"] is False
    assert evidence["host"]["device_count"] == 1

    smoke = evidence["device_world_smoke"]
    assert smoke["dimensions"] == [16, 12]
    assert smoke["bgra128_bytes"] == smoke["opencl_upload_bytes"] == 3072
    assert smoke["opencl_download_bytes"] == 3072
    assert smoke["gpu_selector_error"] == smoke["gpu_device_setup_error"] == 0
    assert smoke["gpu_device_setdown_error"] == 0
    assert smoke["output_pixels_valid"] is True
    assert smoke["float_world_hashes_identical"] is False
    assert smoke["gpu_float_world_sha256"] == (
        "d82c5991c0da941942618cdc73bea901a765dafad172d7e9e9569764482075d4"
    )
    assert smoke["cpu_float_world_sha256"] == (
        "695e591da5d3ca88395b7981ffe4833883251f3dfe3a285ccfa5dfb1cae48933"
    )

    image = evidence["external_image_oracle"]
    assert image["commands"] == ["--smart-image32-opencl", "--smart-image32-cpu"]
    assert image["same_fixture_and_parameters"] is True
    assert image["dimensions"] == [37, 23]
    assert image["public_rgba8_bytes"] == 37 * 23 * 4 == 3404
    assert image["opencl_bgra128_upload_bytes"] == 37 * 23 * 16 == 13616
    assert image["opencl_bgra128_download_bytes"] == 13616
    assert image["opencl_selector_error"] == image["opencl_device_setup_error"] == 0
    assert image["opencl_render_error"] == image["opencl_device_setdown_error"] == 0
    assert image["opencl_output_pixels_valid"] is True
    assert image["opencl_rgba8_sha256"] == image["cpu_rgba8_sha256"] == (
        "6f24052bf442cc05899fdfe3779514c610652c6ab1d8dcba083dbf36f9ad0617"
    )
    assert image["gpu_cpu_different_rgba8_bytes"] == 0
    assert image["gpu_cpu_max_rgba8_error"] == 0
    assert image["rgba8_conformance"] == "pass"

    ownership = evidence["ownership"]
    assert ownership["device_allocations_created"] == 3
    assert ownership["device_allocations_freed"] == 3
    assert ownership["live_device_allocations"] == ownership["live_device_bytes"] == 0
    assert ownership["opencl_sync_failures"] == 0
    assert ownership["gpu_memory_lifetimes_balanced"] is True


def test_opencl_boundary_and_fixture_build_markers_are_present():
    source = source_owners.worker_text()
    source += "\n" + (ROOT / "minihost" / "src" / "l2_cli_dispatch.cpp").read_text(encoding="utf-8")
    transport = (ROOT / "minihost" / "src" / "gpu_memory_world_transport.cpp").read_text(encoding="utf-8")
    backend = (ROOT / "minihost" / "src" / "gpu_opencl_backend.cpp").read_text(encoding="utf-8")
    for marker in (
        'LoadLibraryExW(L"OpenCL.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32)',
        'load_function(g_api.create_buffer, "clCreateBuffer")',
        'load_function(g_api.enqueue_write, "clEnqueueWriteBuffer")',
        'load_function(g_api.enqueue_read, "clEnqueueReadBuffer")',
        'load_function(g_api.finish, "clFinish")',
        "kDeviceTypeGpu",
        "begin_context",
        "end_context",
        "kMaxGpuDevices",
        "device_info_registry()",
    ):
        assert marker in backend

    # #365: the one-shot --smart-image32-opencl was replaced by the session
    # command word, which selects the same backend.
    for marker in (
        'equals(command, L"--smart-session32-opencl-v1")',
    ):
        assert marker in source
    assert "opencl_upload_bytes += input_size" in transport
    assert "opencl_download_bytes += output_size" in transport

    build = (ROOT / "tools" / "build-sdk-invert-opencl.ps1").read_text(encoding="utf-8")
    for marker in (
        "SDK_Invert_ProcAmp_Kernel.cl",
        "/DGF_DEVICE_TARGET_OPENCL=1",
        "/DHAS_CUDA=0",
        "OpenCL.lib",
        "target\\sdk-fixtures",
    ):
        assert marker in build

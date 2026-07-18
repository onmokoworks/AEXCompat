import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
FIXTURE_SHA256 = "6cbbb9a25539c7958e7eb5dce458eb2ef62b9efeecb7108e1c04973270b13428"
OUTPUT_SHA256 = "6f24052bf442cc05899fdfe3779514c610652c6ab1d8dcba083dbf36f9ad0617"


def load_evidence():
    return json.loads(
        (ROOT / "analysis" / "SDK_DIRECTX_DEVICE_WORLD_RESULT_2026-07-16.json").read_text()
    )


def test_directx_device_world_evidence_records_exact_public_rgba8_conformance():
    evidence = load_evidence()
    fixture = evidence["fixture"]
    assert fixture["source_modified"] is False
    assert fixture["sha256"] == FIXTURE_SHA256
    assert evidence["host"]["hardware_adapter_count"] == 2
    assert evidence["host"]["device_index"] == 0

    smoke = evidence["device_world_smoke"]
    assert smoke["dimensions"] == [16, 12]
    for field in (
        "gpu_device_setup_error",
        "gpu_smart_pre_render_error",
        "gpu_smart_render_error",
        "gpu_device_setdown_error",
    ):
        assert smoke[field] == 0
    assert smoke["render_completed"] is True
    assert smoke["output_pixels_valid"] is True

    image = evidence["external_image_oracle"]
    assert image["commands"] == ["--smart-image32-directx", "--smart-image32-cpu"]
    assert image["dimensions"] == [37, 23]
    assert image["directx_exit_code"] == image["cpu_exit_code"] == 0
    assert image["directx_rgba8_bytes"] == image["cpu_rgba8_bytes"] == 37 * 23 * 4
    for field in (
        "directx_device_setup_error",
        "directx_smart_pre_render_error",
        "directx_smart_render_error",
        "directx_device_setdown_error",
    ):
        assert image[field] == 0
    assert image["directx_rgba8_sha256"] == image["cpu_rgba8_sha256"] == OUTPUT_SHA256
    assert image["gpu_cpu_different_rgba8_bytes"] == 0
    assert image["gpu_cpu_max_rgba8_error"] == 0
    assert image["rgba8_conformance"] == "pass"

    ownership = evidence["ownership"]
    assert ownership["device_allocations_created"] == ownership["device_allocations_freed"] == 3
    assert ownership["live_device_allocations"] == ownership["live_device_bytes"] == 0
    assert ownership["directx_sync_failures"] == 0
    assert ownership["gpu_memory_lifetimes_balanced"] is True
    assert evidence["isolation"]["seh_exceptions"] == 0


def test_directx_implementation_build_and_readme_markers_are_present():
    main = (ROOT / "minihost" / "src" / "l2_main.cpp").read_text()
    main += "\n" + (ROOT / "minihost" / "src" / "l2_cli_dispatch.cpp").read_text()
    source = (ROOT / "minihost" / "src" / "gpu_directx_backend.cpp").read_text()
    for marker in (
        'LoadLibraryExW(L"dxgi.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32)',
        'LoadLibraryExW(L"d3d12.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32)',
        'GetProcAddress(g_directx.dxgi_module, "CreateDXGIFactory1")',
        'GetProcAddress(g_directx.d3d12_module, "D3D12CreateDevice")',
        "(description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) == 0",
        "D3D12_COMMAND_LIST_TYPE_COMPUTE",
    ):
        assert marker in source

    for marker in (
        "directx_backend::begin_context",
        "directx_backend::end_context",
        'equals(command, L"--smart-image32-directx")',
        '\\"directx_device_count\\":',
        '\\"directx_device_index\\":',
        '\\"gpu_allocations_created\\":',
        '\\"live_gpu_allocation_count\\":',
    ):
        assert marker in main

    build = (ROOT / "tools" / "build-sdk-invert-directx.ps1").read_text()
    for marker in (
        "SDK_Invert_ProcAmp_Kernel.chlsl",
        "/DGF_DEVICE_TARGET_HLSL=1",
        "-T cs_6_5",
        "/DHAS_HLSL=1",
        "DirectX_Assets",
        "Get-FileHash",
    ):
        assert marker in build

    readme = (ROOT / "README.md").read_text(encoding="utf-8")
    assert "### DirectX SDK fixture" in readme
    assert "SDK_DIRECTX_DEVICE_WORLD_RESULT_2026-07-16.json" in readme

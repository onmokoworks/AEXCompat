from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
HEADER = (ROOT / "minihost" / "src" / "gpu_directx_backend.hpp").read_text()
SOURCE = (ROOT / "minihost" / "src" / "gpu_directx_backend.cpp").read_text()
MAIN = source_owners.L2_MAIN.read_text()
SMART_DISPATCH = (ROOT / "minihost" / "src" / "worker_smart_dispatch.cpp").read_text()
CMAKE = (ROOT / "minihost" / "CMakeLists.txt").read_text()
TRANSPORT = (ROOT / "minihost" / "src" / "gpu_memory_world_transport.cpp").read_text()


def test_directx_backend_owns_secure_dynamic_loader_and_lifecycle():
    assert 'LoadLibraryExW(L"dxgi.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32)' in SOURCE
    assert 'LoadLibraryExW(L"d3d12.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32)' in SOURCE
    assert 'GetProcAddress(g_directx.dxgi_module, "CreateDXGIFactory1")' in SOURCE
    assert 'GetProcAddress(g_directx.d3d12_module, "D3D12CreateDevice")' in SOURCE
    assert SOURCE.index("g_directx.queues[index]->Release()") < SOURCE.index(
        "g_directx.devices[index]->Release()"
    ) < SOURCE.index("g_directx.adapters[index]->Release()")
    assert SOURCE.index("g_directx.factory->Release()") < SOURCE.index(
        "FreeLibrary(g_directx.d3d12_module)"
    ) < SOURCE.index("FreeLibrary(g_directx.dxgi_module)")
    assert "device_info_registry().reset_devices()" in SOURCE


def test_directx_backend_preserves_adapter_ordinals_cleanup_and_registry_wiring():
    assert "ordinal < kMaxGpuDevices" in SOURCE
    assert "EnumAdapters1(ordinal, &adapter)" in SOURCE
    assert "(description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) == 0" in SOURCE
    assert SOURCE.count("adapter->Release();") >= 2
    assert "device->Release();" in SOURCE
    assert "const uint32_t index = g_directx.device_count++;" in SOURCE
    assert "active_device_index >= g_directx.device_count" in SOURCE
    assert "device_info_registry().set_framework(4)" in SOURCE
    assert "index, nullptr, g_directx.devices[index]" in SOURCE
    assert "nullptr, g_directx.queues[index]" in SOURCE


def test_directx_copy_and_shared_transport_have_explicit_boundaries():
    assert "bool copy_buffer(ID3D12Resource*" in HEADER
    assert "WaitForSingleObject(event, 30'000) == WAIT_OBJECT_0" in SOURCE
    assert "if (event) CloseHandle(event);" in SOURCE
    orchestration = MAIN + SMART_DISPATCH
    for marker in (
        "CudaRenderTransport",
        "prepare_cuda_render_transport",
        "finish_cuda_render_transport",
        "begin_backend_context",
    ):
        assert marker in orchestration
    for marker in ("g_device_memory", "directx::copy_buffer",
                   "prepare_render_transport", "finish_render_transport"):
        assert marker in TRANSPORT
    assert "struct DirectXApi" not in MAIN
    assert 'LoadLibraryExW(L"dxgi.dll"' not in MAIN
    assert "src/gpu_directx_backend.cpp" in CMAKE

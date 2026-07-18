from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
HEADER = ROOT / "minihost" / "src" / "gpu_opencl_backend.hpp"
SOURCE = ROOT / "minihost" / "src" / "gpu_opencl_backend.cpp"
MAIN = ROOT / "minihost" / "src" / "l2_main.cpp"
CMAKE = ROOT / "minihost" / "CMakeLists.txt"


def test_opencl_loader_and_context_ownership_are_isolated_and_bounded():
    header = HEADER.read_text(encoding="utf-8")
    source = SOURCE.read_text(encoding="utf-8")
    main = MAIN.read_text(encoding="utf-8")

    assert 'LoadLibraryExW(L"OpenCL.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32)' in source
    assert "std::array<Platform, kMaxGpuDevices>" in source
    assert "std::array<Device, kMaxGpuDevices>" in source
    assert "std::array<Context, kMaxGpuDevices>" in source
    assert "std::array<Queue, kMaxGpuDevices>" in source
    assert "count > kMaxGpuDevices - g_api.device_count" in source
    assert "cleanup <= index" in source
    assert "release_queue(g_api.queues[cleanup])" in source
    assert "release_context(g_api.contexts[cleanup])" in source
    assert source.index("release_queue(g_api.queues[index])") < source.index(
        "release_context(g_api.contexts[index])"
    )
    assert "device_info_registry().reset_devices()" in source
    assert "bool begin_context(uint32_t active_device_index)" in header
    assert "struct OpenClApi" not in main
    assert "LoadLibraryExW(L\"OpenCL.dll\"" not in main


def test_opencl_transport_and_orchestration_stay_in_l2_main():
    source = SOURCE.read_text(encoding="utf-8")
    main = MAIN.read_text(encoding="utf-8")
    cmake = CMAKE.read_text(encoding="utf-8")

    for marker in (
        "g_gpu_device_memory",
        "CudaRenderTransport",
        "smart_opencl",
        "opencl::begin_context(gpu_device_index)",
        "g_opencl_upload_bytes += input_size",
        "g_opencl_download_bytes += output_size",
    ):
        assert marker in main
    assert "CudaRenderTransport" not in source
    assert "g_gpu_device_memory" not in source
    assert "src/gpu_opencl_backend.cpp" in cmake

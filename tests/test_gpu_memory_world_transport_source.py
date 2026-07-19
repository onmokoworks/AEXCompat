from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
HEADER = ROOT / "minihost" / "src" / "gpu_memory_world_transport.hpp"
SOURCE = ROOT / "minihost" / "src" / "gpu_memory_world_transport.cpp"
MAIN = source_owners.L2_MAIN
CMAKE = ROOT / "minihost" / "CMakeLists.txt"


def test_gpu_memory_and_world_ownership_are_isolated_and_bounded():
    header = HEADER.read_text(encoding="utf-8")
    source = SOURCE.read_text(encoding="utf-8")
    main = MAIN.read_text(encoding="utf-8")

    assert "kMaxGpuAllocations = 256" in source
    assert "kMaxGpuAllocationBytes = 256u * 1024u * 1024u" in source
    assert "std::unordered_map<void*, std::size_t> g_device_memory" in source
    assert "std::unordered_map<void*, std::size_t> g_host_memory" in source
    assert "std::unordered_map<void*, uint32_t> g_created_worlds" in source
    assert "index != active_gpu_device_index()" in source
    assert "width > kMaxWorldDimension" in source
    assert "rowbytes > kMaxWorldDimension * 16" in source
    assert "configure_host_world_fallback" in header
    assert "g_gpu_device_memory" not in main
    assert "g_gpu_created_worlds" not in main


def test_shared_transport_owns_all_backend_copy_and_suite_callbacks():
    source = SOURCE.read_text(encoding="utf-8")
    main = MAIN.read_text(encoding="utf-8")
    cmake = CMAKE.read_text(encoding="utf-8")

    for marker in (
        "cuda_backend().copy_to_device",
        "opencl::enqueue_write",
        "directx::copy_buffer",
        "prepare_render_transport",
        "finish_render_transport",
        "begin_backend_context",
        "end_backend_context",
        "std::array<void*, 15> gpu_device_suite1",
    ):
        assert marker in source
    assert "copy_to_device" not in main
    assert "opencl::enqueue_write" not in main
    assert "directx_backend::copy_buffer" not in main
    assert "src/gpu_memory_world_transport.cpp" in cmake

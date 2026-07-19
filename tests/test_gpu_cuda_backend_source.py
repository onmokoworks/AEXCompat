from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
HEADER = ROOT / "minihost" / "src" / "gpu_cuda_backend.hpp"
SOURCE = ROOT / "minihost" / "src" / "gpu_cuda_backend.cpp"
MAIN = ROOT / "minihost" / "src" / "l2_main.cpp"
SMART_DISPATCH = ROOT / "minihost" / "src" / "worker_smart_dispatch.cpp"
TRANSPORT = ROOT / "minihost" / "src" / "gpu_memory_world_transport.cpp"
CMAKE = ROOT / "minihost" / "CMakeLists.txt"


def test_cuda_loader_and_context_ownership_are_isolated_and_fail_closed():
    header = HEADER.read_text(encoding="utf-8")
    source = SOURCE.read_text(encoding="utf-8")
    main = MAIN.read_text(encoding="utf-8")

    assert "src/gpu_cuda_backend.cpp" in CMAKE.read_text(encoding="utf-8")
    assert "class CudaBackend" in header
    assert 'LoadLibraryExW(L"nvcuda.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32)' in source
    assert "static_assert(kMaxGpuDevices == 16)" in source
    assert "discovered_count > static_cast<int32_t>(kMaxGpuDevices)" in source
    assert "active_device_index >= last_discovered_device_count_" in source
    assert "LoadLibraryExW(L\"nvcuda.dll\"" not in main

    retain = source.index("state->primary_retain")
    push = source.index("state->context_push(state->contexts[active_device_index])")
    publish = source.index("registry.set_device_count")
    assert retain < push < publish
    assert source.count("end();\n    return false;") >= 4


def test_cuda_pop_release_and_registry_cleanup_preserve_ownership_order():
    source = SOURCE.read_text(encoding="utf-8")
    end = source[source.index("bool CudaBackend::end()") :]
    end = end[: end.index("bool CudaBackend::active()")]

    pop = end.index("state->context_pop")
    release = end.index("state->primary_release")
    unload = end.index("FreeLibrary(state->module)")
    clear = end.index("state_ = nullptr")
    registry = end.index("device_info_registry().reset_devices()")
    assert pop < release < unload < clear < registry
    assert "popped == state->contexts[state->active_device_index]" in end
    assert "--state->retained_count" in end


def test_smart_gpu_stage_and_module_audit_semantics_remain_in_orchestration():
    orchestration = SMART_DISPATCH.read_text(encoding="utf-8")
    setup_audit = orchestration.index("if (plan.gpu_negotiation) hooks.capture_module_audit();")
    setup_stage = orchestration.index('"stage:gpu_device_setup_begin')
    render_stage = orchestration.index('"stage:"\n            << (result.gpu_render_dispatched')
    setdown_audit = orchestration.index("hooks.capture_module_audit();", render_stage)
    setdown_stage = orchestration.index('"stage:gpu_device_setdown_begin', setdown_audit)
    cleanup_audit = orchestration.index(
        "if (plan.gpu_negotiation) hooks.capture_module_audit();", setdown_stage
    )
    cleanup = orchestration.index("end_backend_context(gpu_framework)", cleanup_audit)
    assert setup_audit < setup_stage < render_stage < setdown_audit < setdown_stage
    assert setdown_stage < cleanup_audit < cleanup

    transport = TRANSPORT.read_text(encoding="utf-8")
    assert "g_device_memory" in transport
    main = MAIN.read_text(encoding="utf-8")
    assert "CudaRenderTransport" in main
    assert "prepare_cuda_render_transport" in main
    assert "prepare_render_transport" in transport

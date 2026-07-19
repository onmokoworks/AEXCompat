from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MAIN = (ROOT / "minihost/src/l2_main.cpp").read_text(encoding="utf-8")
HEADER = (ROOT / "minihost/src/worker_handle_runtime.hpp").read_text(encoding="utf-8")
SOURCE = (ROOT / "minihost/src/worker_handle_runtime.cpp").read_text(encoding="utf-8")
CMAKE = (ROOT / "minihost/CMakeLists.txt").read_text(encoding="utf-8")


def test_pf_handle_ownership_is_bounded_and_hidden_behind_snapshots():
    for marker in (
        "kMaxHandleBytes = 256ULL * 1024ULL * 1024ULL",
        "kMaxHandleCount = 1024",
        "Statistics statistics()",
        "bool host_handle_is_live(const void* handle)",
        "void record_automatic_pre_render_disposal()",
    ):
        assert marker in HEADER
    for marker in (
        "std::unordered_set<HandleRecord*> g_handles",
        "record->lock_count != 0",
        "g_statistics.live_bytes > kMaxHandleBytes - size",
        "g_statistics.locks == g_statistics.unlocks",
    ):
        assert marker in SOURCE
    assert "g_handle_mutex" not in MAIN
    assert "g_handles.count" not in MAIN


def test_aegp_memory_handle_family_keeps_suite_abi_and_fail_closed_limits():
    assert "static_assert(sizeof(AegpMemorySuite) == 8 * sizeof(void*))" in HEADER
    for marker in (
        "kMaxAegpMemoryHandles = 256",
        "kMaxAegpMemoryBytes = 16ULL * 1024ULL * 1024ULL",
        "AegpMemoryStatistics aegp_memory_statistics()",
    ):
        assert marker in HEADER
    for marker in (
        "g_aegp_memory.size() >= kMaxAegpMemoryHandles",
        "found->second->lock_count != 0",
        "++g_aegp_statistics.invalid_operations",
        "g_aegp_memory.empty() && g_aegp_statistics.live_bytes == 0",
    ):
        assert marker in SOURCE
    assert "g_aegp_memory_mutex" not in MAIN
    assert "worker_handle_runtime.cpp" in CMAKE


def test_handle_runtime_does_not_absorb_world_or_gpu_state():
    lowered = (HEADER + SOURCE).lower()
    for forbidden in ("d3d", "cuda", "opencl", "gpu_world", "aegp_world"):
        assert forbidden not in lowered

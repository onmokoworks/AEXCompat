import json
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
MAIN = source_owners.L2_MAIN.read_text(encoding="utf-8")
HEADER = (ROOT / "minihost/src/worker_handle_runtime.hpp").read_text(encoding="utf-8")
SOURCE = (ROOT / "minihost/src/worker_handle_runtime.cpp").read_text(encoding="utf-8")
CMAKE = (ROOT / "minihost/CMakeLists.txt").read_text(encoding="utf-8")
SELFTEST = (ROOT / "tests/native/worker_handle_runtime_selftest.cpp").read_text(
    encoding="utf-8"
)


def test_pf_handle_ownership_is_bounded_and_hidden_behind_snapshots():
    for marker in (
        "kMaxHandleBytes = 2ULL * 1024ULL * 1024ULL * 1024ULL",
        "kObservedLargeHandleBytes = 333294848ULL",
        "kMaxHandleCount = 16384",
        "Statistics statistics()",
        "bool host_handle_is_live(const void* handle)",
        "void record_automatic_pre_render_disposal()",
        "locks_released_on_dispose",
    ):
        assert marker in HEADER
    for marker in (
        "std::unordered_set<HandleRecord*> g_handles",
        "g_statistics.locks_released_on_dispose += record->lock_count",
        "g_statistics.live_bytes > kMaxHandleBytes - size",
        "g_statistics.unlocks + g_statistics.locks_released_on_dispose",
        "callback:new_handle_failed reason=budget",
        "callback:resize_handle_failed reason=data",
    ):
        assert marker in SOURCE
    assert "g_handle_mutex" not in MAIN
    assert "g_handles.count" not in MAIN
    for marker in (
        "worker_handle_runtime_selftest",
        "tests/native/worker_handle_runtime_selftest.cpp",
    ):
        assert marker in CMAKE
    for marker in (
        "new_handle(kObservedLargeHandleBytes)",
        "large_data[kObservedLargeHandleBytes - 1]",
        "regression_handles.reserve(1025)",
        "resized != stable_handle",
        "replacement[31] != 0x78",
        "handle_lifetimes_balanced()",
        "dispose_handle(locked_dispose)",
        "dispose_handle(&foreign_data)",
    ):
        assert marker in SELFTEST
    for schema_name in (
        "parameterized_smartfx_render_report.schema.json",
        "smartfx_suite_fault_report.schema.json",
    ):
        schema = json.loads((ROOT / "contracts" / "aex" / schema_name).read_text())
        properties = schema["$defs"]["run"]["properties"]
        for field in (
            "handles_created",
            "handles_disposed",
            "handle_locks",
            "handle_unlocks",
        ):
            assert properties[field]["maximum"] == 4294967295
        assert properties["live_handle_count"]["maximum"] == 16384


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

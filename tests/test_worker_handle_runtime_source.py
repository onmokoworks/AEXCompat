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





import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SCATTERMAP_THREADED_RENDER_RESULT_2026-07-13.md"
WORKER = ROOT / "minihost" / "src" / "l2_main.cpp"
CLASSIC_RUNTIME = ROOT / "minihost" / "src" / "worker_classic_runtime.cpp"
BROKER = ROOT / "broker" / "crates" / "broker" / "src" / "render.rs"


class ThreadedRenderResultTests(unittest.TestCase):
    def test_result_records_same_module_concurrency_and_safety(self):
        text = RESULT.read_text(encoding="utf-8")
        for expected in (
            "SUPPORTS_THREADED_RENDERING",
            "loads and initializes the AEX once",
            "two native threads simultaneously",
            "All four renders returned error 0",
            "threaded_render_valid: true",
            "fixed SHA-256 allowlist",
        ):
            self.assertIn(expected, text)

    def test_worker_and_broker_enforce_both_threads(self):
        worker = WORKER.read_text(encoding="utf-8")
        runtime = CLASSIC_RUNTIME.read_text(encoding="utf-8")
        broker = BROKER.read_text(encoding="utf-8")
        self.assertIn('case_id == "threaded_default"', worker)
        self.assertIn("std::thread first", worker)
        self.assertIn("thread_local Context* g_active_context", runtime)
        self.assertIn("g_dispatch_count.fetch_add(1, std::memory_order_acq_rel)", runtime)
        for expected in (
            'case_id != "threaded_default"',
            'r.get("thread_1_error")',
            'r.get("thread_2_error")',
            "thread_hash_matches",
            'r.get("thread_1_guards_intact")',
            'r.get("thread_2_guards_intact")',
        ):
            self.assertIn(expected, broker)


if __name__ == "__main__":
    unittest.main()

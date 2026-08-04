import unittest
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SCATTERMAP_THREADED_RENDER_RESULT_2026-07-13.md"
WORKER = source_owners.L2_MAIN
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



if __name__ == "__main__":
    unittest.main()

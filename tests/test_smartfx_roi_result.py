import unittest
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SCATTERMAP_SMARTFX_ROI_RESULT_2026-07-13.md"
WORKER = source_owners.L2_MAIN
SMART_RUNTIME = ROOT / "minihost" / "src" / "worker_smart_runtime.cpp"
SMART_SETUP = ROOT / "minihost" / "src" / "worker_smart_setup.cpp"
SMART_DISPATCH = ROOT / "minihost" / "src" / "worker_smart_dispatch.cpp"
BROKER = ROOT / "broker" / "crates" / "broker" / "src" / "smart.rs"


class SmartFxRoiResultTests(unittest.TestCase):
    def test_result_records_fixed_partial_request_and_safety_boundary(self):
        result = RESULT.read_text(encoding="utf-8")
        for expected in (
            "left=3, top=2",
            "right=11, bottom=8",
            "source-layer checkout",
            "optional map",
            "checkout (parameter 6",
            "roi_contract_valid: true",
            "create-new result policy",
            "does not yet claim",
        ):
            self.assertIn(expected, result)



if __name__ == "__main__":
    unittest.main()

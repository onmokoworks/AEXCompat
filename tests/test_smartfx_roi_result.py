import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SCATTERMAP_SMARTFX_ROI_RESULT_2026-07-13.md"
WORKER = ROOT / "minihost" / "src" / "l2_main.cpp"
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

    def test_worker_and_broker_enforce_roi_observation(self):
        worker = WORKER.read_text(encoding="utf-8")
        broker = BROKER.read_text(encoding="utf-8")
        self.assertIn('case_id == "partial_output_request"', worker)
        self.assertIn("g_input_checkout_request == expected_request", worker)
        self.assertIn("g_map_checkout_request == expected_request", worker)
        self.assertIn('case_id != "partial_output_request"', broker)
        self.assertIn('json!([3, 2, 11, 8])', broker)


if __name__ == "__main__":
    unittest.main()

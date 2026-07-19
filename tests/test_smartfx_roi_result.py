import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SCATTERMAP_SMARTFX_ROI_RESULT_2026-07-13.md"
WORKER = ROOT / "minihost" / "src" / "l2_main.cpp"
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

    def test_worker_and_broker_enforce_roi_observation(self):
        worker = WORKER.read_text(encoding="utf-8")
        runtime = SMART_RUNTIME.read_text(encoding="utf-8")
        setup = SMART_SETUP.read_text(encoding="utf-8")
        dispatch = SMART_DISPATCH.read_text(encoding="utf-8")
        broker = BROKER.read_text(encoding="utf-8")
        self.assertIn('case_id == "partial_output_request"', setup)
        self.assertIn("runtime.input_checkout_request == expected_request", dispatch)
        self.assertIn("runtime.map_checkout_request == expected_request", dispatch)
        self.assertIn("snapshot_->input_checkout_request = state_.input_checkout_request", runtime)
        self.assertIn('case_id != "partial_output_request"', broker)
        self.assertIn('json!([3, 2, 11, 8])', broker)


if __name__ == "__main__":
    unittest.main()

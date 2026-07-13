import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
STATUS = ROOT / "docs" / "SAFETY_GATE_STATUS_2026-07-13.md"


class SafetyGateStatusTests(unittest.TestCase):
    def test_status_records_ae_reference_parity_and_prior_contracts(self):
        text = STATUS.read_text(encoding="utf-8")
        self.assertIn("gate_state: target_ae_reference_argb8_matrix_verified_repeat_edge_gap_explained", text)
        self.assertIn("fixed-fixture controls and re-audit", text)
        self.assertIn("SmartFX PreRender/Render", text)
        self.assertIn("GPU lifecycle", text)
        self.assertIn("propagates error 4", text)
        self.assertIn("malformed frame contract", text)
        self.assertIn("42/2/24", text)
        self.assertIn("H-4 default reference capture is satisfied", text)
        self.assertIn("zero byte or pixel differences", text)
        self.assertIn("amount 0 and 500", text)
        self.assertIn("connected 5x3 map", text)
        self.assertIn("Repeat Edge remains the only", text)
        self.assertIn("current false but", text)

    def test_all_gate_items_are_explicit(self):
        text = STATUS.read_text(encoding="utf-8")
        for gate in range(1, 9):
            self.assertIn(f"G-{gate}", text)
        for missing in ("G-1 fixture approval | Satisfied", "G-2 loader approval receipt | Satisfied for L2", "G-3 dependency review | Satisfied", "G-8 cleanroom and licensing decisions | Satisfied"):
            self.assertIn(missing, text)

    def test_l1_minihost_exists_after_approval(self):
        self.assertTrue((ROOT / "minihost" / "src" / "main.cpp").is_file())


if __name__ == "__main__":
    unittest.main()

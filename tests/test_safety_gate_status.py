import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
STATUS = ROOT / "docs" / "SAFETY_GATE_STATUS_2026-07-13.md"


class SafetyGateStatusTests(unittest.TestCase):
    def test_status_keeps_phase_d_closed(self):
        text = STATUS.read_text(encoding="utf-8")
        self.assertIn("gate_state: closed_missing_required_human_evidence", text)
        self.assertIn("not a gate-opening declaration", text)
        self.assertIn("Phase D remains", text)
        self.assertIn("forbidden until every G-1 through G-8", text)

    def test_all_gate_items_are_explicit(self):
        text = STATUS.read_text(encoding="utf-8")
        for gate in range(1, 9):
            self.assertIn(f"G-{gate}", text)
        for missing in ("G-1 fixture approval | Satisfied", "G-2 loader approval receipt | Satisfied for L1", "G-3 dependency review | Satisfied", "G-8 cleanroom and licensing decisions | Satisfied"):
            self.assertIn(missing, text)

    def test_phase_d_directory_is_absent_before_human_declaration(self):
        self.assertFalse((ROOT / "minihost").exists())


if __name__ == "__main__":
    unittest.main()

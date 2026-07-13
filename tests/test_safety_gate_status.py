import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
STATUS = ROOT / "docs" / "SAFETY_GATE_STATUS_2026-07-13.md"


class SafetyGateStatusTests(unittest.TestCase):
    def test_status_keeps_render_stage_closed(self):
        text = STATUS.read_text(encoding="utf-8")
        self.assertIn("gate_state: classic_default_render_executed_extended_render_closed", text)
        self.assertIn("not blanket native execution approval", text)
        self.assertIn("Extended render cases", text)
        self.assertIn("later stage remain forbidden", text)

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

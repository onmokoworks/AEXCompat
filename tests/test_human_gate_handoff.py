import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
HANDOFF = ROOT / "docs" / "HUMAN_GATE_HANDOFF_2026-07-13.md"
STATUS = ROOT / "docs" / "SAFETY_GATE_STATUS_2026-07-13.md"


class HumanGateHandoffTests(unittest.TestCase):
    def test_handoff_requests_every_human_decision(self):
        text = HANDOFF.read_text(encoding="utf-8")
        for item in ("H-1", "H-2", "H-3", "Origin", "Rights", "Identity authorization", "Public-document cleanroom"):
            self.assertIn(item, text)

    def test_handoff_has_no_approval_or_opening_effect(self):
        text = HANDOFF.read_text(encoding="utf-8")
        self.assertIn("not an approval", text)
        self.assertIn("does not authorize loading", text)
        self.assertIn("Phase D and H-4 remain closed", text)
        self.assertNotIn("--explicit-user-approval", text)
        self.assertNotIn("APPROVE_AEX_LOAD_GATE", text)

    def test_status_does_not_trust_test_generated_answers(self):
        text = STATUS.read_text(encoding="utf-8")
        self.assertIn("Test artifacts remain non-evidence", text)
        self.assertIn("owner confirms self-authorship", text)


if __name__ == "__main__":
    unittest.main()

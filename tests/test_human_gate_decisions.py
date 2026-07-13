import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class HumanGateDecisionTests(unittest.TestCase):
    def test_owner_statement_records_bounded_native_approval(self):
        text = (ROOT / "analysis" / "FIXTURE_PROVENANCE_OWNER_STATEMENT_2026-07-13.md").read_text(encoding="utf-8")
        self.assertIn("self-authored", text)
        self.assertIn("approve_for_separate_loader_gate_preparation", text)
        self.assertIn("223FF5EC542DD74374C727F16AA6C068073D1C2D7A5CABF20512CB289F0716EB", text)
        self.assertIn("does not waive dependency review", text)

    def test_cleanroom_decision_confines_sdk_to_instruments(self):
        text = (ROOT / "docs" / "ABI_PROVENANCE_DECISION_2026-07-13.md").read_text(encoding="utf-8")
        self.assertIn("public-document cleanroom", text)
        self.assertIn("may be read only", text)
        self.assertIn("under `instruments/`", text)
        self.assertIn("may not be copied", text)

    def test_sdk_note_records_external_local_root(self):
        text = (ROOT / "analysis" / "AE_SDK_LICENSE_NOTE_2026-07-13.md").read_text(encoding="utf-8")
        self.assertIn("local SDK root confirmed", text)
        self.assertIn(r"C:\Program Files\Adobe\AfterEffectsSDK", text)
        self.assertIn("H-2 is satisfied", text)
        self.assertIn("SDK files stay outside Git", text)

    def test_l1_receipt_is_hash_bound_bounded_and_expiring(self):
        text = (ROOT / "analysis" / "SCATTERMAP_LOADER_APPROVAL_RECEIPT_2026-07-13.md").read_text(encoding="utf-8")
        self.assertIn("approve_native_load_l1", text)
        self.assertIn("223FF5EC542DD74374C727F16AA6C068073D1C2D7A5CABF20512CB289F0716EB", text)
        self.assertIn("expires: `2026-08-12T23:59:59+09:00`", text)
        self.assertIn("does not approve L2", text)


if __name__ == "__main__":
    unittest.main()

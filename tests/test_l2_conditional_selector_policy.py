import unittest
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
WORKER = source_owners.L2_SOURCE
BROKER = ROOT / "broker" / "crates" / "broker" / "src" / "l2.rs"
PROFILES = ROOT / "broker" / "crates" / "broker" / "src" / "fixture_profiles" / "mod.rs"
RESULT = ROOT / "analysis" / "SCATTERMAP_L2_RESULT_2026-07-13.md"


class L2ConditionalSelectorPolicyTests(unittest.TestCase):


    def test_result_explains_why_conditional_selectors_are_omitted(self):
        text = RESULT.read_text(encoding="utf-8")
        self.assertIn("SEND_UPDATE_PARAMS_UI", text)
        self.assertIn("SUPPORTS_QUERY_DYNAMIC_FLAGS", text)
        self.assertIn("correctly omitted", text)


if __name__ == "__main__":
    unittest.main()

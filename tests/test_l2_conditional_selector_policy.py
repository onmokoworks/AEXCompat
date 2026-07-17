import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKER = ROOT / "minihost" / "src" / "l2_main.cpp"
BROKER = ROOT / "broker" / "crates" / "broker" / "src" / "l2.rs"
PROFILES = ROOT / "broker" / "crates" / "broker" / "src" / "fixture_profiles" / "mod.rs"
RESULT = ROOT / "analysis" / "SCATTERMAP_L2_RESULT_2026-07-13.md"


class L2ConditionalSelectorPolicyTests(unittest.TestCase):
    def test_worker_derives_selector_policy_from_advertised_flags(self):
        worker = WORKER.read_text(encoding="utf-8")
        self.assertIn("update_params_ui_advertised", worker)
        self.assertIn("query_dynamic_flags_advertised", worker)
        self.assertIn("(1u << 26)", worker)
        self.assertIn("dispatch_conditional_ui_selectors", worker)
        self.assertIn("kUpdateParamsUi = 14", worker)
        self.assertIn("kQueryDynamicFlags = 18", worker)
        self.assertIn('std::strcmp(name, "PF Param Utils Suite")', worker)

    def test_broker_enforces_profile_flags_and_conditional_dispatch(self):
        broker = BROKER.read_text(encoding="utf-8")
        profiles = PROFILES.read_text(encoding="utf-8")
        for expected in (
            'worker_report.get("update_params_ui_advertised")',
            'worker_report.get("query_dynamic_flags_advertised")',
            'worker_report.get("conditional_ui_selectors_dispatched")',
        ):
            self.assertIn(expected, broker)
        for expected in ("33_554_432", "167_777_280", "L2ObservationPolicy"):
            self.assertIn(expected, profiles)
        self.assertNotIn("ScatterMap", broker)

    def test_result_explains_why_conditional_selectors_are_omitted(self):
        text = RESULT.read_text(encoding="utf-8")
        self.assertIn("SEND_UPDATE_PARAMS_UI", text)
        self.assertIn("SUPPORTS_QUERY_DYNAMIC_FLAGS", text)
        self.assertIn("correctly omitted", text)


if __name__ == "__main__":
    unittest.main()

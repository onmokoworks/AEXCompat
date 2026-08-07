import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "REAL_AEX_RENDER_GATE_RESULT_2026-07-17.json"
EXPECTED_OUTPUT = "91d436a039c7f5ef56c4418e97dcb48dd4a69b8b9d984f224a06f97fe3ff8578"


class AexRenderGateTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.data = json.loads(RESULT.read_text(encoding="utf-8-sig"))

    def test_schema_status_and_scope_are_fixed(self):
        self.assertEqual(self.data["schema_version"], 2)
        self.assertEqual(self.data["gate"], "real_aex_render_gate")
        self.assertFalse(self.data["oracle"])
        self.assertIn(self.data["status"], {"passed", "blocked"})
        self.assertIn("not an ae oracle", self.data["scope"]["does_not_prove"].lower())

    def test_current_gate_uses_the_session_transport(self):
        invocation = self.data["invocation"]
        self.assertEqual(invocation["transport"], "render-experimental-session")
        self.assertEqual(invocation["adapter"], "tools/refresh-runtime-session.py")
        self.assertNotIn("--render-image", json.dumps(invocation).lower())

    def test_fixed_vector_and_hashes(self):
        case = self.data["case"]
        self.assertEqual(case, {
            "render_path": "classic", "parameter_state": "default", "frame": 0,
            "pixel_format": "rgba8", "width": 16, "height": 12, "rowbytes": 64,
        })
        self.assertEqual(self.data["authenticated_artifacts"]["reference"]["sha256"], EXPECTED_OUTPUT)
        if self.data["status"] == "passed":
            self.assertEqual(self.data["output"]["sha256"], EXPECTED_OUTPUT)
            self.assertEqual(self.data["output"]["expected_sha256"], EXPECTED_OUTPUT)
            self.assertEqual(self.data["output"]["differing_bytes"], 0)
        else:
            self.assertIn("failure", self.data)
            self.assertEqual(self.data["checks"]["fail_closed"], True)

    def test_success_checks_are_all_true_or_blocked_is_explicit(self):
        if self.data["status"] == "passed":
            self.assertTrue(all(self.data["checks"].values()))
            self.assertEqual(self.data["session"]["status"], "render_completed")
            self.assertTrue(self.data["session"]["passed"])
        else:
            self.assertEqual(self.data["classification"], "session_transport_failure")
            self.assertTrue(self.data["checks"]["structured_failure_recorded"])
            self.assertTrue(self.data["checks"]["fail_closed"])
            self.assertNotEqual(self.data["status"], "passed")


if __name__ == "__main__":
    unittest.main()

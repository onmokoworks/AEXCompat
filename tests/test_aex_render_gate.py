import json
import re
import unittest
from pathlib import Path, PurePosixPath


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "REAL_AEX_RENDER_GATE_RESULT_2026-07-17.json"
SCRIPT = ROOT / "tools" / "run-aex-render-gate.ps1"
EXPECTED_OUTPUT = "91d436a039c7f5ef56c4418e97dcb48dd4a69b8b9d984f224a06f97fe3ff8578"


class AexRenderGateTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.data = json.loads(RESULT.read_text(encoding="utf-8-sig"))

    def test_schema_status_and_scope_are_fixed(self):
        self.assertEqual(self.data["schema_version"], 1)
        self.assertEqual(self.data["gate"], "real_aex_render_gate")
        self.assertEqual(self.data["classification"], "host_regression_exact")
        self.assertFalse(self.data["oracle"])
        self.assertEqual(self.data["status"], "passed")
        self.assertIn("not an AE oracle", self.data["scope"]["does_not_prove"])

    def test_fixed_vector_and_hashes(self):
        case = self.data["case"]
        self.assertEqual(case, {
            "render_path": "classic", "parameter_state": "default", "frame": 0,
            "pixel_format": "rgba8", "width": 16, "height": 12, "rowbytes": 64,
        })
        self.assertEqual(self.data["output"]["sha256"], EXPECTED_OUTPUT)
        self.assertEqual(self.data["output"]["expected_sha256"], EXPECTED_OUTPUT)
        self.assertEqual(self.data["authenticated_artifacts"]["reference"]["sha256"], EXPECTED_OUTPUT)
        script = SCRIPT.read_text(encoding="utf-8-sig").lower()
        self.assertIn(EXPECTED_OUTPUT, script)

    def test_worker_and_safety_checks_pass(self):
        worker = self.data["worker"]
        self.assertEqual(self.data["invocation"]["exit_code"], 0)
        self.assertEqual(worker["stage"], "classic_render")
        self.assertEqual(worker["status"], "render_completed")
        self.assertTrue(worker["guard_bytes_intact"])
        self.assertTrue(worker["suite_leases_balanced"])
        self.assertEqual(worker["live_suite_lease_count"], 0)
        self.assertEqual(worker["suite_acquires"], worker["suite_releases"])
        self.assertTrue(worker["handle_lifetimes_balanced"])
        self.assertTrue(worker["world_lifetimes_balanced"])
        self.assertEqual(worker["last_seh_exception_code"], 0)
        self.assertTrue(all(self.data["checks"].values()))

    def test_evidence_contains_only_repository_relative_paths(self):
        for artifact in self.data["authenticated_artifacts"].values():
            path = artifact["path"]
            self.assertFalse(PurePosixPath(path).is_absolute())
            self.assertNotRegex(path, re.compile(r"^[A-Za-z]:[\\/]"))
            self.assertNotIn("..", PurePosixPath(path).parts)
        serialized = RESULT.read_text(encoding="utf-8-sig")
        self.assertNotRegex(serialized, re.compile(r"[A-Za-z]:\\"))


if __name__ == "__main__":
    unittest.main()

import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "AEX_DISCOVERY_CONFORMANCE_MATRIX_2026-07-17.json"


class AexDiscoveryConformanceMatrixResultTests(unittest.TestCase):
    def test_matrix_covers_independent_parameter_surfaces(self):
        report = json.loads(RESULT.read_text(encoding="utf-8"))
        self.assertEqual(report["schema_version"], 1)
        self.assertEqual(report["stage"], "aex_discovery_conformance_matrix")
        self.assertTrue(report["passed"])

        cases = {case["name"]: case for case in report["cases"]}
        self.assertEqual(
            set(cases), {"paramarama", "colorgrid", "pathmaster", "smartypants"}
        )
        self.assertEqual(cases["paramarama"]["parameter_count"], 8)
        self.assertIn("point3d", cases["paramarama"]["kinds"])
        self.assertIn("button", cases["paramarama"]["kinds"])
        self.assertEqual(cases["colorgrid"]["kinds"], ["arbitrary_data"])
        self.assertIn("path", cases["pathmaster"]["kinds"])
        self.assertEqual(cases["smartypants"]["parameter_count"], 2)
        for case in cases.values():
            self.assertTrue(case["passed"])
            self.assertRegex(case["aex_sha256"], r"^[0-9A-F]{64}$")


if __name__ == "__main__":
    unittest.main()

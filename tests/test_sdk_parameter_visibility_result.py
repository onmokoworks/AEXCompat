import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_PARAMETER_VISIBILITY_RESULT_2026-07-15.json"


class SdkParameterVisibilityResultTests(unittest.TestCase):
    def test_current_host_version_enables_modern_sdk_parameters(self):
        result = json.loads(RESULT.read_text(encoding="utf-8"))
        self.assertEqual(result["host_spec_version"], {"major": 13, "minor": 28})
        self.assertEqual(result["paramarama"]["status"], "parameters_inspected")
        self.assertEqual(result["paramarama"]["modern_ae_only_types_present"], [18, 15])
        self.assertEqual(len(result["paramarama"]["parameter_types"]), 8)

    def test_path_is_visible_without_requiring_render_lifecycle(self):
        result = json.loads(RESULT.read_text(encoding="utf-8"))["pathmaster"]
        self.assertEqual(result["status"], "parameters_inspected")
        self.assertEqual(result["parameter_count"], 6)
        self.assertEqual(result["parameter_types"][0], 12)
        self.assertEqual(
            result["broker_path_parameter"],
            {"slot": 1, "name": "Path", "kind": "path"},
        )
        self.assertFalse(result["render_lifecycle_dispatched"])
        self.assertEqual(
            result["selectors_dispatched"],
            ["GLOBAL_SETUP", "PARAMS_SETUP", "GLOBAL_SETDOWN"],
        )


if __name__ == "__main__":
    unittest.main()

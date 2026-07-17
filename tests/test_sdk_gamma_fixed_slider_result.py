import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_GAMMA_FIXED_SLIDER_RESULT_2026-07-15.json"


class SdkGammaFixedSliderResultTests(unittest.TestCase):
    def test_fixed_slider_descriptor_matches_the_sdk_fixture(self):
        result = json.loads(RESULT.read_text(encoding="utf-8"))
        parameter = result["inspection"]["parameter"]
        self.assertEqual(result["inspection"]["reported_num_params"], 2)
        self.assertEqual(parameter["type"], 2)
        self.assertEqual(parameter["name"], "Gamma")
        self.assertEqual(
            [parameter["valid_min"], parameter["valid_max"], parameter["default"]],
            [0.0, 2.0, 1.0],
        )
        self.assertEqual(parameter["precision"], 1)

    def test_non_default_fixed_value_renders_through_iterate(self):
        result = json.loads(RESULT.read_text(encoding="utf-8"))
        render = result["render"]
        self.assertEqual(render["requested_kind"], "float")
        self.assertEqual(render["requested_value"], 1.5)
        self.assertEqual(render["status"], "render_completed")
        self.assertEqual(render["render_error"], 0)
        self.assertTrue(render["guard_bytes_intact"])
        self.assertNotEqual(render["input_sha256"], render["output_sha256"])
        self.assertTrue(all(error == 0 for error in render["lifecycle_errors"].values()))


if __name__ == "__main__":
    unittest.main()

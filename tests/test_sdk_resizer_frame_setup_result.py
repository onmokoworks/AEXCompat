import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_RESIZER_FRAME_SETUP_RESULT_2026-07-15.json"


class SdkResizerFrameSetupResultTests(unittest.TestCase):
    def test_frame_setup_resizes_and_offsets_the_input(self):
        result = json.loads(RESULT.read_text(encoding="utf-8"))
        render = result["render"]
        self.assertEqual(render["input_dimensions"], [2, 2])
        self.assertEqual(render["output_dimensions"], [4, 4])
        self.assertEqual(render["origin"], [1, 1])
        self.assertEqual(render["border_rgba"], [128, 255, 255, 255])
        self.assertEqual(len(render["center_rgba"]), 4)
        self.assertTrue(render["guard_bytes_intact"])

    def test_8_and_16_bpc_converge_to_the_same_rgba_output(self):
        render = json.loads(RESULT.read_text(encoding="utf-8"))["render"]
        self.assertEqual(render["argb8_status"], "render_completed")
        self.assertEqual(render["argb16_status"], "render_completed")
        self.assertEqual(render["rgba8_output_sha256"], render["argb16_to_rgba8_output_sha256"])

    def test_fixture_exercises_resize_specific_host_contracts(self):
        result = json.loads(RESULT.read_text(encoding="utf-8"))
        self.assertTrue(result["inspection"]["query_dynamic_flags_advertised"])
        contracts = set(result["host_contracts"])
        self.assertIn("PF_Cmd_FRAME_SETUP output width height and origin", contracts)
        self.assertIn("PF_Point origin components read as signed 32-bit A_long at offsets 88 and 92", contracts)
        self.assertIn("PF Fill Matte Suite 2", contracts)
        self.assertIn("PF World Transform Suite 1 origin-aware copy", contracts)

    def test_origin_components_are_observed_as_two_32_bit_values(self):
        regression = json.loads(RESULT.read_text(encoding="utf-8"))["origin32_regression"]
        self.assertTrue(regression["passed"])
        self.assertEqual(regression["input_dimensions"], [37, 23])
        self.assertEqual(regression["output_dimensions"], [137, 123])
        self.assertEqual(regression["output_origin"], [50, 50])
        self.assertTrue(regression["legacy_int16_y_offset_would_fail"])
        self.assertTrue(regression["guard_bytes_intact"])
        self.assertTrue(regression["world_lifetimes_balanced"])
        self.assertTrue(regression["param_checkouts_balanced"])


if __name__ == "__main__":
    unittest.main()

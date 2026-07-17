import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_CONVOLUTRIX_WORLD_UTILS_RESULT_2026-07-15.json"


class SdkConvolutrixWorldUtilsResultTests(unittest.TestCase):
    def test_convolution_changes_pixels_and_matches_across_depths(self):
        convolve = json.loads(RESULT.read_text(encoding="utf-8"))["convolve"]
        self.assertEqual([convolve["argb8_status"], convolve["argb16_status"]],
                         ["render_completed", "render_completed"])
        self.assertNotEqual(convolve["input_rgba_sha256"], convolve["argb8_to_rgba8_sha256"])
        self.assertEqual(convolve["argb8_to_rgba8_sha256"], convolve["argb16_to_rgba8_sha256"])
        self.assertTrue(convolve["guard_bytes_intact"])

    def test_temporary_blend_world_is_released(self):
        blend = json.loads(RESULT.read_text(encoding="utf-8"))["blend"]
        self.assertEqual(blend["status"], "render_completed")
        self.assertEqual([blend["suite_acquires"], blend["suite_releases"]], [2, 2])
        self.assertEqual([blend["worlds_created"], blend["worlds_disposed"]], [1, 1])
        self.assertTrue(blend["suite_leases_balanced"])
        self.assertTrue(blend["handle_lifetimes_balanced"])
        self.assertTrue(blend["world_lifetimes_balanced"])

    def test_fixture_covers_legacy_and_suite_world_paths(self):
        contracts = set(json.loads(RESULT.read_text(encoding="utf-8"))["host_contracts"])
        self.assertIn("PF World Transform Suite 1 convolve and blend", contracts)
        self.assertIn("legacy PF_UtilCallbacks blend convolve fill new_world dispose_world", contracts)


if __name__ == "__main__":
    unittest.main()

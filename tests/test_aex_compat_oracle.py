import unittest
from pathlib import Path
from tools.aex_compat_oracle import compare_images, safe_ppm
from tools.ppm_fixture_tool import PpmImage

class CompatOracleTests(unittest.TestCase):
    def image(self, values, width=1, height=1): return PpmImage(width,height,bytes(values))
    def test_identity(self):
        r=compare_images(self.image([10,20,30]),self.image([10,20,30]),0)
        self.assertEqual("identical",r["match_state"]); self.assertEqual(0,r["max_channel_delta"])
    def test_invert_nonmatching(self):
        r=compare_images(self.image([0,10,20]),self.image([255,245,235]),0)
        self.assertEqual("nonmatching",r["match_state"]); self.assertEqual(1,r["exceeding_pixel_count"])
    def test_within_tolerance(self):
        r=compare_images(self.image([10,20,30]),self.image([12,19,33]),3)
        self.assertEqual("within_tolerance",r["match_state"]); self.assertEqual(3,r["max_channel_delta"]); self.assertEqual(0,r["exceeding_pixel_count"])
    def test_dimension_mismatch_skips_pixels(self):
        r=compare_images(self.image([1,2,3]),self.image([1,2,3,4,5,6],2,1),0)
        self.assertEqual("nonmatching",r["match_state"]); self.assertIsNone(r["max_channel_delta"]); self.assertIsNone(r["exceeding_pixel_count"])
    def test_tolerance_range(self):
        with self.assertRaises(ValueError): compare_images(self.image([0,0,0]),self.image([0,0,0]),256)
    def test_report_never_contains_pixels(self):
        r=compare_images(self.image([1,2,3]),self.image([1,2,4]),0)
        self.assertNotIn("pixels",r); self.assertFalse(r["render_performed"]); self.assertFalse(r["pixel_values_serialized"])
    def test_non_ppm_input_is_rejected(self):
        with self.assertRaises(ValueError): safe_ppm(Path("fixture.aex"))
if __name__ == "__main__": unittest.main()

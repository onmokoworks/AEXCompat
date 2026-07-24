import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock
from tools import aex_compat_oracle
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

    def test_cli_writes_new_report(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            reference = root / "reference.ppm"
            candidate = root / "candidate.ppm"
            destination = root / "out" / "report.json"
            for path in (reference, candidate):
                path.write_bytes(b"P6\n1 1\n255\n\x01\x02\x03")
            with (
                mock.patch.object(aex_compat_oracle, "TARGET_ROOT", root),
                mock.patch.object(aex_compat_oracle, "OUTPUT_ROOT", root / "out"),
                contextlib.redirect_stdout(io.StringIO()),
            ):
                code = aex_compat_oracle.main(["--reference", str(reference), "--candidate", str(candidate), "--out", str(destination)])
            self.assertEqual(0, code)
            self.assertEqual("compat_oracle", json.loads(destination.read_text(encoding="utf-8"))["report_kind"])

    def test_cli_preserves_competing_report_after_preflight(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            reference = root / "reference.ppm"
            candidate = root / "candidate.ppm"
            destination = root / "out" / "report.json"
            for path in (reference, candidate):
                path.write_bytes(b"P6\n1 1\n255\n\x01\x02\x03")
            original_compare = aex_compat_oracle.compare_images

            def create_competing_report(*args):
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_text("sentinel", encoding="utf-8")
                return original_compare(*args)

            with (
                mock.patch.object(aex_compat_oracle, "TARGET_ROOT", root),
                mock.patch.object(aex_compat_oracle, "OUTPUT_ROOT", root / "out"),
                mock.patch.object(aex_compat_oracle, "compare_images", side_effect=create_competing_report),
                contextlib.redirect_stdout(io.StringIO()),
                contextlib.redirect_stderr(io.StringIO()),
            ):
                code = aex_compat_oracle.main(["--reference", str(reference), "--candidate", str(candidate), "--out", str(destination)])
            self.assertEqual(2, code)
            self.assertEqual("sentinel", destination.read_text(encoding="utf-8"))
if __name__ == "__main__": unittest.main()

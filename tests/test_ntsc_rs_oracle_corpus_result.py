import json
import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "NTSC_RS_ORACLE_CORPUS_RESULT_2026-07-19.json"
SHA256 = re.compile(r"^[0-9a-f]{64}$")


class NtscRsOracleCorpusResultTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.document = json.loads(RESULT.read_text(encoding="utf-8-sig"))

    def test_document_covers_every_required_axis_of_issue_31(self):
        axes = {case["axis"] for case in self.document["cases"]}
        self.assertLessEqual(
            {"popup_parameter", "alpha_input", "odd_dimensions", "resolution_4k"},
            axes)

    def test_tolerance_is_a_one_lsb_8bit_judgment(self):
        tolerance = self.document["tolerance"]
        self.assertGreaterEqual(tolerance, 1 / 255)
        self.assertLess(tolerance, 2 / 255)

    def test_every_case_matches_within_one_lsb_and_keeps_alpha_exact(self):
        tolerance = self.document["tolerance"]
        for case in self.document["cases"]:
            comparison = case["comparison"]
            with self.subTest(case=case["name"]):
                self.assertTrue(comparison["match"])
                self.assertEqual(comparison["over_tolerance_channels"], 0)
                self.assertEqual(comparison["tolerance"], tolerance)
                self.assertIsNone(comparison["first_mismatch"])
                for channel in ("r", "g", "b"):
                    self.assertLessEqual(
                        comparison["max_abs_error"][channel], tolerance)
                self.assertEqual(comparison["max_abs_error"]["a"], 0)
                self.assertEqual(comparison["formats"],
                                 {"raw": "rgba8", "render": "png_rgba8"})
                self.assertEqual(
                    comparison["dimensions"],
                    {"width": case["input"]["width"],
                     "height": case["input"]["height"]})

    def test_comparison_hashes_bind_the_recorded_host_and_ae_artifacts(self):
        for case in self.document["cases"]:
            comparison = case["comparison"]
            with self.subTest(case=case["name"]):
                self.assertEqual(comparison["hashes"]["raw_sha256"],
                                 case["host"]["output_raw_rgba8_sha256"])
                self.assertEqual(comparison["hashes"]["render_sha256"],
                                 case["ae_capture"]["output_png_sha256"])

    def test_recorded_identities_are_well_formed(self):
        self.assertTrue(SHA256.match(self.document["environment"]["plugin_sha256"]))
        for case in self.document["cases"]:
            with self.subTest(case=case["name"]):
                self.assertTrue(SHA256.match(case["input"]["sha256"]))
                self.assertTrue(SHA256.match(case["input"]["decoded_rgba_sha256"]))
                self.assertTrue(SHA256.match(case["host"]["output_png_sha256"]))
                self.assertTrue(SHA256.match(case["ae_capture"]["output_png_sha256"]))
                self.assertEqual(case["ae_capture"]["frame"], 0)
                self.assertEqual(case["ae_capture"]["bpc"], 8)

    def test_popup_case_records_the_parameter_binding(self):
        popup = next(case for case in self.document["cases"]
                     if case["axis"] == "popup_parameter")
        parameter = popup["parameter"]
        self.assertEqual(parameter["kind"], "popup")
        self.assertEqual(parameter["host_slot"], 4)
        self.assertEqual(parameter["ae_property_name"], "Use field")
        self.assertEqual(parameter["value"], 6)
        self.assertEqual(parameter["choice_label"], "Both")
        self.assertEqual(popup["host"]["render_flag"],
                         "--render-experimental-smart-param")

    def test_document_serializes_no_absolute_paths(self):
        text = RESULT.read_text(encoding="utf-8-sig")
        self.assertNotRegex(text, r"[A-Za-z]:\\\\")
        self.assertNotRegex(text, r"[A-Za-z]:/")
        self.assertNotIn("\\\\Users", text)


if __name__ == "__main__":
    unittest.main()

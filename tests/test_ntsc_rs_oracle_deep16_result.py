import json
import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "NTSC_RS_ORACLE_DEEP16_RESULT_2026-07-19.json"
SHA256 = re.compile(r"^[0-9a-f]{64}$")
REFRESH = ROOT / "tools" / "refresh-ntsc-rs-oracle-deep16-evidence.ps1"


class NtscRsOracleDeep16ResultTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.document = json.loads(RESULT.read_text(encoding="utf-8-sig"))

    def test_document_covers_both_inputs_and_both_fps_configurations(self):
        names = {case["name"] for case in self.document["cases"]}
        self.assertLessEqual(
            {"gradient-fps24", "gradient-fps1", "generated-fps24"}, names)

    def test_legacy_capture_is_not_presented_as_identity_bound_oracle(self):
        identity = self.document["oracle_identity"]
        self.assertEqual(identity["state"], "unverified")
        self.assertFalse(identity["exact_claim_allowed"])
        self.assertIn("RequireLoadedAexIdentity", identity["recapture_requirement"])

    def test_tolerance_is_a_sub_8bit_transport_code_judgment(self):
        tolerance = self.document["tolerance"]
        # Well below one 8-bit LSB (1/255): this document judges full 16-bit
        # precision, not the 8-bit boundary the corpus document uses.
        self.assertLess(tolerance, 1 / 255 / 8)
        # But it must admit quantization-boundary residue of a few transport
        # codes (1/32768 each), so it cannot be tighter than 2 codes.
        self.assertGreaterEqual(tolerance, 2 / 32768)

    def test_every_case_matches_at_full_16bit_precision_with_exact_alpha(self):
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
                                 {"raw": "rgba16le", "render": "png_rgba16"})
                self.assertEqual(comparison["raw_integer_max"], 32768)
                self.assertEqual(
                    comparison["dimensions"],
                    {"width": case["input"]["width"],
                     "height": case["input"]["height"]})

    def test_comparison_hashes_bind_the_recorded_host_and_ae_artifacts(self):
        for case in self.document["cases"]:
            comparison = case["comparison"]
            with self.subTest(case=case["name"]):
                self.assertEqual(comparison["hashes"]["raw_sha256"],
                                 case["host"]["output_raw_rgba16le_sha256"])
                self.assertEqual(comparison["hashes"]["render_sha256"],
                                 case["ae_capture"]["output_png_sha256"])

    def test_gradient_cases_share_one_host_render_and_ae_is_fps_invariant(self):
        by_name = {case["name"]: case for case in self.document["cases"]}
        fps24 = by_name["gradient-fps24"]
        fps1 = by_name["gradient-fps1"]
        self.assertEqual(fps24["host"]["output_raw_rgba16le_sha256"],
                         fps1["host"]["output_raw_rgba16le_sha256"])
        self.assertEqual(fps24["ae_capture"]["output_png_sha256"],
                         fps1["ae_capture"]["output_png_sha256"])
        self.assertEqual(fps24["ae_capture"]["fps"], 24)
        self.assertEqual(fps1["ae_capture"]["fps"], 1)
        self.assertTrue(self.document["ae_fps_invariance"]["observed"])

    def test_mechanism_manifest_is_recomputed_and_holds(self):
        mechanism = self.document["mechanism"]
        self.assertEqual(mechanism["verified_by"],
                         "tools/verify-deep16-mechanism.py")
        manifest = mechanism["manifest"]
        self.assertTrue(manifest["holds"])
        promotion = manifest["host_promotion"]
        self.assertTrue(promotion["holds"])
        self.assertEqual(promotion["mismatched_samples"], 0)
        self.assertGreater(promotion["total_samples"], 0)
        self.assertTrue(SHA256.match(promotion["smart_input_dump_sha256"]))
        ae_map = manifest["ae_composed_map"]
        self.assertTrue(ae_map["holds"])
        self.assertTrue(ae_map["mapping_deterministic"])
        self.assertTrue(ae_map["deviation_bounded_by_one"])
        self.assertTrue(ae_map["roundtrip_round_v16_div_257_exact"])
        self.assertLessEqual(set(ae_map["deviation_histogram"]),
                             {"-1", "0", "1"})
        self.assertEqual(ae_map["distinct_8bit_values_observed"], 256)
        artifacts = mechanism["artifacts"]
        self.assertEqual(
            artifacts["smart_input_dump"],
            "target/oracle-deep16/host-gradient-smart-input.rgba16le")
        self.assertEqual(artifacts["noeffect_capture_png"],
                         "target/oracle-deep16/ae-noeffect-16.png")

    def test_recorded_identities_are_well_formed(self):
        self.assertTrue(SHA256.match(self.document["environment"]["plugin_sha256"]))
        self.assertEqual(self.document["environment"]["host_pixel_format"], "argb16")
        for case in self.document["cases"]:
            with self.subTest(case=case["name"]):
                self.assertTrue(SHA256.match(case["input"]["sha256"]))
                self.assertTrue(SHA256.match(case["input"]["decoded_rgba_sha256"]))
                self.assertTrue(SHA256.match(case["host"]["output_png16_sha256"]))
                self.assertTrue(SHA256.match(case["ae_capture"]["output_png_sha256"]))
                self.assertEqual(case["ae_capture"]["frame"], 0)
                self.assertEqual(case["ae_capture"]["bpc"], 16)
                self.assertEqual(case["host"]["render_flag"],
                                 "--render-experimental-smart-16-deep")

    def test_document_serializes_no_absolute_paths(self):
        text = RESULT.read_text(encoding="utf-8-sig")
        self.assertNotRegex(text, r"[A-Za-z]:\\\\")
        self.assertNotRegex(text, r"[A-Za-z]:/")
        self.assertNotIn("\\\\Users", text)

    def test_refresh_recomputes_pixels_and_requires_loaded_module_identity(self):
        source = REFRESH.read_text(encoding="utf-8")
        self.assertIn("loaded_aex_identity.state", source)
        self.assertIn("-RequireLoadedAexIdentity", source)
        self.assertIn("canonical_path_sha256 -notmatch", source)
        self.assertIn("file_id -notmatch", source)
        self.assertIn("currentInstalledIdentity.canonical_path_sha256", source)
        self.assertIn("currentInstalledIdentity.file_id", source)
        self.assertIn("compare-pixel-oracles.py", source)
        self.assertIn("--raw-format rgba16le", source)
        self.assertIn("--raw-integer-max 32768", source)
        self.assertIn("--tolerance 0.000125", source)
        self.assertNotIn("compare-{0}.json", source)


if __name__ == "__main__":
    unittest.main()

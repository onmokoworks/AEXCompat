import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PROBE = ROOT / "instruments" / "pf-path-curve-probe"


class PfPathCurveProbeSourceTest(unittest.TestCase):
    def test_probe_exercises_curved_path_length_evaluation_and_derivative(self):
        source = (PROBE / "pf_path_curve_probe.cpp").read_text(encoding="utf-8")
        for marker in (
            "PF_PathPrepareSegLength",
            "PF_PathGetSegLength",
            "PF_PathEvalSegLength(",
            "PF_PathEvalSegLengthDeriv1",
            "r.eval_x",
            "r.deriv_dx",
        ):
            self.assertIn(marker, source)

    def test_frequency_and_length_boundaries_are_explicit(self):
        source = (PROBE / "pf_path_curve_probe.cpp").read_text(encoding="utf-8")
        self.assertIn("{{-1, 0, 1, 1023, 1024, 1025}}", source)
        self.assertIn("std::nextafter(0.0, inf)", source)
        self.assertIn("std::nextafter(length, -inf)", source)
        self.assertIn("std::numeric_limits<double>::quiet_NaN()", source)
        self.assertIn("std::numeric_limits<double>::infinity()", source)

    def test_oracle_has_fixed_reversible_crc_protected_layout(self):
        source = (PROBE / "pf_path_curve_probe.cpp").read_text(encoding="utf-8")
        self.assertIn("kHeaderSize = 24", source)
        self.assertIn("kRecordSize = 112", source)
        self.assertIn("kSentinelBits = UINT64_C(0x7ff4a5a5deadbeef)", source)
        self.assertIn("crc32(bytes.data() + kHeaderSize", source)
        self.assertIn("sizeof(Component) == 1 ? 1 : 257", source)
        self.assertIn("capacity = width * height * 3", source)
        self.assertIn("row[x].alpha", source)
        self.assertIn("PF_PathEvalSegLength(", source)
        self.assertIn("PF_PathEvalSegLengthDeriv1(", source)
        self.assertIn("checked_output_capacity", source)
        self.assertIn("catch (const std::bad_alloc&)", source)
        self.assertIn("if (!out_data) return PF_Err_BAD_CALLBACK_PARAM", source)

    def test_ownership_order_is_cleanup_checkin_then_release(self):
        source = (PROBE / "pf_path_curve_probe.cpp").read_text(encoding="utf-8")
        cleanup = source.index("data->PF_PathCleanupSegLength")
        checkin = source.index("query->PF_CheckinPath")
        release_data = source.index("ReleaseSuite(\n      kPFPathDataSuite")
        release_query = source.index("ReleaseSuite(\n      kPFPathQuerySuite")
        self.assertLess(cleanup, checkin)
        self.assertLess(checkin, release_data)
        self.assertLess(release_data, release_query)
        self.assertIn("if (prep)", source)
        self.assertIn("if (path)", source)

    def test_observation_errors_do_not_destroy_the_render(self):
        source = (PROBE / "pf_path_curve_probe.cpp").read_text(encoding="utf-8")
        self.assertIn("void retain_first_error(PF_Err error, PF_Err* result)", source)
        self.assertIn("Prepare/eval/cleanup failures are observations, not render failures", source)
        self.assertNotIn("retain_first_error(cleanup_error, &result);", source)
        self.assertIn("retain_first_error(query->PF_CheckinPath", source)
        self.assertEqual(
            source.count("pica_basicP->ReleaseSuite("),
            2,
        )

    def test_cleanup_and_prep_states_are_recorded(self):
        source = (PROBE / "pf_path_curve_probe.cpp").read_text(encoding="utf-8")
        self.assertIn("std::array<std::uint8_t, 5> prep_state", source)
        self.assertIn("r.prep_state[0] = prep_after_prepare", source)
        self.assertIn("r.prep_state[3] = prep != nullptr", source)
        self.assertIn("records[i].prep_state[4] = prep != nullptr", source)
        self.assertIn("records[i].cleanup_error = cleanup_error", source)

    def test_probe_is_ae_loadable_and_has_reproducible_builder(self):
        cmake = (PROBE / "CMakeLists.txt").read_text(encoding="utf-8")
        resource = (PROBE / "pf_path_curve_probe.rc").read_text(encoding="utf-8")
        script = (ROOT / "tools" / "build-pf-path-curve-probe.ps1").read_text(
            encoding="utf-8"
        )
        self.assertIn("add_library(pf_path_curve_probe MODULE", cmake)
        self.assertIn("pf_path_curve_probe.rc", cmake)
        self.assertIn("EffectMain", resource)
        self.assertIn("AEXCompat PF Path Curve", resource)
        self.assertIn("PF_DEEP_COLOR_AWARE=1", cmake)
        self.assertIn("--target pf_path_curve_probe", script)
        self.assertIn("Get-FileHash", script)

    def test_dedicated_ae_runner_uses_large_opaque_mask_carrier(self):
        runner = (ROOT / "tools" / "ae-path-curve-oracle-run.jsx").read_text(
            encoding="utf-8"
        )
        for marker in (
            'addComp("AEXCompat Path Curve Oracle", 128, 64',
            'addProperty("ADBE Mask Atom")',
            "shape.inTangents",
            "shape.outTangents",
            "pathParameter.setValue(1)",
            "comp.saveFrameToPng(0, output)",
            "CloseOptions.DO_NOT_SAVE_CHANGES",
        ):
            self.assertIn(marker, runner)


if __name__ == "__main__":
    unittest.main()

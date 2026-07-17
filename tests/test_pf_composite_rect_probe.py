import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PROBE = ROOT / "instruments" / "pf-composite-rect-probe"


class PfCompositeRectProbeSourceTest(unittest.TestCase):
    def test_probe_acquires_world_transform_and_calls_composite_rect(self):
        source = (PROBE / "pf_composite_rect_probe.cpp").read_text(encoding="utf-8")
        self.assertIn("PF_WorldTransformSuite1", source)
        self.assertIn("kPFWorldTransformSuiteVersion1", source)
        self.assertIn("AcquireSuite", source)
        self.assertIn("ReleaseSuite", source)
        self.assertIn("suite->composite_rect", source)
        self.assertIn("kPFWorldSuiteVersion2", source)
        self.assertIn("world_suite->PF_GetPixelFormat", source)
        self.assertIn("world_suite->PF_NewWorld", source)
        self.assertIn("world_suite->PF_DisposeWorld", source)
        self.assertNotIn("std::malloc", source)
        self.assertNotIn("std::free", source)
        self.assertIn("case PF_Cmd_RENDER:", source)

    def test_owned_source_and_both_suite_leases_are_cleaned_up(self):
        source = (PROBE / "pf_composite_rect_probe.cpp").read_text(encoding="utf-8")
        self.assertIn("bool source_created = false", source)
        self.assertIn("if (source_created)", source)
        self.assertIn("const SPErr world_release_err", source)
        self.assertIn("const SPErr release_err", source)
        self.assertEqual(source.count("PF_DisposeWorld(in_data->effect_ref, &source)"), 1)

    def test_visual_vectors_cover_modes_opacity_clipping_fields_and_aliasing(self):
        source = (PROBE / "pf_composite_rect_probe.cpp").read_text(encoding="utf-8")
        for marker in (
            "PF_Xfer_COPY",
            "PF_Xfer_BEHIND",
            "PF_Xfer_IN_FRONT",
            "PF_Field_FRAME",
            "PF_Field_UPPER",
            "PF_Field_LOWER",
            "rect.left -= band / 2",
            "output, output, rect",
            "rect.left + 1",
        ):
            self.assertIn(marker, source)
        self.assertRegex(source, r"rect, 128,")
        self.assertGreaterEqual(source.count("rect, 255,"), 6)

    def test_probe_is_ae_loadable_and_build_is_reproducible(self):
        cmake = (PROBE / "CMakeLists.txt").read_text(encoding="utf-8")
        resource = (PROBE / "pf_composite_rect_probe.rc").read_text(encoding="utf-8")
        script = (ROOT / "tools" / "build-pf-composite-rect-probe.ps1").read_text(
            encoding="utf-8"
        )
        self.assertIn("add_library(pf_composite_rect_probe MODULE", cmake)
        self.assertIn("pf_composite_rect_probe.rc", cmake)
        self.assertIn("EffectMain", resource)
        self.assertIn("AEXCompat PF Composite Rect", resource)
        self.assertIn("PF_DEEP_COLOR_AWARE=1", cmake)
        self.assertIn("--target pf_composite_rect_probe", script)
        self.assertIn("Get-FileHash", script)


if __name__ == "__main__":
    unittest.main()

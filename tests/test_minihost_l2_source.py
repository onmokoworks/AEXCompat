import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"


class MinihostL2SourceTests(unittest.TestCase):
    def test_layout_matches_observed_contract(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in ("kInSize = 408", "kOutSize = 408", "kParamSize = 176",
                       "kInAddParam = 16", "kInGlobalData = 312", "kOutGlobalData = 40"):
            self.assertIn(marker, text)

    def test_l2_is_non_rendering_and_bounded(self):
        text = SOURCE.read_text(encoding="utf-8")
        self.assertIn("kMaxParams = 64", text)
        self.assertIn('render_performed\\\":false', text)
        self.assertNotIn("PF_Cmd_RENDER", text)
        self.assertNotIn("AE_Effect.h", text)

    def test_l2_provides_bounded_movable_handle_callbacks(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in ("kUtilsSize = 552", "kUtilsNewHandle = 160", "new_handle(uint64_t size)",
                       "64 * 1024 * 1024", "g_handles.count", "dispose_handle"):
            self.assertIn(marker, text)

    def test_l2_pica_is_default_deny_except_handle_suite(self):
        text = SOURCE.read_text(encoding="utf-8")
        self.assertIn('std::strcmp(name, "PF Handle Suite")', text)
        self.assertIn("version == 2", text)
        self.assertIn("*suite = nullptr", text)
        self.assertIn("return 1", text)

    def test_l2_decodes_supported_parameter_descriptors(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in ("record.type == 1", "record.type == 7", "record.type == 4",
                       "record.type == 10", "valid_min", "default_value", "choices"):
            self.assertIn(marker, text)


if __name__ == "__main__":
    unittest.main()

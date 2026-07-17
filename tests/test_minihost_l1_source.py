import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "main.cpp"


class MinihostL1SourceTests(unittest.TestCase):
    def test_l1_revalidates_identity_and_uses_fixed_search(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in (
            "BCRYPT_SHA256_ALGORITHM",
            "LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR",
            "LOAD_LIBRARY_SEARCH_SYSTEM32",
            "GetProcAddress",
            "FreeLibrary",
        ):
            self.assertIn(marker, text)

    def test_l1_never_dispatches_selectors_or_renders(self):
        text = SOURCE.read_text(encoding="utf-8")
        self.assertNotIn("PF_Cmd_", text)
        self.assertIn('selectors_executed\\\":false', text)
        self.assertIn('render_performed\\\":false', text)

    def test_minihost_has_no_sdk_include(self):
        text = SOURCE.read_text(encoding="utf-8")
        for forbidden in ("AE_Effect", "AEConfig", "Param_Utils", "SPBasic"):
            self.assertNotIn(forbidden, text)

    def test_entrypoint_errors_are_captured_without_load_stage_confusion(self):
        text = SOURCE.read_text(encoding="utf-8")
        for export in ("EffectMain", "EntryPointFunc"):
            call = f'GetProcAddress(module, "{export}")'
            position = text.index(call)
            self.assertIn("SetLastError(ERROR_SUCCESS)", text[position - 100:position])
            self.assertIn("GetLastError()", text[position:position + 150])
        missing = text[text.index('report("entrypoint_missing"') - 300:]
        self.assertIn("true, true, false, error", missing)
        self.assertIn("ERROR_PROC_NOT_FOUND", missing)


if __name__ == "__main__":
    unittest.main()

import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
INSTRUMENTS = ROOT / "instruments"


class InstrumentsB3SourceTests(unittest.TestCase):
    def test_callback_tracer_records_required_surfaces(self):
        source = (INSTRUMENTS / "pf-callback-tracer" / "pf_callback_tracer.cpp").read_text(encoding="utf-8")
        for marker in ("selector_dispatch", "world_descriptor", "callback_invoke", "suite_acquire", "suite_release"):
            self.assertIn(marker, source)
        self.assertIn("PF_Cmd_RENDER", source)

    def test_crashkit_default_is_none_and_faults_require_nondefault_mode(self):
        source = (INSTRUMENTS / "pf-crashkit" / "pf_crashkit.cpp").read_text(encoding="utf-8")
        self.assertIn("kModeNone = 1", source)
        self.assertIn("PF_ADD_POPUP", source)
        self.assertIn("5, kModeNone", source)
        self.assertIn("case kModeNone", source)
        for mode in ("kModeCrash", "kModeHang", "kModeBigAlloc", "kModePfError"):
            self.assertIn(f"case {mode}", source)

    def test_b3_sources_do_not_start_ae_or_use_network(self):
        directories = (INSTRUMENTS / "pf-callback-tracer", INSTRUMENTS / "pf-crashkit")
        text = "\n".join(file.read_text(encoding="utf-8") for directory in directories for file in directory.rglob("*.*"))
        for token in ("CreateProcess", "ShellExecute", "WinHttp", "socket(", "URLDownload"):
            self.assertNotIn(token, text)


if __name__ == "__main__":
    unittest.main()

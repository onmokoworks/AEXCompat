import os
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BROKER_ROOT = ROOT / "broker"
MINIHOST_ROOT = ROOT / "minihost"


class NativeCodeGuardTests(unittest.TestCase):
    def test_broker_sources_exclude_native_loader_symbols_and_plugin_extension(self):
        sources = sorted(BROKER_ROOT.rglob("*.rs"))
        self.assertTrue(sources)
        forbidden = ("LoadLibrary", "GetProcAddress", "." + "aex")
        for path in sources:
            text = path.read_text(encoding="utf-8")
            for token in forbidden:
                with self.subTest(path=path, token=token):
                    self.assertNotIn(token, text)

    def test_broker_has_no_network_or_shell_process_dependencies(self):
        cargo_files = sorted(BROKER_ROOT.rglob("Cargo.toml"))
        combined = "\n".join(path.read_text(encoding="utf-8") for path in cargo_files)
        for dependency in ("reqwest", "hyper", "tokio", "std::net"):
            self.assertNotIn(dependency, combined)
        for path in BROKER_ROOT.rglob("*.rs"):
            text = path.read_text(encoding="utf-8")
            self.assertNotIn("cmd.exe", text)
            self.assertNotIn("powershell", text.lower())

    def test_windows_isolation_markers_cannot_be_removed(self):
        source = (BROKER_ROOT / "crates" / "broker" / "src" / "windows_process.rs").read_text(encoding="utf-8")
        for marker in (
            "JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE",
            "PROC_THREAD_ATTRIBUTE_HANDLE_LIST",
            "CREATE_SUSPENDED",
            "AssignProcessToJobObject",
            "TerminateJobObject",
        ):
            self.assertIn(marker, source)

    @unittest.skipUnless(MINIHOST_ROOT.exists(), "minihost is intentionally absent before Phase D")
    def test_minihost_cleanroom_boundary(self):
        for path in MINIHOST_ROOT.rglob("*"):
            if path.suffix.lower() not in {".c", ".cc", ".cpp", ".h", ".hpp"}:
                continue
            text = path.read_text(encoding="utf-8", errors="replace")
            self.assertNotIn("instruments/", text)
            self.assertNotIn("AE_SDK", text)

    def test_cargo_execution_policy_is_explicit(self):
        value = os.environ.get("AEXCOMPAT_HAS_CARGO")
        if value is not None:
            self.assertIn(value, {"0", "1"})


if __name__ == "__main__":
    unittest.main()

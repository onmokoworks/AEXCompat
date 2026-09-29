import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCES = (
    ROOT / "broker/crates/host-core/src/parameter.rs",
    ROOT / "broker/crates/host-core-ffi/src/lib.rs",
    ROOT / "broker/crates/broker/include/aexcompat_host_core_abi.h",
    ROOT / "broker/crates/broker/include/aexcompat_host_core_adapter.hpp",
    ROOT / "minihost/src/parameter_animation_transport.hpp",
    ROOT / "minihost/src/parameter_animation_transport.cpp",
    ROOT / "tests/native/rust_host_core_parameter_animation_dual_run_selftest.cpp",
    ROOT / "tools/test-rust-host-core-parameter-animation.ps1",
)


class RustHostCorePhase9Tests(unittest.TestCase):
    def test_phase9_sources_are_strict_utf8_without_nul_or_bom(self):
        for path in SOURCES:
            with self.subTest(path=path.name):
                raw = path.read_bytes()
                self.assertFalse(raw.startswith(b"\xef\xbb\xbf"))
                self.assertNotIn(b"\0", raw)
                self.assertEqual(raw.decode("utf-8").encode("utf-8"), raw)

    @unittest.skipUnless(os.name == "nt", "the native dual-run requires Windows MSVC/SEH")
    def test_native_dual_run(self):
        temporary_root = os.environ.get("AEXCOMPAT_NATIVE_TEMP_ROOT")
        with tempfile.TemporaryDirectory(
            prefix="aexcompat-issue636-native-", dir=temporary_root
        ) as temporary:
            result = subprocess.run(
                [
                    "powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass",
                    "-File", str(ROOT / "tools/test-rust-host-core-parameter-animation.ps1"),
                    "-BuildDirectory", temporary,
                ],
                cwd=ROOT,
                capture_output=True,
                timeout=240,
            )
            diagnostic = (
                result.stdout.decode("utf-8", errors="replace") + "\n" +
                result.stderr.decode("utf-8", errors="replace")
            )
            self.assertEqual(result.returncode, 0, diagnostic)
            reports = [
                json.loads(line) for line in diagnostic.splitlines()
                if line.startswith("{")
            ]
            self.assertEqual(len(reports), 1, diagnostic)
            self.assertEqual(
                reports[0]["rust_host_core_parameter_animation_dual_run"],
                "passed",
            )
            self.assertGreaterEqual(reports[0]["checks"], 100)


if __name__ == "__main__":
    unittest.main()

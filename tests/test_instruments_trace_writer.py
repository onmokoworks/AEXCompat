import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

from tools.ae_trace_intake import intake
from tools.trace_contract_validator import validate_event


ROOT = Path(__file__).resolve().parents[1]
INSTRUMENTS = ROOT / "instruments"
SELFTEST = ROOT / "target" / "instruments-build" / "trace_writer_selftest.exe"


class InstrumentsTraceWriterTests(unittest.TestCase):
    def test_sdk_headers_are_confined_to_instrument_plugin_sources(self):
        common = "\n".join(path.read_text(encoding="utf-8") for path in (INSTRUMENTS / "common").rglob("*.*"))
        for header in ("AEConfig.h", "AE_Effect.h", "entry.h"):
            self.assertNotIn(header, common)
        plugin = (INSTRUMENTS / "pf-null-echo" / "pf_null_echo.cpp").read_text(encoding="utf-8")
        self.assertIn("AE_Effect.h", plugin)

    def test_cmake_skips_sdk_plugin_when_sdk_is_absent(self):
        cmake = (INSTRUMENTS / "CMakeLists.txt").read_text(encoding="utf-8")
        self.assertIn("AE_SDK_ROOT", cmake)
        self.assertIn("add_subdirectory(pf-null-echo)", cmake)
        self.assertIn("pf-null-echo build is skipped", cmake)

    @unittest.skipUnless(SELFTEST.exists(), "trace writer selftest executable has not been built")
    def test_selftest_trace_passes_event_contract_and_intake(self):
        with tempfile.TemporaryDirectory() as tmp:
            env = os.environ.copy()
            env["AEX_INSTRUMENT_TRACE_DIR"] = tmp
            completed = subprocess.run([SELFTEST], env=env, capture_output=True, text=True, check=False)
            self.assertEqual(0, completed.returncode, completed.stderr)
            trace = Path(completed.stdout.strip())
            self.assertTrue(trace.is_relative_to(Path(tmp)))
            events = [json.loads(line) for line in trace.read_text(encoding="utf-8").splitlines()]
            self.assertEqual(7, len(events))
            for event in events:
                self.assertEqual([], validate_event(event))
            sanitized, report = intake(events, [], redact=False)
            self.assertTrue(report["accepted"])
            self.assertEqual(events, sanitized)
            self.assertEqual(list(range(7)), [event["event_index"] for event in events])

    @unittest.skipUnless(SELFTEST.exists(), "trace writer selftest executable has not been built")
    def test_unset_trace_directory_produces_no_output(self):
        env = os.environ.copy()
        env.pop("AEX_INSTRUMENT_TRACE_DIR", None)
        completed = subprocess.run([SELFTEST], env=env, capture_output=True, text=True, check=False)
        self.assertEqual(2, completed.returncode)
        self.assertEqual("", completed.stdout)


if __name__ == "__main__":
    unittest.main()

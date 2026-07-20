import ctypes
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
        # The writer consumes a broker-inherited AEX_INSTRUMENT_TRACE_HANDLE (issue
        # #15), not a directory, so drive it through a real inheritable handle and
        # read the events the writer actually produced (issue #245).
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "trace.jsonl"
            kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

            class SecurityAttributes(ctypes.Structure):
                _fields_ = [
                    ("nLength", ctypes.c_ulong),
                    ("lpSecurityDescriptor", ctypes.c_void_p),
                    ("bInheritHandle", ctypes.c_int),
                ]

            security = SecurityAttributes(ctypes.sizeof(SecurityAttributes), None, 1)
            kernel32.CreateFileW.restype = ctypes.c_void_p
            handle = kernel32.CreateFileW(
                str(path), 0x40000000, 0, ctypes.byref(security), 1, 0x80, None
            )
            self.assertNotIn(handle, (None, ctypes.c_void_p(-1).value))
            env = os.environ.copy()
            env.pop("AEX_INSTRUMENT_TRACE_DIR", None)
            env["AEX_INSTRUMENT_TRACE_HANDLE"] = str(handle)
            startup = subprocess.STARTUPINFO()
            startup.lpAttributeList = {"handle_list": [handle]}
            try:
                completed = subprocess.run(
                    [SELFTEST], env=env, capture_output=True, text=True, check=False,
                    close_fds=True, startupinfo=startup,
                )
            finally:
                kernel32.CloseHandle(ctypes.c_void_p(handle))
            self.assertEqual(0, completed.returncode, completed.stderr)
            events = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()]
            self.assertEqual(events[0]["event_kind"], "session_start")
            self.assertEqual(events[-1]["event_kind"], "session_end")
            self.assertEqual([event["event_index"] for event in events], list(range(len(events))))
            for event in events:
                self.assertEqual([], validate_event(event))
            sanitized, report = intake(events, [], redact=False)
            self.assertTrue(report["accepted"])
            self.assertEqual(events, sanitized)

    @unittest.skipUnless(SELFTEST.exists(), "trace writer selftest executable has not been built")
    def test_absent_trace_handle_produces_no_output(self):
        env = os.environ.copy()
        # The writer keys off the handle, so clear both the legacy directory and
        # the handle variable to assert the fully-disabled path (issue #245).
        env.pop("AEX_INSTRUMENT_TRACE_DIR", None)
        env.pop("AEX_INSTRUMENT_TRACE_HANDLE", None)
        completed = subprocess.run([SELFTEST], env=env, capture_output=True, text=True, check=False)
        self.assertEqual(2, completed.returncode)
        self.assertEqual("", completed.stdout)


if __name__ == "__main__":
    unittest.main()

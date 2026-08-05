import json
import ctypes
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

from tools.trace_contract_validator import EVENT_KINDS, validate_event


ROOT = Path(__file__).resolve().parents[1]
MINIHOST = ROOT / "minihost"
TRACE_SELFTEST = ROOT / "target" / "instruments-build" / "trace_writer_selftest.exe"
if not TRACE_SELFTEST.exists():
    TRACE_SELFTEST = next(ROOT.glob("target/**/trace_writer_selftest.exe"), TRACE_SELFTEST)


class ProductionWorkerTraceTests(unittest.TestCase):




    @unittest.skipUnless(TRACE_SELFTEST.exists(), "native trace writer selftest has not been built")
    def test_invalid_inherited_handle_fails_closed(self):
        env = os.environ.copy()
        env.pop("AEX_INSTRUMENT_TRACE_DIR", None)
        env["AEX_INSTRUMENT_TRACE_HANDLE"] = "1"
        completed = subprocess.run(
            [str(TRACE_SELFTEST)], capture_output=True, text=True, env=env, check=False
        )
        self.assertEqual(completed.returncode, 16)

    @unittest.skipUnless(TRACE_SELFTEST.exists(), "native trace writer selftest has not been built")
    def test_native_writer_keeps_bounded_and_ordered_jsonl(self):
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
                    [str(TRACE_SELFTEST)], capture_output=True, text=True, env=env,
                    check=False, close_fds=True, startupinfo=startup,
                )
            finally:
                kernel32.CloseHandle(ctypes.c_void_p(handle))
            self.assertEqual(completed.returncode, 0, completed.stderr)
            events = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()]
            self.assertEqual(events[0]["event_kind"], "session_start")
            self.assertEqual(events[-1]["event_kind"], "session_end")
            self.assertEqual([event["event_index"] for event in events], list(range(len(events))))
            self.assertEqual(sum(event["event_kind"] == "callback_invoke" for event in events), 513)
            self.assertTrue(all(validate_event(event) == [] for event in events))
            self.assertTrue(all("private" not in json.dumps(event) for event in events))


class Issue17VerbosityWiringTests(unittest.TestCase):
    """issue #17: the previously dead TraceWriter API is wired into real worker
    choke points, gated so a default trace stays within the bounded budget."""





    def test_wired_event_kinds_are_already_in_the_trace_contract(self):
        for kind in ("world_descriptor", "callback_invoke", "error", "unimplemented"):
            self.assertIn(kind, EVENT_KINDS)


if __name__ == "__main__":
    unittest.main()

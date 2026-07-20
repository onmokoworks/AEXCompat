import json
import ctypes
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

from tools.trace_contract_validator import EVENT_KINDS, validate_event
import source_owners


ROOT = Path(__file__).resolve().parents[1]
MINIHOST = ROOT / "minihost"
TRACE_SELFTEST = ROOT / "target" / "instruments-build" / "trace_writer_selftest.exe"
if not TRACE_SELFTEST.exists():
    TRACE_SELFTEST = next(ROOT.glob("target/**/trace_writer_selftest.exe"), TRACE_SELFTEST)


class ProductionWorkerTraceTests(unittest.TestCase):
    def test_three_production_workers_link_the_shared_writer(self):
        cmake = (MINIHOST / "CMakeLists.txt").read_text(encoding="utf-8")
        self.assertIn("../instruments/common/trace_writer.cpp", cmake)
        self.assertIn(
            "target_link_libraries(aex_worker_runtime_core PRIVATE aexcompat_trace_writer)",
            cmake,
        )
        for worker in ("aex_l2_worker", "aex_render_worker", "aex_smart_worker"):
            self.assertIn(
                f"target_link_libraries({worker} PRIVATE bcrypt aexcompat_trace_writer)",
                cmake,
            )
        source = source_owners.L2_MAIN.read_text(encoding="utf-8")
        self.assertIn('#include "trace_writer.hpp"', source)
        self.assertEqual(source.count("trace_worker_label()"), 2)

    def test_selector_and_lease_events_are_recorded_at_ordered_boundaries(self):
        source = source_owners.L2_MAIN.read_text(encoding="utf-8")
        dispatch = (MINIHOST / "src" / "worker_selector_dispatch.cpp").read_text(
            encoding="utf-8"
        )
        audited = dispatch[dispatch.index("int32_t audited_effect_call"):
                           dispatch.index("}  // namespace")]
        self.assertLess(
            audited.index("g_selector_trace"), audited.index("entry(command"))
        self.assertIn("g_trace_writer->selector_dispatch(selector)", source)
        registry = (MINIHOST / "src" / "worker_suite_registry.cpp").read_text(
            encoding="utf-8"
        )
        self.assertIn("trace_writer->suite_acquire(safe_name, version, true)", registry)
        self.assertIn(
            "trace_writer->suite_release(safe_name, std::max<int32_t>(version, 0), released)",
            registry,
        )

    def test_writer_contract_is_bounded_and_does_not_emit_private_paths(self):
        writer = (ROOT / "instruments" / "common" / "trace_writer.cpp").read_text(encoding="utf-8")
        self.assertIn("kMaxEvents", writer)
        self.assertIn("kMaxStringBytes", writer)
        self.assertIn("kMaxPayloadBytes", writer)
        self.assertIn("strict_utf8(value)", writer)
        self.assertIn("value[i] == '/'", writer)
        self.assertIn("value[i] == '\\\\' && value[i + 1] == '\\\\'", writer)

    def test_broker_passes_only_a_preopened_trace_handle(self):
        launcher = (ROOT / "broker" / "crates" / "broker" / "src" / "windows_process.rs").read_text(encoding="utf-8")
        writer = (ROOT / "instruments" / "common" / "trace_writer.cpp").read_text(encoding="utf-8")
        self.assertIn("PROC_THREAD_ATTRIBUTE_HANDLE_LIST", launcher)
        self.assertIn("create_trace_file_for_launch", launcher)
        self.assertIn("AEX_INSTRUMENT_TRACE_HANDLE", launcher)
        self.assertNotIn("AEX_INSTRUMENT_TRACE_DIR", writer)
        self.assertNotIn("CreateFileW", writer)

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

    def test_writer_exposes_a_verbose_opt_in_from_the_environment(self):
        header = (ROOT / "instruments" / "common" / "trace_writer.hpp").read_text(encoding="utf-8")
        writer = (ROOT / "instruments" / "common" / "trace_writer.cpp").read_text(encoding="utf-8")
        self.assertIn("bool verbose() const;", header)
        self.assertIn("AEX_INSTRUMENT_TRACE_VERBOSE", writer)
        # The verbose flag never changes what the emit methods write, so the
        # bounded/ordered selftest contract above stays valid.
        self.assertNotIn("verbose_", writer[writer.index("void TraceWriter::write_base"):])

    def test_unknown_suite_emits_an_unimplemented_trace_error(self):
        registry = (ROOT / "minihost" / "src" / "worker_suite_registry.cpp").read_text(encoding="utf-8")
        reject = registry[registry.index("SuiteRegistry::reject_unknown"):]
        reject = reject[: reject.index("return 1;")]
        self.assertIn(
            'trace_writer->error("unimplemented_suite", safe_name, /*unimplemented=*/true)',
            reject,
        )

    def test_world_creation_emits_a_verbose_gated_world_descriptor(self):
        registry = (ROOT / "minihost" / "src" / "worker_world_registry.cpp").read_text(encoding="utf-8")
        new_world = registry[registry.index("int32_t __cdecl new_world("):]
        new_world = new_world[: new_world.index("int32_t __cdecl legacy_new_world(")]
        self.assertIn("g_trace_writer->verbose()", new_world)
        self.assertIn("g_trace_writer->world_descriptor(", new_world)
        # Pixel-format tags are mapped to contract strings, never emitted raw.
        self.assertIn("trace_pixel_format(pixel_format)", new_world)

    def test_handle_callbacks_emit_verbose_gated_callback_events(self):
        handles = (ROOT / "minihost" / "src" / "worker_handle_runtime.cpp").read_text(encoding="utf-8")
        self.assertIn("g_trace_writer->verbose()", handles)
        self.assertIn("g_trace_writer->callback_invoke()", handles)
        # Wired at the existing new_handle / lock_handle callback markers.
        self.assertEqual(handles.count("trace_callback_invoke()"), 3)

    def test_wired_event_kinds_are_already_in_the_trace_contract(self):
        for kind in ("world_descriptor", "callback_invoke", "error", "unimplemented"):
            self.assertIn(kind, EVENT_KINDS)


if __name__ == "__main__":
    unittest.main()

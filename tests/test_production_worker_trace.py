import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

from tools.trace_contract_validator import validate_event


ROOT = Path(__file__).resolve().parents[1]
MINIHOST = ROOT / "minihost"
TRACE_SELFTEST = ROOT / "target" / "instruments-build" / "trace_writer_selftest.exe"
if not TRACE_SELFTEST.exists():
    TRACE_SELFTEST = next(ROOT.glob("target/**/trace_writer_selftest.exe"), TRACE_SELFTEST)


class ProductionWorkerTraceTests(unittest.TestCase):
    def test_three_production_workers_link_the_shared_writer(self):
        cmake = (MINIHOST / "CMakeLists.txt").read_text(encoding="utf-8")
        self.assertIn("../instruments/common/trace_writer.cpp", cmake)
        self.assertEqual(cmake.count("aexcompat_trace_writer)"), 3)
        source = (MINIHOST / "src" / "l2_main.cpp").read_text(encoding="utf-8")
        self.assertIn('#include "trace_writer.hpp"', source)
        self.assertEqual(source.count("trace_worker_label()"), 2)

    def test_selector_and_lease_events_are_recorded_at_ordered_boundaries(self):
        source = (MINIHOST / "src" / "l2_main.cpp").read_text(encoding="utf-8")
        audited = source[source.index("int32_t audited_effect_call"):
                         source.index("int32_t invoke_entry_seh")]
        self.assertLess(
            audited.index("selector_dispatch"), audited.index("entry(command"))
        acquire = source[source.index("void record_suite_acquire"):
                         source.index("void record_missing_suite")]
        self.assertIn("suite_acquire(name, version, true)", acquire)
        release = source[source.index("int32_t __cdecl release_suite"):
                         source.index("bool verify_suite_release_without_acquire_rejected")]
        self.assertIn("suite_release(name, std::max<int32_t>(version, 0), released)", release)

    def test_writer_contract_is_bounded_and_does_not_emit_private_paths(self):
        writer = (ROOT / "instruments" / "common" / "trace_writer.cpp").read_text(encoding="utf-8")
        self.assertIn("kMaxEvents", writer)
        self.assertIn("kMaxStringBytes", writer)
        self.assertIn("kMaxPayloadBytes", writer)
        self.assertIn("value.front() == '/'", writer)
        self.assertIn("value[i + 2] == '\\\\'", writer)

    @unittest.skipUnless(TRACE_SELFTEST.exists(), "native trace writer selftest has not been built")
    def test_native_writer_keeps_bounded_and_ordered_jsonl(self):
        with tempfile.TemporaryDirectory() as tmp:
            env = os.environ.copy()
            env["AEX_INSTRUMENT_TRACE_DIR"] = tmp
            completed = subprocess.run(
                [str(TRACE_SELFTEST)], capture_output=True, text=True, env=env, check=False
            )
            self.assertEqual(completed.returncode, 0, completed.stderr)
            path = Path(completed.stdout.strip())
            events = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()]
            self.assertEqual([event["event_kind"] for event in events], [
                "session_start", "selector_dispatch", "suite_acquire", "world_descriptor",
                "callback_invoke", "suite_release", "session_end",
            ])
            self.assertTrue(all(validate_event(event) == [] for event in events))
            self.assertTrue(all("private" not in json.dumps(event) for event in events))


if __name__ == "__main__":
    unittest.main()

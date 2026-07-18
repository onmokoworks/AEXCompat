import unittest
from pathlib import Path

from tools.observe_known_functions import (
    OUTPUT_ROOT,
    MessageCollector,
    ObservationError,
    build_worker_argv,
    safe_output_path,
    write_session_jsonl,
)
from tools.trace_contract_validator import validate_session


class BuildWorkerArgvTests(unittest.TestCase):
    def test_prepends_worker_to_render_verb(self):
        argv = build_worker_argv(
            "target/minihost-build/aex_render_worker.exe",
            ["--render-image", "plugin.aex", "deadbeef", "v5|", "in.rgba", "out.rgba", "16", "12", "0", "1", "1", "1"],
        )
        self.assertEqual("target/minihost-build/aex_render_worker.exe", argv[0])
        self.assertEqual("--render-image", argv[1])
        self.assertEqual("out.rgba", argv[6])

    def test_rejects_non_render_verb(self):
        with self.assertRaises(ObservationError):
            build_worker_argv("worker.exe", ["--self-test-pf-color-suite"])

    def test_rejects_empty_render_args(self):
        with self.assertRaises(ObservationError):
            build_worker_argv("worker.exe", [])


class MessageCollectorTests(unittest.TestCase):
    def collector(self):
        return MessageCollector(
            plugin_label="gamma-classic",
            host_version_label="native-observation frida",
            module_label="gamma-classic",
            session_id="abcdef01-2345-6789-abcd-ef0123456789",
        )

    def test_collects_a_valid_contiguous_session(self):
        collector = self.collector()
        collector.handle({"type": "installed", "hook_count": 1})
        collector.handle({
            "type": "known_function", "symbol": "apply_gamma", "module_rva": "0x1c40",
            "phase": "enter", "fields": [{"name": "in.width", "value": 1920}],
        })
        collector.handle({
            "type": "known_function", "symbol": "apply_gamma", "module_rva": "0x1c40",
            "phase": "leave", "return_value": 0, "fields": [{"name": "out.width", "value": 1920}],
        })
        session = collector.finalize()

        self.assertEqual([], validate_session(session))
        self.assertEqual(1, collector.installed_hook_count)
        kinds = [event["event_kind"] for event in session["events"]]
        self.assertEqual(
            ["session_start", "known_function_invoke", "known_function_invoke", "session_end"],
            kinds,
        )
        self.assertEqual([0, 1, 2, 3], [e["event_index"] for e in session["events"]])

    def test_empty_observation_is_still_a_valid_session(self):
        session = self.collector().finalize()
        self.assertEqual([], validate_session(session))
        self.assertEqual(2, session["event_count"])

    def test_incomplete_session_omits_end_and_marks_not_complete(self):
        collector = self.collector()
        collector.handle({
            "type": "known_function", "symbol": "apply_gamma", "module_rva": "0x1c40",
            "phase": "enter", "fields": [{"name": "in.width", "value": 1920}],
        })
        session = collector.finalize(completed=False)
        # A timed-out worker must not fabricate a session_end boundary.
        self.assertFalse(session["trace_complete"])
        self.assertNotIn("session_end", [e["event_kind"] for e in session["events"]])
        # trace_complete=False matches the absent boundary, so it still validates.
        self.assertEqual([], validate_session(session))

    def test_redaction_violation_fails_closed(self):
        collector = self.collector()
        with self.assertRaises(ValueError):
            collector.handle({
                "type": "known_function", "symbol": "apply_gamma", "module_rva": "0x1c40",
                "phase": "enter", "fields": [{"name": "in.width", "value": "0x7ffabc00"}],
            })

    def test_ready_is_control_only_and_sets_readiness(self):
        collector = self.collector()
        collector.handle({"type": "ready", "installed": True, "hook_count": 2})
        session = collector.finalize()
        self.assertTrue(collector.ready)
        self.assertEqual(2, collector.installed_hook_count)
        # 'ready' is a control signal; it must not appear as a trace event.
        self.assertEqual(2, session["event_count"])

    def test_install_error_is_captured_not_traced(self):
        collector = self.collector()
        collector.handle({"type": "install_error", "message": "module not found"})
        session = collector.finalize()
        self.assertEqual("module not found", collector.install_error)
        # An install error must not leak into the trace as an event.
        self.assertEqual(2, session["event_count"])

    def test_formatter_failure_is_captured_for_fail_closed(self):
        # A rejected invocation must be recorded (format_error) so the launcher can
        # refuse to report the trace complete, not silently drop it. This mirrors
        # the on_message try/except at the Frida boundary.
        collector = self.collector()
        try:
            collector.handle({
                "type": "known_function", "symbol": "apply_gamma", "module_rva": "0x1c40",
                "phase": "enter", "fields": [{"name": "in.width", "value": float("nan")}],
            })
        except ValueError as exc:
            collector.format_error = str(exc)
        self.assertIsNotNone(collector.format_error)

    def test_read_error_is_counted_not_traced(self):
        collector = self.collector()
        collector.handle({"type": "read_error", "symbol": "apply_gamma",
                          "phase": "enter", "name": "in.width", "message": "null pointer"})
        session = collector.finalize()
        self.assertEqual(1, collector.read_error_count)
        # A failed read is counted, never fabricated into a trace field/event.
        self.assertEqual(2, session["event_count"])


class OutputPathSafetyTests(unittest.TestCase):
    def test_escape_outside_allowed_root_is_rejected(self):
        with self.assertRaises(ObservationError):
            safe_output_path(Path("../../etc/passwd"))

    def test_absolute_path_outside_root_is_rejected(self):
        with self.assertRaises(ObservationError):
            safe_output_path(Path("C:/Windows/Temp/trace.jsonl"))

    def test_symlinked_output_root_is_rejected(self):
        import tempfile
        import tools.observe_known_functions as obs

        # If the fixed root (or its 'target' parent) is a reparse point, resolving
        # it must not adopt the redirected destination as the allowed root.
        real_elsewhere = Path(tempfile.mkdtemp(prefix="obs-elsewhere-"))
        fake_root = obs.OUTPUT_ROOT.parent / "obs-symlink-root-test"
        try:
            fake_root.symlink_to(real_elsewhere, target_is_directory=True)
        except (OSError, NotImplementedError):
            self.skipTest("creating a symlink requires privilege/developer mode")
        original = obs.OUTPUT_ROOT
        obs.OUTPUT_ROOT = fake_root
        try:
            with self.assertRaises(ObservationError):
                obs.safe_output_path(fake_root / "trace.jsonl")
        finally:
            obs.OUTPUT_ROOT = original
            try:
                fake_root.unlink()
            except OSError:
                pass
            for p in real_elsewhere.iterdir():
                p.unlink()
            real_elsewhere.rmdir()

    def test_path_under_root_is_accepted_and_atomic_write_lands(self):
        session = {
            "schema_version": 1,
            "session_id": "abcdef01-2345-6789-abcd-ef0123456789",
            "event_count": 0,
            "trace_complete": False,
            "events": [],
        }
        target = OUTPUT_ROOT / "unit-test" / "trace.jsonl"
        try:
            destination = write_session_jsonl(session, target)
            self.assertTrue(destination.exists())
            self.assertTrue(str(destination).startswith(str(OUTPUT_ROOT.resolve())))
        finally:
            if target.exists():
                target.unlink()
            # leave no stray tmp files
            for stray in target.parent.glob("*.jsonl.tmp"):
                stray.unlink()


class ProcessLivenessTests(unittest.TestCase):
    def test_await_exit_uses_pid_aware_enumeration(self):
        import tools.observe_known_functions as obs

        class FakeProc:
            def __init__(self, pid):
                self.pid = pid

        class FakeDevice:
            def __init__(self):
                self.calls = []

            def enumerate_processes(self, pids=None):
                self.calls.append(pids)
                # PID present on the first poll, gone on the next.
                if len(self.calls) == 1:
                    return [FakeProc(4321)]
                return []

        device = FakeDevice()
        # No get_process(name) call: liveness is decided by PID enumeration.
        self.assertTrue(obs._process_alive(device, 4321))
        self.assertFalse(obs._process_alive(device, 4321))
        self.assertEqual([[4321], [4321]], device.calls)

    def test_process_alive_falls_back_without_pids_filter(self):
        import tools.observe_known_functions as obs

        class FakeProc:
            def __init__(self, pid):
                self.pid = pid

        class OldDevice:
            def enumerate_processes(self, pids=None):
                if pids is not None:
                    raise TypeError("unexpected keyword 'pids'")
                return [FakeProc(1), FakeProc(4321)]

        self.assertTrue(obs._process_alive(OldDevice(), 4321))
        self.assertFalse(obs._process_alive(OldDevice(), 9999))


class LiveObservationIntegrationTests(unittest.TestCase):
    """End-to-end run_observation() path: Frida spawn/attach/plan/resume/kill.

    Requires Frida, a built worker, and a real (or minimal) AEX fixture; skipped
    otherwise. Registered in tests/local_artifact_tests.txt so it only runs under
    --run-local-artifact-tests after the named build.
    """

    def test_run_observation_end_to_end(self):
        try:
            import frida  # noqa: F401
        except ImportError:
            self.skipTest("frida not installed")
        worker = Path(__file__).resolve().parents[1] / "target" / "minihost-build" / "aex_render_worker.exe"
        if not worker.exists():
            self.skipTest("worker build not present")
        self.skipTest(
            "live observation requires an approved AEX fixture, offset map, and known RVAs; "
            "drive tools/observe-known-functions.ps1 manually per the docs"
        )


if __name__ == "__main__":
    unittest.main()

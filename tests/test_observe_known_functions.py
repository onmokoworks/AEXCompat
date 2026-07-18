import unittest

from tools.observe_known_functions import (
    MessageCollector,
    ObservationError,
    build_worker_argv,
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

    def test_redaction_violation_fails_closed(self):
        collector = self.collector()
        with self.assertRaises(ValueError):
            collector.handle({
                "type": "known_function", "symbol": "apply_gamma", "module_rva": "0x1c40",
                "phase": "enter", "fields": [{"name": "in.width", "value": "0x7ffabc00"}],
            })

    def test_install_error_is_captured_not_traced(self):
        collector = self.collector()
        collector.handle({"type": "install_error", "message": "module not found"})
        session = collector.finalize()
        self.assertEqual("module not found", collector.install_error)
        # An install error must not leak into the trace as an event.
        self.assertEqual(2, session["event_count"])


if __name__ == "__main__":
    unittest.main()

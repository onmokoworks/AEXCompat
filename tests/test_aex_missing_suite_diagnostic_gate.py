import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SUMMARY = ROOT / "analysis" / "AEX_MISSING_SUITE_DIAGNOSTIC_SUMMARY_2026-07-18.json"
SPEC = importlib.util.spec_from_file_location("diagnostic_gate", ROOT / "tools" / "aex_missing_suite_diagnostic_gate.py")
gate = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gate)


class MissingSuiteDiagnosticGateTests(unittest.TestCase):
    def test_extracts_only_bounded_valid_unique_suites(self):
        stderr = 'failed: diagnostics={"missing_suites":[{"name":"PF World Suite","version":2},{"name":"PF World Suite","version":2},{"name":"C:\\\\private","version":1},{"name":"Bad","version":0}]}, report='
        self.assertEqual(gate.missing_suites(stderr), [{"name": "PF World Suite", "version": 2}])

    def test_aggregate_ranks_sha_coverage_before_event_count(self):
        suite_a = [{"name": "Suite A", "version": 1}]
        suite_b = [{"name": "Suite B", "version": 2}]
        rows = gate.aggregate([("a" * 64, suite_a), ("b" * 64, suite_a), ("c" * 64, suite_b), ("c" * 64, suite_b)])
        self.assertEqual(rows[0], {"name": "Suite A", "version": 1, "sha_count": 2, "event_count": 2})
        self.assertEqual(rows[1]["event_count"], 2)

    def test_extracts_only_bounded_valid_unique_unsupported_calls(self):
        stderr = (
            'failed: diagnostics={"unsupported_suite_calls":['
            '{"name":"AEGP Comp Suite","version":21,"slot":7,"call_count":2},'
            '{"name":"AEGP Comp Suite","version":21,"slot":7,"call_count":9},'
            '{"name":"C:\\\\private","version":1,"slot":1,"call_count":1},'
            '{"name":"Bad","version":1,"slot":2048,"call_count":1}]}'
        )
        self.assertEqual(gate.unsupported_suite_calls(stderr), [{
            "name": "AEGP Comp Suite", "version": 21, "slot": 7, "call_count": 2,
        }])
        self.assertEqual(
            gate.unsupported_suite_calls(
                'failed: diagnostics={"unsupported_suite_calls":null}'
            ),
            [],
        )

    def test_unsupported_call_aggregate_ranks_sha_then_event_then_call_count(self):
        slot_a = [{
            "name": "Suite A", "version": 1, "slot": 3, "call_count": 2,
        }]
        slot_b = [{
            "name": "Suite B", "version": 2, "slot": 4, "call_count": 20,
        }]
        rows = gate.aggregate_unsupported_calls([
            ("a" * 64, slot_a), ("b" * 64, slot_a), ("c" * 64, slot_b),
        ])
        self.assertEqual(rows[0], {
            "name": "Suite A", "version": 1, "slot": 3,
            "sha_count": 2, "event_count": 2, "call_count": 4,
        })
        self.assertEqual(rows[1]["call_count"], 20)

    def test_persisted_event_has_no_path_or_private_stderr(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            failure = {"kind": "missing_suite", "process_exit_code": 1, "worker_exit_code": 20,
                       "failure_stage": "global_setup", "selector_error": 4, "plugin_kind": None}
            path = gate.persist_event(root, "a" * 64, 123, [{"name": "PF World Suite", "version": 2}], failure, "test")
            value = json.loads(path.read_text(encoding="utf-8"))
            text = json.dumps(value)
            self.assertNotIn("path", text.lower())
            self.assertNotIn("stderr", text.lower())
            self.assertEqual(value["identity"], {"sha256": "a" * 64, "size": 123})

    def test_classifies_selector_error_without_exporting_stderr(self):
        stderr = ('AEX parameter inspection worker failed safely: '
                  '{"classification":"nonzero_exit","exit_code":14,"failure_stage":"global_setdown",'
                  '"first_failure_stage":"global_setup",'
                  '"stage_events":[{"stage":"global_setup","state":"end","errors":{"error":512}}]}')
        self.assertEqual(gate.classify_failure(stderr, 1), {
            "kind": "selector_error", "process_exit_code": 1, "worker_exit_code": 14,
            "failure_stage": "global_setup", "selector_error": 512, "plugin_kind": None,
        })

    def test_classifies_structured_unsupported_suite_call(self):
        stderr = (
            'AEX parameter inspection worker failed safely: '
            '{"classification":"nonzero_exit","exit_code":14,'
            '"unsupported_suite_calls":[{"name":"AEGP Comp Suite",'
            '"version":21,"slot":7,"call_count":1}]}'
        )
        self.assertEqual(
            gate.classify_failure(stderr, 1)["kind"], "unsupported_suite_call"
        )

    def test_classifies_non_effect_entrypoint(self):
        stderr = ('AEX parameter inspection worker failed safely: '
                  '{"classification":"nonzero_exit","exit_code":12,"failure_stage":null,'
                  '"stage_events":[],"plugin_kind":"aegp_candidate"}')
        self.assertEqual(gate.classify_failure(stderr, 1)["kind"],
                         "unsupported_plugin_kind_for_pf_inspect")

    def test_classifies_invalid_pipl(self):
        stderr = ('AEX parameter inspection worker failed safely: '
                  '{"classification":"nonzero_exit","exit_code":12,"failure_stage":null,'
                  '"stage_events":[],"plugin_kind":"invalid_pipl"}')
        result = gate.classify_failure(stderr, 1)
        self.assertEqual(result["kind"], "invalid_pipl_for_pf_inspect")
        self.assertEqual(result["plugin_kind"], "invalid_pipl")

    def test_recorded_sdk_sweep_separates_effect_failures_from_aegp(self):
        summary = json.loads(SUMMARY.read_text(encoding="utf-8"))
        self.assertEqual((summary["fixture_count"], summary["effect_fixture_count"]), (23, 22))
        self.assertEqual((summary["inspect_success_count"], summary["effect_inspect_failure_count"]), (21, 1))
        self.assertEqual(summary["out_of_scope_plugin_count"], 1)
        failed = {case["fixture"]: case["failure"] for case in summary["cases"] if case["failure"]}
        self.assertEqual(failed["GLator"]["failure_stage"], "global_setup")
        self.assertEqual(failed["Grabba"]["plugin_kind"], "aegp_candidate")
        self.assertNotIn(":\\", SUMMARY.read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()

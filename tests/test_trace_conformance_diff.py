import contextlib
import copy
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools import trace_conformance_diff
from tools.trace_normalizer import normalize, read_jsonl


ROOT = Path(__file__).resolve().parents[1]
SYNTHETIC = ROOT / "contracts" / "trace" / "examples" / "synthetic_session.jsonl"
RULES = ROOT / "contracts" / "trace" / "conformance_rules.json"


class TraceConformanceDiffTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.events = normalize(read_jsonl(SYNTHETIC))["events"]
        cls.rules = trace_conformance_diff.load_rules(RULES)

    def test_identical_traces_are_conformant_and_deterministic(self):
        first = trace_conformance_diff.compare(self.events, copy.deepcopy(self.events), self.rules)
        second = trace_conformance_diff.compare(self.events, copy.deepcopy(self.events), self.rules)
        self.assertEqual(first, second)
        self.assertEqual("conformant", first["conformance_state"])
        self.assertEqual([], first["must_match_failures"])

    def test_selector_order_change_is_nonconformant(self):
        candidate = copy.deepcopy(self.events)
        selector = next(event for event in candidate if event["event_kind"] == "selector_dispatch")
        extra = copy.deepcopy(selector)
        extra["selector"] = "PF_Cmd_PARAMS_SETUP"
        reference = copy.deepcopy(self.events)
        reference.insert(2, extra)
        candidate.insert(1, extra)
        report = trace_conformance_diff.compare(reference, candidate, self.rules)
        self.assertEqual("nonconformant", report["conformance_state"])
        self.assertTrue(report["must_match_failures"])

    def test_missing_suite_is_nonconformant(self):
        candidate = [event for event in self.events if event["event_kind"] != "suite_acquire"]
        report = trace_conformance_diff.compare(self.events, candidate, self.rules)
        self.assertEqual("nonconformant", report["conformance_state"])

    def test_world_difference_is_should_match_only(self):
        candidate = copy.deepcopy(self.events)
        next(event for event in candidate if event["event_kind"] == "world_descriptor")["world"]["width"] += 1
        report = trace_conformance_diff.compare(self.events, candidate, self.rules)
        self.assertEqual("conformant", report["conformance_state"])
        self.assertTrue(report["should_match_mismatches"])

    def test_missing_rules_fail_closed(self):
        with self.assertRaises(FileNotFoundError):
            trace_conformance_diff.load_rules(ROOT / "contracts" / "trace" / "missing.json")

    def test_rules_outside_contract_root_are_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "rules.json"
            path.write_text(RULES.read_text(encoding="utf-8"), encoding="utf-8")
            with self.assertRaises(ValueError):
                trace_conformance_diff.load_rules(path)

    def test_cli_writes_new_conformant_report(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            reference = root / "reference.json"
            candidate = root / "candidate.json"
            rules = root / "rules.json"
            destination = root / "out" / "report.json"
            payload = {"schema_version": 1, "report_kind": "normalized_host_trace", "events": self.events}
            for path in (reference, candidate):
                path.write_text(json.dumps(payload), encoding="utf-8")
            rules.write_text(RULES.read_text(encoding="utf-8"), encoding="utf-8")
            with (
                mock.patch.object(trace_conformance_diff, "RULES_ROOT", root),
                mock.patch.object(trace_conformance_diff, "OUTPUT_ROOT", root / "out"),
                contextlib.redirect_stdout(io.StringIO()),
            ):
                code = trace_conformance_diff.main(["--reference", str(reference), "--candidate", str(candidate), "--rules", str(rules), "--out", str(destination)])
            self.assertEqual(0, code)
            self.assertEqual("conformant", json.loads(destination.read_text(encoding="utf-8"))["conformance_state"])

    def test_cli_preserves_competing_report_after_preflight(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            reference = root / "reference.json"
            candidate = root / "candidate.json"
            rules = root / "rules.json"
            destination = root / "out" / "report.json"
            payload = {"schema_version": 1, "report_kind": "normalized_host_trace", "events": self.events}
            for path in (reference, candidate):
                path.write_text(json.dumps(payload), encoding="utf-8")
            rules.write_text(RULES.read_text(encoding="utf-8"), encoding="utf-8")
            original_compare = trace_conformance_diff.compare

            def create_competing_report(*args):
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_text("sentinel", encoding="utf-8")
                return original_compare(*args)

            with (
                mock.patch.object(trace_conformance_diff, "RULES_ROOT", root),
                mock.patch.object(trace_conformance_diff, "OUTPUT_ROOT", root / "out"),
                mock.patch.object(trace_conformance_diff, "compare", side_effect=create_competing_report),
                contextlib.redirect_stdout(io.StringIO()),
                contextlib.redirect_stderr(io.StringIO()),
            ):
                code = trace_conformance_diff.main(["--reference", str(reference), "--candidate", str(candidate), "--rules", str(rules), "--out", str(destination)])
            self.assertEqual(2, code)
            self.assertEqual("sentinel", destination.read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()

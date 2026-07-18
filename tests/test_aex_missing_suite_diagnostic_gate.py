import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
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

    def test_persisted_event_has_no_path_or_private_stderr(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = gate.persist_event(root, "a" * 64, 123, [{"name": "PF World Suite", "version": 2}], "test")
            value = json.loads(path.read_text(encoding="utf-8"))
            text = json.dumps(value)
            self.assertNotIn("path", text.lower())
            self.assertNotIn("stderr", text.lower())
            self.assertEqual(value["identity"], {"sha256": "a" * 64, "size": 123})


if __name__ == "__main__":
    unittest.main()

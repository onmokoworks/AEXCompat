import json
import unittest
from pathlib import Path

import jsonschema


ROOT = Path(__file__).resolve().parents[1]


class L1ResultTests(unittest.TestCase):
    def test_checked_in_result_record_is_l1_only(self):
        text = (ROOT / "analysis" / "SCATTERMAP_L1_RESULT_2026-07-13.md").read_text(encoding="utf-8")
        self.assertIn("loaded_and_unloaded", text)
        self.assertIn("selectors executed: false", text)
        self.assertIn("render performed: false", text)
        self.assertIn("not initialization", text)

    def test_worker_report_schema_keeps_l1_non_dispatching(self):
        schema = json.loads((ROOT / "contracts" / "aex" / "l1_worker_report.schema.json").read_text(encoding="utf-8"))
        properties = schema["properties"]
        self.assertEqual(properties["stage"]["const"], "L1")
        self.assertFalse(properties["selectors_executed"]["const"])
        self.assertFalse(properties["render_performed"]["const"])

    def test_worker_report_status_invariants_are_table_driven(self):
        schema = json.loads((ROOT / "contracts" / "aex" / "l1_worker_report.schema.json").read_text(encoding="utf-8"))
        cases = [
            ("invalid_request", False, False, False, 0),
            ("identity_read_failed", False, False, False, 0),
            ("identity_mismatch", False, False, False, 0),
            ("dll_policy_failed", True, False, False, 5),
            ("load_failed", True, False, False, 126),
            ("load_failed", True, False, False, 127),
            ("load_failed", True, False, False, 193),
            ("entrypoint_missing", True, True, False, 127),
            ("loaded_and_unloaded", True, True, True, 0),
        ]
        for status, identity, loaded, entrypoint, error in cases:
            report = {"schema_version": 1, "stage": "L1", "status": status,
                "identity_verified": identity, "module_loaded": loaded,
                "entrypoint_resolved": entrypoint, "selectors_executed": False,
                "render_performed": False, "win32_error": error}
            jsonschema.validate(report, schema)

            wrong_stage = dict(report, module_loaded=not loaded)
            with self.assertRaises(jsonschema.ValidationError, msg=status):
                jsonschema.validate(wrong_stage, schema)


if __name__ == "__main__":
    unittest.main()

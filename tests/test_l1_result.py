import json
import unittest
from pathlib import Path


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


if __name__ == "__main__":
    unittest.main()

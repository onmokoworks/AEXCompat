import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class SmartFxSuiteFaultContractTests(unittest.TestCase):
    def test_report_requires_double_run_and_broker_survival(self):
        schema = json.loads(
            (ROOT / "contracts/aex/smartfx_suite_fault_report.schema.json").read_text(
                encoding="utf-8"
            )
        )
        self.assertFalse(schema["additionalProperties"])
        required = set(schema["required"])
        for field in (
            "fault_id",
            "expected_outcome",
            "run_1",
            "run_2",
            "broker_survived",
            "passed",
        ):
            self.assertIn(field, required)
        self.assertTrue(schema["properties"]["broker_survived"]["const"])



if __name__ == "__main__":
    unittest.main()

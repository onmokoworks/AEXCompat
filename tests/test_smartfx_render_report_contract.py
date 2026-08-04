import copy
import json
import re
import unittest
from pathlib import Path

from jsonschema import Draft202012Validator


ROOT = Path(__file__).resolve().parents[1]
SCHEMA_PATH = ROOT / "contracts/aex/smartfx_render_report.schema.json"
SMART_SOURCE = ROOT / "broker/crates/broker/src/smart.rs"


def _run(classification, output_sha256, smart_render_error, guard_bytes_intact, rect):
    return {
        "classification": classification,
        "output_sha256": output_sha256,
        "smart_render_error": smart_render_error,
        "guard_bytes_intact": guard_bytes_intact,
        "input_checkout_request": rect,
        "map_checkout_request": rect,
    }


def _report(case_id, run, *, expected_oracle_sha256, expected_error=False, expected_crash=False,
            passed=True):
    return {
        "schema_version": 1,
        "stage": "smartfx_render",
        "plugin_id": "scattermap",
        "receipt_id": "receipt-smartfx-1",
        "fixture_sha256": "A" * 64,
        "case_id": case_id,
        "expected_oracle_sha256": expected_oracle_sha256,
        "run_1": copy.deepcopy(run),
        "run_2": copy.deepcopy(run),
        "deterministic": True,
        "oracle_match": True,
        "gpu_negotiation_valid": True,
        "expected_error": expected_error,
        "error_contract_valid": True,
        "temporal_context_valid": True,
        "roi_contract_valid": True,
        "expected_crash": expected_crash,
        "crash_contract_valid": True,
        "broker_survived": True,
        "passed": passed,
    }


class SmartfxRenderReportContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
        Draft202012Validator.check_schema(cls.schema)

    def assert_valid(self, report):
        errors = sorted(Draft202012Validator(self.schema).iter_errors(report), key=str)
        self.assertEqual([], errors, errors)


    def test_success_report_validates(self):
        report = _report(
            "default",
            _run("ok", "B" * 64, 0, True, [0, 0, 64, 64]),
            expected_oracle_sha256="C" * 64,
        )
        self.assert_valid(report)

    def test_expected_error_report_validates_without_being_successful_native_render(self):
        report = _report(
            "error_missing_input",
            _run("nonzero_exit", "C" * 64, 516, True, [-1, -1, -1, -1]),
            expected_oracle_sha256="C" * 64,
            expected_error=True,
        )
        self.assert_valid(report)

    def test_expected_crash_report_validates_with_missing_worker_payload(self):
        report = _report(
            "crash_null_output_world",
            _run("crashed", "", None, None, None),
            expected_oracle_sha256="",
            expected_crash=True,
        )
        self.assert_valid(report)

    def test_invalid_report_cannot_claim_passed(self):
        report = _report(
            "default",
            _run("ok", "B" * 64, 0, True, [0, 0, 64, 64]),
            expected_oracle_sha256="C" * 64,
        )
        report["oracle_match"] = False
        self.assertTrue(list(Draft202012Validator(self.schema).iter_errors(report)))

        report = _report(
            "default",
            _run("ok", "B" * 64, 0, True, [0, 0, 64, 64]),
            expected_oracle_sha256="C" * 64,
        )
        report["unexpected"] = True
        self.assertTrue(list(Draft202012Validator(self.schema).iter_errors(report)))


if __name__ == "__main__":
    unittest.main()

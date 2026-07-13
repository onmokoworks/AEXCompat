import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from tools import contract_schema_validator


ROOT = Path(__file__).resolve().parents[1]


class ContractSchemaValidatorTests(unittest.TestCase):
    def test_promoted_contracts_validate_cleanly(self):
        issues = []
        for path in (ROOT / "contracts").rglob("*.json"):
            issues.extend(contract_schema_validator.validate_contract(path))
        self.assertEqual([], issues)

    def test_rejects_absolute_path_leaks(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "bad.json"
            path.write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "plugin_path": "D:\\Private\\Plugin.aex",
                    }
                ),
                encoding="utf-8",
            )
            issues = contract_schema_validator.validate_contract(path)
        self.assertTrue(any("absolute path" in issue for issue in issues))

    def test_rejects_payload_bearing_keys(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "bad.json"
            path.write_text(
                json.dumps({"schema_version": 1, "raw_payload": "abc"}),
                encoding="utf-8",
            )
            issues = contract_schema_validator.validate_contract(path)
        self.assertTrue(any("payload-bearing" in issue for issue in issues))

    def test_warning_mode_reports_issues_without_failing(self):
        result, report = self._run_cli(strict=False)
        self.assertEqual(0, result.returncode)
        self.assertFalse(report["strict_mode"])
        self.assertEqual("warning", report["mode"])
        self.assertEqual(1, report["issue_count"])

    def test_strict_mode_promotes_issues_to_failure(self):
        result, report = self._run_cli(strict=True)
        self.assertNotEqual(0, result.returncode)
        self.assertTrue(report["strict_mode"])
        self.assertEqual("strict", report["mode"])
        self.assertEqual(1, report["issue_count"])

    def _run_cli(self, *, strict: bool):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "bad.json"
            path.write_text(json.dumps({"unsafe": True}), encoding="utf-8")
            command = [sys.executable, str(ROOT / "tools" / "contract_schema_validator.py")]
            if strict:
                command.append("--strict")
            command.append(str(path))
            result = subprocess.run(command, capture_output=True, text=True, check=False)
        return result, json.loads(result.stdout)


if __name__ == "__main__":
    unittest.main()

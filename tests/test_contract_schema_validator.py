import json
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


if __name__ == "__main__":
    unittest.main()


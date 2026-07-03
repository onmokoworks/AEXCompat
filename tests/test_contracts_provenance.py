import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CONTRACTS = ROOT / "contracts" / "aex"
PROVENANCE = ROOT / "contracts" / "PROVENANCE.md"


class ContractProvenanceTests(unittest.TestCase):
    def test_promoted_aex_contracts_are_valid_json_objects(self):
        expected = [
            "image_probe_request.schema.json",
            "worker_capability_report.schema.json",
            "loader_readiness_gate.schema.json",
            "image_probe_allowlist.example.json",
        ]
        for name in expected:
            with self.subTest(name=name):
                data = json.loads((CONTRACTS / name).read_text(encoding="utf-8"))
                self.assertIsInstance(data, dict)
                self.assertTrue(
                    "schema_version" in data or "schema_name" in data,
                    f"{name} should declare schema metadata",
                )

    def test_provenance_maps_every_promoted_contract(self):
        text = PROVENANCE.read_text(encoding="utf-8")
        for name in [
            "image_probe_request.schema.json",
            "worker_capability_report.schema.json",
            "loader_readiness_gate.schema.json",
            "image_probe_allowlist.example.json",
        ]:
            with self.subTest(name=name):
                self.assertIn(name, text)


if __name__ == "__main__":
    unittest.main()


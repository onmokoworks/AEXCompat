import json
import unittest
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]


class ClassicRenderContractTests(unittest.TestCase):



    def test_contract_requires_two_hash_bound_runs_and_guards(self):
        schema = json.loads((ROOT / "contracts" / "aex" / "classic_render_report.schema.json").read_text(encoding="utf-8"))
        required = set(schema["required"])
        for key in ("input_sha256", "run_1", "run_2", "deterministic",
                    "guard_bytes_intact", "broker_survived"):
            self.assertIn(key, required)
        self.assertEqual(schema["properties"]["pixel_format"]["const"], "argb8")

    def test_contract_bounds_dimensions_and_runtime(self):
        schema = json.loads((ROOT / "contracts" / "aex" / "classic_render_report.schema.json").read_text(encoding="utf-8"))
        self.assertEqual(schema["properties"]["width"]["maximum"], 1024)
        self.assertEqual(schema["properties"]["height"]["maximum"], 1024)
        self.assertEqual(schema["$defs"]["run"]["properties"]["elapsed_ms"]["maximum"], 30000)


if __name__ == "__main__":
    unittest.main()

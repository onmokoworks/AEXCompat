import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class RenderParameterGateContractTests(unittest.TestCase):
    def test_request_is_strict_and_caller_cannot_supply_descriptors(self):
        schema = json.loads((ROOT / "contracts/aex/render_parameter_request.schema.json").read_text(encoding="utf-8"))
        self.assertFalse(schema["additionalProperties"])
        assignments = schema["properties"]["assignments"]
        self.assertFalse(assignments["additionalProperties"])
        self.assertNotIn("valid_min", assignments["properties"])
        self.assertEqual(len(assignments["properties"]), 5)

    def test_report_proves_pre_dispatch_rejection(self):
        schema = json.loads((ROOT / "contracts/aex/render_parameter_gate_report.schema.json").read_text(encoding="utf-8"))
        required = set(schema["required"])
        self.assertIn("native_dispatch_permitted", required)
        self.assertIn("native_process_started", required)
        self.assertFalse(schema["properties"]["native_process_started"]["const"])
        self.assertEqual(schema["properties"]["assignment_count"]["maximum"], 5)

    def test_rust_route_owns_ranges_and_never_starts_worker(self):
        gate = (ROOT / "broker/crates/broker/src/parameter_gate.rs").read_text(encoding="utf-8")
        route = (ROOT / "broker/crates/broker/src/render_request.rs").read_text(encoding="utf-8")
        main = (ROOT / "broker/crates/broker/src/main.rs").read_text(encoding="utf-8")
        for marker in ("Scatter Amount", "500.0", "Random Seed", "10_000.0", "Invert Map"):
            self.assertIn(marker, gate)
        self.assertIn("native_process_started: false", route)
        self.assertNotIn("run_isolated", route)
        self.assertIn("validate-render-request-scattermap", main)
        self.assertNotIn('.expect("validate render request")', main)


if __name__ == "__main__":
    unittest.main()

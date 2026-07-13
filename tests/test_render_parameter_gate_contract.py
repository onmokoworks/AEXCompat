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
        core = (ROOT / "broker/crates/broker/src/host_core/parameter.rs").read_text(encoding="utf-8")
        profile = (ROOT / "broker/crates/broker/src/fixture_profiles/scattermap.rs").read_text(encoding="utf-8")
        route = (ROOT / "broker/crates/broker/src/render_request.rs").read_text(encoding="utf-8")
        main = (ROOT / "broker/crates/broker/src/main.rs").read_text(encoding="utf-8")
        for marker in ("Scatter Amount", "500.0", "Random Seed", "10_000.0", "Invert Map"):
            self.assertIn(marker, profile)
            self.assertNotIn(marker, core)
        self.assertIn("native_process_started: false", route)
        self.assertIn("run_isolated", route)
        self.assertIn("argb8_hash", route)
        self.assertIn("validate-render-request-scattermap", main)
        self.assertIn("render-parameter-request-scattermap", main)
        self.assertNotIn('.expect("validate render request")', main)

    def test_parameterized_execution_contract_separates_rejection_and_native_success(self):
        schema = json.loads((ROOT / "contracts/aex/parameterized_classic_render_report.schema.json").read_text(encoding="utf-8"))
        self.assertFalse(schema["additionalProperties"])
        text = json.dumps(schema, sort_keys=True)
        for marker in ("expected_oracle_sha256", "fixture_sha256", "deterministic", "broker_survived"):
            self.assertIn(marker, text)
        self.assertIn('"native_process_started": {"const": false}', text)
        self.assertIn('"native_process_started": {"const": true}', text)

    def test_worker_revalidates_and_echoes_bound_values(self):
        source = (ROOT / "minihost/src/l2_main.cpp").read_text(encoding="utf-8")
        for marker in ('L"--render-request"', "parse_i32_arg", "parse_double_arg",
                       "requested_amount", "requested_direction", "requested_seed",
                       "requested_mix", "requested_invert_map", "std::setprecision(17)"):
            self.assertIn(marker, source)
        self.assertLess(source.index("if (request_mode &&"), source.index("if (!sha256(argv[2]"))

    def test_parameterized_smartfx_contract_requires_both_selectors_and_rects(self):
        schema = json.loads((ROOT / "contracts/aex/parameterized_smartfx_render_report.schema.json").read_text(encoding="utf-8"))
        self.assertFalse(schema["additionalProperties"])
        run = schema["$defs"]["run"]
        for marker in ("pre_render_error", "smart_render_error", "result_rects_valid",
                       "guard_bytes_intact", "request_mode"):
            self.assertIn(marker, run["required"])
        main = (ROOT / "broker/crates/broker/src/main.rs").read_text(encoding="utf-8")
        route = (ROOT / "broker/crates/broker/src/render_request.rs").read_text(encoding="utf-8")
        self.assertIn("smart-parameter-request-scattermap", main)
        self.assertIn("execute_smart", route)
        self.assertIn('"--smart-request"', route)


if __name__ == "__main__":
    unittest.main()

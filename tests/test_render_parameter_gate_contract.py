import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class RenderParameterGateContractTests(unittest.TestCase):
    def test_request_is_strict_and_caller_cannot_supply_descriptors(self):
        schema = json.loads((ROOT / "contracts/aex/render_parameter_request.schema.json").read_text(encoding="utf-8"))
        self.assertEqual(schema["schema_version"], 3)
        self.assertEqual(schema["properties"]["schema_version"]["enum"], [2, 3])
        self.assertFalse(schema["additionalProperties"])
        assignments = schema["properties"]["assignments"]
        alternatives = assignments["additionalProperties"]["oneOf"]
        self.assertEqual(alternatives[0], {"type": "number"})
        self.assertEqual(alternatives[1], {"$ref": "#/$defs/color"})
        self.assertEqual(
            schema["allOf"][0]["then"]["properties"]["assignments"]["additionalProperties"],
            {"type": "number"},
        )
        self.assertEqual(assignments["maxProperties"], 64)
        self.assertIn("propertyNames", assignments)
        self.assertNotIn("properties", assignments)

    def test_report_proves_pre_dispatch_rejection(self):
        schema = json.loads((ROOT / "contracts/aex/render_parameter_gate_report.schema.json").read_text(encoding="utf-8"))
        required = set(schema["required"])
        self.assertIn("native_dispatch_permitted", required)
        self.assertIn("native_process_started", required)
        self.assertFalse(schema["properties"]["native_process_started"]["const"])
        self.assertEqual(schema["properties"]["assignment_count"]["maximum"], 64)

    def test_rust_route_owns_ranges_and_never_starts_worker(self):
        core = (ROOT / "broker/crates/broker/src/host_core/parameter.rs").read_text(encoding="utf-8")
        manifest = (ROOT / "profiles/scattermap/parameter_descriptors.json").read_text(encoding="utf-8")
        route = (ROOT / "broker/crates/broker/src/render_request.rs").read_text(encoding="utf-8")
        main = (ROOT / "broker/crates/broker/src/main.rs").read_text(encoding="utf-8")
        for marker in ("Scatter Amount", '"maximum": 500', "Random Seed", '"maximum": 10000', "Invert Map"):
            self.assertIn(marker, manifest)
            self.assertNotIn(marker, core)
        self.assertIn("load_manifest", route)
        self.assertIn("encode_worker_payload", route)
        self.assertIn("native_process_started: false", route)
        self.assertIn("run_isolated", route)
        self.assertIn("argb8_hash", route)
        self.assertIn('args[1] == "validate-render-request"', main)
        self.assertIn('args[1] == "render-parameter-request"', main)
        self.assertNotIn("validate-render-request-scattermap", main)
        self.assertNotIn("render-parameter-request-scattermap", main)
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
        for marker in ('L"--render-request"', "parse_parameter_payload", "valid_parameter_id",
                       'encoded.compare(0, 3, L"v2|")', "encoded.size() > 4096",
                       'encoded.compare(0, 3, L"v3|")', 'kind_text == L"argb8"',
                       "validate_requested_assignments", "apply_requested_assignments",
                       "initialize_parameter_definitions",
                       "g_params[static_cast<std::size_t>(assignment.index - 1)]",
                       "requested_parameters_json", "requested_parameters",
                       "requested_amount", "requested_direction", "requested_seed",
                       "requested_mix", "requested_invert_map", "std::setprecision(17)"):
            self.assertIn(marker, source)
        self.assertLess(source.index("if (request_mode &&"), source.index("if (!sha256(argv[2]"))

    def test_parameterized_smartfx_contract_requires_both_selectors_and_rects(self):
        schema = json.loads((ROOT / "contracts/aex/parameterized_smartfx_render_report.schema.json").read_text(encoding="utf-8"))
        self.assertFalse(schema["additionalProperties"])
        run = schema["$defs"]["run"]
        for marker in ("pre_render_error", "smart_render_error", "result_rects_valid",
                       "guard_bytes_intact", "request_mode", "requested_parameters"):
            self.assertIn(marker, run["required"])
        main = (ROOT / "broker/crates/broker/src/main.rs").read_text(encoding="utf-8")
        route = (ROOT / "broker/crates/broker/src/render_request.rs").read_text(encoding="utf-8")
        self.assertIn('args[1] == "smart-parameter-request"', main)
        self.assertNotIn("smart-parameter-request-scattermap", main)
        self.assertIn("execute_smart", route)
        registry = (ROOT / "broker/crates/broker/src/fixture_profiles/mod.rs").read_text(encoding="utf-8")
        self.assertIn('request_mode: "--smart-request"', registry)
        self.assertIn("worker_spec.request_mode", route)
        self.assertIn("SmartFX render is not supported for plugin profile", route)
        self.assertIn("classic render is not supported for plugin profile", route)


if __name__ == "__main__":
    unittest.main()

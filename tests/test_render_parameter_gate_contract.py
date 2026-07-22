import re
import json
import unittest
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
WORKER = source_owners.L2_MAIN
CLI_DISPATCH = ROOT / "minihost" / "src" / "l2_cli_dispatch.cpp"
RUNTIME_ADMISSION = ROOT / "minihost" / "src" / "worker_runtime_admission.cpp"
ENTRY_ADMISSION = ROOT / "minihost" / "src" / "worker_entry_admission.cpp"
REQUEST_PARSER = ROOT / "minihost" / "src" / "worker_request_parser.cpp"
RENDER_REPORT = ROOT / "minihost" / "src" / "worker_render_report.cpp"
PARAMETER_EXECUTION = ROOT / "minihost" / "src" / "worker_parameter_execution.cpp"


class RenderParameterGateContractTests(unittest.TestCase):
    def test_request_is_strict_and_caller_cannot_supply_descriptors(self):
        schema = json.loads((ROOT / "contracts/aex/render_parameter_request.schema.json").read_text(encoding="utf-8"))
        self.assertEqual(schema["schema_version"], 4)
        self.assertEqual(schema["properties"]["schema_version"]["enum"], [2, 3, 4])
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
        host_context = schema["properties"]["host_context"]
        masks = host_context["properties"]["mask_scene"]["properties"]["masks"]
        self.assertEqual(masks["maxItems"], 8)
        self.assertEqual(schema["$defs"]["mask"]["properties"]["vertices"]["maxItems"], 64)
        self.assertEqual(schema["$defs"]["mask"]["properties"]["open"], {"type": "boolean"})
        self.assertIn("tangent_in", schema["$defs"]["point"]["properties"])
        self.assertIn("tangent_out", schema["$defs"]["point"]["properties"])

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
        # The route still launches a worker for the accepted path, but through the
        # sealed load tree under a restricted token instead of normal-token
        # run_isolated with an argv plug-in path (issue #312). Every launch in this
        # module goes through secure_launch, and none may regress to run_isolated:
        # an argv path is a TOCTOU window the worker would re-open.
        self.assertNotIn("run_isolated", route)
        self.assertIn("secure_launch(", route)
        self.assertIn("SealedLoadTree::create(", route)
        self.assertIn("load_v2_load_tree(", route)
        self.assertIn("require_module_audit: true", route)
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
        worker = WORKER.read_text(encoding="utf-8")
        worker_family = (worker +
            (source_owners.SRC / "worker_l2_payload_parsers.cpp").read_text(encoding="utf-8") +
            ENTRY_ADMISSION.read_text(encoding="utf-8") + RENDER_REPORT.read_text(encoding="utf-8") + PARAMETER_EXECUTION.read_text(encoding="utf-8"))
        cli_dispatch = CLI_DISPATCH.read_text(encoding="utf-8")
        for marker in ('L"--render-request"', 'L"--smart-mask-context-request"'):
            self.assertIn(marker, cli_dispatch)
        for marker in ("parse_parameter_payload", "valid_parameter_id",
                       'encoded.compare(0, 3, L"v2|")', "encoded.size() > 16384",
                       'encoded.compare(0, 3, L"v3|")', 'kind_text == L"argb8"',
                       'kind_text == L"arbhex"',
                       "validate_requested_assignments", "apply_requested_assignments",
                       "initialize_parameter_definitions",
                       "runtime().records[static_cast<std::size_t>(assignment.index - 1)]",
                       "requested_parameters_json", "requested_parameters",
                       "requested_amount", "requested_direction", "requested_seed",
                       "requested_mix", "requested_invert_map", "std::setprecision(17)",
                       "parse_mask_context_payload", "encoded.size() > 8192",
                       "total_vertices > 128"):
            self.assertIn(marker, worker_family)
        # Payload rejection is delegated through the request parser before
        # worker runtime admission can load the plug-in.
        parser = REQUEST_PARSER.read_text(encoding="utf-8")
        self.assertIn("hooks.parse_parameters(argv[4]", parser)
        self.assertLess(worker.index("request_parser::parse("),
                        worker_family.index("admit_worker_entry("))
        admission = RUNTIME_ADMISSION.read_text(encoding="utf-8")
        self.assertIn("hooks.hash_file(request.plugin_argument", admission)
        self.assertLess(admission.index("hooks.hash_file(request.plugin_argument"),
                        admission.index("LoadLibraryExW(plugin_path.c_str()"))

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


class RenderRequestSecureLaunchContractTests(unittest.TestCase):
    """Every worker launch in render_request.rs goes through the sealed load tree.

    Before issue #312 three of the four routes (execute, execute_smart_suite_fault,
    execute_smart_mask_scene) launched with normal-token run_isolated and handed the
    worker the plug-in as an argv path, which the worker re-opened. That is a TOCTOU
    window: the path can be swapped between the broker's check and the worker's open.
    These assertions keep all four routes on secure_launch.
    """

    ROUTE = ROOT / "broker/crates/broker/src/render_request.rs"

    def test_no_route_launches_with_normal_token_run_isolated(self):
        route = self.ROUTE.read_text(encoding="utf-8")
        self.assertNotIn("run_isolated", route)
        self.assertNotIn("windows_process", route)

    def test_every_route_launches_through_the_sealed_load_tree(self):
        route = self.ROUTE.read_text(encoding="utf-8")
        # Counted against the number of launches rather than a fixed 4, so a
        # correctly-sealed fifth route passes while an unsealed one fails:
        # execute (parameter request), execute_smart, execute_smart_suite_fault,
        # execute_smart_mask_scene today.
        launches = route.count("secure_launch(")
        self.assertGreaterEqual(launches, 4)
        for marker in (
            "SealedLoadTree::create(",
            "load_v2_load_tree(",
            "require_module_audit: true",
        ):
            self.assertEqual(route.count(marker), launches, marker)

    def test_every_launch_pins_the_approval_across_determinism_runs(self):
        route = self.ROUTE.read_text(encoding="utf-8")
        # The receipt is reloaded per determinism run, so each route must compare
        # the whole approved identity between runs. The sealed manifest digest
        # covers every dependency; the fixture digest alone would miss a swapped
        # worker build or dependency set.
        launches = route.count("secure_launch(")
        self.assertEqual(route.count("let identity = (\n            tree.manifest_digest(),"), launches)
        self.assertEqual(route.count("approved_identity = Some(identity);"), launches)
        self.assertEqual(route.count("approval changed between determinism runs"), launches)

    def test_no_route_passes_the_plugin_as_an_argv_path(self):
        route = self.ROUTE.read_text(encoding="utf-8")
        # secure_launch injects the sealed plug-in between the before/after argv
        # slices, so no route may serialize a plug-in path into its own args.
        self.assertNotIn("plugin_path.to_string_lossy()", route)
        self.assertIn("plugin_basename: &plugin_basename", route)

    def test_each_launch_is_pinned_to_the_receipt_worker(self):
        route = self.ROUTE.read_text(encoding="utf-8")
        # The profile-declared executable must match the receipt's trusted worker,
        # compared canonically so a symlink or junction cannot substitute it.
        launches = route.count("secure_launch(")
        self.assertEqual(
            route.count("fs::canonicalize(&worker)? != fs::canonicalize(&receipt_worker)?"),
            launches,
        )
        self.assertEqual(route.count("worker_program: &receipt_worker"), launches)

    def test_migrated_reports_take_identity_from_the_receipt(self):
        route = self.ROUTE.read_text(encoding="utf-8")
        # The schema-v1 `approved_entry` / `render::entry` helpers no longer supply
        # the reported identity; it comes from the schema-v2 receipt and the
        # approval policy, so the report names the same approval the launch used.
        self.assertNotIn("approved_entry", route)
        self.assertNotIn("crate::render::entry", route)
        launches = route.count("secure_launch(")
        self.assertEqual(route.count('"receipt_id":worker_spec.approval.receipt_id'), launches)
        self.assertEqual(route.count('"fixture_sha256":approved_fixture_sha256'), launches)

    def test_migrated_reports_add_no_keys_outside_their_contract_schema(self):
        """The three migrated routes must not grow report keys their schema forbids.

        Each report schema is `additionalProperties: false`, so emitting sealed-launch
        provenance would break contract conformance. `execute_smart` already emits
        `secure_launch_*` keys that its schema does not declare (tracked separately);
        the migrated routes must not add to that divergence.
        """
        route = self.ROUTE.read_text(encoding="utf-8")
        stages = {
            "parameterized_classic_render": "parameterized_classic_render_report",
            "smartfx_suite_fault": "smartfx_suite_fault_report",
            "smartfx_mask_scene": "smartfx_mask_scene_report",
        }
        for stage, schema_name in stages.items():
            schema = json.loads(
                (ROOT / "contracts/aex" / f"{schema_name}.schema.json").read_text(encoding="utf-8")
            )
            self.assertFalse(schema["additionalProperties"], stage)
            declared = set(schema["properties"])
            marker = f'"stage":"{stage}"'
            # A stage can be emitted more than once (the classic route writes a
            # pre-dispatch rejection report as well as the success report), so every
            # occurrence is checked -- not just the first.
            occurrences = 0
            start = route.find(marker)
            while start != -1:
                occurrences += 1
                body = route[start : route.index("});", start)]
                emitted = set(re.findall(r'"([a-z0-9_]+)"\s*:', body))
                self.assertTrue(
                    emitted <= declared,
                    f"{stage} emits keys absent from {schema_name}: {sorted(emitted - declared)}",
                )
                start = route.find(marker, start + 1)
            self.assertGreater(occurrences, 0, stage)

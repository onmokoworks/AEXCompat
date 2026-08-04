import re
import json
import unittest
from pathlib import Path
from jsonschema import Draft202012Validator
import source_owners


ROOT = Path(__file__).resolve().parents[1]
WORKER = source_owners.L2_SOURCE
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


    def test_parameterized_execution_contract_separates_rejection_and_native_success(self):
        schema = json.loads((ROOT / "contracts/aex/parameterized_classic_render_report.schema.json").read_text(encoding="utf-8"))
        self.assertFalse(schema["additionalProperties"])
        text = json.dumps(schema, sort_keys=True)
        for marker in ("expected_oracle_sha256", "fixture_sha256", "deterministic", "broker_survived"):
            self.assertIn(marker, text)
        self.assertIn('"native_process_started": {"const": false}', text)
        self.assertIn('"native_process_started": {"const": true}', text)


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


class RenderRequestSecureLaunchContractTests(unittest.TestCase):
    """Every worker launch in render_request.rs goes through the sealed load tree.

    Before issue #312 three of the four routes (execute, execute_smart_suite_fault,
    execute_smart_mask_scene) launched with normal-token run_isolated and handed the
    worker the plug-in as an argv path, which the worker re-opened. That is a TOCTOU
    window: the path can be swapped between the broker's check and the worker's open.
    These assertions keep all four routes on secure_launch.
    """

    ROUTE = ROOT / "broker/crates/broker/src/render_request.rs"

    def literal_end(self, text, index):
        """End index of the Rust literal starting at `index`, or None if none does.

        Handles `"..."` (with escapes), raw `r"..."` / `r#*"..."#*`, and char literals.
        A lifetime (`'a`) is not a literal and returns None, which is safe: it carries
        no brace, quote or comment opener.
        """
        char = text[index]
        # `b` is the one preceding character where the raw reading is still correct
        # (`br"..."`), so it must not be treated as an identifier prefix here.
        start = index - 1 if index and text[index - 1] == "b" else index
        if char == "r" and not (start and (text[start - 1].isalnum() or text[start - 1] == "_")):
            hashes = 0
            cursor = index + 1
            while text[cursor : cursor + 1] == "#":
                hashes += 1
                cursor += 1
            if text[cursor : cursor + 1] != '"':
                return None
            terminator = '"' + "#" * hashes
            close = text.find(terminator, cursor + 1)
            return len(text) if close == -1 else close + len(terminator)
        if char == '"':
            cursor = index + 1
            while cursor < len(text) and text[cursor] != '"':
                cursor += 2 if text[cursor] == "\\" else 1
            return min(cursor + 1, len(text))
        if char == "'":
            closing = index + (3 if text[index + 1 : index + 2] == "\\" else 2)
            return closing + 1 if text[closing : closing + 1] == "'" else None
        return None

    def strip_comments(self, text):
        """`text` with `//` and `/* */` comments removed, literals left intact."""
        kept = []
        index = 0
        end = len(text)
        while index < end:
            stop = self.literal_end(text, index)
            if stop is not None:
                kept.append(text[index:stop])
                index = stop
                continue
            pair = text[index : index + 2]
            if pair == "//":
                newline = text.find("\n", index)
                index = end if newline == -1 else newline
                continue
            if pair == "/*":
                close = text.find("*/", index + 2)
                index = end if close == -1 else close + 2
                continue
            kept.append(text[index])
            index += 1
        return "".join(kept)

    def route_source(self):
        """render_request.rs with its `#[cfg(test)]` module and all comments removed.

        Every assertion in this class counts over this one text, so the two sides of an
        `== launches` comparison can never disagree because one of them saw a comment
        or a Rust unit test that the other did not.
        """
        route = self.ROUTE.read_text(encoding="utf-8")
        cut = route.find("#[cfg(test)]")
        return self.strip_comments(route if cut == -1 else route[:cut])

    def launch_sites(self, route):
        """Number of real `secure_launch(` call sites in `route`.

        Asserts a floor so a module that lost every launch cannot make the
        `== launches` checks pass vacuously as `0 == 0`.
        """
        sites = len(re.findall(r"(?<![a-z_])secure_launch\(", route))
        self.assertGreaterEqual(sites, 4, "render_request.rs lost its sealed launches")
        return sites

    def report_object(self, route, marker_at, stage):
        """The full `let report = json!({ ... })` text enclosing `marker_at`.

        The end is found by balancing braces from the opening one, skipping literals. A
        `}` inside a literal would otherwise drive the depth to zero early and return a
        truncated body, dropping later keys from the check unnoticed -- `"stage"` is the
        second key in every report, so the marker assertion below would not catch it.
        Comments are already gone (`route_source`).

        Known limitation: `declared` is the schema's top-level properties only, so a
        report value that is itself a `json!({...})` would have its inner keys
        reported as undeclared. No report does that today.
        """
        anchor = "let report = json!({"
        opening = route.rfind(anchor, 0, marker_at)
        self.assertNotEqual(opening, -1, f"{stage} marker has no `{anchor}` before it")
        brace = opening + len(anchor) - 1
        self.assertEqual(route[brace], "{", f"{stage} anchor does not end at its brace")
        depth = 0
        index = brace
        end = len(route)
        while index < end:
            stop = self.literal_end(route, index)
            if stop is not None:
                index = stop
                continue
            char = route[index]
            if char == "{":
                depth += 1
            elif char == "}":
                depth -= 1
                if depth == 0:
                    body = route[opening : index + 1]
                    self.assertIn(f'"stage":"{stage}"', body)
                    return body
            index += 1
        self.fail(f"{stage} report object is unterminated")

    def test_no_route_launches_with_normal_token_run_isolated(self):
        route = self.route_source()
        # Two assertNotIns alone would also pass on an empty string, so share the
        # launch floor its siblings use: the module must still have its launches.
        self.launch_sites(route)
        self.assertNotIn("run_isolated", route)
        self.assertNotIn("windows_process", route)

    def test_every_route_launches_through_the_sealed_load_tree(self):
        route = self.route_source()
        # Counted against the number of launches rather than a fixed 4, so a
        # correctly-sealed fifth route passes while an unsealed one fails:
        # execute (parameter request), execute_smart, execute_smart_suite_fault,
        # execute_smart_mask_scene today.
        launches = self.launch_sites(route)
        for marker in (
            "SealedLoadTree::create(",
            "load_v2_load_tree(",
            "require_module_audit: true",
        ):
            self.assertEqual(route.count(marker), launches, marker)

    def test_every_launch_pins_the_approval_across_determinism_runs(self):
        route = self.route_source()
        # The receipt is reloaded per determinism run, so each route must compare
        # the whole approved identity between runs. The sealed manifest digest
        # covers every dependency; the fixture digest alone would miss a swapped
        # worker build or dependency set.
        launches = self.launch_sites(route)
        self.assertEqual(route.count("let identity = (\n            tree.manifest_digest(),"), launches)
        self.assertEqual(route.count("approved_identity = Some(identity);"), launches)
        self.assertEqual(route.count("approval changed between determinism runs"), launches)

    def test_no_route_passes_the_plugin_as_an_argv_path(self):
        route = self.route_source()
        # secure_launch injects the sealed plug-in between the before/after argv
        # slices, so no route may serialize a plug-in path into its own args.
        self.assertNotIn("plugin_path.to_string_lossy()", route)
        # #405 widened SecureLaunchRequest::plugin_basename to Option<&str> so the
        # cluster discovery session can launch with no positional plug-in at all.
        # Every render_request route still carries one, so each launch site must
        # name the basename explicitly — as the pre-#405 `&plugin_basename` or the
        # Option-wrapped `Some(&plugin_basename)` — and `plugin_basename: None`
        # must never appear here (it belongs to the discovery session only).
        launches = self.launch_sites(route)
        named = route.count("plugin_basename: &plugin_basename") + route.count(
            "plugin_basename: Some(&plugin_basename)"
        )
        self.assertEqual(named, launches)
        self.assertNotIn("plugin_basename: None", route)

    def test_each_launch_is_pinned_to_the_receipt_worker(self):
        route = self.route_source()
        # The profile-declared executable must match the receipt's trusted worker,
        # compared canonically so a symlink or junction cannot substitute it.
        launches = self.launch_sites(route)
        self.assertEqual(
            route.count("fs::canonicalize(&worker)? != fs::canonicalize(&receipt_worker)?"),
            launches,
        )
        self.assertEqual(route.count("worker_program: &receipt_worker"), launches)

    def test_migrated_reports_take_identity_from_the_receipt(self):
        route = self.route_source()
        # The schema-v1 `approved_entry` / `render::entry` helpers no longer supply
        # the reported identity; it comes from the schema-v2 receipt and the
        # approval policy, so the report names the same approval the launch used.
        self.assertNotIn("approved_entry", route)
        self.assertNotIn("crate::render::entry", route)
        launches = self.launch_sites(route)
        self.assertEqual(route.count('"receipt_id":worker_spec.approval.receipt_id'), launches)
        self.assertEqual(route.count('"fixture_sha256":approved_fixture_sha256'), launches)

    def test_migrated_reports_add_no_keys_outside_their_contract_schema(self):
        """The three migrated routes must not grow report keys their schema forbids.

        Each report schema is `additionalProperties: false`, so emitting sealed-launch
        provenance would break contract conformance. `execute_smart` already emits
        `secure_launch_*` keys that its schema does not declare (tracked separately);
        the migrated routes must not add to that divergence.
        """
        route = self.route_source()
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
            # occurrence is checked -- not just the first. Each slice spans the whole
            # `let report = json!({ ... })` binding: anchoring at the marker would hide
            # keys emitted ahead of "stage" (schema_version today), and stopping at the
            # first `});` would silently truncate the slice -- a false pass -- once any
            # value is itself a `json!({...})`.
            occurrences = 0
            start = route.find(marker)
            while start != -1:
                occurrences += 1
                body = self.report_object(route, start, stage)
                emitted = set(re.findall(r'"([a-z0-9_]+)"\s*:', body))
                self.assertTrue(
                    emitted <= declared,
                    f"{stage} emits keys absent from {schema_name}: {sorted(emitted - declared)}",
                )
                start = route.find(marker, start + 1)
            self.assertGreater(occurrences, 0, stage)

    def test_secure_launch_provenance_is_declared_and_validated(self):
        schema_names = (
            "parameterized_classic_render_report.schema.json",
            "parameterized_smartfx_render_report.schema.json",
            "smartfx_suite_fault_report.schema.json",
            "smartfx_mask_scene_report.schema.json",
        )
        for schema_name in schema_names:
            schema = json.loads(
                (ROOT / "contracts/aex" / schema_name).read_text(encoding="utf-8")
            )
            Draft202012Validator.check_schema(schema)
            properties = schema["properties"]
            for key in (
                "secure_launch_1",
                "secure_launch_2",
                "secure_launch_count",
                "normal_token_fallback",
            ):
                self.assertIn(key, properties, schema_name)
            self.assertFalse(schema["$defs"]["secure_launch"]["additionalProperties"])

        smart_schema = json.loads(
            (ROOT / "contracts/aex/parameterized_smartfx_render_report.schema.json").read_text(
                encoding="utf-8"
            )
        )
        secure_launch = {
            "launch_mode": "sealed_load_tree_restricted_token",
            "worker_authenticated": True,
            "worker_size_bytes": 123,
            "worker_sha256": "a" * 64,
            "plugin_basename": "fixture.aex",
            "plugin_sha256": "b" * 64,
            "sealed_manifest_sha256": "c" * 64,
            "module_audit_required": True,
            "module_audit": None,
            "dismissed_windows": [],
        }
        run = {key: None for key in smart_schema["$defs"]["run"]["required"]}
        run["classification"] = "timeout_killed"
        report = {
            "schema_version": 1,
            "stage": "parameterized_smartfx_render",
            "plugin_id": "fixture",
            "assignment_count": 1,
            "accepted": True,
            "native_process_started": True,
            "passed": False,
            "receipt_id": "receipt-1",
            "fixture_sha256": "D" * 64,
            "parameters": {"mix": 1},
            "expected_oracle_sha256": "E" * 64,
            "run_1": run,
            "run_2": run,
            "secure_launch_1": secure_launch,
            "secure_launch_2": secure_launch,
            "secure_launch_count": 2,
            "normal_token_fallback": False,
            "deterministic": False,
            "broker_survived": True,
        }
        Draft202012Validator(smart_schema).validate(report)

        invalid = json.loads(json.dumps(report))
        invalid["secure_launch_1"]["unexpected"] = True
        self.assertTrue(list(Draft202012Validator(smart_schema).iter_errors(invalid)))

    def test_one_shot_projection_carries_the_windows_the_broker_closed(self):
        """A one-shot launch's report must show what the host closed (#351).

        The parameterized path hand-projects its `secure_launch_N` object, so
        the observation reaching `SecureLaunchResult` and the session
        diagnostics does not reach here by itself. A dispatch where the broker
        answered a plug-in's dialog and one where it did not are otherwise
        indistinguishable in the report.
        """
        source = (ROOT / "broker/crates/broker/src/render_request.rs").read_text(
            encoding="utf-8"
        )
        self.assertIn('"dismissed_windows":isolated.dismissed_windows', source)

        for schema_name in (
            "parameterized_classic_render_report.schema.json",
            "parameterized_smartfx_render_report.schema.json",
            "smartfx_suite_fault_report.schema.json",
            "smartfx_mask_scene_report.schema.json",
        ):
            schema = json.loads(
                (ROOT / "contracts/aex" / schema_name).read_text(encoding="utf-8")
            )
            secure_launch = schema["$defs"]["secure_launch"]
            self.assertIn("dismissed_windows", secure_launch["properties"], schema_name)
            # Required, so a report that simply drops the field fails the
            # contract instead of reading as "no window appeared".
            self.assertIn("dismissed_windows", secure_launch["required"], schema_name)
            window = schema["$defs"]["dismissed_windows"]["items"]
            self.assertFalse(window["additionalProperties"], schema_name)
            # `asked_to_close` apart from `closed`: a dialog that ignored the
            # broker is not a dialog the broker closed.
            for field in ("title", "class", "closed", "asked_to_close"):
                self.assertIn(field, window["properties"], schema_name)
            # Bounded, like every other capture that reaches a report.
            self.assertEqual(schema["$defs"]["dismissed_windows"]["maxItems"], 32)
            self.assertEqual(window["properties"]["title"]["maxLength"], 256)

        smart_schema = json.loads(
            (
                ROOT / "contracts/aex/parameterized_smartfx_render_report.schema.json"
            ).read_text(encoding="utf-8")
        )
        window = {
            "title": "Error at loading of ippCV library",
            "class": "#32770",
            "closed": True,
            "asked_to_close": True,
        }
        Draft202012Validator(
            smart_schema["$defs"]["dismissed_windows"]
        ).validate([window])
        for broken in (
            {**window, "unexpected": True},
            {key: value for key, value in window.items() if key != "asked_to_close"},
        ):
            self.assertTrue(
                list(
                    Draft202012Validator(
                        smart_schema["$defs"]["dismissed_windows"]
                    ).iter_errors([broken])
                )
            )


if __name__ == "__main__":
    unittest.main()

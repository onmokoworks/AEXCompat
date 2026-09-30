import json
import unittest
from pathlib import Path
from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parents[1]

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

class RenderRequestSecureLaunchContractTests(unittest.TestCase):
    def test_one_shot_projection_carries_the_windows_the_broker_closed(self):
        """A one-shot launch's report must show what the host closed (#351).

        The parameterized path hand-projects its `secure_launch_N` object, so
        the observation reaching `SecureLaunchResult` and the session
        diagnostics does not reach here by itself. A dispatch where the broker
        answered a plug-in's dialog and one where it did not are otherwise
        indistinguishable in the report.
        """
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

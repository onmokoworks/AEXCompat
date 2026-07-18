import copy
import json
import unittest
from pathlib import Path

from tools.trace_contract_validator import validate_event, validate_session


ROOT = Path(__file__).resolve().parents[1]
TRACE_ROOT = ROOT / "contracts" / "trace"


def load_json(name):
    return json.loads((TRACE_ROOT / name).read_text(encoding="utf-8"))


def load_events():
    path = TRACE_ROOT / "examples" / "synthetic_session.jsonl"
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()]


def load_native_observation_events():
    path = TRACE_ROOT / "examples" / "native_observation_session.jsonl"
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()]


class TraceContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.event_schema = load_json("host_trace_event.schema.json")
        cls.session_schema = load_json("host_trace_session.schema.json")

    def test_schemas_are_valid_draft_2020_12(self):
        self.assertEqual("https://json-schema.org/draft/2020-12/schema", self.event_schema["$schema"])
        self.assertEqual("https://json-schema.org/draft/2020-12/schema", self.session_schema["$schema"])
        self.assertEqual(1, self.event_schema["schema_version"])
        self.assertEqual(1, self.session_schema["schema_version"])

    def test_synthetic_jsonl_events_validate(self):
        events = load_events()
        self.assertTrue(events)
        for event in events:
            self.assertEqual([], validate_event(event))

    def test_complete_synthetic_session_validates(self):
        events = load_events()
        session = {
            "schema_version": 1,
            "session_id": "12345678-1234-5678-9234-567812345678",
            "event_count": len(events),
            "trace_complete": True,
            "events": events,
        }
        self.assertEqual([], validate_session(session))

    def test_forbidden_event_fields_are_rejected(self):
        base = load_events()[0]
        for key in ("raw_payload", "binary_payload", "pixels", "pointer"):
            with self.subTest(key=key):
                event = {**base, key: "forbidden"}
                self.assertTrue(validate_event(event))

    def test_absolute_paths_are_rejected(self):
        event = copy.deepcopy(load_events()[1])
        event["selector"] = "D:\\Private\\selector"
        self.assertTrue(validate_event(event))

        event = copy.deepcopy(load_events()[0])
        event["plugin_label"] = "D:\\Private\\Plugin.aex"
        self.assertTrue(validate_event(event))

    def test_incomplete_or_noncontiguous_sessions_are_rejected(self):
        events = load_events()
        session = {
            "schema_version": 1,
            "session_id": "12345678-1234-5678-9234-567812345678",
            "event_count": len(events),
            "trace_complete": True,
            "events": copy.deepcopy(events),
        }
        session["events"][2]["event_index"] = 9
        self.assertIn("event indexes are not contiguous from zero", validate_session(session))

        session["events"] = session["events"][:-1]
        session["event_count"] -= 1
        self.assertIn("trace_complete does not match boundary events", validate_session(session))

    def test_native_observation_jsonl_events_validate(self):
        events = load_native_observation_events()
        self.assertTrue(events)
        kinds = [event["event_kind"] for event in events]
        self.assertEqual(
            ["session_start", "known_function_invoke", "known_function_invoke", "session_end"],
            kinds,
        )
        for event in events:
            self.assertEqual("native_observation", event["host_kind"])
            self.assertEqual([], validate_event(event))

    def test_complete_native_observation_session_validates(self):
        events = load_native_observation_events()
        session = {
            "schema_version": 1,
            "session_id": "abcdef01-2345-6789-abcd-ef0123456789",
            "event_count": len(events),
            "trace_complete": True,
            "events": events,
        }
        self.assertEqual([], validate_session(session))

    def test_known_function_requires_payload(self):
        base = load_native_observation_events()[0]
        event = {**base, "event_kind": "known_function_invoke"}
        errors = validate_event(event)
        self.assertTrue(any("known_function" in error for error in errors))

    def test_known_function_module_rva_must_be_module_relative(self):
        enter = copy.deepcopy(load_native_observation_events()[1])
        enter["known_function"]["module_rva"] = "0x7FFABC001C40"
        self.assertIn(
            "known_function.module_rva must be a lowercase-hex module offset",
            validate_event(enter),
        )

    def test_known_function_rejects_smuggled_foreign_payload(self):
        # A known_function_invoke must not also carry an `error` payload, which
        # would otherwise slip an absolute path past redaction via the error branch.
        enter = copy.deepcopy(load_native_observation_events()[1])
        enter["error"] = {"code_label": "x", "message": "C:\\secret\\leak"}
        errors = validate_event(enter)
        self.assertTrue(any("not allowed for event_kind known_function_invoke" in e for e in errors))

    def test_selector_payload_rejected_on_wrong_kind(self):
        event = copy.deepcopy(load_events()[0])  # session_start
        event["selector"] = "PF_Cmd_RENDER"
        self.assertTrue(any("not allowed for event_kind session_start" in e for e in validate_event(event)))

    def test_known_function_requires_native_observation_host_kind(self):
        # Observation payloads must not masquerade as evidence-tier host events.
        enter = copy.deepcopy(load_native_observation_events()[1])
        enter["host_kind"] = "minihost"
        self.assertIn(
            "known_function_invoke requires host_kind native_observation",
            validate_event(enter),
        )

    def test_native_observation_limited_to_observation_kinds(self):
        # A native_observation selector_dispatch would otherwise slip into the
        # event_kind-keyed conformance comparison despite carrying no integrity.
        event = {
            "schema_version": 1,
            "event_index": 0,
            "event_kind": "selector_dispatch",
            "host_kind": "native_observation",
            "host_version_label": "native-observation frida",
            "plugin_label": "gamma-classic",
            "selector": "PF_Cmd_RENDER",
        }
        self.assertIn(
            "host_kind native_observation is limited to session boundaries and known_function_invoke",
            validate_event(event),
        )

    def test_native_observation_boundaries_are_allowed(self):
        for event in (load_native_observation_events()[0], load_native_observation_events()[-1]):
            self.assertEqual([], validate_event(event))

    def test_known_function_lowercase_absolute_address_is_rejected(self):
        # A lowercased 64-bit ASLR address matches the hex shape but is not a
        # module-relative offset; the magnitude bound must reject it.
        enter = copy.deepcopy(load_native_observation_events()[1])
        enter["known_function"]["module_rva"] = "0x7ffabc001c40"
        self.assertIn(
            "known_function.module_rva exceeds the module-relative bound (looks absolute)",
            validate_event(enter),
        )

    def test_known_function_field_absolute_path_is_rejected(self):
        enter = copy.deepcopy(load_native_observation_events()[1])
        enter["known_function"]["fields"][0]["name"] = "C:\\Private\\in.width"
        self.assertTrue(validate_event(enter))

    def test_known_function_field_forward_slash_absolute_path_is_rejected(self):
        # A forward-slash absolute path (C:/Users/...) must also fail closed; the
        # backslash-only ABSOLUTE_PATH regex alone would miss it.
        enter = copy.deepcopy(load_native_observation_events()[1])
        enter["known_function"]["fields"][0]["name"] = "C:/Users/alice/secret"
        self.assertIn(
            "known_function.fields[0].name must not contain a path separator",
            validate_event(enter),
        )

    def test_known_function_rejects_raw_pointer_scalar(self):
        enter = copy.deepcopy(load_native_observation_events()[1])
        enter["known_function"]["fields"][0]["value"] = "0x7ffabc00"
        self.assertIn(
            "known_function.fields[0].value must be a numeric or boolean scalar",
            validate_event(enter),
        )

    def test_known_function_rejects_non_finite_values(self):
        enter = copy.deepcopy(load_native_observation_events()[1])
        enter["known_function"]["fields"][0]["value"] = float("nan")
        self.assertTrue(validate_event(enter))
        leave = copy.deepcopy(load_native_observation_events()[2])
        leave["known_function"]["return_value"] = float("inf")
        self.assertIn(
            "known_function.return_value must be a numeric scalar",
            validate_event(leave),
        )

    def test_known_function_return_value_rejects_nonscalar(self):
        leave = copy.deepcopy(load_native_observation_events()[2])
        leave["known_function"]["return_value"] = "noErr"
        self.assertIn(
            "known_function.return_value must be a numeric scalar",
            validate_event(leave),
        )

    def test_forbidden_fields_rejected_on_native_observation(self):
        base = load_native_observation_events()[1]
        for key in ("raw_payload", "binary_payload", "pixels", "pointer"):
            with self.subTest(key=key):
                event = {**copy.deepcopy(base), key: "forbidden"}
                self.assertTrue(validate_event(event))

    def test_conformance_rules_have_required_initial_policy(self):
        rules = load_json("conformance_rules.json")["rules"]
        indexed = {(rule["event_kind"], rule["field"]): rule for rule in rules}
        self.assertEqual("must_match", indexed[("selector_dispatch", "selector")]["level"])
        self.assertEqual("ordered_sequence", indexed[("selector_dispatch", "selector")]["comparison"])
        self.assertEqual("must_match", indexed[("suite_acquire", "suite.name")]["level"])
        self.assertEqual("set", indexed[("suite_acquire", "suite.name")]["comparison"])
        for field in ("world.width", "world.height", "world.rowbytes", "world.pixel_format"):
            self.assertEqual("should_match", indexed[("world_descriptor", field)]["level"])

    def test_native_observation_stays_outside_conformance(self):
        # Observation output has no integrity/provenance; it must never become
        # After Effects equivalence evidence, so no rule may target it.
        rules = load_json("conformance_rules.json")["rules"]
        for rule in rules:
            self.assertNotEqual("known_function_invoke", rule["event_kind"])
            self.assertFalse(rule["field"].startswith("known_function"))


if __name__ == "__main__":
    unittest.main()

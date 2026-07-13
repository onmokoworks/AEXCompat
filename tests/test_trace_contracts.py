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

    def test_conformance_rules_have_required_initial_policy(self):
        rules = load_json("conformance_rules.json")["rules"]
        indexed = {(rule["event_kind"], rule["field"]): rule for rule in rules}
        self.assertEqual("must_match", indexed[("selector_dispatch", "selector")]["level"])
        self.assertEqual("ordered_sequence", indexed[("selector_dispatch", "selector")]["comparison"])
        self.assertEqual("must_match", indexed[("suite_acquire", "suite.name")]["level"])
        self.assertEqual("set", indexed[("suite_acquire", "suite.name")]["comparison"])
        for field in ("world.width", "world.height", "world.rowbytes", "world.pixel_format"):
            self.assertEqual("should_match", indexed[("world_descriptor", field)]["level"])


if __name__ == "__main__":
    unittest.main()

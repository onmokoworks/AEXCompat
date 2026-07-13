import json
import unittest
from pathlib import Path

from tools.trace_normalizer import normalize, read_jsonl


ROOT = Path(__file__).resolve().parents[1]
SYNTHETIC = ROOT / "contracts" / "trace" / "examples" / "synthetic_session.jsonl"


class TraceNormalizerTests(unittest.TestCase):
    def test_normalizes_and_reindexes_deterministically(self):
        events = read_jsonl(SYNTHETIC)
        for index, event in enumerate(events):
            event["event_index"] = index + 20
            event["timestamp"] = f"volatile-{index}"
            event["session_id"] = "volatile"
        first = normalize(events)
        second = normalize(events)
        self.assertEqual(first, second)
        self.assertEqual(list(range(len(events))), [event["event_index"] for event in first["events"]])
        self.assertNotIn("timestamp", json.dumps(first))
        self.assertNotIn("session_id", json.dumps(first))

    def test_rejects_invalid_event_after_volatile_removal(self):
        events = read_jsonl(SYNTHETIC)
        events[0]["raw_payload"] = "forbidden"
        with self.assertRaises(ValueError):
            normalize(events)

    def test_same_input_serializes_byte_identically(self):
        payload = normalize(read_jsonl(SYNTHETIC))
        first = json.dumps(payload, indent=2, ensure_ascii=False, sort_keys=True) + "\n"
        second = json.dumps(payload, indent=2, ensure_ascii=False, sort_keys=True) + "\n"
        self.assertEqual(first.encode(), second.encode())


if __name__ == "__main__":
    unittest.main()

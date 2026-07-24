import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools import trace_normalizer
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

    def test_cli_writes_new_normalized_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            destination = root / "normalized.json"
            with mock.patch.object(trace_normalizer, "OUTPUT_ROOT", root), contextlib.redirect_stdout(io.StringIO()):
                code = trace_normalizer.main(["--input", str(SYNTHETIC), "--out", str(destination)])
            self.assertEqual(0, code)
            self.assertEqual("normalized_host_trace", json.loads(destination.read_text(encoding="utf-8"))["report_kind"])

    def test_cli_does_not_overwrite_competing_output_after_preflight(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            destination = root / "normalized.json"
            original_normalize = trace_normalizer.normalize

            def create_competing_output(events):
                destination.write_text("sentinel", encoding="utf-8")
                return original_normalize(events)

            with (
                mock.patch.object(trace_normalizer, "OUTPUT_ROOT", root),
                mock.patch.object(trace_normalizer, "normalize", side_effect=create_competing_output),
                contextlib.redirect_stdout(io.StringIO()),
                contextlib.redirect_stderr(io.StringIO()),
            ):
                code = trace_normalizer.main(["--input", str(SYNTHETIC), "--out", str(destination)])
            self.assertEqual(2, code)
            self.assertEqual("sentinel", destination.read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()

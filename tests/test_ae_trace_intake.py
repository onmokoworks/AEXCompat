import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools import ae_trace_intake


ROOT = Path(__file__).resolve().parents[1]
SYNTHETIC = ROOT / "contracts" / "trace" / "examples" / "synthetic_session.jsonl"


class AeTraceIntakeTests(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.root = Path(self.temp_dir.name)
        self.report_root = self.root / "intake"
        self.corpus_root = self.root / "corpus"
        self.patches = (
            mock.patch.object(ae_trace_intake, "REPORT_ROOT", self.report_root),
            mock.patch.object(ae_trace_intake, "CORPUS_ROOT", self.corpus_root),
        )
        for patch in self.patches:
            patch.start()

    def tearDown(self):
        for patch in reversed(self.patches):
            patch.stop()
        self.temp_dir.cleanup()

    def run_main(self, raw, *, redact=False, report_name="report.json", trace_name="trace.jsonl"):
        report = self.report_root / report_name
        sanitized = self.corpus_root / trace_name
        args = ["--raw-trace", str(raw), "--out", str(report), "--sanitized-out", str(sanitized)]
        if redact:
            args.append("--redact")
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            code = ae_trace_intake.main(args)
        report_text = report.read_text(encoding="utf-8") if report.exists() else ""
        payload = json.loads(report_text) if report_text.startswith("{") else None
        return code, payload, sanitized

    def write_events(self, events):
        path = self.root / "raw.jsonl"
        path.write_text("".join(json.dumps(event) + "\n" for event in events), encoding="utf-8")
        return path

    def test_accepts_synthetic_trace(self):
        code, report, sanitized = self.run_main(SYNTHETIC)
        self.assertEqual(0, code)
        self.assertTrue(report["accepted"])
        self.assertEqual(0, report["redaction_count"])
        self.assertFalse(report["ae_invoked"])
        self.assertTrue(sanitized.exists())
        self.assertEqual(SYNTHETIC.read_text(encoding="utf-8").splitlines(), sanitized.read_text(encoding="utf-8").splitlines())

    def test_rejects_absolute_path_without_redact(self):
        event = json.loads(SYNTHETIC.read_text(encoding="utf-8").splitlines()[1])
        event["selector"] = "D:\\Private\\selector"
        code, report, sanitized = self.run_main(self.write_events([event]))
        self.assertEqual(1, code)
        self.assertFalse(report["accepted"])
        self.assertFalse(sanitized.exists())
        self.assertTrue(any("absolute path" in reason for reason in report["rejection_reasons"]))

    def test_rejects_native_observation_from_ae_corpus(self):
        # Observation traces carry no provenance and must not enter the AE corpus.
        event = {
            "schema_version": 1,
            "event_index": 0,
            "event_kind": "session_start",
            "host_kind": "native_observation",
            "host_version_label": "native-observation frida",
            "plugin_label": "gamma-classic",
        }
        code, report, sanitized = self.run_main(self.write_events([event]))
        self.assertEqual(1, code)
        self.assertFalse(report["accepted"])
        self.assertFalse(sanitized.exists())
        self.assertTrue(any("native_observation" in reason for reason in report["rejection_reasons"]))

    def test_redacts_absolute_path_and_accepts(self):
        event = json.loads(SYNTHETIC.read_text(encoding="utf-8").splitlines()[1])
        event["selector"] = "D:\\Private\\selector"
        code, report, sanitized = self.run_main(self.write_events([event]), redact=True)
        self.assertEqual(0, code)
        self.assertTrue(report["accepted"])
        self.assertEqual(1, report["redaction_count"])
        self.assertEqual("<redacted-path>", json.loads(sanitized.read_text(encoding="utf-8"))["selector"])
        self.assertNotIn("Private", json.dumps(report))

    def test_redact_does_not_rescue_forbidden_field(self):
        event = json.loads(SYNTHETIC.read_text(encoding="utf-8").splitlines()[0])
        event["raw_payload"] = "D:\\Private\\payload.bin"
        code, report, sanitized = self.run_main(self.write_events([event]), redact=True)
        self.assertEqual(1, code)
        self.assertFalse(report["accepted"])
        self.assertEqual(1, report["redaction_count"])
        self.assertFalse(sanitized.exists())
        self.assertTrue(any("forbidden field" in reason for reason in report["rejection_reasons"]))

    def test_refuses_create_new_violation_before_writing(self):
        self.report_root.mkdir(parents=True)
        report = self.report_root / "report.json"
        report.write_text("sentinel", encoding="utf-8")
        code, payload, sanitized = self.run_main(SYNTHETIC)
        self.assertEqual(2, code)
        self.assertIsNone(payload)
        self.assertEqual("sentinel", report.read_text(encoding="utf-8"))
        self.assertFalse(sanitized.exists())

    def test_cli_does_not_import_process_launch_modules(self):
        source = (ROOT / "tools" / "ae_trace_intake.py").read_text(encoding="utf-8")
        self.assertNotIn("subprocess", source)
        self.assertNotIn("os.system", source)


if __name__ == "__main__":
    unittest.main()

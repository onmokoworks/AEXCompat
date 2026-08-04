import importlib.util
import json
import os
import sys
import time
import unittest
from pathlib import Path


LAB_ROOT = Path(__file__).resolve().parents[1]


def load_tool(name: str):
    path = LAB_ROOT / "tools" / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


aex_synthetic_pipl_parser_selftest = load_tool("aex_synthetic_pipl_parser_selftest")


def make_redacted_schema_verifier() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_redacted_schema_verifier",
        "verifier_state": "redacted_schema_verifier_ready_no_real_schema",
        "verifier_ready": True,
        "real_redacted_schema_available": False,
        "real_parameter_schema_available": False,
        "payload_parser_enabled": False,
        "synthetic_schema_fixture_used": True,
        "redaction_contract": {
            "state": "allowlist_and_forbidden_fields_ready",
            "raw_payload_values_allowed": False,
            "real_schema_values_allowed": False,
        },
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "aepx_file_modified": False,
        "aep_binary_modified": False,
        "ae_project_write_performed": False,
        "ofx_plugin_built": False,
        "ofx_describe_performed": False,
        "ofx_render_performed": False,
        "aex_render_performed": False,
        "render_validation_performed": False,
        "pipl_payload_parsed": False,
        "parameter_schema_emitted": False,
        "redacted_schema_emitted": False,
    }


def build_report(**overrides):
    verifier = make_redacted_schema_verifier()
    verifier.update(overrides)
    return aex_synthetic_pipl_parser_selftest.build_selftest_report(
        redacted_schema_verifier=verifier,
        verifier_path=Path("verifier.json"),
    )


class AexSyntheticPiplParserSelftestTests(unittest.TestCase):
    def test_selftest_passes_with_synthetic_payloads_only(self):
        report = build_report()
        self.assertEqual(report["report_kind"], "aex_synthetic_pipl_parser_selftest")
        self.assertEqual(report["selftest_state"], "synthetic_pipl_parser_selftest_passed_no_real_payload")
        self.assertTrue(report["synthetic_parser_ready"])
        self.assertTrue(report["synthetic_payloads_used"])
        self.assertFalse(report["synthetic_payloads_serialized"])
        self.assertFalse(report["real_pipl_payload_parser_enabled"])
        self.assertFalse(report["real_pipl_payload_parsed"])
        self.assertFalse(report["pipl_payload_parsed"])
        self.assertFalse(report["parameter_schema_emitted"])
        self.assertFalse(report["redacted_schema_emitted"])
        self.assertFalse(report["raw_payload_serialized"])
        self.assertEqual(report["summary"]["failed_count"], 0)
        self.assertEqual(report["summary"]["raw_payload_serialized_count"], 0)
        self.assertIn("metadata_counts_only_no_values", report["parser_contract"]["output_policy"])

    def test_synthetic_parser_rejects_malformed_and_oversized_inputs(self):
        valid = aex_synthetic_pipl_parser_selftest.synthetic_payload([(1, b"type")])
        parsed = aex_synthetic_pipl_parser_selftest.parse_synthetic_payload(valid)
        self.assertEqual(parsed["parse_state"], "parsed_synthetic_payload_metadata_only")
        self.assertEqual(parsed["metadata"]["known_tag_counts"]["parameter_type_category"], 1)
        self.assertNotIn('"type"', json.dumps(parsed))

        truncated = aex_synthetic_pipl_parser_selftest.SYNTHETIC_MAGIC + bytes([1, 8]) + b"tiny"
        parsed = aex_synthetic_pipl_parser_selftest.parse_synthetic_payload(truncated)
        self.assertEqual(parsed["parse_state"], "rejected_malformed_synthetic_payload")

        oversized = aex_synthetic_pipl_parser_selftest.SYNTHETIC_MAGIC + b"\x01\x01x" * 90
        parsed = aex_synthetic_pipl_parser_selftest.parse_synthetic_payload(oversized)
        self.assertEqual(parsed["parse_state"], "rejected_oversized_synthetic_payload")

    def test_invalid_verifier_is_rejected(self):
        with self.assertRaises(ValueError):
            build_report(verifier_ready=False)

        with self.assertRaises(ValueError):
            build_report(payload_parser_enabled=True)

        verifier = make_redacted_schema_verifier()
        verifier["redaction_contract"]["raw_payload_values_allowed"] = True
        with self.assertRaises(ValueError):
            aex_synthetic_pipl_parser_selftest.build_selftest_report(
                redacted_schema_verifier=verifier,
                verifier_path=Path("verifier.json"),
            )

    def test_paths_are_confined_and_report_is_create_new(self):
        verifier_root = LAB_ROOT / "target" / "redacted-schema-verifier"
        verifier_root.mkdir(parents=True, exist_ok=True)
        source = verifier_root / f"{time.time_ns()}-{os.getpid()}-verifier.local.json"
        source.write_text(json.dumps(make_redacted_schema_verifier()), encoding="utf-8")
        verifier, resolved = aex_synthetic_pipl_parser_selftest.load_redacted_schema_verifier(source)
        self.assertEqual(resolved, source.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-synthetic-parser.json"
        outside.write_text(json.dumps(make_redacted_schema_verifier()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_synthetic_pipl_parser_selftest.load_redacted_schema_verifier(outside)

        report = aex_synthetic_pipl_parser_selftest.build_selftest_report(
            redacted_schema_verifier=verifier,
            verifier_path=resolved,
        )
        out = LAB_ROOT / "target" / "synthetic-pipl-parser-selftest" / f"{time.time_ns()}-{os.getpid()}-selftest.local.json"
        written = aex_synthetic_pipl_parser_selftest.write_json_create_new(out, report)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_synthetic_pipl_parser_selftest.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aex_synthetic_pipl_parser_selftest.write_json_create_new(
                LAB_ROOT / "target" / "outside-synthetic-parser.json",
                report,
            )


if __name__ == "__main__":
    unittest.main()

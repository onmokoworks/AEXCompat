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


aex_redacted_schema_verifier = load_tool("aex_redacted_schema_verifier")


def make_review_packet() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aex_parameter_schema_review_packet",
        "review_state": "parameter_schema_review_ready_no_payload",
        "parser_design_state": "payload_parser_design_review_ready_parser_disabled",
        "redaction_policy_state": "redaction_policy_ready_no_schema_output",
        "ofx_describe_policy_state": "ofx_describe_mapping_deferred_until_redacted_schema",
        "payload_parser_enabled": False,
        "real_parameter_schema_available": False,
        "redacted_schema_available": False,
        "ofx_describe_mapping_ready": False,
        "redaction_policy": {
            "state": "redaction_policy_ready_no_schema_output",
            "public_schema_output_state": "not_emitted",
            "allowed_after_review": [
                "schema_version",
                "compatibility_class",
                "mapping_state_counts",
                "parameter_count_if_non_identifying",
                "parameter_type_categories_if_non_identifying",
                "range_presence_flags_if_non_identifying",
            ],
            "forbidden_without_additional_approval": [
                "raw_pipl_payload_bytes",
                "private_resource_payload",
                "unredacted_parameter_names",
                "unredacted_default_values",
                "unredacted_value_ranges",
                "binary_hashes",
                "absolute_source_paths",
            ],
        },
        "summary": {
            "candidate_count": 2,
            "mapping_state_counts": {
                "primary_schema_mapping_candidate_pending_payload_parser": 1,
                "host_contract_review_before_schema_mapping": 1,
            },
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
    packet = make_review_packet()
    packet.update(overrides)
    return aex_redacted_schema_verifier.build_verifier_report(
        review_packet=packet,
        review_packet_path=Path("review.json"),
    )


class AexRedactedSchemaVerifierTests(unittest.TestCase):
    def test_verifier_ready_without_emitting_real_or_redacted_schema(self):
        report = build_report()
        self.assertEqual(report["report_kind"], "aex_redacted_schema_verifier")
        self.assertEqual(report["verifier_state"], "redacted_schema_verifier_ready_no_real_schema")
        self.assertTrue(report["verifier_ready"])
        self.assertFalse(report["real_redacted_schema_available"])
        self.assertFalse(report["real_parameter_schema_available"])
        self.assertFalse(report["payload_parser_enabled"])
        self.assertFalse(report["redacted_schema_emitted"])
        self.assertFalse(report["parameter_schema_emitted"])
        self.assertFalse(report["pipl_payload_parsed"])
        self.assertFalse(report["ofx_describe_performed"])
        self.assertTrue(report["synthetic_schema_fixture_used"])
        self.assertFalse(report["synthetic_fixture_serialized"])
        self.assertEqual(report["synthetic_fixture_summary"]["candidate_error_count"], 0)
        self.assertIn("raw_pipl_payload_bytes", report["redaction_contract"]["forbidden_fields"])
        self.assertTrue(all(check["passed"] for check in report["verifier_checks"]))

    def test_schema_candidate_validation_rejects_forbidden_fields_and_paths(self):
        packet = make_review_packet()
        policy = packet["redaction_policy"]
        allowed = set(policy["allowed_after_review"])
        forbidden = set(policy["forbidden_without_additional_approval"])
        candidate = {
            "schema_version": 1,
            "compatibility_class": "synthetic",
            "mapping_state_counts": {"ok": 1},
            "parameter_count_if_non_identifying": 1,
            "parameter_type_categories_if_non_identifying": [],
            "range_presence_flags_if_non_identifying": {},
            "unredacted_parameter_names": ["Secret"],
        }
        errors = aex_redacted_schema_verifier.validate_schema_candidate(
            candidate,
            allowed=allowed,
            forbidden=forbidden,
        )
        self.assertIn("candidate contains forbidden keys: unredacted_parameter_names", errors)

        candidate.pop("unredacted_parameter_names")
        candidate["compatibility_class"] = "D:\\Projects\\secret.aex"
        errors = aex_redacted_schema_verifier.validate_schema_candidate(
            candidate,
            allowed=allowed,
            forbidden=forbidden,
        )
        self.assertIn("candidate contains absolute path-like strings", errors)

    def test_invalid_review_packets_are_rejected(self):
        with self.assertRaises(ValueError):
            build_report(payload_parser_enabled=True)

        packet = make_review_packet()
        packet["redaction_policy"]["forbidden_without_additional_approval"] = []
        with self.assertRaises(ValueError):
            aex_redacted_schema_verifier.build_verifier_report(
                review_packet=packet,
                review_packet_path=Path("review.json"),
            )

    def test_paths_are_confined_and_report_is_create_new(self):
        review_root = LAB_ROOT / "target" / "parameter-schema-review"
        review_root.mkdir(parents=True, exist_ok=True)
        source = review_root / f"{time.time_ns()}-{os.getpid()}-review.local.json"
        source.write_text(json.dumps(make_review_packet()), encoding="utf-8")
        packet, resolved = aex_redacted_schema_verifier.load_review_packet(source)
        self.assertEqual(resolved, source.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-verifier.json"
        outside.write_text(json.dumps(make_review_packet()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_redacted_schema_verifier.load_review_packet(outside)

        report = aex_redacted_schema_verifier.build_verifier_report(
            review_packet=packet,
            review_packet_path=resolved,
        )
        out = LAB_ROOT / "target" / "redacted-schema-verifier" / f"{time.time_ns()}-{os.getpid()}-verifier.local.json"
        written = aex_redacted_schema_verifier.write_json_create_new(out, report)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_redacted_schema_verifier.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aex_redacted_schema_verifier.write_json_create_new(LAB_ROOT / "target" / "outside-verifier.json", report)


if __name__ == "__main__":
    unittest.main()

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


aex_pipl_payload_adapter_review_packet = load_tool("aex_pipl_payload_adapter_review_packet")


SAFETY_FALSE = {
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
    "real_pipl_payload_parser_enabled": False,
    "real_pipl_payload_parsed": False,
    "resource_payload_opened": False,
    "resource_payload_extracted": False,
    "raw_payload_serialized": False,
}


def gate() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_pipl_parser_gate",
        "gate_state": "pipl_parser_gate_closed_no_real_payload",
        "gate_ready_for_review": True,
        "metadata_budget_ready": True,
        "real_pipl_payload_parser_enabled": False,
        "real_pipl_payload_parsed": False,
        "resource_payload_opened": False,
        "raw_payload_serialized": False,
        "parser_input_contract": {
            "state": "metadata_size_budget_ready",
            "real_payload_input_allowed_now": False,
            "proposed_real_parser_limit_bytes": 4096,
        },
        "parser_gate_checks": [{"check_id": "metadata_budget_rows_available", "passed": True}],
        "candidate_budget_rows": [
            {
                "relative_path": "Fixture.aex",
                "parser_gate_action": "eligible_for_future_real_payload_parser_review",
                "pipl_resource_total_size": 326,
                "resource_payload_opened": False,
                "resource_payload_serialized": False,
            }
        ],
        "summary": {
            "candidate_count": 1,
            "eligible_future_parser_candidate_count": 1,
            "hold_candidate_count": 0,
            "parser_gate_action_counts": {"eligible_for_future_real_payload_parser_review": 1},
            "observed_pipl_resource_total_size": 326,
            "observed_pipl_resource_max_size": 326,
            "proposed_real_parser_limit_bytes": 4096,
        },
        **SAFETY_FALSE,
    }


def synthetic_payload_parser(gate_path: Path) -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_synthetic_pipl_payload_parser",
        "source_pipl_parser_gate": str(gate_path.resolve()),
        "source_synthetic_pipl_parser_selftest": str(Path("selftest.json").resolve()),
        "synthetic_payload_parser_state": "synthetic_pipl_payload_parser_ready_real_payload_closed",
        "synthetic_payload_parser_ready": True,
        "synthetic_parser_implemented": True,
        "synthetic_bounds_harness_reused": True,
        "synthetic_payload_cases_passed": True,
        "synthetic_payloads_used": True,
        "synthetic_payloads_serialized": False,
        "real_payload_input_allowed_now": False,
        "output_metadata_only": True,
        "real_pipl_payload_parser_enabled": False,
        "real_pipl_payload_parsed": False,
        "resource_payload_opened": False,
        "raw_payload_serialized": False,
        "parser_case_count": 8,
        "parser_case_passed_count": 8,
        "parser_case_failed_count": 0,
        "parser_api_contract": {
            "state": "synthetic_parser_implementation_ready_real_payload_closed",
            "real_pipl_payload_input_allowed": False,
            "resource_payload_file_input_allowed": False,
            "aex_path_input_allowed": False,
            "raw_payload_output_allowed": False,
            "parameter_schema_output_allowed": False,
        },
        "parser_case_results": [{"case_id": "valid", "passed": True, "raw_payload_serialized": False}],
        "summary": {"parser_case_count": 8, "parser_case_failed_count": 0},
        **SAFETY_FALSE,
    }


def consistency_audit(gate_path: Path) -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_pipl_resource_consistency_audit",
        "source_static_report": str(Path("static.json").resolve()),
        "source_pipl_resource_catalog": str(Path("catalog.json").resolve()),
        "source_pipl_parser_gate": str(gate_path.resolve()),
        "audit_state": "pipl_resource_consistency_audit_passed_no_payload",
        "audit_passed": True,
        "source_chain_valid": True,
        "catalog_summary_recomputed": True,
        "catalog_rows_recomputed": True,
        "gate_budget_rows_recomputed": True,
        "gate_summary_recomputed": True,
        "metadata_consistency_ready": True,
        "real_payload_input_allowed_now": False,
        "real_pipl_payload_parser_enabled": False,
        "real_pipl_payload_parsed": False,
        "resource_payload_opened": False,
        "resource_payload_extracted": False,
        "raw_payload_serialized": False,
        "checks": [{"check_id": "gate_summary_recomputed_from_catalog", "passed": True}],
        "summary": {
            "gate_budget_row_count": 1,
            "pipl_resource_entry_count": 1,
            "pipl_resource_total_size": 326,
            "pipl_resource_max_size": 326,
            "eligible_future_parser_candidate_count": 1,
            "hold_candidate_count": 0,
        },
        **SAFETY_FALSE,
    }


def parameter_schema_review() -> dict:
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
        "parser_review_items": [{"item_id": "bounds", "severity": "blocker"}],
        "redaction_policy": {"state": "redaction_policy_ready_no_schema_output"},
        "ofx_describe_policy": {"state": "ofx_describe_mapping_deferred_until_redacted_schema"},
        "candidate_review_rows": [{"relative_path": "Fixture.aex", "payload_policy": "metadata_anchor_only_no_pipl_payload"}],
        "summary": {
            "candidate_count": 1,
            "payload_parser_required_count": 1,
            "host_contract_review_count": 0,
            "mapping_state_counts": {"primary_schema_mapping_candidate_pending_payload_parser": 1},
        },
        "blockers": ["payload_parser_not_implemented", "redacted_schema_not_emitted"],
        **SAFETY_FALSE,
    }


class AexPiplPayloadAdapterReviewPacketTests(unittest.TestCase):
    def test_builds_review_packet_without_enabling_payload_adapter(self):
        gate_path = Path("gate.json").resolve()
        parser_path = Path("parser.json").resolve()
        audit_path = Path("audit.json").resolve()
        packet = aex_pipl_payload_adapter_review_packet.build_adapter_review_packet(
            pipl_parser_gate=gate(),
            pipl_parser_gate_path=gate_path,
            synthetic_payload_parser=synthetic_payload_parser(gate_path),
            synthetic_payload_parser_path=parser_path,
            consistency_audit=consistency_audit(gate_path),
            consistency_audit_path=audit_path,
            parameter_schema_review=parameter_schema_review(),
            parameter_schema_review_path=Path("schema-review.json").resolve(),
        )

        self.assertEqual(packet["packet_kind"], "aex_pipl_payload_adapter_review_packet")
        self.assertEqual(packet["adapter_review_state"], "pipl_payload_adapter_review_ready_real_payload_closed")
        self.assertTrue(packet["adapter_review_ready"])
        self.assertTrue(packet["source_chain_valid"])
        self.assertTrue(packet["synthetic_parser_contract_reviewed"])
        self.assertTrue(packet["metadata_consistency_reviewed"])
        self.assertTrue(packet["metadata_budget_reviewed"])
        self.assertTrue(packet["parameter_schema_reviewed"])
        self.assertTrue(packet["redaction_policy_reviewed"])
        self.assertTrue(packet["ofx_describe_policy_reviewed"])
        self.assertFalse(packet["real_payload_adapter_allowed_now"])
        self.assertFalse(packet["real_payload_input_allowed_now"])
        self.assertFalse(packet["real_pipl_payload_parser_enabled"])
        self.assertFalse(packet["real_pipl_payload_parsed"])
        self.assertFalse(packet["resource_payload_opened"])
        self.assertFalse(packet["resource_payload_extracted"])
        self.assertFalse(packet["raw_payload_serialized"])
        self.assertTrue(packet["output_metadata_only"])
        self.assertFalse(packet["parameter_schema_emission_allowed_now"])
        self.assertFalse(packet["parameter_schema_emitted"])
        self.assertFalse(packet["redacted_schema_emitted"])
        self.assertFalse(packet["aex_file_opened"])
        self.assertGreater(packet["review_item_count"], 0)
        self.assertEqual(packet["blocking_review_item_count"], packet["review_item_count"])
        self.assertIn("--payload-file", packet["adapter_review_contract"]["current_forbidden_inputs"])
        self.assertIn("parse_real_pipl_payload", packet["adapter_review_contract"]["blocked_actions"])
        self.assertEqual(packet["candidate_review_budget"]["eligible_future_parser_candidate_count"], 1)
        self.assertEqual(packet["candidate_review_budget"]["schema_payload_parser_required_count"], 1)

    def test_rejects_open_real_payload_parser_or_bad_source_chain(self):
        gate_path = Path("gate.json").resolve()
        parser = synthetic_payload_parser(gate_path)
        parser["real_pipl_payload_parser_enabled"] = True
        with self.assertRaises(ValueError) as ctx:
            aex_pipl_payload_adapter_review_packet.build_adapter_review_packet(
                pipl_parser_gate=gate(),
                pipl_parser_gate_path=gate_path,
                synthetic_payload_parser=parser,
                synthetic_payload_parser_path=Path("parser.json"),
                consistency_audit=consistency_audit(gate_path),
                consistency_audit_path=Path("audit.json"),
                parameter_schema_review=parameter_schema_review(),
                parameter_schema_review_path=Path("schema-review.json"),
            )
        self.assertIn("real parser must be disabled", str(ctx.exception))

        parser = synthetic_payload_parser(Path("other-gate.json").resolve())
        with self.assertRaises(ValueError) as ctx:
            aex_pipl_payload_adapter_review_packet.build_adapter_review_packet(
                pipl_parser_gate=gate(),
                pipl_parser_gate_path=gate_path,
                synthetic_payload_parser=parser,
                synthetic_payload_parser_path=Path("parser.json"),
                consistency_audit=consistency_audit(gate_path),
                consistency_audit_path=Path("audit.json"),
                parameter_schema_review=parameter_schema_review(),
                parameter_schema_review_path=Path("schema-review.json"),
            )
        self.assertIn("source_pipl_parser_gate must match gate input", str(ctx.exception))

        schema_review = parameter_schema_review()
        schema_review["payload_parser_enabled"] = True
        with self.assertRaises(ValueError) as ctx:
            aex_pipl_payload_adapter_review_packet.build_adapter_review_packet(
                pipl_parser_gate=gate(),
                pipl_parser_gate_path=gate_path,
                synthetic_payload_parser=synthetic_payload_parser(gate_path),
                synthetic_payload_parser_path=Path("parser.json"),
                consistency_audit=consistency_audit(gate_path),
                consistency_audit_path=Path("audit.json"),
                parameter_schema_review=schema_review,
                parameter_schema_review_path=Path("schema-review.json"),
            )
        self.assertIn("payload_parser_enabled must be false", str(ctx.exception))

    def test_rejects_forbidden_cli_tokens(self):
        aex_pipl_payload_adapter_review_packet.reject_forbidden_cli_inputs(["--pipl-parser-gate", "gate.json"])
        for token in ("--aex", "--payload-file=raw.bin", "--resource-payload", "APPROVE_AEX_LOAD_GATE"):
            with self.assertRaises(ValueError):
                aex_pipl_payload_adapter_review_packet.reject_forbidden_cli_inputs([token])

    def test_paths_are_confined_and_packet_is_create_new(self):
        gate_root = LAB_ROOT / "target" / "pipl-parser-gate"
        parser_root = LAB_ROOT / "target" / "synthetic-pipl-payload-parser"
        audit_root = LAB_ROOT / "target" / "pipl-resource-consistency-audit"
        schema_root = LAB_ROOT / "target" / "parameter-schema-review"
        gate_root.mkdir(parents=True, exist_ok=True)
        parser_root.mkdir(parents=True, exist_ok=True)
        audit_root.mkdir(parents=True, exist_ok=True)
        schema_root.mkdir(parents=True, exist_ok=True)
        stamp = f"{time.time_ns()}-{os.getpid()}"
        gate_path = gate_root / f"{stamp}-gate.local.json"
        parser_path = parser_root / f"{stamp}-parser.local.json"
        audit_path = audit_root / f"{stamp}-audit.local.json"
        schema_path = schema_root / f"{stamp}-schema-review.local.json"
        gate_path.write_text(json.dumps(gate()), encoding="utf-8")
        parser_path.write_text(json.dumps(synthetic_payload_parser(gate_path)), encoding="utf-8")
        audit_path.write_text(json.dumps(consistency_audit(gate_path)), encoding="utf-8")
        schema_path.write_text(json.dumps(parameter_schema_review()), encoding="utf-8")

        loaded_gate, resolved_gate = aex_pipl_payload_adapter_review_packet.load_pipl_parser_gate(gate_path)
        loaded_parser, resolved_parser = aex_pipl_payload_adapter_review_packet.load_synthetic_payload_parser(parser_path)
        loaded_audit, resolved_audit = aex_pipl_payload_adapter_review_packet.load_consistency_audit(audit_path)
        loaded_schema, resolved_schema = aex_pipl_payload_adapter_review_packet.load_parameter_schema_review(schema_path)
        self.assertEqual(resolved_gate, gate_path.resolve())
        self.assertEqual(resolved_parser, parser_path.resolve())
        self.assertEqual(resolved_audit, audit_path.resolve())
        self.assertEqual(resolved_schema, schema_path.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-gate.json"
        outside.write_text(json.dumps(gate()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_pipl_payload_adapter_review_packet.load_pipl_parser_gate(outside)

        packet = aex_pipl_payload_adapter_review_packet.build_adapter_review_packet(
            pipl_parser_gate=loaded_gate,
            pipl_parser_gate_path=resolved_gate,
            synthetic_payload_parser=loaded_parser,
            synthetic_payload_parser_path=resolved_parser,
            consistency_audit=loaded_audit,
            consistency_audit_path=resolved_audit,
            parameter_schema_review=loaded_schema,
            parameter_schema_review_path=resolved_schema,
        )
        out = LAB_ROOT / "target" / "pipl-payload-adapter-review" / f"{time.time_ns()}-{os.getpid()}-review.local.json"
        written = aex_pipl_payload_adapter_review_packet.write_json_create_new(out, packet)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_pipl_payload_adapter_review_packet.write_json_create_new(out, packet)
        with self.assertRaises(ValueError):
            aex_pipl_payload_adapter_review_packet.write_json_create_new(
                LAB_ROOT / "target" / "outside-review.json",
                packet,
            )


if __name__ == "__main__":
    unittest.main()

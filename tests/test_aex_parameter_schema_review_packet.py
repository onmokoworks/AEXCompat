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


aex_parameter_schema_review_packet = load_tool("aex_parameter_schema_review_packet")


def make_schema_plan() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_parameter_schema_plan",
        "plan_state": "parameter_schema_plan_ready_no_payload",
        "schema_plan_ready": True,
        "real_parameter_schema_available": False,
        "payload_parser_enabled": False,
        "payload_policy": "metadata_only_no_pipl_payload",
        "summary": {
            "candidate_count": 2,
            "primary_mapping_candidate_count": 1,
            "payload_parser_required_count": 1,
            "host_contract_review_count": 1,
        },
        "candidate_schema_rows": [
            {
                "relative_path": "AEPluginBuild\\ScatterMap.aex",
                "review_bucket": "primary_fixture_candidate",
                "mapping_state": "primary_schema_mapping_candidate_pending_payload_parser",
                "fixture_candidate_score": 95,
                "payload_policy": "do_not_parse_or_copy_pipl_payload",
            },
            {
                "relative_path": "AEPluginBuild\\Hosty.aex",
                "review_bucket": "hold_for_host_contract_review",
                "mapping_state": "host_contract_review_before_schema_mapping",
                "fixture_candidate_score": 70,
                "payload_policy": "do_not_parse_or_copy_pipl_payload",
            },
        ],
        "blockers": [
            "pipl_payload_parser_disabled",
            "no_parameter_names_defaults_or_ranges",
            "no_redacted_public_schema",
            "no_ofx_describe_mapping",
            "load_gate_closed",
        ],
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
    }


def make_publication_boundary() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_publication_boundary_audit",
        "boundary_state": "local_only_not_publishable",
        "publishable_now": False,
        "publication_blockers": ["local-only", "no redaction review"],
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


def make_ofx_route_contract() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_ofx_route_contract_probe",
        "contract_state": "ofx_route_contract_ready_route_closed",
        "real_route_open": False,
        "mock_route_ready": True,
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
    }


def build_packet(**overrides):
    values = {
        "schema_plan": make_schema_plan(),
        "publication_boundary": make_publication_boundary(),
        "ofx_route_contract": make_ofx_route_contract(),
    }
    values.update(overrides)
    return aex_parameter_schema_review_packet.build_review_packet(
        schema_plan=values["schema_plan"],
        schema_plan_path=Path("schema-plan.json"),
        publication_boundary=values["publication_boundary"],
        publication_boundary_path=Path("publication.json"),
        ofx_route_contract=values["ofx_route_contract"],
        ofx_route_contract_path=Path("ofx.json"),
    )


class AexParameterSchemaReviewPacketTests(unittest.TestCase):
    def test_packet_ready_but_keeps_parser_schema_and_ofx_describe_closed(self):
        packet = build_packet()
        self.assertEqual(packet["packet_kind"], "aex_parameter_schema_review_packet")
        self.assertEqual(packet["review_state"], "parameter_schema_review_ready_no_payload")
        self.assertEqual(packet["parser_design_state"], "payload_parser_design_review_ready_parser_disabled")
        self.assertEqual(packet["redaction_policy_state"], "redaction_policy_ready_no_schema_output")
        self.assertEqual(packet["ofx_describe_policy_state"], "ofx_describe_mapping_deferred_until_redacted_schema")
        self.assertFalse(packet["payload_parser_enabled"])
        self.assertFalse(packet["real_parameter_schema_available"])
        self.assertFalse(packet["redacted_schema_available"])
        self.assertFalse(packet["ofx_describe_mapping_ready"])
        self.assertFalse(packet["pipl_payload_parsed"])
        self.assertFalse(packet["parameter_schema_emitted"])
        self.assertFalse(packet["redacted_schema_emitted"])
        self.assertFalse(packet["ofx_describe_performed"])
        self.assertEqual(packet["summary"]["candidate_count"], 2)
        self.assertIn("raw_pipl_payload_bytes", packet["redaction_policy"]["forbidden_without_additional_approval"])
        self.assertIn("build_ofx_describe_from_aex", packet["ofx_describe_policy"]["blocked_actions"])
        primary = packet["candidate_review_rows"][0]
        self.assertEqual(primary["parser_action"], "first_parser_contract_candidate_after_approval")
        self.assertEqual(primary["payload_policy"], "metadata_anchor_only_no_pipl_payload")

    def test_invalid_sources_are_rejected(self):
        plan = make_schema_plan()
        plan["payload_parser_enabled"] = True
        with self.assertRaises(ValueError):
            build_packet(schema_plan=plan)

        boundary = make_publication_boundary()
        boundary["publishable_now"] = True
        with self.assertRaises(ValueError):
            build_packet(publication_boundary=boundary)

        ofx = make_ofx_route_contract()
        ofx["real_route_open"] = True
        with self.assertRaises(ValueError):
            build_packet(ofx_route_contract=ofx)

    def test_paths_are_confined_and_packet_is_create_new(self):
        roots = {
            "plan": LAB_ROOT / "target" / "parameter-schema-plan",
            "publication": LAB_ROOT / "target" / "publication-boundary",
            "ofx": LAB_ROOT / "target" / "ofx-route-contract",
        }
        for root in roots.values():
            root.mkdir(parents=True, exist_ok=True)
        stamp = f"{time.time_ns()}-{os.getpid()}"
        paths = {
            "plan": roots["plan"] / f"{stamp}-schema-plan.local.json",
            "publication": roots["publication"] / f"{stamp}-publication.local.json",
            "ofx": roots["ofx"] / f"{stamp}-ofx.local.json",
        }
        payloads = {
            "plan": make_schema_plan(),
            "publication": make_publication_boundary(),
            "ofx": make_ofx_route_contract(),
        }
        for label, path in paths.items():
            path.write_text(json.dumps(payloads[label]), encoding="utf-8")

        plan, plan_path = aex_parameter_schema_review_packet.load_schema_plan(paths["plan"])
        publication, publication_path = aex_parameter_schema_review_packet.load_publication_boundary(paths["publication"])
        ofx, ofx_path = aex_parameter_schema_review_packet.load_ofx_route_contract(paths["ofx"])
        self.assertEqual(plan_path, paths["plan"].resolve())
        self.assertEqual(publication_path, paths["publication"].resolve())
        self.assertEqual(ofx_path, paths["ofx"].resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-review.json"
        outside.write_text(json.dumps(make_schema_plan()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_parameter_schema_review_packet.load_schema_plan(outside)

        packet = aex_parameter_schema_review_packet.build_review_packet(
            schema_plan=plan,
            schema_plan_path=plan_path,
            publication_boundary=publication,
            publication_boundary_path=publication_path,
            ofx_route_contract=ofx,
            ofx_route_contract_path=ofx_path,
        )
        out = LAB_ROOT / "target" / "parameter-schema-review" / f"{time.time_ns()}-{os.getpid()}-schema-review.local.json"
        written = aex_parameter_schema_review_packet.write_json_create_new(out, packet)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_parameter_schema_review_packet.write_json_create_new(out, packet)
        with self.assertRaises(ValueError):
            aex_parameter_schema_review_packet.write_json_create_new(LAB_ROOT / "target" / "outside-review.json", packet)


if __name__ == "__main__":
    unittest.main()

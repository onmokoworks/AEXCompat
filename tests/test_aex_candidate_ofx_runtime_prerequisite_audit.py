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


aex_candidate_ofx_runtime_prerequisite_audit = load_tool(
    "aex_candidate_ofx_runtime_prerequisite_audit"
)


CANDIDATE = r"AEPluginBuild\ScatterMap.aex"
SAFETY_FALSE = {
    "native_load_enabled": False,
    "native_load_performed": False,
    "dll_load_performed": False,
    "render_performed": False,
    "ae_invoked": False,
    "ofx_route_invoked": False,
    "ofx_runtime_invoked": False,
    "host_process_launch_enabled": False,
    "private_payload_copied": False,
    "aex_file_opened": False,
    "aex_file_hashed": False,
    "aex_file_copied": False,
    "aepx_file_modified": False,
    "aep_binary_modified": False,
    "ae_project_write_performed": False,
    "ofx_plugin_built": False,
    "ofx_describe_performed": False,
    "ofx_render_performed": False,
    "aex_render_performed": False,
    "render_validation_performed": False,
    "ppm_pixel_read_performed": False,
    "pipl_payload_parsed": False,
    "parameter_schema_emitted": False,
    "redacted_schema_emitted": False,
    "real_pipl_payload_parser_enabled": False,
    "real_pipl_payload_parsed": False,
    "resource_payload_opened": False,
    "resource_payload_extracted": False,
    "raw_payload_serialized": False,
}


def approval_verifier_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_ofx_runtime_approval_verifier",
        "candidate_relative_path": CANDIDATE,
        "runtime_approval_verifier_state": "candidate_ofx_runtime_approval_verifier_ready_no_approval",
        "runtime_approval_verifier_ready": True,
        "runtime_approval_verified_not_approved": True,
        "current_runtime_approval_valid": False,
        "runtime_approval_satisfied": False,
        "runtime_approval_manifest_created": False,
        "runtime_approval_can_be_issued_now": False,
        "runtime_approval_gate_stays_closed": True,
        "boundary_contract_cross_checked": True,
        "boundary_contract_matches_request": True,
        "explicit_runtime_approval_present": False,
        "approval_blocker_count": 5,
        "request_blockers_clear": False,
        "fixture_approval_satisfied": False,
        "ofx_host_binary_review_ready": False,
        "runtime_containment_selftest_ready": False,
        "schema_and_render_validation_ready": False,
        "path_acceptance_closed": True,
        "synthetic_runtime_approval_checks_passed": True,
        "ofx_runtime_invocation_ready": False,
        "host_process_launch_enabled": False,
        "path_acceptance_ready": False,
        "aex_path_acceptance_enabled": False,
        "accepted_aex_path": None,
        "real_route_open": False,
        "ofx_runtime_invoked": False,
        "ppm_pixel_read_performed": False,
        "runtime_approval_verifier_path_payload_exported": False,
        **SAFETY_FALSE,
    }


def runtime_selftest_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_native_loader_runtime_selftest",
        "runtime_selftest_state": "runtime_containment_selftest_passed_no_load",
        "runtime_containment_selftest_passed": True,
        "runtime_containment_ready": True,
        "path_acceptance_ready": False,
        "aex_path_acceptance_enabled": False,
        "accepted_aex_path": None,
        "path_payload_supplied": False,
        "synthetic_subprocess_only": True,
        "normal_exit_case_passed": True,
        "stderr_capture_passed": True,
        "timeout_case_passed": True,
        "child_cleanup_passed": True,
        **SAFETY_FALSE,
    }


def render_contract_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_render_validation_contract",
        "contract_state": "render_validation_contract_ready_render_closed",
        "no_load_validation_ready": True,
        "real_render_open": False,
        "render_contract": {"real_render_open": False},
        **SAFETY_FALSE,
    }


def parameter_schema_review_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aex_parameter_schema_review_packet",
        "review_state": "parameter_schema_review_ready_no_payload",
        "parser_design_state": "payload_parser_design_review_ready_parser_disabled",
        "redaction_policy_state": "redaction_policy_ready_no_schema_output",
        "ofx_describe_policy_state": "ofx_describe_mapping_deferred_until_redacted_schema",
        "payload_parser_enabled": False,
        "redacted_schema_available": False,
        "ofx_describe_mapping_ready": False,
        **SAFETY_FALSE,
    }


def build_report(
    verifier: dict | None = None,
    runtime_selftest: dict | None = None,
    render_contract: dict | None = None,
    schema_review: dict | None = None,
) -> dict:
    return aex_candidate_ofx_runtime_prerequisite_audit.build_runtime_prerequisite_audit(
        runtime_approval_verifier=verifier or approval_verifier_payload(),
        runtime_approval_verifier_path=(
            LAB_ROOT
            / "target"
            / "candidate-ofx-runtime-approval-verifier"
            / "verifier.local.json"
        ),
        runtime_selftest=runtime_selftest or runtime_selftest_payload(),
        runtime_selftest_path=LAB_ROOT / "target" / "native-loader-runtime-selftest" / "selftest.local.json",
        render_validation_contract=render_contract or render_contract_payload(),
        render_validation_contract_path=LAB_ROOT / "target" / "render-validation-contract" / "render.local.json",
        parameter_schema_review=schema_review or parameter_schema_review_payload(),
        parameter_schema_review_path=LAB_ROOT / "target" / "parameter-schema-review" / "schema.local.json",
    )


class AexCandidateOfxRuntimePrerequisiteAuditTests(unittest.TestCase):
    def test_builds_audit_with_ready_no_load_evidence_and_remaining_blockers(self):
        report = build_report()
        self.assertEqual(report["report_kind"], "aex_candidate_ofx_runtime_prerequisite_audit")
        self.assertEqual(
            report["runtime_prerequisite_audit_state"],
            "candidate_ofx_runtime_prerequisite_audit_ready_gates_closed",
        )
        self.assertTrue(report["runtime_prerequisite_audit_ready"])
        self.assertFalse(report["runtime_prerequisites_complete"])
        self.assertFalse(report["runtime_prerequisites_all_satisfied"])
        self.assertFalse(report["runtime_invocation_prerequisites_ready"])
        self.assertFalse(report["approval_can_be_issued_now"])
        self.assertEqual(report["failed_evidence_count"], 0)
        self.assertEqual(report["blocking_prerequisite_count"], 4)
        self.assertEqual(report["runtime_prerequisite_count"], 8)
        self.assertEqual(report["runtime_prerequisite_satisfied_count"], 4)
        self.assertEqual(report["runtime_prerequisite_blocker_count"], 4)
        self.assertEqual(report["audit_summary"]["failed_evidence_count"], 0)
        self.assertTrue(report["runtime_approval_verified_not_approved"])
        self.assertFalse(report["current_runtime_approval_valid"])
        self.assertFalse(report["runtime_approval_satisfied"])
        self.assertTrue(report["boundary_contract_cross_checked"])
        self.assertFalse(report["explicit_runtime_approval_present"])
        self.assertFalse(report["fixture_approval_satisfied"])
        self.assertFalse(report["ofx_host_binary_review_ready"])
        self.assertTrue(report["runtime_containment_contract_ready"])
        self.assertFalse(report["runtime_containment_selftest_ready"])
        self.assertTrue(report["runtime_containment_selftest_synthetic_passed"])
        self.assertTrue(report["runtime_containment_selftest_passed"])
        self.assertTrue(report["parameter_schema_review_policy_ready"])
        self.assertFalse(report["payload_parser_enabled"])
        self.assertFalse(report["redacted_schema_available"])
        self.assertTrue(report["render_validation_contract_ready"])
        self.assertTrue(report["no_load_validation_ready"])
        self.assertFalse(report["schema_and_render_validation_ready"])
        self.assertTrue(report["ofx_route_contract_closed"])
        self.assertFalse(report["ofx_runtime_invocation_ready"])
        self.assertFalse(report["host_process_launch_enabled"])
        self.assertFalse(report["path_acceptance_ready"])
        self.assertFalse(report["real_route_open"])
        self.assertTrue(report["mock_route_ready"])
        self.assertFalse(report["ofx_runtime_invoked"])
        self.assertFalse(report["ppm_pixel_read_performed"])
        self.assertFalse(report["runtime_prerequisite_audit_path_payload_exported"])
        blocker_ids = {item["id"] for item in report["runtime_prerequisite_blockers"]}
        self.assertEqual(
            blocker_ids,
            {
                "explicit_runtime_approval",
                "fixture_approval",
                "ofx_host_binary_review",
                "real_schema_and_render_validation",
            },
        )

    def test_rejects_unsafe_runtime_selftest(self):
        selftest = runtime_selftest_payload()
        selftest["path_acceptance_ready"] = True
        with self.assertRaises(ValueError) as ctx:
            build_report(runtime_selftest=selftest)
        self.assertIn("runtime selftest path_acceptance_ready must be false", str(ctx.exception))

    def test_rejects_open_render_contract(self):
        render = render_contract_payload()
        render["real_render_open"] = True
        with self.assertRaises(ValueError) as ctx:
            build_report(render_contract=render)
        self.assertIn("render validation contract real_render_open must be false", str(ctx.exception))

    def test_paths_are_confined_and_output_is_create_new(self):
        roots = {
            "verifier": LAB_ROOT / "target" / "candidate-ofx-runtime-approval-verifier",
            "selftest": LAB_ROOT / "target" / "native-loader-runtime-selftest",
            "render": LAB_ROOT / "target" / "render-validation-contract",
            "schema": LAB_ROOT / "target" / "parameter-schema-review",
        }
        for root in roots.values():
            root.mkdir(parents=True, exist_ok=True)
        stamp = f"{time.time_ns()}-{os.getpid()}"
        paths = {
            "verifier": roots["verifier"] / f"ae-candidate-ofx-runtime-approval-verifier-{stamp}.local.json",
            "selftest": roots["selftest"] / f"ae-native-loader-runtime-selftest-{stamp}.local.json",
            "render": roots["render"] / f"ae-render-validation-contract-{stamp}.local.json",
            "schema": roots["schema"] / f"ae-parameter-schema-review-{stamp}.local.json",
        }
        paths["verifier"].write_text(json.dumps(approval_verifier_payload()), encoding="utf-8")
        paths["selftest"].write_text(json.dumps(runtime_selftest_payload()), encoding="utf-8")
        paths["render"].write_text(json.dumps(render_contract_payload()), encoding="utf-8")
        paths["schema"].write_text(json.dumps(parameter_schema_review_payload()), encoding="utf-8")

        verifier, verifier_path = aex_candidate_ofx_runtime_prerequisite_audit.load_runtime_approval_verifier(
            Path("target") / "candidate-ofx-runtime-approval-verifier" / paths["verifier"].name
        )
        selftest, selftest_path = aex_candidate_ofx_runtime_prerequisite_audit.load_runtime_selftest(
            Path("target") / "native-loader-runtime-selftest" / paths["selftest"].name
        )
        render, render_path = aex_candidate_ofx_runtime_prerequisite_audit.load_render_validation_contract(
            Path("target") / "render-validation-contract" / paths["render"].name
        )
        schema, schema_path = aex_candidate_ofx_runtime_prerequisite_audit.load_parameter_schema_review(
            Path("target") / "parameter-schema-review" / paths["schema"].name
        )

        outside = LAB_ROOT / "target" / f"{stamp}-outside-runtime-prerequisite.local.json"
        outside.write_text(json.dumps(approval_verifier_payload()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_candidate_ofx_runtime_prerequisite_audit.load_runtime_approval_verifier(outside)

        report = aex_candidate_ofx_runtime_prerequisite_audit.build_runtime_prerequisite_audit(
            runtime_approval_verifier=verifier,
            runtime_approval_verifier_path=verifier_path,
            runtime_selftest=selftest,
            runtime_selftest_path=selftest_path,
            render_validation_contract=render,
            render_validation_contract_path=render_path,
            parameter_schema_review=schema,
            parameter_schema_review_path=schema_path,
        )
        out = (
            LAB_ROOT
            / "target"
            / "candidate-ofx-runtime-prerequisite-audit"
            / f"ae-candidate-ofx-runtime-prerequisite-audit-{stamp}.local.json"
        )
        written = aex_candidate_ofx_runtime_prerequisite_audit.write_json_create_new(out, report)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_candidate_ofx_runtime_prerequisite_audit.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aex_candidate_ofx_runtime_prerequisite_audit.write_json_create_new(
                LAB_ROOT / "target" / "outside-candidate-ofx-runtime-prerequisite-audit.json",
                report,
            )


if __name__ == "__main__":
    unittest.main()

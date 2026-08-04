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


aex_candidate_ofx_runtime_approval_verifier = load_tool(
    "aex_candidate_ofx_runtime_approval_verifier"
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


def approval_request_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_ofx_runtime_approval_request_packet",
        "candidate_relative_path": CANDIDATE,
        "runtime_approval_request_state": "candidate_ofx_runtime_approval_request_ready_pending_manual_approval",
        "runtime_approval_request_ready": True,
        "runtime_approval_request_created": True,
        "runtime_approval_can_be_issued_now": False,
        "runtime_approval_manifest_created": False,
        "runtime_approval_gate_stays_closed": True,
        "approval_request_kind": "ofx_runtime_invocation_manual_approval_request",
        "requires_explicit_user_approval": True,
        "required_approval_token_name": "APPROVE_OFX_RUNTIME_INVOCATION",
        "approval_token_not_stored_in_manifest": True,
        "approval_only_prepares_runtime_review": True,
        "source_boundary_contract_state": "candidate_ofx_runtime_boundary_contract_ready_no_runtime_route_closed",
        "source_boundary_contract_ready": True,
        "source_contract_state": "candidate_ofx_runtime_boundary_contract_ready_runtime_closed",
        "source_ofx_runtime_allowed_now": False,
        "source_ofx_runtime_invocation_ready": False,
        "source_host_process_launch_enabled": False,
        "source_path_acceptance_ready": False,
        "source_real_route_open": False,
        "source_mock_route_ready": True,
        "source_ofx_runtime_invoked": False,
        "source_ppm_pixel_read_performed": False,
        "source_fixture_approval_satisfied": False,
        "source_requires_future_runtime_approval": True,
        "source_requires_future_fixture_approval": True,
        "source_requires_future_render_validation_approval": True,
        "review_checklist": [{"id": "explicit_runtime_approval"}],
        "review_checklist_count": 6,
        "approval_blockers": [{"id": "explicit_runtime_approval"}],
        "approval_blocker_count": 5,
        "required_before_runtime_invocation": ["explicit user approval"],
        "blocked_actions_after_request": [
            "instantiate_ofx_runtime",
            "launch_ofx_host_process",
            "accept_ofx_host_path",
            "accept_ofx_plugin_binary_path",
            "load_ofx_plugin",
            "ofx_describe_from_aex",
            "ofx_render_with_aex",
        ],
        "blocked_actions": ["instantiate_ofx_runtime", "launch_ofx_host_process"],
        "ofx_runtime_allowed_now": False,
        "ofx_runtime_invocation_ready": False,
        "ofx_runtime_instantiation_performed": False,
        "host_process_launch_enabled": False,
        "ofx_host_path_payload_supplied": False,
        "ofx_plugin_binary_path_payload_supplied": False,
        "path_acceptance_ready": False,
        "aex_path_acceptance_enabled": False,
        "accepted_aex_path": None,
        "real_route_open": False,
        "real_ofx_route_ready": False,
        "mock_route_ready": True,
        "ofx_runtime_invoked": False,
        "aex_runtime_invoked": False,
        "ofx_describe_ready": False,
        "ofx_render_ready": False,
        "render_equivalence_claim_ready": False,
        "ppm_pixel_read_performed": False,
        "absolute_ppm_paths_exported": False,
        "absolute_aex_paths_exported": False,
        "runtime_approval_path_payload_exported": False,
        **SAFETY_FALSE,
    }


def boundary_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_ofx_runtime_boundary_contract",
        "candidate_relative_path": CANDIDATE,
        "candidate_ofx_runtime_boundary_contract_state": (
            "candidate_ofx_runtime_boundary_contract_ready_no_runtime_route_closed"
        ),
        "contract_state": "candidate_ofx_runtime_boundary_contract_ready_runtime_closed",
        "runtime_boundary_ready": True,
        "ofx_runtime_allowed_now": False,
        "ofx_runtime_invocation_ready": False,
        "host_process_launch_enabled": False,
        "path_acceptance_ready": False,
        "real_route_open": False,
        "mock_route_ready": True,
        "ofx_runtime_invoked": False,
        "ppm_pixel_read_performed": False,
        "source_fixture_approval_satisfied": False,
        "requires_future_runtime_approval": True,
        "requires_future_fixture_approval": True,
        "requires_future_render_validation_approval": True,
        "runtime_boundary_path_payload_exported": False,
        "accepted_aex_path": None,
        "blocked_actions": [
            "instantiate_ofx_runtime",
            "launch_ofx_host_process",
            "accept_ofx_host_path",
            "accept_ofx_plugin_binary_path",
            "ofx_describe_from_aex",
            "ofx_render_with_aex",
        ],
        **SAFETY_FALSE,
    }


def future_approval_manifest() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "manifest_kind": "aex_candidate_ofx_runtime_approval_manifest",
        "approval_state": "user_approved_for_ofx_runtime_review",
        "explicit_user_approval": True,
        "candidate_relative_path": CANDIDATE,
        "approved_actions": ["prepare_ofx_runtime_review"],
        **SAFETY_FALSE,
    }


def build_report(payload: dict | None = None, boundary: dict | None = None) -> dict:
    return aex_candidate_ofx_runtime_approval_verifier.build_runtime_approval_verifier(
        runtime_approval_request=payload or approval_request_payload(),
        runtime_approval_request_path=(
            LAB_ROOT
            / "target"
            / "candidate-ofx-runtime-approval-request"
            / "request.local.json"
        ),
        runtime_boundary_contract=boundary if boundary is not None else boundary_payload(),
        runtime_boundary_contract_path=(
            LAB_ROOT
            / "target"
            / "candidate-ofx-runtime-boundary-contract"
            / "boundary.local.json"
        ),
    )


class AexCandidateOfxRuntimeApprovalVerifierTests(unittest.TestCase):
    def test_builds_verifier_for_current_request_without_approval(self):
        report = build_report()
        self.assertEqual(report["report_kind"], "aex_candidate_ofx_runtime_approval_verifier")
        self.assertEqual(
            report["runtime_approval_verifier_state"],
            "candidate_ofx_runtime_approval_verifier_ready_no_approval",
        )
        self.assertTrue(report["runtime_approval_verifier_ready"])
        self.assertTrue(report["runtime_approval_request_ready"])
        self.assertTrue(report["runtime_approval_verified_not_approved"])
        self.assertFalse(report["current_runtime_approval_valid"])
        self.assertFalse(report["runtime_approval_satisfied"])
        self.assertTrue(report["runtime_approval_gate_stays_closed"])
        self.assertTrue(report["runtime_approval_gate_closed"])
        self.assertTrue(report["boundary_contract_cross_checked"])
        self.assertTrue(report["boundary_contract_matches_request"])
        self.assertFalse(report["explicit_runtime_approval_present"])
        self.assertEqual(report["required_approval_token_name"], "APPROVE_OFX_RUNTIME_INVOCATION")
        self.assertTrue(report["approval_token_not_stored_in_manifest"])
        self.assertTrue(report["approval_only_prepares_runtime_review"])
        self.assertEqual(report["approval_blocker_count"], 5)
        self.assertFalse(report["request_blockers_clear"])
        self.assertFalse(report["fixture_approval_satisfied"])
        self.assertFalse(report["ofx_host_binary_review_ready"])
        self.assertFalse(report["runtime_containment_selftest_ready"])
        self.assertFalse(report["schema_and_render_validation_ready"])
        self.assertTrue(report["path_acceptance_closed"])
        self.assertFalse(report["runtime_approval_verifier_path_payload_exported"])
        self.assertTrue(report["synthetic_runtime_approval_checks_passed"])
        self.assertIn(
            "manifest_kind_not_runtime_approval",
            report["current_runtime_approval_evaluation"]["reasons"],
        )
        self.assertFalse(report["ofx_runtime_invocation_ready"])
        self.assertFalse(report["host_process_launch_enabled"])
        self.assertFalse(report["path_acceptance_ready"])
        self.assertFalse(report["real_route_open"])
        self.assertFalse(report["ofx_runtime_invoked"])
        self.assertFalse(report["ppm_pixel_read_performed"])
        self.assertFalse(report["aex_file_opened"])

    def test_evaluates_future_valid_runtime_approval_shape(self):
        evaluation = aex_candidate_ofx_runtime_approval_verifier.evaluate_runtime_approval_manifest(
            future_approval_manifest(),
            candidate_relative_path=CANDIDATE,
            request_ready=True,
            request_blockers_clear=True,
            fixture_approval_satisfied=True,
            runtime_boundary_ready=True,
            host_binary_review_ready=True,
            runtime_containment_selftest_ready=True,
            schema_and_render_validation_ready=True,
            path_acceptance_closed=True,
        )
        self.assertTrue(evaluation["valid"])
        self.assertTrue(evaluation["approval_only_prepares_runtime_review"])
        self.assertEqual(evaluation["forbidden_approved_actions"], [])

    def test_rejects_future_approval_with_runtime_action(self):
        manifest = future_approval_manifest()
        manifest["approved_actions"] = ["prepare_ofx_runtime_review", "instantiate_ofx_runtime"]
        evaluation = aex_candidate_ofx_runtime_approval_verifier.evaluate_runtime_approval_manifest(
            manifest,
            candidate_relative_path=CANDIDATE,
            request_ready=True,
            request_blockers_clear=True,
            fixture_approval_satisfied=True,
            runtime_boundary_ready=True,
            host_binary_review_ready=True,
            runtime_containment_selftest_ready=True,
            schema_and_render_validation_ready=True,
            path_acceptance_closed=True,
        )
        self.assertFalse(evaluation["valid"])
        self.assertIn("forbidden_runtime_action_approved", evaluation["reasons"])
        self.assertEqual(evaluation["forbidden_approved_actions"], ["instantiate_ofx_runtime"])

    def test_rejects_request_with_open_runtime_flag(self):
        payload = approval_request_payload()
        payload["ofx_runtime_invocation_ready"] = True
        with self.assertRaises(ValueError) as ctx:
            build_report(payload)
        self.assertIn("runtime approval request ofx_runtime_invocation_ready must be false", str(ctx.exception))

    def test_rejects_boundary_mismatch(self):
        boundary = boundary_payload()
        boundary["real_route_open"] = True
        with self.assertRaises(ValueError) as ctx:
            build_report(boundary=boundary)
        self.assertIn("runtime boundary real_route_open must match approval request", str(ctx.exception))

    def test_paths_are_confined_and_output_is_create_new(self):
        root = LAB_ROOT / "target" / "candidate-ofx-runtime-approval-request"
        boundary_root = LAB_ROOT / "target" / "candidate-ofx-runtime-boundary-contract"
        root.mkdir(parents=True, exist_ok=True)
        boundary_root.mkdir(parents=True, exist_ok=True)
        stamp = f"{time.time_ns()}-{os.getpid()}"
        source = root / f"ae-candidate-ofx-runtime-approval-request-{stamp}.local.json"
        boundary_source = boundary_root / f"ae-candidate-ofx-runtime-boundary-contract-{stamp}.local.json"
        source.write_text(json.dumps(approval_request_payload()), encoding="utf-8")
        boundary_source.write_text(json.dumps(boundary_payload()), encoding="utf-8")
        request, resolved_request = aex_candidate_ofx_runtime_approval_verifier.load_runtime_approval_request(
            Path("target") / "candidate-ofx-runtime-approval-request" / source.name
        )
        boundary, resolved_boundary = aex_candidate_ofx_runtime_approval_verifier.load_runtime_boundary_contract(
            Path("target") / "candidate-ofx-runtime-boundary-contract" / boundary_source.name
        )
        report = aex_candidate_ofx_runtime_approval_verifier.build_runtime_approval_verifier(
            runtime_approval_request=request,
            runtime_approval_request_path=resolved_request,
            runtime_boundary_contract=boundary,
            runtime_boundary_contract_path=resolved_boundary,
        )

        outside = LAB_ROOT / "target" / f"{stamp}-outside-runtime-approval-request.local.json"
        outside.write_text(json.dumps(approval_request_payload()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_candidate_ofx_runtime_approval_verifier.load_runtime_approval_request(outside)

        out = (
            LAB_ROOT
            / "target"
            / "candidate-ofx-runtime-approval-verifier"
            / f"ae-candidate-ofx-runtime-approval-verifier-{stamp}.local.json"
        )
        written = aex_candidate_ofx_runtime_approval_verifier.write_json_create_new(out, report)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_candidate_ofx_runtime_approval_verifier.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aex_candidate_ofx_runtime_approval_verifier.write_json_create_new(
                LAB_ROOT / "target" / "outside-candidate-ofx-runtime-approval-verifier.json",
                report,
            )


if __name__ == "__main__":
    unittest.main()

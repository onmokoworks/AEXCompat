"""Readiness requirement construction group extracted from the CLI facade."""

from __future__ import annotations

from typing import Any

try:
    from aex_readiness_matrix_core import artifact_state, requirement
except ModuleNotFoundError:
    from tools.aex_readiness_matrix_core import artifact_state, requirement


def build_requirement_group_b(state: dict[str, Any]) -> list[dict[str, Any]]:
    (
        artifacts,
        clean_evidence,
        static_ready,
        pipl_catalog_ready,
        parameter_schema_plan_ready,
        parameter_schema_review_ready,
        redacted_schema_verifier_ready,
        synthetic_pipl_parser_selftest_ready,
        pipl_parser_gate_ready,
        synthetic_pipl_payload_parser_ready,
        pipl_resource_consistency_audit_ready,
        pipl_payload_adapter_review_ready,
        aepx_probe_ready,
        aepx_edit_plan_ready,
        aepx_roundtrip_ready,
        aepx_redacted_text_inventory_ready,
        aepx_redacted_text_classifier_ready,
        matrix_ready,
        dependency_ready,
        dependency_preflight_ready,
        dependency_review_ready,
        sandbox_policy_ready,
        image_suite_ready,
        image_validation_ready,
        image_suite_selftest_ready,
        image_input_smoke_ready,
        render_contract_ready,
        fixture_ready,
        fixture_manual_review_ready,
        fixture_provenance_review_ready,
        fixture_provenance_answer_template_ready,
        fixture_provenance_answer_validator_selftest_ready,
        fixture_approval_verifier_ready,
        fixture_approval_request_ready,
        candidate_test_handoff_ready,
        candidate_test_runner_dryrun_ready,
        candidate_test_runner_ready,
        candidate_compatibility_card_ready,
        candidate_image_compat_mock_check,
        candidate_image_compat_mock_ready,
        candidate_ofx_bridge_ready,
        candidate_ofx_host_harness_dryrun_ready,
        candidate_ofx_host_harness_selftest_ready,
        candidate_ofx_runtime_boundary_contract_ready,
        candidate_ofx_runtime_approval_request_ready,
        candidate_ofx_runtime_approval_verifier_ready,
        candidate_ofx_runtime_prerequisite_audit_ready,
        candidate_ofx_host_binary_review_request_ready,
        candidate_dependency_scope_ready,
        candidate_load_gate_dryrun_ready,
        worker_ready,
        gate_closed,
        loader_refused,
        native_loader_design_ready,
        native_loader_broker_selftest_ready,
        native_loader_runtime_contract_ready,
        native_loader_runtime_selftest_ready,
        native_loader_path_policy_selftest_ready,
        ofx_deferred,
        ofx_mock_ready,
        ofx_suite_ready,
        ofx_contract_ready,
        publication_local_only,
    ) = (
        state["artifacts"],
        state["clean_evidence"],
        state["static_ready"],
        state["pipl_catalog_ready"],
        state["parameter_schema_plan_ready"],
        state["parameter_schema_review_ready"],
        state["redacted_schema_verifier_ready"],
        state["synthetic_pipl_parser_selftest_ready"],
        state["pipl_parser_gate_ready"],
        state["synthetic_pipl_payload_parser_ready"],
        state["pipl_resource_consistency_audit_ready"],
        state["pipl_payload_adapter_review_ready"],
        state["aepx_probe_ready"],
        state["aepx_edit_plan_ready"],
        state["aepx_roundtrip_ready"],
        state["aepx_redacted_text_inventory_ready"],
        state["aepx_redacted_text_classifier_ready"],
        state["matrix_ready"],
        state["dependency_ready"],
        state["dependency_preflight_ready"],
        state["dependency_review_ready"],
        state["sandbox_policy_ready"],
        state["image_suite_ready"],
        state["image_validation_ready"],
        state["image_suite_selftest_ready"],
        state["image_input_smoke_ready"],
        state["render_contract_ready"],
        state["fixture_ready"],
        state["fixture_manual_review_ready"],
        state["fixture_provenance_review_ready"],
        state["fixture_provenance_answer_template_ready"],
        state["fixture_provenance_answer_validator_selftest_ready"],
        state["fixture_approval_verifier_ready"],
        state["fixture_approval_request_ready"],
        state["candidate_test_handoff_ready"],
        state["candidate_test_runner_dryrun_ready"],
        state["candidate_test_runner_ready"],
        state["candidate_compatibility_card_ready"],
        state["candidate_image_compat_mock_check"],
        state["candidate_image_compat_mock_ready"],
        state["candidate_ofx_bridge_ready"],
        state["candidate_ofx_host_harness_dryrun_ready"],
        state["candidate_ofx_host_harness_selftest_ready"],
        state["candidate_ofx_runtime_boundary_contract_ready"],
        state["candidate_ofx_runtime_approval_request_ready"],
        state["candidate_ofx_runtime_approval_verifier_ready"],
        state["candidate_ofx_runtime_prerequisite_audit_ready"],
        state["candidate_ofx_host_binary_review_request_ready"],
        state["candidate_dependency_scope_ready"],
        state["candidate_load_gate_dryrun_ready"],
        state["worker_ready"],
        state["gate_closed"],
        state["loader_refused"],
        state["native_loader_design_ready"],
        state["native_loader_broker_selftest_ready"],
        state["native_loader_runtime_contract_ready"],
        state["native_loader_runtime_selftest_ready"],
        state["native_loader_path_policy_selftest_ready"],
        state["ofx_deferred"],
        state["ofx_mock_ready"],
        state["ofx_suite_ready"],
        state["ofx_contract_ready"],
        state["publication_local_only"],
    )
    return [
        requirement(
            "candidate_ofx_bridge",
            "Candidate OFX bridge packet",
            "satisfied_deferred" if clean_evidence and candidate_ofx_bridge_ready else "failed",
            [
                "candidate_compatibility_card",
                "candidate_image_compat_mock",
                "ofx_facade",
                "ofx_route_contract",
                "candidate_ofx_bridge",
            ],
            {
                "bridge_state": artifact_state(artifacts, "candidate_ofx_bridge", "bridge_state"),
                "bridge_ready": artifact_state(artifacts, "candidate_ofx_bridge", "bridge_ready"),
                "candidate_image_mock_available": artifact_state(
                    artifacts, "candidate_ofx_bridge", "candidate_image_mock_available"
                ),
                "ofx_closed_route_contract_available": artifact_state(
                    artifacts, "candidate_ofx_bridge", "ofx_closed_route_contract_available"
                ),
                "source_compatibility_card_state": artifact_state(
                    artifacts, "candidate_ofx_bridge", "source_compatibility_card_state"
                ),
                "source_image_mock_state": artifact_state(
                    artifacts, "candidate_ofx_bridge", "source_image_mock_state"
                ),
                "source_ofx_facade_state": artifact_state(
                    artifacts, "candidate_ofx_bridge", "source_ofx_facade_state"
                ),
                "source_ofx_route_contract_state": artifact_state(
                    artifacts, "candidate_ofx_bridge", "source_ofx_route_contract_state"
                ),
                "bridge_allowed_route": artifact_state(
                    artifacts, "candidate_ofx_bridge", "bridge_allowed_route"
                ),
                "mock_route_ready": artifact_state(artifacts, "candidate_ofx_bridge", "mock_route_ready"),
                "real_route_open": artifact_state(artifacts, "candidate_ofx_bridge", "real_route_open"),
                "real_ofx_route_ready": artifact_state(
                    artifacts, "candidate_ofx_bridge", "real_ofx_route_ready"
                ),
                "ofx_runtime_invoked": artifact_state(
                    artifacts, "candidate_ofx_bridge", "ofx_runtime_invoked"
                ),
                "aex_runtime_invoked": artifact_state(
                    artifacts, "candidate_ofx_bridge", "aex_runtime_invoked"
                ),
                "ofx_describe_ready": artifact_state(
                    artifacts, "candidate_ofx_bridge", "ofx_describe_ready"
                ),
                "ofx_render_ready": artifact_state(artifacts, "candidate_ofx_bridge", "ofx_render_ready"),
                "render_equivalence_claim_ready": artifact_state(
                    artifacts, "candidate_ofx_bridge", "render_equivalence_claim_ready"
                ),
                "absolute_ppm_paths_exported": artifact_state(
                    artifacts, "candidate_ofx_bridge", "absolute_ppm_paths_exported"
                ),
                "absolute_aex_paths_exported": artifact_state(
                    artifacts, "candidate_ofx_bridge", "absolute_aex_paths_exported"
                ),
                "ofx_bridge_path_payload_exported": artifact_state(
                    artifacts, "candidate_ofx_bridge", "ofx_bridge_path_payload_exported"
                ),
            },
            "The selected candidate image mock is now tied to the closed OFX facade/route contract as JSON-only handoff evidence.",
            "Use this bridge for no-op OFX harness planning only; real OFX describe/render and AEX-backed routes remain closed.",
        ),
        requirement(
            "candidate_ofx_host_harness_dryrun",
            "Candidate OFX host harness dry-run",
            "satisfied_deferred" if clean_evidence and candidate_ofx_host_harness_dryrun_ready else "failed",
            [
                "candidate_ofx_bridge",
                "candidate_ofx_host_harness_dryrun",
            ],
            {
                "harness_dryrun_state": artifact_state(
                    artifacts, "candidate_ofx_host_harness_dryrun", "harness_dryrun_state"
                ),
                "harness_dryrun_ready": artifact_state(
                    artifacts, "candidate_ofx_host_harness_dryrun", "harness_dryrun_ready"
                ),
                "dry_run_only": artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "dry_run_only"),
                "would_execute": artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "would_execute"),
                "execution_performed": artifact_state(
                    artifacts, "candidate_ofx_host_harness_dryrun", "execution_performed"
                ),
                "host_harness_kind": artifact_state(
                    artifacts, "candidate_ofx_host_harness_dryrun", "host_harness_kind"
                ),
                "planned_case_count": artifact_state(
                    artifacts, "candidate_ofx_host_harness_dryrun", "planned_case_count"
                ),
                "planned_noop_describe_case_count": artifact_state(
                    artifacts, "candidate_ofx_host_harness_dryrun", "planned_noop_describe_case_count"
                ),
                "planned_noop_render_case_count": artifact_state(
                    artifacts, "candidate_ofx_host_harness_dryrun", "planned_noop_render_case_count"
                ),
                "planned_real_describe_case_count": artifact_state(
                    artifacts, "candidate_ofx_host_harness_dryrun", "planned_real_describe_case_count"
                ),
                "planned_real_render_case_count": artifact_state(
                    artifacts, "candidate_ofx_host_harness_dryrun", "planned_real_render_case_count"
                ),
                "source_bridge_state": artifact_state(
                    artifacts, "candidate_ofx_host_harness_dryrun", "source_bridge_state"
                ),
                "source_bridge_allowed_route": artifact_state(
                    artifacts, "candidate_ofx_host_harness_dryrun", "source_bridge_allowed_route"
                ),
                "real_route_open": artifact_state(
                    artifacts, "candidate_ofx_host_harness_dryrun", "real_route_open"
                ),
                "real_ofx_route_ready": artifact_state(
                    artifacts, "candidate_ofx_host_harness_dryrun", "real_ofx_route_ready"
                ),
                "ofx_runtime_invoked": artifact_state(
                    artifacts, "candidate_ofx_host_harness_dryrun", "ofx_runtime_invoked"
                ),
                "ofx_describe_ready": artifact_state(
                    artifacts, "candidate_ofx_host_harness_dryrun", "ofx_describe_ready"
                ),
                "ofx_render_ready": artifact_state(
                    artifacts, "candidate_ofx_host_harness_dryrun", "ofx_render_ready"
                ),
                "render_equivalence_claim_ready": artifact_state(
                    artifacts, "candidate_ofx_host_harness_dryrun", "render_equivalence_claim_ready"
                ),
                "host_harness_path_payload_exported": artifact_state(
                    artifacts, "candidate_ofx_host_harness_dryrun", "host_harness_path_payload_exported"
                ),
                "requires_future_runtime_approval": artifact_state(
                    artifacts, "candidate_ofx_host_harness_dryrun", "requires_future_runtime_approval"
                ),
            },
            "The selected candidate now has a no-execution dry-run for a future no-op OFX host harness.",
            "Use this only to plan harness cases; do not build or invoke an OFX runtime until explicit approval and route evidence exist.",
        ),
        requirement(
            "candidate_ofx_host_harness_selftest",
            "Candidate OFX host harness selftest",
            "satisfied_deferred" if clean_evidence and candidate_ofx_host_harness_selftest_ready else "failed",
            [
                "candidate_ofx_host_harness_dryrun",
                "candidate_ofx_host_harness_selftest",
            ],
            {
                "host_harness_selftest_state": artifact_state(
                    artifacts, "candidate_ofx_host_harness_selftest", "host_harness_selftest_state"
                ),
                "host_harness_selftest_ready": artifact_state(
                    artifacts, "candidate_ofx_host_harness_selftest", "host_harness_selftest_ready"
                ),
                "host_harness_kind": artifact_state(
                    artifacts, "candidate_ofx_host_harness_selftest", "host_harness_kind"
                ),
                "synthetic_only": artifact_state(
                    artifacts, "candidate_ofx_host_harness_selftest", "synthetic_only"
                ),
                "synthetic_contract_checks_performed": artifact_state(
                    artifacts, "candidate_ofx_host_harness_selftest", "synthetic_contract_checks_performed"
                ),
                "real_harness_execution_performed": artifact_state(
                    artifacts, "candidate_ofx_host_harness_selftest", "real_harness_execution_performed"
                ),
                "source_harness_dryrun_state": artifact_state(
                    artifacts, "candidate_ofx_host_harness_selftest", "source_harness_dryrun_state"
                ),
                "source_bridge_state": artifact_state(
                    artifacts, "candidate_ofx_host_harness_selftest", "source_bridge_state"
                ),
                "checked_case_count": artifact_state(
                    artifacts, "candidate_ofx_host_harness_selftest", "checked_case_count"
                ),
                "checked_noop_describe_case_count": artifact_state(
                    artifacts, "candidate_ofx_host_harness_selftest", "checked_noop_describe_case_count"
                ),
                "checked_noop_render_case_count": artifact_state(
                    artifacts, "candidate_ofx_host_harness_selftest", "checked_noop_render_case_count"
                ),
                "checked_real_describe_case_count": artifact_state(
                    artifacts, "candidate_ofx_host_harness_selftest", "checked_real_describe_case_count"
                ),
                "checked_real_render_case_count": artifact_state(
                    artifacts, "candidate_ofx_host_harness_selftest", "checked_real_render_case_count"
                ),
                "case_passed_count": artifact_state(
                    artifacts, "candidate_ofx_host_harness_selftest", "case_passed_count"
                ),
                "descriptor_contract_checked": artifact_state(
                    artifacts, "candidate_ofx_host_harness_selftest", "descriptor_contract_checked"
                ),
                "render_identity_contract_checked": artifact_state(
                    artifacts, "candidate_ofx_host_harness_selftest", "render_identity_contract_checked"
                ),
                "ppm_pixel_read_performed": artifact_state(
                    artifacts, "candidate_ofx_host_harness_selftest", "ppm_pixel_read_performed"
                ),
                "ofx_runtime_invoked": artifact_state(
                    artifacts, "candidate_ofx_host_harness_selftest", "ofx_runtime_invoked"
                ),
                "ofx_describe_ready": artifact_state(
                    artifacts, "candidate_ofx_host_harness_selftest", "ofx_describe_ready"
                ),
                "ofx_render_ready": artifact_state(
                    artifacts, "candidate_ofx_host_harness_selftest", "ofx_render_ready"
                ),
                "host_harness_path_payload_exported": artifact_state(
                    artifacts, "candidate_ofx_host_harness_selftest", "host_harness_path_payload_exported"
                ),
                "requires_future_runtime_approval": artifact_state(
                    artifacts, "candidate_ofx_host_harness_selftest", "requires_future_runtime_approval"
                ),
            },
            "The selected candidate now has a synthetic no-load selftest for the no-op OFX host harness contract.",
            "Use this only as contract evidence; real OFX runtime, describe, render, and AEX-backed routes remain closed.",
        ),
        requirement(
            "candidate_ofx_runtime_boundary_contract",
            "Candidate OFX runtime boundary contract",
            "satisfied_deferred" if clean_evidence and candidate_ofx_runtime_boundary_contract_ready else "failed",
            [
                "candidate_ofx_bridge",
                "candidate_ofx_host_harness_dryrun",
                "candidate_ofx_host_harness_selftest",
                "ofx_route_contract",
                "native_loader_runtime_contract",
                "candidate_ofx_runtime_boundary_contract",
            ],
            {
                "candidate_ofx_runtime_boundary_contract_state": artifact_state(
                    artifacts,
                    "candidate_ofx_runtime_boundary_contract",
                    "candidate_ofx_runtime_boundary_contract_state",
                ),
                "contract_state": artifact_state(
                    artifacts, "candidate_ofx_runtime_boundary_contract", "contract_state"
                ),
                "runtime_boundary_ready": artifact_state(
                    artifacts, "candidate_ofx_runtime_boundary_contract", "runtime_boundary_ready"
                ),
                "source_bridge_state": artifact_state(
                    artifacts, "candidate_ofx_runtime_boundary_contract", "source_bridge_state"
                ),
                "source_harness_dryrun_state": artifact_state(
                    artifacts, "candidate_ofx_runtime_boundary_contract", "source_harness_dryrun_state"
                ),
                "source_host_harness_selftest_state": artifact_state(
                    artifacts, "candidate_ofx_runtime_boundary_contract", "source_host_harness_selftest_state"
                ),
                "source_ofx_route_contract_state": artifact_state(
                    artifacts, "candidate_ofx_runtime_boundary_contract", "source_ofx_route_contract_state"
                ),
                "source_native_runtime_contract_state": artifact_state(
                    artifacts, "candidate_ofx_runtime_boundary_contract", "source_native_runtime_contract_state"
                ),
                "ofx_runtime_allowed_now": artifact_state(
                    artifacts, "candidate_ofx_runtime_boundary_contract", "ofx_runtime_allowed_now"
                ),
                "ofx_runtime_invocation_ready": artifact_state(
                    artifacts, "candidate_ofx_runtime_boundary_contract", "ofx_runtime_invocation_ready"
                ),
                "host_process_launch_enabled": artifact_state(
                    artifacts, "candidate_ofx_runtime_boundary_contract", "host_process_launch_enabled"
                ),
                "path_acceptance_ready": artifact_state(
                    artifacts, "candidate_ofx_runtime_boundary_contract", "path_acceptance_ready"
                ),
                "real_route_open": artifact_state(
                    artifacts, "candidate_ofx_runtime_boundary_contract", "real_route_open"
                ),
                "mock_route_ready": artifact_state(
                    artifacts, "candidate_ofx_runtime_boundary_contract", "mock_route_ready"
                ),
                "ofx_runtime_invoked": artifact_state(
                    artifacts, "candidate_ofx_runtime_boundary_contract", "ofx_runtime_invoked"
                ),
                "ppm_pixel_read_performed": artifact_state(
                    artifacts, "candidate_ofx_runtime_boundary_contract", "ppm_pixel_read_performed"
                ),
                "runtime_boundary_path_payload_exported": artifact_state(
                    artifacts, "candidate_ofx_runtime_boundary_contract", "runtime_boundary_path_payload_exported"
                ),
                "approval_gate_count": artifact_state(
                    artifacts, "candidate_ofx_runtime_boundary_contract", "approval_gate_count"
                ),
                "requires_future_runtime_approval": artifact_state(
                    artifacts, "candidate_ofx_runtime_boundary_contract", "requires_future_runtime_approval"
                ),
            },
            "The selected candidate now has a JSON-only boundary contract for a future reviewed OFX runtime.",
            "Keep runtime invocation, host process launch, OFX describe/render, AEX paths, and real routes closed until explicit approvals exist.",
        ),
        requirement(
            "candidate_ofx_runtime_approval_request",
            "Candidate OFX runtime approval request",
            "satisfied_deferred" if clean_evidence and candidate_ofx_runtime_approval_request_ready else "failed",
            [
                "candidate_ofx_runtime_boundary_contract",
                "candidate_ofx_runtime_approval_request",
            ],
            {
                "runtime_approval_request_state": artifact_state(
                    artifacts,
                    "candidate_ofx_runtime_approval_request",
                    "runtime_approval_request_state",
                ),
                "runtime_approval_request_ready": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_request", "runtime_approval_request_ready"
                ),
                "runtime_approval_request_created": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_request", "runtime_approval_request_created"
                ),
                "runtime_approval_can_be_issued_now": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_request", "runtime_approval_can_be_issued_now"
                ),
                "runtime_approval_manifest_created": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_request", "runtime_approval_manifest_created"
                ),
                "runtime_approval_gate_stays_closed": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_request", "runtime_approval_gate_stays_closed"
                ),
                "requires_explicit_user_approval": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_request", "requires_explicit_user_approval"
                ),
                "required_approval_token_name": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_request", "required_approval_token_name"
                ),
                "approval_token_not_stored_in_manifest": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_request", "approval_token_not_stored_in_manifest"
                ),
                "approval_only_prepares_runtime_review": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_request", "approval_only_prepares_runtime_review"
                ),
                "source_boundary_contract_state": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_request", "source_boundary_contract_state"
                ),
                "source_contract_state": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_request", "source_contract_state"
                ),
                "source_ofx_runtime_allowed_now": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_request", "source_ofx_runtime_allowed_now"
                ),
                "source_host_process_launch_enabled": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_request", "source_host_process_launch_enabled"
                ),
                "source_fixture_approval_satisfied": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_request", "source_fixture_approval_satisfied"
                ),
                "review_checklist_count": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_request", "review_checklist_count"
                ),
                "approval_blocker_count": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_request", "approval_blocker_count"
                ),
                "ofx_runtime_invocation_ready": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_request", "ofx_runtime_invocation_ready"
                ),
                "host_process_launch_enabled": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_request", "host_process_launch_enabled"
                ),
                "path_acceptance_ready": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_request", "path_acceptance_ready"
                ),
                "real_route_open": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_request", "real_route_open"
                ),
                "mock_route_ready": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_request", "mock_route_ready"
                ),
                "ofx_runtime_invoked": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_request", "ofx_runtime_invoked"
                ),
                "ppm_pixel_read_performed": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_request", "ppm_pixel_read_performed"
                ),
                "runtime_approval_path_payload_exported": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_request", "runtime_approval_path_payload_exported"
                ),
            },
            "The selected candidate now has a local-only checklist packet for requesting future OFX runtime review.",
            "This is not approval; runtime invocation, host process launch, path acceptance, OFX describe/render, and AEX-backed routes remain closed.",
        ),
        requirement(
            "candidate_ofx_runtime_approval_verifier",
            "Candidate OFX runtime approval verifier",
            "satisfied_deferred" if clean_evidence and candidate_ofx_runtime_approval_verifier_ready else "failed",
            [
                "candidate_ofx_runtime_boundary_contract",
                "candidate_ofx_runtime_approval_request",
                "candidate_ofx_runtime_approval_verifier",
            ],
            {
                "runtime_approval_verifier_state": artifact_state(
                    artifacts,
                    "candidate_ofx_runtime_approval_verifier",
                    "runtime_approval_verifier_state",
                ),
                "runtime_approval_verified_not_approved": artifact_state(
                    artifacts,
                    "candidate_ofx_runtime_approval_verifier",
                    "runtime_approval_verified_not_approved",
                ),
                "source_runtime_approval_request_state": artifact_state(
                    artifacts,
                    "candidate_ofx_runtime_approval_verifier",
                    "source_runtime_approval_request_state",
                ),
                "current_runtime_approval_valid": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_verifier", "current_runtime_approval_valid"
                ),
                "runtime_approval_satisfied": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_verifier", "runtime_approval_satisfied"
                ),
                "runtime_approval_gate_stays_closed": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_verifier", "runtime_approval_gate_stays_closed"
                ),
                "boundary_contract_cross_checked": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_verifier", "boundary_contract_cross_checked"
                ),
                "boundary_contract_matches_request": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_verifier", "boundary_contract_matches_request"
                ),
                "explicit_runtime_approval_present": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_verifier", "explicit_runtime_approval_present"
                ),
                "required_approval_token_name": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_verifier", "required_approval_token_name"
                ),
                "approval_blocker_count": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_verifier", "approval_blocker_count"
                ),
                "fixture_approval_satisfied": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_verifier", "fixture_approval_satisfied"
                ),
                "ofx_host_binary_review_ready": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_verifier", "ofx_host_binary_review_ready"
                ),
                "runtime_containment_selftest_ready": artifact_state(
                    artifacts,
                    "candidate_ofx_runtime_approval_verifier",
                    "runtime_containment_selftest_ready",
                ),
                "schema_and_render_validation_ready": artifact_state(
                    artifacts,
                    "candidate_ofx_runtime_approval_verifier",
                    "schema_and_render_validation_ready",
                ),
                "path_acceptance_closed": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_verifier", "path_acceptance_closed"
                ),
                "synthetic_runtime_approval_checks_passed": artifact_state(
                    artifacts,
                    "candidate_ofx_runtime_approval_verifier",
                    "synthetic_runtime_approval_checks_passed",
                ),
                "ofx_runtime_invocation_ready": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_verifier", "ofx_runtime_invocation_ready"
                ),
                "host_process_launch_enabled": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_verifier", "host_process_launch_enabled"
                ),
                "path_acceptance_ready": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_verifier", "path_acceptance_ready"
                ),
                "real_route_open": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_verifier", "real_route_open"
                ),
                "ofx_runtime_invoked": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_verifier", "ofx_runtime_invoked"
                ),
                "ppm_pixel_read_performed": artifact_state(
                    artifacts, "candidate_ofx_runtime_approval_verifier", "ppm_pixel_read_performed"
                ),
                "runtime_approval_verifier_path_payload_exported": artifact_state(
                    artifacts,
                    "candidate_ofx_runtime_approval_verifier",
                    "runtime_approval_verifier_path_payload_exported",
                ),
            },
            "The selected candidate now has no-load evidence that the OFX runtime approval request is still not approval.",
            "Keep approval, runtime invocation, host process launch, path acceptance, OFX describe/render, and AEX-backed routes closed until all prerequisite evidence is reviewed.",
        ),
        requirement(
            "candidate_ofx_runtime_prerequisite_audit",
            "Candidate OFX runtime prerequisite audit",
            "satisfied_deferred" if clean_evidence and candidate_ofx_runtime_prerequisite_audit_ready else "failed",
            [
                "candidate_ofx_runtime_approval_verifier",
                "native_loader_runtime_selftest",
                "render_validation_contract",
                "parameter_schema_review",
                "candidate_ofx_runtime_prerequisite_audit",
            ],
            {
                "runtime_prerequisite_audit_state": artifact_state(
                    artifacts,
                    "candidate_ofx_runtime_prerequisite_audit",
                    "runtime_prerequisite_audit_state",
                ),
                "runtime_invocation_prerequisites_ready": artifact_state(
                    artifacts,
                    "candidate_ofx_runtime_prerequisite_audit",
                    "runtime_invocation_prerequisites_ready",
                ),
                "approval_can_be_issued_now": artifact_state(
                    artifacts, "candidate_ofx_runtime_prerequisite_audit", "approval_can_be_issued_now"
                ),
                "failed_evidence_count": artifact_state(
                    artifacts, "candidate_ofx_runtime_prerequisite_audit", "failed_evidence_count"
                ),
                "blocking_prerequisite_count": artifact_state(
                    artifacts, "candidate_ofx_runtime_prerequisite_audit", "blocking_prerequisite_count"
                ),
                "runtime_prerequisite_satisfied_count": artifact_state(
                    artifacts,
                    "candidate_ofx_runtime_prerequisite_audit",
                    "runtime_prerequisite_satisfied_count",
                ),
                "runtime_approval_verified_not_approved": artifact_state(
                    artifacts,
                    "candidate_ofx_runtime_prerequisite_audit",
                    "runtime_approval_verified_not_approved",
                ),
                "runtime_approval_satisfied": artifact_state(
                    artifacts, "candidate_ofx_runtime_prerequisite_audit", "runtime_approval_satisfied"
                ),
                "explicit_runtime_approval_present": artifact_state(
                    artifacts,
                    "candidate_ofx_runtime_prerequisite_audit",
                    "explicit_runtime_approval_present",
                ),
                "fixture_approval_satisfied": artifact_state(
                    artifacts, "candidate_ofx_runtime_prerequisite_audit", "fixture_approval_satisfied"
                ),
                "ofx_host_binary_review_ready": artifact_state(
                    artifacts, "candidate_ofx_runtime_prerequisite_audit", "ofx_host_binary_review_ready"
                ),
                "runtime_containment_contract_ready": artifact_state(
                    artifacts,
                    "candidate_ofx_runtime_prerequisite_audit",
                    "runtime_containment_contract_ready",
                ),
                "runtime_containment_selftest_synthetic_passed": artifact_state(
                    artifacts,
                    "candidate_ofx_runtime_prerequisite_audit",
                    "runtime_containment_selftest_synthetic_passed",
                ),
                "runtime_containment_selftest_ready": artifact_state(
                    artifacts,
                    "candidate_ofx_runtime_prerequisite_audit",
                    "runtime_containment_selftest_ready",
                ),
                "parameter_schema_review_policy_ready": artifact_state(
                    artifacts,
                    "candidate_ofx_runtime_prerequisite_audit",
                    "parameter_schema_review_policy_ready",
                ),
                "schema_and_render_validation_ready": artifact_state(
                    artifacts,
                    "candidate_ofx_runtime_prerequisite_audit",
                    "schema_and_render_validation_ready",
                ),
                "render_validation_contract_ready": artifact_state(
                    artifacts,
                    "candidate_ofx_runtime_prerequisite_audit",
                    "render_validation_contract_ready",
                ),
                "real_render_open": artifact_state(
                    artifacts, "candidate_ofx_runtime_prerequisite_audit", "real_render_open"
                ),
                "ofx_route_contract_closed": artifact_state(
                    artifacts, "candidate_ofx_runtime_prerequisite_audit", "ofx_route_contract_closed"
                ),
                "real_route_open": artifact_state(
                    artifacts, "candidate_ofx_runtime_prerequisite_audit", "real_route_open"
                ),
                "ofx_runtime_invoked": artifact_state(
                    artifacts, "candidate_ofx_runtime_prerequisite_audit", "ofx_runtime_invoked"
                ),
                "host_process_launch_enabled": artifact_state(
                    artifacts,
                    "candidate_ofx_runtime_prerequisite_audit",
                    "host_process_launch_enabled",
                ),
                "runtime_prerequisite_audit_path_payload_exported": artifact_state(
                    artifacts,
                    "candidate_ofx_runtime_prerequisite_audit",
                    "runtime_prerequisite_audit_path_payload_exported",
                ),
            },
            "The selected candidate now has a no-load audit that separates available prerequisite evidence from remaining runtime blockers.",
            "This audit does not reduce blockers or approve runtime invocation; fixture approval, explicit runtime approval, host review, real schema, and real render validation remain pending.",
        ),
        requirement(
            "candidate_ofx_host_binary_review_request",
            "Candidate OFX host binary review request",
            "satisfied_deferred" if clean_evidence and candidate_ofx_host_binary_review_request_ready else "failed",
            [
                "candidate_ofx_runtime_prerequisite_audit",
                "candidate_ofx_host_harness_dryrun",
                "candidate_ofx_host_harness_selftest",
                "candidate_ofx_runtime_boundary_contract",
                "candidate_ofx_host_binary_review_request",
            ],
            {
                "ofx_host_binary_review_request_state": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "ofx_host_binary_review_request_state",
                ),
                "host_binary_review_request_state": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "host_binary_review_request_state",
                ),
                "host_binary_review_request_ready": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "host_binary_review_request_ready",
                ),
                "ofx_host_binary_review_ready": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "ofx_host_binary_review_ready",
                ),
                "host_binary_review_satisfied": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "host_binary_review_satisfied",
                ),
                "host_binary_review_can_be_approved_now": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "host_binary_review_can_be_approved_now",
                ),
                "host_binary_review_manifest_created": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "host_binary_review_manifest_created",
                ),
                "host_binary_review_gate_stays_closed": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "host_binary_review_gate_stays_closed",
                ),
                "source_prerequisite_audit_state": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "source_prerequisite_audit_state",
                ),
                "source_failed_evidence_count": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "source_failed_evidence_count",
                ),
                "source_runtime_invocation_prerequisites_ready": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "source_runtime_invocation_prerequisites_ready",
                ),
                "source_ofx_host_binary_review_ready": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "source_ofx_host_binary_review_ready",
                ),
                "source_harness_dryrun_ready": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "source_harness_dryrun_ready",
                ),
                "source_host_harness_selftest_ready": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "source_host_harness_selftest_ready",
                ),
                "source_boundary_contract_ready": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "source_boundary_contract_ready",
                ),
                "source_boundary_host_process_launch_enabled": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "source_boundary_host_process_launch_enabled",
                ),
                "source_boundary_ofx_host_path_payload_supplied": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "source_boundary_ofx_host_path_payload_supplied",
                ),
                "source_boundary_ofx_plugin_binary_path_payload_supplied": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "source_boundary_ofx_plugin_binary_path_payload_supplied",
                ),
                "review_checklist_count": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "review_checklist_count",
                ),
                "host_binary_review_blocker_count": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "host_binary_review_blocker_count",
                ),
                "host_binary_path_acceptance_ready": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "host_binary_path_acceptance_ready",
                ),
                "host_binary_path_payload_exported": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "host_binary_path_payload_exported",
                ),
                "accepted_ofx_host_path": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "accepted_ofx_host_path",
                ),
                "accepted_ofx_plugin_binary_path": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "accepted_ofx_plugin_binary_path",
                ),
                "ofx_runtime_invocation_ready": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "ofx_runtime_invocation_ready",
                ),
                "host_process_launch_enabled": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "host_process_launch_enabled",
                ),
                "path_acceptance_ready": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "path_acceptance_ready",
                ),
                "real_route_open": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "real_route_open",
                ),
                "ofx_runtime_invoked": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "ofx_runtime_invoked",
                ),
                "ppm_pixel_read_performed": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "ppm_pixel_read_performed",
                ),
                "ofx_host_binary_review_path_payload_exported": artifact_state(
                    artifacts,
                    "candidate_ofx_host_binary_review_request",
                    "ofx_host_binary_review_path_payload_exported",
                ),
            },
            "The selected candidate now has a no-load request packet that turns host/shim binary review into explicit manual requirements.",
            "This request is not host approval; it accepts no host/plugin path, launches no host process, invokes no OFX runtime, and leaves the host binary review gate closed.",
        ),
        requirement(
            "candidate_dependency_scope",
            "Candidate-scoped dependency review",
            "satisfied_deferred" if clean_evidence and candidate_dependency_scope_ready else "failed",
            ["fixture_manual_review_packet", "dependency_review", "load_gate", "candidate_dependency_scope"],
            {
                "review_packet_state": artifact_state(
                    artifacts, "fixture_manual_review_packet", "review_packet_state"
                ),
                "dependency_review_state": artifact_state(artifacts, "dependency_review", "review_state"),
                "dependency_native_load_recommendation": artifact_state(
                    artifacts, "dependency_review", "native_load_recommendation"
                ),
                "load_gate_state": artifact_state(artifacts, "load_gate", "gate_state"),
                "candidate_dependency_scope_state": artifact_state(
                    artifacts, "candidate_dependency_scope", "candidate_dependency_scope_state"
                ),
                "candidate_scope_ready": artifact_state(
                    artifacts, "candidate_dependency_scope", "candidate_scope_ready"
                ),
                "candidate_dependency_blockers_present": artifact_state(
                    artifacts, "candidate_dependency_scope", "candidate_dependency_blockers_present"
                ),
                "candidate_dependency_blocker_count": artifact_state(
                    artifacts, "candidate_dependency_scope", "candidate_dependency_blocker_count"
                ),
                "candidate_dependency_review_count": artifact_state(
                    artifacts, "candidate_dependency_scope", "candidate_dependency_review_count"
                ),
                "candidate_dependency_missing_or_api_set_review_count": artifact_state(
                    artifacts,
                    "candidate_dependency_scope",
                    "candidate_dependency_missing_or_api_set_review_count",
                ),
                "candidate_dependency_found_paths_exported": artifact_state(
                    artifacts, "candidate_dependency_scope", "candidate_dependency_found_paths_exported"
                ),
                "global_dependency_blockers_present": artifact_state(
                    artifacts, "candidate_dependency_scope", "global_dependency_blockers_present"
                ),
                "global_dependency_blockers_apply_to_candidate": artifact_state(
                    artifacts, "candidate_dependency_scope", "global_dependency_blockers_apply_to_candidate"
                ),
                "scoped_gate_recommendation": artifact_state(
                    artifacts, "candidate_dependency_scope", "scoped_gate_recommendation"
                ),
            },
            "The selected fixture candidate has a dependency-scoped review separating its imports from global dependency blockers.",
            "Keep native load closed until a reviewed scoped gate policy replaces the current global dependency blocker.",
        ),
        requirement(
            "candidate_load_gate_dryrun",
            "Candidate-scoped load gate dry-run",
            "satisfied_deferred" if clean_evidence and candidate_load_gate_dryrun_ready else "failed",
            [
                "fixture_decision",
                "fixture_manual_review_packet",
                "candidate_dependency_scope",
                "candidate_load_gate_dryrun",
                "load_gate",
            ],
            {
                "candidate_load_gate_dryrun_state": artifact_state(
                    artifacts, "candidate_load_gate_dryrun", "candidate_load_gate_dryrun_state"
                ),
                "candidate_scoped_load_gate_dry_run_state": artifact_state(
                    artifacts,
                    "candidate_load_gate_dryrun",
                    "candidate_scoped_load_gate_dry_run_state",
                ),
                "native_load_gate": artifact_state(artifacts, "candidate_load_gate_dryrun", "native_load_gate"),
                "fixture_approval_satisfied": artifact_state(
                    artifacts, "candidate_load_gate_dryrun", "fixture_approval_satisfied"
                ),
                "fixture_decision_state": artifact_state(
                    artifacts, "candidate_load_gate_dryrun", "fixture_decision_state"
                ),
                "candidate_dependencies_clear": artifact_state(
                    artifacts, "candidate_load_gate_dryrun", "candidate_dependencies_clear"
                ),
                "candidate_dependency_blockers_present": artifact_state(
                    artifacts, "candidate_load_gate_dryrun", "candidate_dependency_blockers_present"
                ),
                "global_dependency_blockers_present": artifact_state(
                    artifacts, "candidate_load_gate_dryrun", "global_dependency_blockers_present"
                ),
                "global_dependency_blockers_apply_to_candidate": artifact_state(
                    artifacts,
                    "candidate_load_gate_dryrun",
                    "global_dependency_blockers_apply_to_candidate",
                ),
                "source_load_gate_state": artifact_state(
                    artifacts, "candidate_load_gate_dryrun", "source_load_gate_state"
                ),
                "scoped_gate_recommendation": artifact_state(
                    artifacts, "candidate_load_gate_dryrun", "scoped_gate_recommendation"
                ),
            },
            "The candidate-scoped dry-run shows ScatterMap dependencies are clear while the native load gate remains closed for missing fixture approval.",
            "Create explicit fixture approval and a separate loader design before any AEX path can be accepted.",
        ),
        requirement(
            "no_load_worker_harness",
            "No-load worker and image fixture selftest",
            "satisfied"
            if clean_evidence
            and worker_ready
            and image_suite_ready
            and image_validation_ready
            and image_suite_selftest_ready
            and image_input_smoke_ready
            and render_contract_ready
            else "failed",
            [
                "worker_design",
                "worker_selftest",
                "image_fixture_suite",
                "image_fixture_validation",
                "image_suite_selftest",
                "image_input_smoke",
                "render_validation_contract",
            ],
            {
                "worker_selftest_passed": artifact_state(artifacts, "worker_selftest", "worker_selftest_passed"),
                "sandbox_policy_state": artifact_state(artifacts, "sandbox_policy", "sandbox_policy_state"),
                "image_fixture_suite_state": artifact_state(artifacts, "image_fixture_suite", "suite_state"),
                "image_fixture_validation_state": artifact_state(
                    artifacts, "image_fixture_validation", "validation_state"
                ),
                "image_suite_selftest_state": artifact_state(
                    artifacts, "image_suite_selftest", "suite_selftest_state"
                ),
                "image_input_smoke_state": artifact_state(artifacts, "image_input_smoke", "smoke_state"),
                "image_input_worker_identity_passed": artifact_state(
                    artifacts, "image_input_smoke", "worker_identity_passed"
                ),
                "image_input_ofx_identity_passed": artifact_state(
                    artifacts, "image_input_smoke", "ofx_identity_passed"
                ),
                "render_validation_contract_state": artifact_state(
                    artifacts, "render_validation_contract", "contract_state"
                ),
                "real_render_open": artifact_state(artifacts, "render_validation_contract", "real_render_open"),
            },
            "The subprocess harness handles the validated image suite and a one-image smoke tool while the render contract remains closed.",
        ),
        requirement(
            "render_validation_contract",
            "Render validation contract",
            "satisfied_deferred" if clean_evidence and render_contract_ready else "failed",
            ["image_fixture_validation", "image_input_smoke", "load_gate", "ofx_route_contract", "render_validation_contract"],
            {
                "image_fixture_validation_state": artifact_state(
                    artifacts, "image_fixture_validation", "validation_state"
                ),
                "image_input_smoke_state": artifact_state(artifacts, "image_input_smoke", "smoke_state"),
                "load_gate_state": artifact_state(artifacts, "load_gate", "gate_state"),
                "ofx_contract_state": artifact_state(artifacts, "ofx_route_contract", "contract_state"),
                "render_contract_state": artifact_state(artifacts, "render_validation_contract", "contract_state"),
                "real_render_open": artifact_state(artifacts, "render_validation_contract", "real_render_open"),
                "no_load_validation_ready": artifact_state(
                    artifacts, "render_validation_contract", "no_load_validation_ready"
                ),
            },
            "Generated image validation evidence is ready for no-load use, but real AEX/OFX render validation is closed.",
            "Create a sandboxed render worker only after fixture approval and a passing native load gate.",
        ),
        requirement(
            "native_load_gate",
            "Native AEX load gate",
            "intentionally_closed" if clean_evidence and gate_closed and loader_refused else "failed",
            ["load_gate", "native_loader_stub"],
            {
                "gate_state": artifact_state(artifacts, "load_gate", "gate_state"),
                "stub_state": artifact_state(artifacts, "native_loader_stub", "stub_state"),
                "dependency_review_state": artifact_state(artifacts, "dependency_review", "review_state"),
                "dependency_native_load_recommendation": artifact_state(
                    artifacts, "dependency_review", "native_load_recommendation"
                ),
            },
            "The loader-facing path refuses work because fixture approval is not present.",
            "Only create a real loader after explicit user approval and a passing gate check.",
        ),
        requirement(
            "native_loader_design_contract",
            "Native loader design contract",
            "satisfied_deferred" if clean_evidence and native_loader_design_ready else "failed",
            [
                "candidate_load_gate_dryrun",
                "sandbox_policy",
                "native_loader_stub",
                "render_validation_contract",
                "ofx_route_contract",
                "native_loader_design_contract",
            ],
            {
                "native_loader_design_state": artifact_state(
                    artifacts, "native_loader_design_contract", "native_loader_design_state"
                ),
                "contract_state": artifact_state(artifacts, "native_loader_design_contract", "contract_state"),
                "loader_design_ready": artifact_state(
                    artifacts, "native_loader_design_contract", "loader_design_ready"
                ),
                "native_load_gate": artifact_state(artifacts, "native_loader_design_contract", "native_load_gate"),
                "approval_required_before_aex_path": artifact_state(
                    artifacts, "native_loader_design_contract", "approval_required_before_aex_path"
                ),
                "runtime_approval_required_before_load": artifact_state(
                    artifacts, "native_loader_design_contract", "runtime_approval_required_before_load"
                ),
                "separate_process_required": artifact_state(
                    artifacts, "native_loader_design_contract", "separate_process_required"
                ),
                "accepts_aex_path": artifact_state(
                    artifacts, "native_loader_design_contract", "accepts_aex_path"
                ),
                "accepted_aex_path": artifact_state(
                    artifacts, "native_loader_design_contract", "accepted_aex_path"
                ),
                "controller_loads_aex": artifact_state(
                    artifacts, "native_loader_design_contract", "controller_loads_aex"
                ),
                "candidate_dependencies_clear": artifact_state(
                    artifacts, "native_loader_design_contract", "candidate_dependencies_clear"
                ),
                "fixture_approval_satisfied": artifact_state(
                    artifacts, "native_loader_design_contract", "fixture_approval_satisfied"
                ),
                "source_stub_state": artifact_state(
                    artifacts, "native_loader_design_contract", "source_stub_state"
                ),
                "source_sandbox_policy_state": artifact_state(
                    artifacts, "native_loader_design_contract", "source_sandbox_policy_state"
                ),
            },
            "A separate loader boundary is specified, but it accepts no AEX path and requires explicit approval before any runtime work.",
            "Implement only a pathless loader broker selftest before revisiting fixture approval.",
        ),
        requirement(
            "native_loader_broker_selftest",
            "Pathless native loader broker selftest",
            "satisfied_deferred" if clean_evidence and native_loader_broker_selftest_ready else "failed",
            ["native_loader_design_contract", "native_loader_broker_selftest"],
            {
                "broker_selftest_state": artifact_state(
                    artifacts, "native_loader_broker_selftest", "broker_selftest_state"
                ),
                "pathless_broker_ready": artifact_state(
                    artifacts, "native_loader_broker_selftest", "pathless_broker_ready"
                ),
                "native_loader_design_ready": artifact_state(
                    artifacts, "native_loader_broker_selftest", "native_loader_design_ready"
                ),
                "accepts_aex_path": artifact_state(
                    artifacts, "native_loader_broker_selftest", "accepts_aex_path"
                ),
                "accepted_aex_path": artifact_state(
                    artifacts, "native_loader_broker_selftest", "accepted_aex_path"
                ),
                "path_payload_supplied": artifact_state(
                    artifacts, "native_loader_broker_selftest", "path_payload_supplied"
                ),
                "blocked_action_count": artifact_state(
                    artifacts, "native_loader_broker_selftest", "blocked_action_count"
                ),
                "candidate_dependencies_clear": artifact_state(
                    artifacts, "native_loader_broker_selftest", "candidate_dependencies_clear"
                ),
                "fixture_approval_satisfied": artifact_state(
                    artifacts, "native_loader_broker_selftest", "fixture_approval_satisfied"
                ),
            },
            "The future native-loader broker can start pathless, report native_load_enabled=false, and reject native/AEX messages without receiving an AEX path.",
            "Keep the broker pathless until fixture approval and runtime containment are reviewed.",
        ),
        requirement(
            "native_loader_runtime_contract",
            "Runtime containment and path allowlist contract",
            "satisfied_deferred" if clean_evidence and native_loader_runtime_contract_ready else "failed",
            [
                "native_loader_design_contract",
                "native_loader_broker_selftest",
                "candidate_load_gate_dryrun",
                "sandbox_policy",
                "native_loader_runtime_contract",
            ],
            {
                "native_loader_runtime_contract_state": artifact_state(
                    artifacts, "native_loader_runtime_contract", "native_loader_runtime_contract_state"
                ),
                "contract_state": artifact_state(artifacts, "native_loader_runtime_contract", "contract_state"),
                "runtime_containment_ready": artifact_state(
                    artifacts, "native_loader_runtime_contract", "runtime_containment_ready"
                ),
                "path_allowlist_state": artifact_state(
                    artifacts, "native_loader_runtime_contract", "path_allowlist_state"
                ),
                "path_acceptance_ready": artifact_state(
                    artifacts, "native_loader_runtime_contract", "path_acceptance_ready"
                ),
                "aex_path_acceptance_enabled": artifact_state(
                    artifacts, "native_loader_runtime_contract", "aex_path_acceptance_enabled"
                ),
                "accepted_aex_path": artifact_state(
                    artifacts, "native_loader_runtime_contract", "accepted_aex_path"
                ),
                "path_payload_supplied": artifact_state(
                    artifacts, "native_loader_runtime_contract", "path_payload_supplied"
                ),
                "broker_selftest_passed": artifact_state(
                    artifacts, "native_loader_runtime_contract", "broker_selftest_passed"
                ),
                "process_isolation_required": artifact_state(
                    artifacts, "native_loader_runtime_contract", "process_isolation_required"
                ),
                "source_broker_selftest_state": artifact_state(
                    artifacts, "native_loader_runtime_contract", "source_broker_selftest_state"
                ),
                "source_candidate_load_gate_state": artifact_state(
                    artifacts, "native_loader_runtime_contract", "source_candidate_load_gate_state"
                ),
            },
            "The next runtime boundary is specified as pathless and out-of-process with timeout, crash, child cleanup, log, and path allowlist rules, but no AEX path is accepted.",
            "Do not enable path acceptance until fixture approval and a reviewed allowlist/runtime plan exist.",
        ),
        requirement(
            "native_loader_runtime_selftest",
            "Synthetic runtime containment selftest",
            "satisfied_deferred" if clean_evidence and native_loader_runtime_selftest_ready else "failed",
            ["native_loader_runtime_contract", "native_loader_runtime_selftest"],
            {
                "runtime_selftest_state": artifact_state(
                    artifacts, "native_loader_runtime_selftest", "runtime_selftest_state"
                ),
                "runtime_containment_selftest_passed": artifact_state(
                    artifacts, "native_loader_runtime_selftest", "runtime_containment_selftest_passed"
                ),
                "source_native_loader_runtime_contract_state": artifact_state(
                    artifacts,
                    "native_loader_runtime_selftest",
                    "source_native_loader_runtime_contract_state",
                ),
                "synthetic_subprocess_only": artifact_state(
                    artifacts, "native_loader_runtime_selftest", "synthetic_subprocess_only"
                ),
                "normal_exit_case_passed": artifact_state(
                    artifacts, "native_loader_runtime_selftest", "normal_exit_case_passed"
                ),
                "stderr_capture_passed": artifact_state(
                    artifacts, "native_loader_runtime_selftest", "stderr_capture_passed"
                ),
                "timeout_case_passed": artifact_state(
                    artifacts, "native_loader_runtime_selftest", "timeout_case_passed"
                ),
                "child_cleanup_passed": artifact_state(
                    artifacts, "native_loader_runtime_selftest", "child_cleanup_passed"
                ),
                "path_acceptance_ready": artifact_state(
                    artifacts, "native_loader_runtime_selftest", "path_acceptance_ready"
                ),
                "aex_path_acceptance_enabled": artifact_state(
                    artifacts, "native_loader_runtime_selftest", "aex_path_acceptance_enabled"
                ),
                "accepted_aex_path": artifact_state(
                    artifacts, "native_loader_runtime_selftest", "accepted_aex_path"
                ),
            },
            "Synthetic subprocess evidence now exercises normal exit, stderr capture, timeout termination, and child cleanup without accepting an AEX path.",
            "Keep using synthetic containment tests until an approved fixture and explicit path-acceptance gate exist.",
        ),
        requirement(
            "native_loader_path_policy_selftest",
            "Closed AEX path policy and redaction selftest",
            "satisfied_deferred" if clean_evidence and native_loader_path_policy_selftest_ready else "failed",
            ["native_loader_runtime_selftest", "native_loader_path_policy_selftest"],
            {
                "path_policy_selftest_state": artifact_state(
                    artifacts, "native_loader_path_policy_selftest", "path_policy_selftest_state"
                ),
                "path_policy_selftest_passed": artifact_state(
                    artifacts, "native_loader_path_policy_selftest", "path_policy_selftest_passed"
                ),
                "source_runtime_selftest_state": artifact_state(
                    artifacts, "native_loader_path_policy_selftest", "source_runtime_selftest_state"
                ),
                "path_allowlist_state": artifact_state(
                    artifacts, "native_loader_path_policy_selftest", "path_allowlist_state"
                ),
                "path_acceptance_ready": artifact_state(
                    artifacts, "native_loader_path_policy_selftest", "path_acceptance_ready"
                ),
                "aex_path_acceptance_enabled": artifact_state(
                    artifacts, "native_loader_path_policy_selftest", "aex_path_acceptance_enabled"
                ),
                "accepted_aex_path": artifact_state(
                    artifacts, "native_loader_path_policy_selftest", "accepted_aex_path"
                ),
                "candidate_path_string_accepted": artifact_state(
                    artifacts, "native_loader_path_policy_selftest", "candidate_path_string_accepted"
                ),
                "absolute_path_rejected": artifact_state(
                    artifacts, "native_loader_path_policy_selftest", "absolute_path_rejected"
                ),
                "traversal_rejected": artifact_state(
                    artifacts, "native_loader_path_policy_selftest", "traversal_rejected"
                ),
                "non_aex_suffix_rejected": artifact_state(
                    artifacts, "native_loader_path_policy_selftest", "non_aex_suffix_rejected"
                ),
                "redaction_passed": artifact_state(
                    artifacts, "native_loader_path_policy_selftest", "redaction_passed"
                ),
                "raw_input_paths_serialized": artifact_state(
                    artifacts, "native_loader_path_policy_selftest", "raw_input_paths_serialized"
                ),
            },
            "Synthetic path policy evidence rejects candidate-like, absolute, traversal, and non-AEX path strings while keeping raw input paths out of the report.",
            "Only replace this closed policy with a real allowlist after fixture approval and explicit path-acceptance approval.",
        ),
        requirement(
            "ofx_groundwork",
            "Deferred OFX facade and no-op mock",
            "satisfied_deferred"
            if clean_evidence
            and ofx_deferred
            and ofx_mock_ready
            and ofx_suite_ready
            and ofx_contract_ready
            and image_input_smoke_ready
            else "failed",
            ["ofx_facade", "ofx_noop_mock", "ofx_suite_selftest", "ofx_route_contract", "image_input_smoke"],
            {
                "facade_state": artifact_state(artifacts, "ofx_facade", "facade_state"),
                "mock_state": artifact_state(artifacts, "ofx_noop_mock", "mock_state"),
                "ofx_suite_selftest_state": artifact_state(
                    artifacts, "ofx_suite_selftest", "ofx_suite_selftest_state"
                ),
                "contract_state": artifact_state(artifacts, "ofx_route_contract", "contract_state"),
                "real_route_open": artifact_state(artifacts, "ofx_route_contract", "real_route_open"),
                "mock_route_ready": artifact_state(artifacts, "ofx_route_contract", "mock_route_ready"),
                "image_input_smoke_state": artifact_state(artifacts, "image_input_smoke", "smoke_state"),
            },
            "OFX-shaped planning, no-op suite identity, a closed route contract, and a one-image smoke path exist, but no real OFX route is open.",
        ),
        requirement(
            "publication_boundary",
            "Cleanroom and publication boundary",
            "satisfied_local_only" if clean_evidence and publication_local_only else "failed",
            ["safety_audit", "publication_boundary"],
            {
                "audit_passed": artifact_state(artifacts, "safety_audit", "audit_passed"),
                "publishable_now": artifact_state(artifacts, "publication_boundary", "publishable_now"),
            },
            "Current evidence is useful locally but is not a publishable or redistributable artifact set.",
            "Prepare a redacted/provenance-reviewed public summary before publication.",
        ),
        requirement(
            "manual_fixture_approval",
            "Manual fixture approval",
            "pending_manual_review" if clean_evidence else "failed",
            ["fixture_decision", "fixture_dossier", "fixture_manual_review_packet", "candidate_dependency_scope"],
            {
                "decision_state": artifact_state(artifacts, "fixture_decision", "decision_state"),
                "approval_state": artifact_state(artifacts, "fixture_decision", "approval_state"),
                "dossier_state": artifact_state(artifacts, "fixture_dossier", "dossier_state"),
                "review_packet_state": artifact_state(
                    artifacts, "fixture_manual_review_packet", "review_packet_state"
                ),
                "recommended_next_decision": artifact_state(
                    artifacts, "fixture_manual_review_packet", "recommended_next_decision"
                ),
                "candidate_dependency_blockers_present": artifact_state(
                    artifacts, "candidate_dependency_scope", "candidate_dependency_blockers_present"
                ),
                "candidate_dependency_blocker_count": artifact_state(
                    artifacts, "candidate_dependency_scope", "candidate_dependency_blocker_count"
                ),
            },
            "The current ScatterMap decision is hold/review, not approval.",
            "User approval must be explicit before native AEX load experiments.",
        ),
        requirement(
            "real_aex_render_or_ofx_route",
            "Real AEX render, AE launch, or OFX route",
            "intentionally_closed" if clean_evidence else "failed",
            [
                "load_gate",
                "native_loader_stub",
                "ofx_facade",
                "ofx_noop_mock",
                "ofx_route_contract",
                "render_validation_contract",
            ],
            {
                "gate_state": artifact_state(artifacts, "load_gate", "gate_state"),
                "stub_state": artifact_state(artifacts, "native_loader_stub", "stub_state"),
                "facade_state": artifact_state(artifacts, "ofx_facade", "facade_state"),
                "contract_state": artifact_state(artifacts, "ofx_route_contract", "contract_state"),
                "real_route_open": artifact_state(artifacts, "ofx_route_contract", "real_route_open"),
                "render_contract_state": artifact_state(artifacts, "render_validation_contract", "contract_state"),
                "real_render_open": artifact_state(artifacts, "render_validation_contract", "real_render_open"),
            },
            "No real AEX load, AE launch, render, or OFX route has been performed.",
            "Start with a separate gated native-loader spike only after manual approval.",
        ),
    ]

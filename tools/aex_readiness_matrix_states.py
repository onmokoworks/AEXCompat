"""Readiness evidence-state evaluation extracted from the CLI facade."""

from __future__ import annotations

from typing import Any

try:
    from aex_readiness_matrix_core import artifact_found, artifact_state, labels_found
except ModuleNotFoundError:
    from tools.aex_readiness_matrix_core import artifact_found, artifact_state, labels_found


def evaluate_readiness_states(
    artifacts: dict[str, dict[str, Any]], clean_evidence: bool
) -> dict[str, Any]:
    static_ready = artifact_found(artifacts, "static_report")
    pipl_catalog_ready = (
        artifact_state(artifacts, "pipl_resource_catalog", "catalog_state")
        == "pipl_resource_catalog_ready_no_payload"
        and artifact_state(artifacts, "pipl_resource_catalog", "payload_policy")
        == "metadata_only_no_resource_payload"
    )
    parameter_schema_plan_ready = (
        artifact_state(artifacts, "parameter_schema_plan", "plan_state")
        == "parameter_schema_plan_ready_no_payload"
        and artifact_state(artifacts, "parameter_schema_plan", "schema_plan_ready") is True
        and artifact_state(artifacts, "parameter_schema_plan", "real_parameter_schema_available") is False
        and artifact_state(artifacts, "parameter_schema_plan", "payload_parser_enabled") is False
    )
    parameter_schema_review_ready = (
        artifact_state(artifacts, "parameter_schema_review", "review_state")
        == "parameter_schema_review_ready_no_payload"
        and artifact_state(artifacts, "parameter_schema_review", "parser_design_state")
        == "payload_parser_design_review_ready_parser_disabled"
        and artifact_state(artifacts, "parameter_schema_review", "redaction_policy_state")
        == "redaction_policy_ready_no_schema_output"
        and artifact_state(artifacts, "parameter_schema_review", "ofx_describe_policy_state")
        == "ofx_describe_mapping_deferred_until_redacted_schema"
        and artifact_state(artifacts, "parameter_schema_review", "payload_parser_enabled") is False
        and artifact_state(artifacts, "parameter_schema_review", "redacted_schema_available") is False
        and artifact_state(artifacts, "parameter_schema_review", "ofx_describe_mapping_ready") is False
    )
    redacted_schema_verifier_ready = (
        artifact_state(artifacts, "redacted_schema_verifier", "verifier_state")
        == "redacted_schema_verifier_ready_no_real_schema"
        and artifact_state(artifacts, "redacted_schema_verifier", "verifier_ready") is True
        and artifact_state(artifacts, "redacted_schema_verifier", "real_redacted_schema_available") is False
        and artifact_state(artifacts, "redacted_schema_verifier", "payload_parser_enabled") is False
        and artifact_state(artifacts, "redacted_schema_verifier", "synthetic_schema_fixture_used") is True
    )
    synthetic_pipl_parser_selftest_ready = (
        artifact_state(artifacts, "synthetic_pipl_parser_selftest", "selftest_state")
        == "synthetic_pipl_parser_selftest_passed_no_real_payload"
        and artifact_state(artifacts, "synthetic_pipl_parser_selftest", "synthetic_parser_ready") is True
        and artifact_state(artifacts, "synthetic_pipl_parser_selftest", "synthetic_payloads_used") is True
        and artifact_state(artifacts, "synthetic_pipl_parser_selftest", "real_pipl_payload_parser_enabled")
        is False
        and artifact_state(artifacts, "synthetic_pipl_parser_selftest", "real_pipl_payload_parsed") is False
        and artifact_state(artifacts, "synthetic_pipl_parser_selftest", "raw_payload_serialized") is False
    )
    pipl_parser_gate_ready = (
        artifact_state(artifacts, "pipl_parser_gate", "gate_state") == "pipl_parser_gate_closed_no_real_payload"
        and artifact_state(artifacts, "pipl_parser_gate", "gate_ready_for_review") is True
        and artifact_state(artifacts, "pipl_parser_gate", "metadata_budget_ready") is True
        and artifact_state(artifacts, "pipl_parser_gate", "real_pipl_payload_parser_enabled") is False
        and artifact_state(artifacts, "pipl_parser_gate", "real_pipl_payload_parsed") is False
        and artifact_state(artifacts, "pipl_parser_gate", "resource_payload_opened") is False
        and artifact_state(artifacts, "pipl_parser_gate", "raw_payload_serialized") is False
    )
    synthetic_pipl_payload_parser_ready = (
        artifact_state(artifacts, "synthetic_pipl_payload_parser", "synthetic_payload_parser_state")
        == "synthetic_pipl_payload_parser_ready_real_payload_closed"
        and artifact_state(artifacts, "synthetic_pipl_payload_parser", "synthetic_payload_parser_ready") is True
        and artifact_state(artifacts, "synthetic_pipl_payload_parser", "synthetic_parser_implemented") is True
        and artifact_state(artifacts, "synthetic_pipl_payload_parser", "synthetic_bounds_harness_reused") is True
        and artifact_state(artifacts, "synthetic_pipl_payload_parser", "synthetic_payload_cases_passed") is True
        and artifact_state(artifacts, "synthetic_pipl_payload_parser", "synthetic_payloads_used") is True
        and artifact_state(artifacts, "synthetic_pipl_payload_parser", "synthetic_payloads_serialized") is False
        and artifact_state(artifacts, "synthetic_pipl_payload_parser", "real_payload_input_allowed_now") is False
        and artifact_state(artifacts, "synthetic_pipl_payload_parser", "output_metadata_only") is True
        and artifact_state(artifacts, "synthetic_pipl_payload_parser", "real_pipl_payload_parser_enabled")
        is False
        and artifact_state(artifacts, "synthetic_pipl_payload_parser", "real_pipl_payload_parsed") is False
        and artifact_state(artifacts, "synthetic_pipl_payload_parser", "resource_payload_opened") is False
        and artifact_state(artifacts, "synthetic_pipl_payload_parser", "raw_payload_serialized") is False
        and (artifact_state(artifacts, "synthetic_pipl_payload_parser", "parser_case_count") or 0) > 0
        and (artifact_state(artifacts, "synthetic_pipl_payload_parser", "parser_case_passed_count") or 0) > 0
        and artifact_state(artifacts, "synthetic_pipl_payload_parser", "parser_case_failed_count") == 0
    )
    pipl_resource_consistency_audit_ready = (
        artifact_state(artifacts, "pipl_resource_consistency_audit", "audit_state")
        == "pipl_resource_consistency_audit_passed_no_payload"
        and artifact_state(artifacts, "pipl_resource_consistency_audit", "audit_passed") is True
        and artifact_state(artifacts, "pipl_resource_consistency_audit", "metadata_consistency_ready") is True
        and artifact_state(artifacts, "pipl_resource_consistency_audit", "source_chain_valid") is True
        and artifact_state(artifacts, "pipl_resource_consistency_audit", "catalog_summary_recomputed") is True
        and artifact_state(artifacts, "pipl_resource_consistency_audit", "catalog_rows_recomputed") is True
        and artifact_state(artifacts, "pipl_resource_consistency_audit", "gate_budget_rows_recomputed") is True
        and artifact_state(artifacts, "pipl_resource_consistency_audit", "gate_summary_recomputed") is True
        and artifact_state(artifacts, "pipl_resource_consistency_audit", "real_payload_input_allowed_now") is False
        and artifact_state(artifacts, "pipl_resource_consistency_audit", "real_pipl_payload_parser_enabled")
        is False
        and artifact_state(artifacts, "pipl_resource_consistency_audit", "real_pipl_payload_parsed") is False
        and artifact_state(artifacts, "pipl_resource_consistency_audit", "resource_payload_opened") is False
        and artifact_state(artifacts, "pipl_resource_consistency_audit", "resource_payload_extracted") is False
        and artifact_state(artifacts, "pipl_resource_consistency_audit", "raw_payload_serialized") is False
    )
    pipl_payload_adapter_review_ready = (
        artifact_state(artifacts, "pipl_payload_adapter_review", "adapter_review_state")
        == "pipl_payload_adapter_review_ready_real_payload_closed"
        and artifact_state(artifacts, "pipl_payload_adapter_review", "adapter_review_ready") is True
        and artifact_state(artifacts, "pipl_payload_adapter_review", "source_chain_valid") is True
        and artifact_state(artifacts, "pipl_payload_adapter_review", "synthetic_parser_contract_reviewed") is True
        and artifact_state(artifacts, "pipl_payload_adapter_review", "metadata_consistency_reviewed") is True
        and artifact_state(artifacts, "pipl_payload_adapter_review", "metadata_budget_reviewed") is True
        and artifact_state(artifacts, "pipl_payload_adapter_review", "parameter_schema_reviewed") is True
        and artifact_state(artifacts, "pipl_payload_adapter_review", "redaction_policy_reviewed") is True
        and artifact_state(artifacts, "pipl_payload_adapter_review", "ofx_describe_policy_reviewed") is True
        and artifact_state(artifacts, "pipl_payload_adapter_review", "real_payload_adapter_allowed_now") is False
        and artifact_state(artifacts, "pipl_payload_adapter_review", "real_payload_input_allowed_now") is False
        and artifact_state(artifacts, "pipl_payload_adapter_review", "real_pipl_payload_parser_enabled") is False
        and artifact_state(artifacts, "pipl_payload_adapter_review", "real_pipl_payload_parsed") is False
        and artifact_state(artifacts, "pipl_payload_adapter_review", "resource_payload_opened") is False
        and artifact_state(artifacts, "pipl_payload_adapter_review", "resource_payload_extracted") is False
        and artifact_state(artifacts, "pipl_payload_adapter_review", "raw_payload_serialized") is False
        and artifact_state(artifacts, "pipl_payload_adapter_review", "output_metadata_only") is True
        and artifact_state(artifacts, "pipl_payload_adapter_review", "parameter_schema_emission_allowed_now")
        is False
        and artifact_state(artifacts, "pipl_payload_adapter_review", "parameter_schema_emitted") is False
        and artifact_state(artifacts, "pipl_payload_adapter_review", "redacted_schema_emitted") is False
        and (artifact_state(artifacts, "pipl_payload_adapter_review", "review_item_count") or 0) > 0
        and artifact_state(artifacts, "pipl_payload_adapter_review", "review_item_count")
        == artifact_state(artifacts, "pipl_payload_adapter_review", "blocking_review_item_count")
    )
    aepx_probe_ready = (
        artifact_state(artifacts, "aepx_static_probe", "probe_state") == "aepx_static_probe_ready_no_write"
        and artifact_state(artifacts, "aepx_static_probe", "xml_parse_state") == "parsed"
    )
    aepx_edit_plan_ready = (
        artifact_state(artifacts, "aepx_edit_plan", "edit_plan_state") == "aepx_edit_plan_ready_no_write"
        and artifact_state(artifacts, "aepx_edit_plan", "write_recommendation") == "do_not_write_project_files"
    )
    aepx_roundtrip_ready = (
        artifact_state(artifacts, "aepx_roundtrip_validator", "roundtrip_state")
        == "aepx_roundtrip_validator_ready_no_write"
        and artifact_state(artifacts, "aepx_roundtrip_validator", "validator_ready") is True
        and artifact_state(artifacts, "aepx_roundtrip_validator", "source_structure_match") is True
        and artifact_state(artifacts, "aepx_roundtrip_validator", "roundtrip_structure_match") is True
        and artifact_state(artifacts, "aepx_roundtrip_validator", "roundtrip_xml_serialized_to_disk") is False
        and artifact_state(artifacts, "aepx_roundtrip_validator", "text_payload_exported") is False
        and artifact_state(artifacts, "aepx_roundtrip_validator", "bdata_payload_exported") is False
    )
    aepx_redacted_text_inventory_ready = (
        artifact_state(artifacts, "aepx_redacted_text_inventory", "inventory_state")
        == "aepx_redacted_text_inventory_ready_no_write"
        and artifact_state(artifacts, "aepx_redacted_text_inventory", "inventory_ready") is True
        and artifact_state(artifacts, "aepx_redacted_text_inventory", "source_roundtrip_state")
        == "aepx_roundtrip_validator_ready_no_write"
        and artifact_state(artifacts, "aepx_redacted_text_inventory", "source_validator_ready") is True
        and artifact_state(artifacts, "aepx_redacted_text_inventory", "text_payload_exported") is False
        and artifact_state(artifacts, "aepx_redacted_text_inventory", "text_payload_hash_exported") is False
        and artifact_state(artifacts, "aepx_redacted_text_inventory", "bdata_payload_exported") is False
        and artifact_state(artifacts, "aepx_redacted_text_inventory", "raw_text_fields_present") is False
        and artifact_state(artifacts, "aepx_redacted_text_inventory", "value_hashes_emitted") is False
        and artifact_state(artifacts, "aepx_redacted_text_inventory", "absolute_source_paths_in_inventory_rows")
        is False
        and artifact_state(artifacts, "aepx_redacted_text_inventory", "raw_payload_serialized") is False
    )
    aepx_redacted_text_classifier_ready = (
        artifact_state(artifacts, "aepx_redacted_text_classifier", "classifier_state")
        == "aepx_redacted_text_classifier_ready_no_write"
        and artifact_state(artifacts, "aepx_redacted_text_classifier", "classifier_ready") is True
        and artifact_state(artifacts, "aepx_redacted_text_classifier", "source_chain_valid") is True
        and artifact_state(artifacts, "aepx_redacted_text_classifier", "source_inventory_state")
        == "aepx_redacted_text_inventory_ready_no_write"
        and artifact_state(artifacts, "aepx_redacted_text_classifier", "source_inventory_ready") is True
        and artifact_state(artifacts, "aepx_redacted_text_classifier", "source_roundtrip_state")
        == "aepx_roundtrip_validator_ready_no_write"
        and artifact_state(artifacts, "aepx_redacted_text_classifier", "source_validator_ready") is True
        and artifact_state(artifacts, "aepx_redacted_text_classifier", "inventory_rows_classified") is True
        and artifact_state(artifacts, "aepx_redacted_text_classifier", "row_count_matches_inventory_summary") is True
        and artifact_state(artifacts, "aepx_redacted_text_classifier", "redacted_text_classification_ready") is True
        and artifact_state(artifacts, "aepx_redacted_text_classifier", "project_write_recommendation")
        == "do_not_write_project_files"
        and artifact_state(artifacts, "aepx_redacted_text_classifier", "project_write_ready") is False
        and artifact_state(artifacts, "aepx_redacted_text_classifier", "project_write_allowed_now") is False
        and artifact_state(artifacts, "aepx_redacted_text_classifier", "classifier_approves_project_write")
        is False
        and artifact_state(artifacts, "aepx_redacted_text_classifier", "schema_write_allowed_now") is False
        and artifact_state(artifacts, "aepx_redacted_text_classifier", "approved_write_candidate_count") == 0
        and artifact_state(artifacts, "aepx_redacted_text_classifier", "unknown_row_count") == 0
        and artifact_state(artifacts, "aepx_redacted_text_classifier", "classification_row_count")
        == artifact_state(artifacts, "aepx_redacted_text_classifier", "no_write_row_count")
        and artifact_state(artifacts, "aepx_redacted_text_classifier", "text_payload_exported") is False
        and artifact_state(artifacts, "aepx_redacted_text_classifier", "text_payload_hash_exported") is False
        and artifact_state(artifacts, "aepx_redacted_text_classifier", "bdata_payload_exported") is False
        and artifact_state(artifacts, "aepx_redacted_text_classifier", "raw_text_fields_present") is False
        and artifact_state(artifacts, "aepx_redacted_text_classifier", "value_hashes_emitted") is False
        and artifact_state(artifacts, "aepx_redacted_text_classifier", "absolute_source_paths_in_classifier_rows")
        is False
        and artifact_state(artifacts, "aepx_redacted_text_classifier", "raw_payload_serialized") is False
    )
    matrix_ready = artifact_state(artifacts, "candidate_matrix", "matrix_state") == "candidate_matrix_ready"
    dependency_ready = artifact_state(artifacts, "dependency_matrix", "dependency_matrix_state") == "dependency_matrix_ready"
    dependency_preflight_ready = (
        artifact_state(artifacts, "dependency_preflight", "preflight_state")
        == "dependency_availability_preflight_ready_no_load"
    )
    dependency_review_ready = artifact_found(artifacts, "dependency_review") and artifact_state(
        artifacts, "dependency_review", "native_load_recommendation"
    ) in {
        "do_not_open_native_load_gate",
        "hold_native_load_until_dependency_review_complete",
        "manual_loader_design_review_only_no_auto_approval",
    }
    sandbox_policy_ready = artifact_state(artifacts, "sandbox_policy", "sandbox_policy_state") == "policy_ready_no_native_load"
    image_suite_ready = artifact_state(artifacts, "image_fixture_suite", "suite_state") == "image_fixture_suite_ready"
    image_validation_ready = (
        artifact_state(artifacts, "image_fixture_validation", "validation_state")
        == "image_fixture_validation_passed_no_load"
    )
    image_suite_selftest_ready = (
        artifact_state(artifacts, "image_suite_selftest", "suite_selftest_state")
        == "image_suite_worker_selftest_passed"
    )
    image_input_smoke_ready = (
        artifact_state(artifacts, "image_input_smoke", "smoke_state")
        == "image_input_smoke_passed_route_closed"
        and artifact_state(artifacts, "image_input_smoke", "worker_identity_passed") is True
        and artifact_state(artifacts, "image_input_smoke", "ofx_identity_passed") is True
    )
    render_contract_ready = (
        artifact_state(artifacts, "render_validation_contract", "contract_state")
        == "render_validation_contract_ready_render_closed"
        and artifact_state(artifacts, "render_validation_contract", "real_render_open") is False
        and artifact_state(artifacts, "render_validation_contract", "no_load_validation_ready") is True
    )
    fixture_ready = labels_found(
        artifacts,
        (
            "candidate_matrix",
            "dependency_matrix",
            "dependency_preflight",
            "dependency_review",
            "sandbox_policy",
            "fixture_manifest",
            "fixture_decision",
            "fixture_dossier",
            "fixture_manual_review_packet",
            "candidate_dependency_scope",
            "candidate_load_gate_dryrun",
        ),
    )
    fixture_manual_review_ready = (
        artifact_state(artifacts, "fixture_manual_review_packet", "review_packet_state")
        == "fixture_manual_review_packet_ready_no_load"
        and artifact_state(artifacts, "fixture_manual_review_packet", "manual_review_ready") is True
        and artifact_state(artifacts, "fixture_manual_review_packet", "approval_ready") is False
    )
    fixture_provenance_review_ready = (
        artifact_state(artifacts, "fixture_provenance_review", "provenance_review_state")
        == "fixture_provenance_review_packet_ready_no_load"
        and artifact_state(artifacts, "fixture_provenance_review", "provenance_review_ready") is True
        and artifact_state(artifacts, "fixture_provenance_review", "manual_review_source_ready") is True
        and artifact_state(artifacts, "fixture_provenance_review", "approval_request_source_ready") is True
        and artifact_state(artifacts, "fixture_provenance_review", "provenance_status")
        == "unknown_requires_user_review"
        and artifact_state(artifacts, "fixture_provenance_review", "license_status")
        == "unknown_requires_user_review"
        and artifact_state(artifacts, "fixture_provenance_review", "local_fixture_safety_status")
        == "no_load_evidence_ready_pending_manual_review"
        and artifact_state(artifacts, "fixture_provenance_review", "approval_request_ready") is True
        and artifact_state(artifacts, "fixture_provenance_review", "approval_can_be_issued_now") is False
        and artifact_state(artifacts, "fixture_provenance_review", "approval_manifest_created") is False
        and artifact_state(artifacts, "fixture_provenance_review", "requires_explicit_user_approval") is True
        and artifact_state(artifacts, "fixture_provenance_review", "current_fixture_approval_valid") is False
        and artifact_state(artifacts, "fixture_provenance_review", "fixture_approval_satisfied") is False
        and artifact_state(artifacts, "fixture_provenance_review", "approval_gate_stays_closed") is True
        and artifact_state(artifacts, "fixture_provenance_review", "native_load_gate") == "closed"
        and artifact_state(artifacts, "fixture_provenance_review", "manual_review_approval_ready") is False
        and artifact_state(artifacts, "fixture_provenance_review", "native_load_gate_stays_closed") is True
        and artifact_state(artifacts, "fixture_provenance_review", "required_approval_token_name")
        == "APPROVE_AEX_LOAD_GATE"
        and artifact_state(artifacts, "fixture_provenance_review", "approval_token_not_stored_in_manifest")
        is True
        and artifact_state(artifacts, "fixture_provenance_review", "approval_only_prepares_next_gate") is True
        and artifact_state(artifacts, "fixture_provenance_review", "accepted_aex_path") is None
        and artifact_state(artifacts, "fixture_provenance_review", "raw_input_paths_serialized") is False
        and artifact_state(artifacts, "fixture_provenance_review", "aex_file_hashed") is False
        and artifact_state(artifacts, "fixture_provenance_review", "aex_file_copied") is False
        and (artifact_state(artifacts, "fixture_provenance_review", "review_item_count") or 0) > 0
        and (artifact_state(artifacts, "fixture_provenance_review", "blocking_review_item_count") or 0) > 0
        and (artifact_state(artifacts, "fixture_provenance_review", "review_question_count") or 0) > 0
        and artifact_state(artifacts, "fixture_provenance_review", "review_question_count")
        == artifact_state(artifacts, "fixture_provenance_review", "unanswered_review_question_count")
    )
    fixture_provenance_answer_template_ready = (
        artifact_state(artifacts, "fixture_provenance_answer_template", "template_state")
        == "fixture_provenance_answer_template_ready_all_answers_pending_no_load"
        and artifact_state(artifacts, "fixture_provenance_answer_template", "template_ready") is True
        and artifact_state(artifacts, "fixture_provenance_answer_template", "answer_template_only") is True
        and artifact_state(artifacts, "fixture_provenance_answer_template", "source_provenance_review_ready")
        is True
        and artifact_state(artifacts, "fixture_provenance_answer_template", "provenance_status")
        == "unknown_requires_user_review"
        and artifact_state(artifacts, "fixture_provenance_answer_template", "license_status")
        == "unknown_requires_user_review"
        and artifact_state(artifacts, "fixture_provenance_answer_template", "local_fixture_safety_status")
        == "no_load_evidence_ready_pending_manual_review"
        and artifact_state(artifacts, "fixture_provenance_answer_template", "answers_present") is False
        and artifact_state(artifacts, "fixture_provenance_answer_template", "answered_question_count") == 0
        and (artifact_state(artifacts, "fixture_provenance_answer_template", "pending_answer_count") or 0) > 0
        and artifact_state(artifacts, "fixture_provenance_answer_template", "pending_answer_count")
        == artifact_state(artifacts, "fixture_provenance_answer_template", "answer_template_entry_count")
        and artifact_state(artifacts, "fixture_provenance_answer_template", "all_answers_pending") is True
        and artifact_state(artifacts, "fixture_provenance_answer_template", "user_answer_artifact_required")
        is True
        and artifact_state(artifacts, "fixture_provenance_answer_template", "answer_template_approves_fixture")
        is False
        and artifact_state(artifacts, "fixture_provenance_answer_template", "answer_template_approves_publication")
        is False
        and artifact_state(artifacts, "fixture_provenance_answer_template", "answer_template_approves_native_load")
        is False
        and artifact_state(artifacts, "fixture_provenance_answer_template", "approval_can_be_issued_now")
        is False
        and artifact_state(artifacts, "fixture_provenance_answer_template", "approval_manifest_created") is False
        and artifact_state(artifacts, "fixture_provenance_answer_template", "current_fixture_approval_valid")
        is False
        and artifact_state(artifacts, "fixture_provenance_answer_template", "fixture_approval_satisfied")
        is False
        and artifact_state(artifacts, "fixture_provenance_answer_template", "approval_gate_stays_closed") is True
        and artifact_state(artifacts, "fixture_provenance_answer_template", "native_load_gate") == "closed"
        and artifact_state(artifacts, "fixture_provenance_answer_template", "native_load_gate_stays_closed")
        is True
        and artifact_state(artifacts, "fixture_provenance_answer_template", "accepted_aex_path") is None
        and artifact_state(artifacts, "fixture_provenance_answer_template", "raw_input_paths_serialized") is False
        and artifact_state(artifacts, "fixture_provenance_answer_template", "aex_file_hashed") is False
        and artifact_state(artifacts, "fixture_provenance_answer_template", "aex_file_copied") is False
    )
    fixture_provenance_answer_validator_selftest_ready = (
        artifact_state(artifacts, "fixture_provenance_answer_validator_selftest", "validator_selftest_state")
        == "fixture_provenance_answer_validator_selftest_passed_no_user_answers"
        and artifact_state(artifacts, "fixture_provenance_answer_validator_selftest", "validator_ready") is True
        and artifact_state(artifacts, "fixture_provenance_answer_validator_selftest", "source_answer_template_ready")
        is True
        and artifact_state(artifacts, "fixture_provenance_answer_validator_selftest", "answer_template_only")
        is True
        and artifact_state(
            artifacts, "fixture_provenance_answer_validator_selftest", "real_user_answer_artifact_consumed"
        )
        is False
        and artifact_state(artifacts, "fixture_provenance_answer_validator_selftest", "synthetic_user_answers_used")
        is True
        and artifact_state(
            artifacts, "fixture_provenance_answer_validator_selftest", "synthetic_payloads_serialized"
        )
        is False
        and artifact_state(artifacts, "fixture_provenance_answer_validator_selftest", "answer_schema_validated")
        is True
        and (artifact_state(artifacts, "fixture_provenance_answer_validator_selftest", "synthetic_case_count") or 0)
        > 0
        and artifact_state(artifacts, "fixture_provenance_answer_validator_selftest", "synthetic_case_count")
        == artifact_state(artifacts, "fixture_provenance_answer_validator_selftest", "synthetic_case_passed_count")
        and artifact_state(
            artifacts, "fixture_provenance_answer_validator_selftest", "synthetic_case_failed_count"
        )
        == 0
        and (
            artifact_state(artifacts, "fixture_provenance_answer_validator_selftest", "synthetic_valid_case_count")
            or 0
        )
        > 0
        and (
            artifact_state(artifacts, "fixture_provenance_answer_validator_selftest", "synthetic_rejected_case_count")
            or 0
        )
        > 0
        and artifact_state(artifacts, "fixture_provenance_answer_validator_selftest", "answers_present") is False
        and artifact_state(artifacts, "fixture_provenance_answer_validator_selftest", "answered_question_count")
        == 0
        and artifact_state(
            artifacts, "fixture_provenance_answer_validator_selftest", "answers_validated_for_manual_review"
        )
        is False
        and artifact_state(
            artifacts, "fixture_provenance_answer_validator_selftest", "approval_can_be_issued_now"
        )
        is False
        and artifact_state(
            artifacts, "fixture_provenance_answer_validator_selftest", "approval_manifest_created"
        )
        is False
        and artifact_state(
            artifacts, "fixture_provenance_answer_validator_selftest", "current_fixture_approval_valid"
        )
        is False
        and artifact_state(
            artifacts, "fixture_provenance_answer_validator_selftest", "fixture_approval_satisfied"
        )
        is False
        and artifact_state(
            artifacts, "fixture_provenance_answer_validator_selftest", "approval_gate_stays_closed"
        )
        is True
        and artifact_state(artifacts, "fixture_provenance_answer_validator_selftest", "native_load_gate")
        == "closed"
        and artifact_state(
            artifacts, "fixture_provenance_answer_validator_selftest", "native_load_gate_stays_closed"
        )
        is True
        and artifact_state(artifacts, "fixture_provenance_answer_validator_selftest", "accepted_aex_path")
        is None
        and artifact_state(
            artifacts, "fixture_provenance_answer_validator_selftest", "raw_input_paths_serialized"
        )
        is False
        and artifact_state(artifacts, "fixture_provenance_answer_validator_selftest", "aex_file_hashed")
        is False
        and artifact_state(artifacts, "fixture_provenance_answer_validator_selftest", "aex_file_copied")
        is False
    )
    fixture_approval_verifier_ready = (
        artifact_state(artifacts, "fixture_approval_verifier", "approval_verifier_state")
        == "fixture_approval_verifier_ready_no_approval"
        and artifact_state(artifacts, "fixture_approval_verifier", "approval_verifier_ready") is True
        and artifact_state(artifacts, "fixture_approval_verifier", "current_fixture_approval_valid") is False
        and artifact_state(artifacts, "fixture_approval_verifier", "fixture_approval_satisfied") is False
        and artifact_state(artifacts, "fixture_approval_verifier", "approval_gate_stays_closed") is True
        and artifact_state(artifacts, "fixture_approval_verifier", "manual_review_ready") is True
        and artifact_state(artifacts, "fixture_approval_verifier", "manual_review_approval_ready") is False
        and artifact_state(artifacts, "fixture_approval_verifier", "candidate_dependencies_clear") is True
        and artifact_state(artifacts, "fixture_approval_verifier", "candidate_dependency_blockers_present")
        is False
        and artifact_state(artifacts, "fixture_approval_verifier", "path_policy_closed") is True
        and artifact_state(artifacts, "fixture_approval_verifier", "raw_input_paths_serialized") is False
        and artifact_state(artifacts, "fixture_approval_verifier", "candidate_load_gate_closed") is True
        and artifact_state(artifacts, "fixture_approval_verifier", "source_candidate_load_gate_state")
        == "candidate_load_gate_dryrun_ready_no_load"
        and artifact_state(artifacts, "fixture_approval_verifier", "synthetic_approval_checks_passed") is True
        and artifact_state(artifacts, "fixture_approval_verifier", "required_approval_token_name")
        == "APPROVE_AEX_LOAD_GATE"
        and artifact_state(artifacts, "fixture_approval_verifier", "approval_token_not_stored_in_manifest")
        is True
        and artifact_state(artifacts, "fixture_approval_verifier", "approval_only_prepares_next_gate") is True
    )
    fixture_approval_request_ready = (
        artifact_state(artifacts, "fixture_approval_request", "approval_request_state")
        == "fixture_approval_request_ready_pending_manual_approval"
        and artifact_state(artifacts, "fixture_approval_request", "approval_request_ready") is True
        and artifact_state(artifacts, "fixture_approval_request", "approval_can_be_issued_now") is False
        and artifact_state(artifacts, "fixture_approval_request", "approval_manifest_created") is False
        and artifact_state(artifacts, "fixture_approval_request", "requires_explicit_user_approval") is True
        and artifact_state(artifacts, "fixture_approval_request", "current_fixture_approval_valid") is False
        and artifact_state(artifacts, "fixture_approval_request", "fixture_approval_satisfied") is False
        and artifact_state(artifacts, "fixture_approval_request", "manual_review_ready") is True
        and artifact_state(artifacts, "fixture_approval_request", "manual_review_approval_ready") is False
        and artifact_state(artifacts, "fixture_approval_request", "approval_gate_stays_closed") is True
        and artifact_state(artifacts, "fixture_approval_request", "native_load_gate") == "closed"
        and artifact_state(artifacts, "fixture_approval_request", "candidate_dependencies_clear") is True
        and artifact_state(artifacts, "fixture_approval_request", "path_policy_closed") is True
        and artifact_state(artifacts, "fixture_approval_request", "candidate_load_gate_closed") is True
        and artifact_state(artifacts, "fixture_approval_request", "required_approval_token_name")
        == "APPROVE_AEX_LOAD_GATE"
        and artifact_state(artifacts, "fixture_approval_request", "approval_token_not_stored_in_manifest")
        is True
        and artifact_state(artifacts, "fixture_approval_request", "approval_only_prepares_next_gate") is True
    )
    candidate_test_handoff_ready = (
        artifact_state(artifacts, "candidate_test_handoff", "handoff_state")
        == "candidate_test_handoff_ready_no_load_native_closed"
        and artifact_state(artifacts, "candidate_test_handoff", "handoff_packet_ready") is True
        and artifact_state(artifacts, "candidate_test_handoff", "no_load_test_handoff_ready") is True
        and artifact_state(artifacts, "candidate_test_handoff", "native_test_handoff_ready") is False
        and artifact_state(artifacts, "candidate_test_handoff", "approval_request_ready") is True
        and artifact_state(artifacts, "candidate_test_handoff", "approval_can_be_issued_now") is False
        and artifact_state(artifacts, "candidate_test_handoff", "approval_manifest_created") is False
        and artifact_state(artifacts, "candidate_test_handoff", "requires_explicit_user_approval") is True
        and artifact_state(artifacts, "candidate_test_handoff", "fixture_approval_satisfied") is False
        and artifact_state(artifacts, "candidate_test_handoff", "native_load_gate") == "closed"
        and artifact_state(artifacts, "candidate_test_handoff", "candidate_dependencies_clear") is True
        and artifact_state(artifacts, "candidate_test_handoff", "global_dependency_blockers_apply_to_candidate")
        is False
        and artifact_state(artifacts, "candidate_test_handoff", "native_loader_design_ready") is True
        and artifact_state(artifacts, "candidate_test_handoff", "runtime_containment_ready") is True
        and artifact_state(artifacts, "candidate_test_handoff", "runtime_containment_selftest_passed") is True
        and artifact_state(artifacts, "candidate_test_handoff", "synthetic_subprocess_only") is True
        and artifact_state(artifacts, "candidate_test_handoff", "normal_exit_case_passed") is True
        and artifact_state(artifacts, "candidate_test_handoff", "stderr_capture_passed") is True
        and artifact_state(artifacts, "candidate_test_handoff", "timeout_case_passed") is True
        and artifact_state(artifacts, "candidate_test_handoff", "child_cleanup_passed") is True
        and artifact_state(artifacts, "candidate_test_handoff", "path_acceptance_ready") is False
        and artifact_state(artifacts, "candidate_test_handoff", "aex_path_acceptance_enabled") is False
        and artifact_state(artifacts, "candidate_test_handoff", "accepted_aex_path") is None
        and artifact_state(artifacts, "candidate_test_handoff", "path_payload_supplied") is False
        and artifact_state(artifacts, "candidate_test_handoff", "path_policy_selftest_passed") is True
        and artifact_state(artifacts, "candidate_test_handoff", "candidate_path_string_accepted") is False
        and artifact_state(artifacts, "candidate_test_handoff", "raw_input_paths_serialized") is False
        and artifact_state(artifacts, "candidate_test_handoff", "no_load_image_test_ready") is True
        and artifact_state(artifacts, "candidate_test_handoff", "image_fixture_validation_passed") is True
        and artifact_state(artifacts, "candidate_test_handoff", "worker_identity_passed") is True
        and artifact_state(artifacts, "candidate_test_handoff", "ofx_identity_passed") is True
        and artifact_state(artifacts, "candidate_test_handoff", "no_load_render_contract_ready") is True
        and artifact_state(artifacts, "candidate_test_handoff", "real_render_open") is False
        and artifact_state(artifacts, "candidate_test_handoff", "no_load_validation_ready") is True
        and artifact_state(artifacts, "candidate_test_handoff", "no_load_ofx_mock_ready") is True
        and artifact_state(artifacts, "candidate_test_handoff", "real_route_open") is False
        and artifact_state(artifacts, "candidate_test_handoff", "mock_route_ready") is True
    )
    candidate_test_runner_dryrun_ready = (
        artifact_state(artifacts, "candidate_test_runner_dryrun", "runner_dryrun_state")
        == "candidate_no_load_test_runner_dryrun_ready_native_closed"
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "runner_dryrun_ready") is True
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "dry_run_only") is True
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "would_execute") is False
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "execution_performed") is False
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "no_load_test_plan_ready") is True
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "native_test_plan_ready") is False
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "real_render_plan_ready") is False
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "real_ofx_route_plan_ready") is False
        and (artifact_state(artifacts, "candidate_test_runner_dryrun", "image_fixture_case_count") or 0) > 0
        and (artifact_state(artifacts, "candidate_test_runner_dryrun", "planned_no_load_case_count") or 0) > 0
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "planned_native_case_count") == 0
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "planned_real_render_case_count") == 0
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "planned_real_ofx_route_case_count") == 0
        and (artifact_state(artifacts, "candidate_test_runner_dryrun", "blocked_case_count") or 0) > 0
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "image_fixture_validation_passed") is True
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "worker_suite_identity_passed") is True
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "ofx_suite_identity_passed") is True
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "image_smoke_identity_passed") is True
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "render_contract_review_ready") is True
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "ofx_route_contract_review_ready") is True
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "approval_manifest_created") is False
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "fixture_approval_satisfied") is False
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "native_load_gate") == "closed"
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "path_acceptance_ready") is False
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "aex_path_acceptance_enabled") is False
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "accepted_aex_path") is None
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "path_payload_supplied") is False
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "real_render_open") is False
        and artifact_state(artifacts, "candidate_test_runner_dryrun", "real_route_open") is False
    )
    candidate_test_runner_ready = (
        artifact_state(artifacts, "candidate_test_runner", "runner_state")
        == "candidate_no_load_test_runner_passed_native_closed"
        and artifact_state(artifacts, "candidate_test_runner", "runner_ready") is True
        and artifact_state(artifacts, "candidate_test_runner", "dry_run_only") is False
        and artifact_state(artifacts, "candidate_test_runner", "would_execute") is True
        and artifact_state(artifacts, "candidate_test_runner", "execution_performed") is True
        and artifact_state(artifacts, "candidate_test_runner", "no_load_test_plan_ready") is True
        and artifact_state(artifacts, "candidate_test_runner", "no_load_execution_performed") is True
        and artifact_state(artifacts, "candidate_test_runner", "native_test_plan_ready") is False
        and artifact_state(artifacts, "candidate_test_runner", "native_execution_performed") is False
        and artifact_state(artifacts, "candidate_test_runner", "real_render_plan_ready") is False
        and artifact_state(artifacts, "candidate_test_runner", "real_render_execution_performed") is False
        and artifact_state(artifacts, "candidate_test_runner", "real_ofx_route_plan_ready") is False
        and artifact_state(artifacts, "candidate_test_runner", "real_ofx_route_execution_performed") is False
        and artifact_state(artifacts, "candidate_test_runner", "worker_invoked") is True
        and artifact_state(artifacts, "candidate_test_runner", "ofx_mock_invoked") is True
        and artifact_state(artifacts, "candidate_test_runner", "ofx_runtime_invoked") is False
        and artifact_state(artifacts, "candidate_test_runner", "worker_identity_passed") is True
        and artifact_state(artifacts, "candidate_test_runner", "ofx_noop_identity_passed") is True
        and artifact_state(artifacts, "candidate_test_runner", "blocked_load_aex_verified") is True
        and artifact_state(artifacts, "candidate_test_runner", "image_fixture_validation_passed") is True
        and artifact_state(artifacts, "candidate_test_runner", "image_smoke_identity_passed") is True
        and artifact_state(artifacts, "candidate_test_runner", "render_contract_review_ready") is True
        and artifact_state(artifacts, "candidate_test_runner", "ofx_route_contract_review_ready") is True
        and (artifact_state(artifacts, "candidate_test_runner", "image_fixture_case_count") or 0) > 0
        and (artifact_state(artifacts, "candidate_test_runner", "planned_no_load_case_count") or 0) > 0
        and artifact_state(artifacts, "candidate_test_runner", "planned_native_case_count") == 0
        and artifact_state(artifacts, "candidate_test_runner", "planned_real_render_case_count") == 0
        and artifact_state(artifacts, "candidate_test_runner", "planned_real_ofx_route_case_count") == 0
        and (artifact_state(artifacts, "candidate_test_runner", "executed_worker_case_count") or 0) > 0
        and (artifact_state(artifacts, "candidate_test_runner", "executed_ofx_noop_case_count") or 0) > 0
        and (artifact_state(artifacts, "candidate_test_runner", "executed_worker_lifecycle_case_count") or 0) > 0
        and artifact_state(artifacts, "candidate_test_runner", "executed_native_case_count") == 0
        and artifact_state(artifacts, "candidate_test_runner", "executed_real_render_case_count") == 0
        and artifact_state(artifacts, "candidate_test_runner", "executed_real_ofx_route_case_count") == 0
        and (artifact_state(artifacts, "candidate_test_runner", "blocked_case_count") or 0) > 0
        and artifact_state(artifacts, "candidate_test_runner", "approval_manifest_created") is False
        and artifact_state(artifacts, "candidate_test_runner", "fixture_approval_satisfied") is False
        and artifact_state(artifacts, "candidate_test_runner", "native_load_gate") == "closed"
        and artifact_state(artifacts, "candidate_test_runner", "path_acceptance_ready") is False
        and artifact_state(artifacts, "candidate_test_runner", "aex_path_acceptance_enabled") is False
        and artifact_state(artifacts, "candidate_test_runner", "accepted_aex_path") is None
        and artifact_state(artifacts, "candidate_test_runner", "path_payload_supplied") is False
        and artifact_state(artifacts, "candidate_test_runner", "real_render_open") is False
        and artifact_state(artifacts, "candidate_test_runner", "real_route_open") is False
    )
    candidate_compatibility_card_ready = (
        artifact_state(artifacts, "candidate_compatibility_card", "compatibility_card_state")
        == "candidate_compatibility_card_ready_no_load"
        and artifact_state(artifacts, "candidate_compatibility_card", "compatibility_card_ready") is True
        and artifact_state(artifacts, "candidate_compatibility_card", "unsafe_exports_present") is False
        and artifact_state(artifacts, "candidate_compatibility_card", "absolute_ppm_paths_exported")
        is False
        and artifact_state(artifacts, "candidate_compatibility_card", "absolute_aex_paths_exported")
        is False
        and artifact_state(artifacts, "candidate_compatibility_card", "approval_can_be_issued_now")
        is False
        and artifact_state(artifacts, "candidate_compatibility_card", "approval_manifest_created")
        is False
        and artifact_state(artifacts, "candidate_compatibility_card", "current_fixture_approval_valid")
        is False
        and artifact_state(artifacts, "candidate_compatibility_card", "fixture_approval_satisfied")
        is False
        and artifact_state(artifacts, "candidate_compatibility_card", "approval_gate_stays_closed")
        is True
        and artifact_state(artifacts, "candidate_compatibility_card", "native_load_gate") == "closed"
        and artifact_state(artifacts, "candidate_compatibility_card", "native_load_gate_stays_closed")
        is True
        and artifact_state(artifacts, "candidate_compatibility_card", "accepted_aex_path") is None
        and artifact_state(artifacts, "candidate_compatibility_card", "path_payload_supplied") is False
        and artifact_state(artifacts, "candidate_compatibility_card", "path_acceptance_ready") is False
        and artifact_state(artifacts, "candidate_compatibility_card", "aex_path_acceptance_enabled")
        is False
        and artifact_state(artifacts, "candidate_compatibility_card", "real_render_open") is False
        and artifact_state(artifacts, "candidate_compatibility_card", "real_route_open") is False
        and artifact_state(artifacts, "candidate_compatibility_card", "aex_file_hashed") is False
        and artifact_state(artifacts, "candidate_compatibility_card", "aex_file_copied") is False
    )
    candidate_image_compat_mock_check = artifact_state(
        artifacts, "candidate_image_compat_mock", "transform_check"
    )
    candidate_image_compat_mock_ready = (
        artifact_state(artifacts, "candidate_image_compat_mock", "mock_state")
        == "candidate_image_compat_mock_passed_no_load"
        and artifact_state(artifacts, "candidate_image_compat_mock", "mock_ready") is True
        and artifact_state(artifacts, "candidate_image_compat_mock", "source_compatibility_card_state")
        == "candidate_compatibility_card_ready_no_load"
        and artifact_state(artifacts, "candidate_image_compat_mock", "source_compatibility_card_ready")
        is True
        and artifact_state(artifacts, "candidate_image_compat_mock", "source_native_load_gate") == "closed"
        and artifact_state(artifacts, "candidate_image_compat_mock", "source_native_load_gate_stays_closed")
        is True
        and artifact_state(artifacts, "candidate_image_compat_mock", "source_real_render_open") is False
        and artifact_state(artifacts, "candidate_image_compat_mock", "source_real_route_open") is False
        and artifact_state(artifacts, "candidate_image_compat_mock", "source_path_acceptance_ready")
        is False
        and artifact_state(artifacts, "candidate_image_compat_mock", "source_aex_path_acceptance_enabled")
        is False
        and artifact_state(artifacts, "candidate_image_compat_mock", "source_fixture_approval_satisfied")
        is False
        and artifact_state(artifacts, "candidate_image_compat_mock", "source_absolute_ppm_paths_exported")
        is False
        and artifact_state(artifacts, "candidate_image_compat_mock", "source_absolute_aex_paths_exported")
        is False
        and artifact_state(artifacts, "candidate_image_compat_mock", "input_ppm_absolute_path_exported")
        is False
        and artifact_state(artifacts, "candidate_image_compat_mock", "output_ppm_absolute_path_exported")
        is False
        and isinstance(candidate_image_compat_mock_check, dict)
        and candidate_image_compat_mock_check.get("pixel_match_expected") is True
        and candidate_image_compat_mock_check.get("dimension_match_expected") is True
        and candidate_image_compat_mock_check.get("input_dimension_match") is True
        and artifact_state(artifacts, "candidate_image_compat_mock", "candidate_image_mock_performed")
        is True
        and artifact_state(artifacts, "candidate_image_compat_mock", "mock_transform_performed") is True
    )
    candidate_ofx_bridge_ready = (
        artifact_state(artifacts, "candidate_ofx_bridge", "bridge_state")
        == "candidate_ofx_bridge_ready_no_load_route_closed"
        and artifact_state(artifacts, "candidate_ofx_bridge", "bridge_ready") is True
        and artifact_state(artifacts, "candidate_ofx_bridge", "candidate_image_mock_available") is True
        and artifact_state(artifacts, "candidate_ofx_bridge", "ofx_closed_route_contract_available")
        is True
        and artifact_state(artifacts, "candidate_ofx_bridge", "ofx_bridge_packet_created") is True
        and artifact_state(artifacts, "candidate_ofx_bridge", "source_compatibility_card_state")
        == "candidate_compatibility_card_ready_no_load"
        and artifact_state(artifacts, "candidate_ofx_bridge", "source_image_mock_state")
        == "candidate_image_compat_mock_passed_no_load"
        and artifact_state(artifacts, "candidate_ofx_bridge", "source_ofx_facade_state")
        == "deferred_loader_not_ready"
        and artifact_state(artifacts, "candidate_ofx_bridge", "source_ofx_route_contract_state")
        == "ofx_route_contract_ready_route_closed"
        and artifact_state(artifacts, "candidate_ofx_bridge", "bridge_allowed_route")
        == "no_op_identity_only"
        and artifact_state(artifacts, "candidate_ofx_bridge", "mock_route_ready") is True
        and artifact_state(artifacts, "candidate_ofx_bridge", "real_route_open") is False
        and artifact_state(artifacts, "candidate_ofx_bridge", "real_ofx_route_ready") is False
        and artifact_state(artifacts, "candidate_ofx_bridge", "ofx_runtime_invoked") is False
        and artifact_state(artifacts, "candidate_ofx_bridge", "aex_runtime_invoked") is False
        and artifact_state(artifacts, "candidate_ofx_bridge", "ofx_describe_ready") is False
        and artifact_state(artifacts, "candidate_ofx_bridge", "ofx_render_ready") is False
        and artifact_state(artifacts, "candidate_ofx_bridge", "render_equivalence_claim_ready")
        is False
        and artifact_state(artifacts, "candidate_ofx_bridge", "absolute_ppm_paths_exported") is False
        and artifact_state(artifacts, "candidate_ofx_bridge", "absolute_aex_paths_exported") is False
        and artifact_state(artifacts, "candidate_ofx_bridge", "ofx_bridge_path_payload_exported")
        is False
    )
    candidate_ofx_host_harness_dryrun_ready = (
        artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "harness_dryrun_state")
        == "candidate_ofx_host_harness_dryrun_ready_route_closed"
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "harness_dryrun_ready")
        is True
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "dry_run_only") is True
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "would_execute") is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "execution_performed")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "host_harness_kind")
        == "ofx_noop_host_harness_planning"
        and (artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "planned_case_count") or 0)
        > 0
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "planned_noop_describe_case_count")
        == 1
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "planned_noop_render_case_count")
        == 1
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "planned_real_describe_case_count")
        == 0
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "planned_real_render_case_count")
        == 0
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "source_bridge_state")
        == "candidate_ofx_bridge_ready_no_load_route_closed"
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "source_bridge_allowed_route")
        == "no_op_identity_only"
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "source_mock_route_ready")
        is True
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "source_real_route_open")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "source_real_ofx_route_ready")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "source_ofx_runtime_invoked")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "source_aex_runtime_invoked")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "source_ofx_describe_ready")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "source_ofx_render_ready")
        is False
        and artifact_state(
            artifacts, "candidate_ofx_host_harness_dryrun", "source_render_equivalence_claim_ready"
        )
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "real_route_open") is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "real_ofx_route_ready")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "ofx_runtime_invoked")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "aex_runtime_invoked")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "ofx_describe_ready")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "ofx_render_ready")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "render_equivalence_claim_ready")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "absolute_ppm_paths_exported")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "absolute_aex_paths_exported")
        is False
        and artifact_state(
            artifacts, "candidate_ofx_host_harness_dryrun", "host_harness_path_payload_exported"
        )
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_dryrun", "requires_future_runtime_approval")
        is True
    )
    candidate_ofx_host_harness_selftest_ready = (
        artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "host_harness_selftest_state")
        == "candidate_ofx_host_harness_selftest_passed_synthetic_route_closed"
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "host_harness_selftest_ready")
        is True
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "host_harness_kind")
        == "ofx_noop_host_harness_synthetic_selftest"
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "synthetic_only") is True
        and artifact_state(
            artifacts, "candidate_ofx_host_harness_selftest", "synthetic_contract_checks_performed"
        )
        is True
        and artifact_state(
            artifacts, "candidate_ofx_host_harness_selftest", "synthetic_contract_execution_performed"
        )
        is True
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "real_execution_performed")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "real_harness_execution_performed")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "dry_run_consumed") is True
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "planned_cases_verified")
        is True
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "source_harness_dryrun_state")
        == "candidate_ofx_host_harness_dryrun_ready_route_closed"
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "source_harness_dryrun_ready")
        is True
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "source_dry_run_only") is True
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "source_would_execute") is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "source_execution_performed")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "source_host_harness_kind")
        == "ofx_noop_host_harness_planning"
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "source_bridge_state")
        == "candidate_ofx_bridge_ready_no_load_route_closed"
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "source_bridge_allowed_route")
        == "no_op_identity_only"
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "checked_case_count") == 2
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "checked_noop_describe_case_count")
        == 1
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "checked_noop_render_case_count")
        == 1
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "checked_real_describe_case_count")
        == 0
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "checked_real_render_case_count")
        == 0
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "case_result_count") == 2
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "case_passed_count") == 2
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "case_failed_count") == 0
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "descriptor_contract_checked")
        is True
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "render_identity_contract_checked")
        is True
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "synthetic_descriptor_created")
        is True
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "synthetic_render_contract_created")
        is True
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "candidate_mock_surface_reused")
        is True
        and artifact_state(
            artifacts, "candidate_ofx_host_harness_selftest", "candidate_mock_surface_reused_as_string_only"
        )
        is True
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "ppm_pixel_read_performed")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "real_route_open") is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "real_ofx_route_ready")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "ofx_runtime_invoked")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "aex_runtime_invoked")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "ofx_describe_ready")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "ofx_render_ready") is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "render_equivalence_claim_ready")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "absolute_ppm_paths_exported")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "absolute_aex_paths_exported")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "host_harness_path_payload_exported")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_harness_selftest", "requires_future_runtime_approval")
        is True
    )
    candidate_ofx_runtime_boundary_contract_ready = (
        artifact_state(
            artifacts,
            "candidate_ofx_runtime_boundary_contract",
            "candidate_ofx_runtime_boundary_contract_state",
        )
        == "candidate_ofx_runtime_boundary_contract_ready_no_runtime_route_closed"
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "contract_state")
        == "candidate_ofx_runtime_boundary_contract_ready_runtime_closed"
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "runtime_boundary_ready")
        is True
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "boundary_contract_ready")
        is True
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "source_bridge_state")
        == "candidate_ofx_bridge_ready_no_load_route_closed"
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "source_bridge_allowed_route")
        == "no_op_identity_only"
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "source_harness_dryrun_state")
        == "candidate_ofx_host_harness_dryrun_ready_route_closed"
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "source_harness_dryrun_ready")
        is True
        and artifact_state(
            artifacts, "candidate_ofx_runtime_boundary_contract", "source_host_harness_selftest_state"
        )
        == "candidate_ofx_host_harness_selftest_passed_synthetic_route_closed"
        and artifact_state(
            artifacts, "candidate_ofx_runtime_boundary_contract", "source_host_harness_selftest_ready"
        )
        is True
        and artifact_state(
            artifacts, "candidate_ofx_runtime_boundary_contract", "source_real_harness_execution_performed"
        )
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "source_ppm_pixel_read_performed")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "source_ofx_route_contract_state")
        == "ofx_route_contract_ready_route_closed"
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "source_ofx_route_allowed_route")
        == "no_op_identity_only"
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "source_ofx_route_real_route_open")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "source_ofx_route_mock_route_ready")
        is True
        and artifact_state(
            artifacts, "candidate_ofx_runtime_boundary_contract", "source_native_runtime_contract_state"
        )
        == "runtime_containment_contract_ready_no_load"
        and artifact_state(
            artifacts, "candidate_ofx_runtime_boundary_contract", "source_native_runtime_contract_ready"
        )
        is True
        and artifact_state(
            artifacts, "candidate_ofx_runtime_boundary_contract", "source_native_runtime_path_acceptance_ready"
        )
        is False
        and artifact_state(
            artifacts, "candidate_ofx_runtime_boundary_contract", "source_native_runtime_process_isolation_required"
        )
        is True
        and artifact_state(
            artifacts, "candidate_ofx_runtime_boundary_contract", "source_native_runtime_candidate_dependencies_clear"
        )
        is True
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "source_fixture_approval_satisfied")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "no_load_boundary_contract_created")
        is True
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "approval_gate_count") == 6
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "required_runtime_evidence_count")
        == 6
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "ofx_runtime_allowed_now")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "ofx_runtime_invocation_ready")
        is False
        and artifact_state(
            artifacts, "candidate_ofx_runtime_boundary_contract", "ofx_runtime_instantiation_performed"
        )
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "host_process_launch_enabled")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "path_acceptance_ready") is False
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "aex_path_acceptance_enabled")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "accepted_aex_path") is None
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "real_route_open") is False
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "mock_route_ready") is True
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "no_op_identity_route_preserved")
        is True
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "ofx_runtime_invoked") is False
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "ofx_describe_ready") is False
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "ofx_render_ready") is False
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "ppm_pixel_read_performed")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "runtime_boundary_path_payload_exported")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "ofx_host_path_payload_supplied")
        is False
        and artifact_state(
            artifacts, "candidate_ofx_runtime_boundary_contract", "ofx_plugin_binary_path_payload_supplied"
        )
        is False
        and artifact_state(
            artifacts, "candidate_ofx_runtime_boundary_contract", "runtime_approval_required_before_invocation"
        )
        is True
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "requires_future_runtime_approval")
        is True
        and artifact_state(artifacts, "candidate_ofx_runtime_boundary_contract", "requires_future_fixture_approval")
        is True
        and artifact_state(
            artifacts, "candidate_ofx_runtime_boundary_contract", "requires_future_render_validation_approval"
        )
        is True
    )
    candidate_ofx_runtime_approval_request_ready = (
        artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "runtime_approval_request_state")
        == "candidate_ofx_runtime_approval_request_ready_pending_manual_approval"
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "runtime_approval_request_ready")
        is True
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "runtime_approval_request_created")
        is True
        and artifact_state(
            artifacts, "candidate_ofx_runtime_approval_request", "runtime_approval_can_be_issued_now"
        )
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "runtime_approval_manifest_created")
        is False
        and artifact_state(
            artifacts, "candidate_ofx_runtime_approval_request", "runtime_approval_gate_stays_closed"
        )
        is True
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "requires_explicit_user_approval")
        is True
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "required_approval_token_name")
        == "APPROVE_OFX_RUNTIME_INVOCATION"
        and artifact_state(
            artifacts, "candidate_ofx_runtime_approval_request", "approval_token_not_stored_in_manifest"
        )
        is True
        and artifact_state(
            artifacts, "candidate_ofx_runtime_approval_request", "approval_only_prepares_runtime_review"
        )
        is True
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "source_boundary_contract_state")
        == "candidate_ofx_runtime_boundary_contract_ready_no_runtime_route_closed"
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "source_boundary_contract_ready")
        is True
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "source_contract_state")
        == "candidate_ofx_runtime_boundary_contract_ready_runtime_closed"
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "source_ofx_runtime_allowed_now")
        is False
        and artifact_state(
            artifacts, "candidate_ofx_runtime_approval_request", "source_ofx_runtime_invocation_ready"
        )
        is False
        and artifact_state(
            artifacts, "candidate_ofx_runtime_approval_request", "source_host_process_launch_enabled"
        )
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "source_path_acceptance_ready")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "source_real_route_open")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "source_mock_route_ready")
        is True
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "source_ofx_runtime_invoked")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "source_ppm_pixel_read_performed")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "source_fixture_approval_satisfied")
        is False
        and artifact_state(
            artifacts, "candidate_ofx_runtime_approval_request", "source_requires_future_runtime_approval"
        )
        is True
        and artifact_state(
            artifacts, "candidate_ofx_runtime_approval_request", "source_requires_future_fixture_approval"
        )
        is True
        and artifact_state(
            artifacts, "candidate_ofx_runtime_approval_request", "source_requires_future_render_validation_approval"
        )
        is True
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "review_checklist_count") == 6
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "approval_blocker_count") >= 1
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "ofx_runtime_invocation_ready")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "host_process_launch_enabled")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "path_acceptance_ready") is False
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "aex_path_acceptance_enabled")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "accepted_aex_path") is None
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "real_route_open") is False
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "mock_route_ready") is True
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "ofx_runtime_invoked") is False
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_request", "ppm_pixel_read_performed") is False
        and artifact_state(
            artifacts, "candidate_ofx_runtime_approval_request", "runtime_approval_path_payload_exported"
        )
        is False
    )
    candidate_ofx_runtime_approval_verifier_ready = (
        artifact_state(artifacts, "candidate_ofx_runtime_approval_verifier", "runtime_approval_verifier_state")
        == "candidate_ofx_runtime_approval_verifier_ready_no_approval"
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_verifier", "runtime_approval_verifier_ready")
        is True
        and artifact_state(
            artifacts, "candidate_ofx_runtime_approval_verifier", "runtime_approval_verified_not_approved"
        )
        is True
        and artifact_state(
            artifacts, "candidate_ofx_runtime_approval_verifier", "source_runtime_approval_request_state"
        )
        == "candidate_ofx_runtime_approval_request_ready_pending_manual_approval"
        and artifact_state(
            artifacts, "candidate_ofx_runtime_approval_verifier", "source_runtime_approval_request_ready"
        )
        is True
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_verifier", "current_runtime_approval_valid")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_verifier", "runtime_approval_satisfied")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_verifier", "runtime_approval_gate_stays_closed")
        is True
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_verifier", "runtime_approval_gate_closed")
        is True
        and artifact_state(
            artifacts, "candidate_ofx_runtime_approval_verifier", "boundary_contract_cross_checked"
        )
        is True
        and artifact_state(
            artifacts, "candidate_ofx_runtime_approval_verifier", "boundary_contract_matches_request"
        )
        is True
        and artifact_state(
            artifacts, "candidate_ofx_runtime_approval_verifier", "explicit_runtime_approval_present"
        )
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_verifier", "requires_explicit_user_approval")
        is True
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_verifier", "required_approval_token_name")
        == "APPROVE_OFX_RUNTIME_INVOCATION"
        and artifact_state(
            artifacts, "candidate_ofx_runtime_approval_verifier", "approval_token_not_stored_in_manifest"
        )
        is True
        and artifact_state(
            artifacts, "candidate_ofx_runtime_approval_verifier", "approval_only_prepares_runtime_review"
        )
        is True
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_verifier", "approval_blocker_count") >= 1
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_verifier", "request_blockers_clear")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_verifier", "source_boundary_contract_state")
        == "candidate_ofx_runtime_boundary_contract_ready_no_runtime_route_closed"
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_verifier", "source_contract_state")
        == "candidate_ofx_runtime_boundary_contract_ready_runtime_closed"
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_verifier", "runtime_boundary_ready")
        is True
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_verifier", "fixture_approval_satisfied")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_verifier", "ofx_host_binary_review_ready")
        is False
        and artifact_state(
            artifacts, "candidate_ofx_runtime_approval_verifier", "runtime_containment_selftest_ready"
        )
        is False
        and artifact_state(
            artifacts, "candidate_ofx_runtime_approval_verifier", "schema_and_render_validation_ready"
        )
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_verifier", "path_acceptance_closed")
        is True
        and artifact_state(
            artifacts, "candidate_ofx_runtime_approval_verifier", "synthetic_runtime_approval_checks_passed"
        )
        is True
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_verifier", "ofx_runtime_invocation_ready")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_verifier", "host_process_launch_enabled")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_verifier", "path_acceptance_ready")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_verifier", "accepted_aex_path") is None
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_verifier", "real_route_open") is False
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_verifier", "mock_route_ready") is True
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_verifier", "ofx_runtime_invoked") is False
        and artifact_state(artifacts, "candidate_ofx_runtime_approval_verifier", "ppm_pixel_read_performed")
        is False
        and artifact_state(
            artifacts, "candidate_ofx_runtime_approval_verifier", "runtime_approval_path_payload_exported"
        )
        is False
        and artifact_state(
            artifacts, "candidate_ofx_runtime_approval_verifier", "runtime_approval_verifier_path_payload_exported"
        )
        is False
    )
    candidate_ofx_runtime_prerequisite_audit_ready = (
        artifact_state(artifacts, "candidate_ofx_runtime_prerequisite_audit", "runtime_prerequisite_audit_state")
        == "candidate_ofx_runtime_prerequisite_audit_ready_gates_closed"
        and artifact_state(
            artifacts, "candidate_ofx_runtime_prerequisite_audit", "runtime_prerequisite_audit_ready"
        )
        is True
        and artifact_state(
            artifacts, "candidate_ofx_runtime_prerequisite_audit", "runtime_prerequisites_complete"
        )
        is False
        and artifact_state(
            artifacts, "candidate_ofx_runtime_prerequisite_audit", "runtime_invocation_prerequisites_ready"
        )
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_prerequisite_audit", "approval_can_be_issued_now")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_prerequisite_audit", "failed_evidence_count") == 0
        and artifact_state(
            artifacts, "candidate_ofx_runtime_prerequisite_audit", "blocking_prerequisite_count"
        )
        >= 1
        and artifact_state(
            artifacts, "candidate_ofx_runtime_prerequisite_audit", "runtime_prerequisite_count"
        )
        == 8
        and artifact_state(
            artifacts, "candidate_ofx_runtime_prerequisite_audit", "runtime_prerequisite_satisfied_count"
        )
        == 4
        and artifact_state(
            artifacts, "candidate_ofx_runtime_prerequisite_audit", "runtime_prerequisite_blocker_count"
        )
        >= 1
        and artifact_state(
            artifacts, "candidate_ofx_runtime_prerequisite_audit", "runtime_approval_verified_not_approved"
        )
        is True
        and artifact_state(
            artifacts, "candidate_ofx_runtime_prerequisite_audit", "current_runtime_approval_valid"
        )
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_prerequisite_audit", "runtime_approval_satisfied")
        is False
        and artifact_state(
            artifacts, "candidate_ofx_runtime_prerequisite_audit", "runtime_approval_gate_stays_closed"
        )
        is True
        and artifact_state(
            artifacts, "candidate_ofx_runtime_prerequisite_audit", "boundary_contract_cross_checked"
        )
        is True
        and artifact_state(
            artifacts, "candidate_ofx_runtime_prerequisite_audit", "boundary_contract_matches_request"
        )
        is True
        and artifact_state(
            artifacts, "candidate_ofx_runtime_prerequisite_audit", "explicit_runtime_approval_present"
        )
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_prerequisite_audit", "fixture_approval_satisfied")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_prerequisite_audit", "ofx_host_binary_review_ready")
        is False
        and artifact_state(
            artifacts, "candidate_ofx_runtime_prerequisite_audit", "runtime_containment_contract_ready"
        )
        is True
        and artifact_state(
            artifacts,
            "candidate_ofx_runtime_prerequisite_audit",
            "runtime_containment_selftest_synthetic_passed",
        )
        is True
        and artifact_state(
            artifacts, "candidate_ofx_runtime_prerequisite_audit", "runtime_containment_selftest_ready"
        )
        is False
        and artifact_state(
            artifacts, "candidate_ofx_runtime_prerequisite_audit", "parameter_schema_review_policy_ready"
        )
        is True
        and artifact_state(artifacts, "candidate_ofx_runtime_prerequisite_audit", "payload_parser_enabled")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_prerequisite_audit", "redacted_schema_available")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_prerequisite_audit", "ofx_describe_mapping_ready")
        is False
        and artifact_state(
            artifacts, "candidate_ofx_runtime_prerequisite_audit", "schema_and_render_validation_ready"
        )
        is False
        and artifact_state(
            artifacts, "candidate_ofx_runtime_prerequisite_audit", "render_validation_contract_ready"
        )
        is True
        and artifact_state(artifacts, "candidate_ofx_runtime_prerequisite_audit", "no_load_validation_ready")
        is True
        and artifact_state(artifacts, "candidate_ofx_runtime_prerequisite_audit", "real_render_open")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_prerequisite_audit", "ofx_route_contract_closed")
        is True
        and artifact_state(artifacts, "candidate_ofx_runtime_prerequisite_audit", "real_route_open") is False
        and artifact_state(artifacts, "candidate_ofx_runtime_prerequisite_audit", "mock_route_ready") is True
        and artifact_state(artifacts, "candidate_ofx_runtime_prerequisite_audit", "path_acceptance_closed")
        is True
        and artifact_state(
            artifacts, "candidate_ofx_runtime_prerequisite_audit", "ofx_runtime_invocation_ready"
        )
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_prerequisite_audit", "host_process_launch_enabled")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_prerequisite_audit", "path_acceptance_ready")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_prerequisite_audit", "accepted_aex_path")
        is None
        and artifact_state(artifacts, "candidate_ofx_runtime_prerequisite_audit", "ofx_runtime_invoked")
        is False
        and artifact_state(artifacts, "candidate_ofx_runtime_prerequisite_audit", "ppm_pixel_read_performed")
        is False
        and artifact_state(
            artifacts,
            "candidate_ofx_runtime_prerequisite_audit",
            "runtime_prerequisite_audit_path_payload_exported",
        )
        is False
    )
    candidate_ofx_host_binary_review_request_ready = (
        artifact_state(
            artifacts,
            "candidate_ofx_host_binary_review_request",
            "ofx_host_binary_review_request_state",
        )
        == "candidate_ofx_host_binary_review_request_ready_pending_manual_review"
        and artifact_state(
            artifacts,
            "candidate_ofx_host_binary_review_request",
            "host_binary_review_request_state",
        )
        == "candidate_ofx_host_binary_review_request_ready_pending_manual_review"
        and artifact_state(
            artifacts, "candidate_ofx_host_binary_review_request", "ofx_host_binary_review_request_ready"
        )
        is True
        and artifact_state(
            artifacts, "candidate_ofx_host_binary_review_request", "host_binary_review_request_ready"
        )
        is True
        and artifact_state(artifacts, "candidate_ofx_host_binary_review_request", "ofx_host_binary_review_ready")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_binary_review_request", "host_binary_review_ready")
        is False
        and artifact_state(
            artifacts, "candidate_ofx_host_binary_review_request", "ofx_host_binary_review_satisfied"
        )
        is False
        and artifact_state(
            artifacts, "candidate_ofx_host_binary_review_request", "host_binary_review_satisfied"
        )
        is False
        and artifact_state(
            artifacts,
            "candidate_ofx_host_binary_review_request",
            "ofx_host_binary_review_can_be_approved_now",
        )
        is False
        and artifact_state(
            artifacts, "candidate_ofx_host_binary_review_request", "host_binary_review_can_be_approved_now"
        )
        is False
        and artifact_state(
            artifacts,
            "candidate_ofx_host_binary_review_request",
            "ofx_host_binary_review_manifest_created",
        )
        is False
        and artifact_state(
            artifacts, "candidate_ofx_host_binary_review_request", "host_binary_review_manifest_created"
        )
        is False
        and artifact_state(
            artifacts,
            "candidate_ofx_host_binary_review_request",
            "ofx_host_binary_review_gate_stays_closed",
        )
        is True
        and artifact_state(
            artifacts, "candidate_ofx_host_binary_review_request", "host_binary_review_gate_stays_closed"
        )
        is True
        and artifact_state(
            artifacts, "candidate_ofx_host_binary_review_request", "requires_explicit_host_binary_review"
        )
        is True
        and artifact_state(
            artifacts, "candidate_ofx_host_binary_review_request", "host_binary_review_request_created"
        )
        is True
        and artifact_state(
            artifacts, "candidate_ofx_host_binary_review_request", "source_prerequisite_audit_state"
        )
        == "candidate_ofx_runtime_prerequisite_audit_ready_gates_closed"
        and artifact_state(artifacts, "candidate_ofx_host_binary_review_request", "source_failed_evidence_count")
        == 0
        and artifact_state(
            artifacts,
            "candidate_ofx_host_binary_review_request",
            "source_runtime_invocation_prerequisites_ready",
        )
        is False
        and artifact_state(
            artifacts, "candidate_ofx_host_binary_review_request", "source_ofx_host_binary_review_ready"
        )
        is False
        and artifact_state(
            artifacts, "candidate_ofx_host_binary_review_request", "source_harness_dryrun_ready"
        )
        is True
        and artifact_state(
            artifacts, "candidate_ofx_host_binary_review_request", "source_host_harness_selftest_ready"
        )
        is True
        and artifact_state(
            artifacts, "candidate_ofx_host_binary_review_request", "source_boundary_contract_ready"
        )
        is True
        and artifact_state(
            artifacts,
            "candidate_ofx_host_binary_review_request",
            "source_boundary_host_process_launch_enabled",
        )
        is False
        and artifact_state(
            artifacts,
            "candidate_ofx_host_binary_review_request",
            "source_boundary_ofx_host_path_payload_supplied",
        )
        is False
        and artifact_state(
            artifacts,
            "candidate_ofx_host_binary_review_request",
            "source_boundary_ofx_plugin_binary_path_payload_supplied",
        )
        is False
        and artifact_state(artifacts, "candidate_ofx_host_binary_review_request", "review_checklist_count")
        == 8
        and artifact_state(artifacts, "candidate_ofx_host_binary_review_request", "review_blocker_count")
        >= 1
        and artifact_state(
            artifacts, "candidate_ofx_host_binary_review_request", "host_binary_review_blocker_count"
        )
        >= 1
        and artifact_state(artifacts, "candidate_ofx_host_binary_review_request", "manual_review_required")
        is True
        and artifact_state(artifacts, "candidate_ofx_host_binary_review_request", "explicit_user_review_required")
        is True
        and artifact_state(
            artifacts, "candidate_ofx_host_binary_review_request", "approval_only_prepares_host_binary_review"
        )
        is True
        and artifact_state(artifacts, "candidate_ofx_host_binary_review_request", "review_does_not_accept_paths")
        is True
        and artifact_state(artifacts, "candidate_ofx_host_binary_review_request", "review_does_not_launch_host")
        is True
        and artifact_state(artifacts, "candidate_ofx_host_binary_review_request", "review_does_not_invoke_runtime")
        is True
        and artifact_state(
            artifacts, "candidate_ofx_host_binary_review_request", "host_binary_path_acceptance_ready"
        )
        is False
        and artifact_state(
            artifacts, "candidate_ofx_host_binary_review_request", "host_binary_path_payload_exported"
        )
        is False
        and artifact_state(
            artifacts, "candidate_ofx_host_binary_review_request", "accepted_ofx_host_path"
        )
        is None
        and artifact_state(
            artifacts, "candidate_ofx_host_binary_review_request", "accepted_ofx_plugin_binary_path"
        )
        is None
        and artifact_state(artifacts, "candidate_ofx_host_binary_review_request", "ofx_runtime_allowed_now")
        is False
        and artifact_state(
            artifacts, "candidate_ofx_host_binary_review_request", "ofx_runtime_invocation_ready"
        )
        is False
        and artifact_state(artifacts, "candidate_ofx_host_binary_review_request", "host_process_launch_enabled")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_binary_review_request", "path_acceptance_ready")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_binary_review_request", "real_route_open")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_binary_review_request", "mock_route_ready")
        is True
        and artifact_state(artifacts, "candidate_ofx_host_binary_review_request", "ofx_runtime_invoked")
        is False
        and artifact_state(artifacts, "candidate_ofx_host_binary_review_request", "ppm_pixel_read_performed")
        is False
        and artifact_state(
            artifacts,
            "candidate_ofx_host_binary_review_request",
            "ofx_host_binary_review_path_payload_exported",
        )
        is False
        and artifact_state(
            artifacts, "candidate_ofx_host_binary_review_request", "host_binary_review_path_payload_exported"
        )
        is False
    )
    candidate_dependency_scope_ready = (
        artifact_state(artifacts, "candidate_dependency_scope", "candidate_dependency_scope_state")
        == "candidate_dependency_scope_ready_no_load"
        and artifact_state(artifacts, "candidate_dependency_scope", "candidate_scope_ready") is True
        and artifact_state(artifacts, "candidate_dependency_scope", "candidate_dependency_blockers_present")
        is False
        and artifact_state(artifacts, "candidate_dependency_scope", "candidate_dependency_blocker_count") == 0
        and artifact_state(artifacts, "candidate_dependency_scope", "candidate_dependency_missing_or_api_set_review_count")
        == 0
        and artifact_state(artifacts, "candidate_dependency_scope", "candidate_dependency_found_paths_exported")
        is False
        and artifact_state(artifacts, "candidate_dependency_scope", "global_dependency_blockers_present") is True
        and artifact_state(artifacts, "candidate_dependency_scope", "global_dependency_blockers_apply_to_candidate")
        is False
    )
    candidate_load_gate_dryrun_ready = (
        artifact_state(artifacts, "candidate_load_gate_dryrun", "candidate_load_gate_dryrun_state")
        == "candidate_load_gate_dryrun_ready_no_load"
        and artifact_state(
            artifacts,
            "candidate_load_gate_dryrun",
            "candidate_scoped_load_gate_dry_run_state",
        )
        == "closed_dry_run_candidate_deps_clear_fixture_approval_missing_global_deps_blocked"
        and artifact_state(artifacts, "candidate_load_gate_dryrun", "candidate_gate_ready_for_separate_loader_design")
        is False
        and artifact_state(artifacts, "candidate_load_gate_dryrun", "native_load_gate") == "closed"
        and artifact_state(artifacts, "candidate_load_gate_dryrun", "fixture_approval_satisfied") is False
        and artifact_state(artifacts, "candidate_load_gate_dryrun", "candidate_scope_ready") is True
        and artifact_state(artifacts, "candidate_load_gate_dryrun", "candidate_dependencies_clear") is True
        and artifact_state(artifacts, "candidate_load_gate_dryrun", "candidate_dependency_blockers_present")
        is False
        and artifact_state(artifacts, "candidate_load_gate_dryrun", "candidate_dependency_blocker_count") == 0
        and artifact_state(
            artifacts,
            "candidate_load_gate_dryrun",
            "candidate_dependency_missing_or_api_set_review_count",
        )
        == 0
        and artifact_state(artifacts, "candidate_load_gate_dryrun", "candidate_dependency_found_paths_exported")
        is False
        and artifact_state(artifacts, "candidate_load_gate_dryrun", "global_dependency_blockers_present") is True
        and artifact_state(
            artifacts,
            "candidate_load_gate_dryrun",
            "global_dependency_blockers_apply_to_candidate",
        )
        is False
        and artifact_state(artifacts, "candidate_load_gate_dryrun", "source_load_gate_state")
        == "closed_dependency_review_or_invalid_approval"
    )
    worker_ready = labels_found(artifacts, ("worker_design", "worker_selftest")) and artifact_state(
        artifacts, "worker_selftest", "worker_selftest_passed"
    ) is True
    gate_closed = artifact_state(artifacts, "load_gate", "gate_state") in {
        "closed_missing_or_invalid_approval",
        "closed_dependency_review_or_invalid_approval",
    }
    loader_refused = artifact_state(artifacts, "native_loader_stub", "stub_state") == "refused_gate_closed"
    native_loader_design_ready = (
        artifact_state(artifacts, "native_loader_design_contract", "native_loader_design_state")
        == "native_loader_design_ready_loader_closed"
        and artifact_state(artifacts, "native_loader_design_contract", "contract_state")
        == "native_loader_design_contract_ready_loader_closed_pending_fixture_approval"
        and artifact_state(artifacts, "native_loader_design_contract", "loader_design_ready") is True
        and artifact_state(artifacts, "native_loader_design_contract", "native_load_gate") == "closed"
        and artifact_state(artifacts, "native_loader_design_contract", "approval_required_before_aex_path") is True
        and artifact_state(artifacts, "native_loader_design_contract", "runtime_approval_required_before_load")
        is True
        and artifact_state(artifacts, "native_loader_design_contract", "separate_process_required") is True
        and artifact_state(artifacts, "native_loader_design_contract", "accepts_aex_path") is False
        and artifact_state(artifacts, "native_loader_design_contract", "accepted_aex_path") is None
        and artifact_state(artifacts, "native_loader_design_contract", "controller_loads_aex") is False
        and artifact_state(artifacts, "native_loader_design_contract", "candidate_dependencies_clear") is True
        and artifact_state(artifacts, "native_loader_design_contract", "fixture_approval_satisfied") is False
        and artifact_state(artifacts, "native_loader_design_contract", "source_stub_state") == "refused_gate_closed"
        and artifact_state(artifacts, "native_loader_design_contract", "source_sandbox_policy_state")
        == "policy_ready_no_native_load"
        and artifact_state(artifacts, "native_loader_design_contract", "source_render_contract_state")
        == "render_validation_contract_ready_render_closed"
        and artifact_state(artifacts, "native_loader_design_contract", "source_ofx_route_contract_state")
        == "ofx_route_contract_ready_route_closed"
    )
    native_loader_broker_selftest_ready = (
        artifact_state(artifacts, "native_loader_broker_selftest", "broker_selftest_state")
        == "pathless_native_loader_broker_selftest_passed"
        and artifact_state(artifacts, "native_loader_broker_selftest", "pathless_broker_ready") is True
        and artifact_state(artifacts, "native_loader_broker_selftest", "native_loader_design_ready") is True
        and artifact_state(artifacts, "native_loader_broker_selftest", "accepts_aex_path") is False
        and artifact_state(artifacts, "native_loader_broker_selftest", "accepted_aex_path") is None
        and artifact_state(artifacts, "native_loader_broker_selftest", "path_payload_supplied") is False
        and artifact_state(artifacts, "native_loader_broker_selftest", "blocked_action_count") == 6
        and artifact_state(artifacts, "native_loader_broker_selftest", "candidate_dependencies_clear") is True
        and artifact_state(artifacts, "native_loader_broker_selftest", "fixture_approval_satisfied") is False
    )
    native_loader_runtime_contract_ready = (
        artifact_state(artifacts, "native_loader_runtime_contract", "native_loader_runtime_contract_state")
        == "runtime_containment_contract_ready_no_load"
        and artifact_state(artifacts, "native_loader_runtime_contract", "contract_state")
        == "runtime_containment_contract_ready_path_acceptance_closed"
        and artifact_state(artifacts, "native_loader_runtime_contract", "runtime_containment_ready") is True
        and artifact_state(artifacts, "native_loader_runtime_contract", "path_allowlist_state")
        == "closed_no_aex_paths_accepted"
        and artifact_state(artifacts, "native_loader_runtime_contract", "path_acceptance_ready") is False
        and artifact_state(artifacts, "native_loader_runtime_contract", "aex_path_acceptance_enabled") is False
        and artifact_state(artifacts, "native_loader_runtime_contract", "approval_required_before_aex_path")
        is True
        and artifact_state(artifacts, "native_loader_runtime_contract", "runtime_approval_required_before_load")
        is True
        and artifact_state(artifacts, "native_loader_runtime_contract", "native_load_gate") == "closed"
        and artifact_state(artifacts, "native_loader_runtime_contract", "accepted_aex_path") is None
        and artifact_state(artifacts, "native_loader_runtime_contract", "path_payload_supplied") is False
        and artifact_state(artifacts, "native_loader_runtime_contract", "broker_selftest_passed") is True
        and artifact_state(artifacts, "native_loader_runtime_contract", "pathless_broker_ready") is True
        and artifact_state(artifacts, "native_loader_runtime_contract", "native_loader_design_ready") is True
        and artifact_state(artifacts, "native_loader_runtime_contract", "process_isolation_required") is True
        and artifact_state(artifacts, "native_loader_runtime_contract", "controller_loads_aex") is False
        and artifact_state(artifacts, "native_loader_runtime_contract", "candidate_dependencies_clear") is True
        and artifact_state(artifacts, "native_loader_runtime_contract", "fixture_approval_satisfied") is False
        and artifact_state(artifacts, "native_loader_runtime_contract", "source_native_loader_design_state")
        == "native_loader_design_ready_loader_closed"
        and artifact_state(artifacts, "native_loader_runtime_contract", "source_broker_selftest_state")
        == "pathless_native_loader_broker_selftest_passed"
        and artifact_state(artifacts, "native_loader_runtime_contract", "source_sandbox_policy_state")
        == "policy_ready_no_native_load"
        and artifact_state(artifacts, "native_loader_runtime_contract", "source_candidate_load_gate_state")
        == "candidate_load_gate_dryrun_ready_no_load"
        and artifact_state(
            artifacts,
            "native_loader_runtime_contract",
            "source_candidate_scoped_load_gate_state",
        )
        == "closed_dry_run_candidate_deps_clear_fixture_approval_missing_global_deps_blocked"
    )
    native_loader_runtime_selftest_ready = (
        artifact_state(artifacts, "native_loader_runtime_selftest", "runtime_selftest_state")
        == "runtime_containment_selftest_passed_no_load"
        and artifact_state(artifacts, "native_loader_runtime_selftest", "runtime_containment_selftest_passed")
        is True
        and artifact_state(artifacts, "native_loader_runtime_selftest", "runtime_containment_ready") is True
        and artifact_state(artifacts, "native_loader_runtime_selftest", "path_allowlist_state")
        == "closed_no_aex_paths_accepted"
        and artifact_state(artifacts, "native_loader_runtime_selftest", "path_acceptance_ready") is False
        and artifact_state(artifacts, "native_loader_runtime_selftest", "aex_path_acceptance_enabled") is False
        and artifact_state(artifacts, "native_loader_runtime_selftest", "accepted_aex_path") is None
        and artifact_state(artifacts, "native_loader_runtime_selftest", "path_payload_supplied") is False
        and artifact_state(artifacts, "native_loader_runtime_selftest", "source_native_loader_runtime_contract_state")
        == "runtime_containment_contract_ready_no_load"
        and artifact_state(artifacts, "native_loader_runtime_selftest", "synthetic_subprocess_only") is True
        and artifact_state(artifacts, "native_loader_runtime_selftest", "normal_exit_case_passed") is True
        and artifact_state(artifacts, "native_loader_runtime_selftest", "stderr_capture_passed") is True
        and artifact_state(artifacts, "native_loader_runtime_selftest", "timeout_case_passed") is True
        and artifact_state(artifacts, "native_loader_runtime_selftest", "child_cleanup_passed") is True
    )
    native_loader_path_policy_selftest_ready = (
        artifact_state(artifacts, "native_loader_path_policy_selftest", "path_policy_selftest_state")
        == "closed_path_policy_selftest_passed_no_aex_path"
        and artifact_state(artifacts, "native_loader_path_policy_selftest", "path_policy_selftest_passed")
        is True
        and artifact_state(artifacts, "native_loader_path_policy_selftest", "source_runtime_selftest_state")
        == "runtime_containment_selftest_passed_no_load"
        and artifact_state(artifacts, "native_loader_path_policy_selftest", "source_runtime_selftest_passed")
        is True
        and artifact_state(artifacts, "native_loader_path_policy_selftest", "path_allowlist_state")
        == "closed_no_aex_paths_accepted"
        and artifact_state(artifacts, "native_loader_path_policy_selftest", "path_acceptance_ready") is False
        and artifact_state(artifacts, "native_loader_path_policy_selftest", "aex_path_acceptance_enabled")
        is False
        and artifact_state(artifacts, "native_loader_path_policy_selftest", "accepted_aex_path") is None
        and artifact_state(artifacts, "native_loader_path_policy_selftest", "path_payload_supplied") is False
        and artifact_state(artifacts, "native_loader_path_policy_selftest", "synthetic_path_inputs_only") is True
        and artifact_state(artifacts, "native_loader_path_policy_selftest", "candidate_path_string_accepted")
        is False
        and artifact_state(artifacts, "native_loader_path_policy_selftest", "absolute_path_rejected") is True
        and artifact_state(artifacts, "native_loader_path_policy_selftest", "traversal_rejected") is True
        and artifact_state(artifacts, "native_loader_path_policy_selftest", "non_aex_suffix_rejected") is True
        and artifact_state(artifacts, "native_loader_path_policy_selftest", "redaction_passed") is True
        and artifact_state(artifacts, "native_loader_path_policy_selftest", "raw_input_paths_serialized")
        is False
        and artifact_state(artifacts, "native_loader_path_policy_selftest", "path_case_count") == 4
    )
    ofx_deferred = artifact_state(artifacts, "ofx_facade", "facade_state") == "deferred_loader_not_ready"
    ofx_mock_ready = artifact_state(artifacts, "ofx_noop_mock", "mock_state") == "mock_identity_completed_route_closed"
    ofx_suite_ready = (
        artifact_state(artifacts, "ofx_suite_selftest", "ofx_suite_selftest_state")
        == "ofx_suite_noop_identity_passed_route_closed"
    )
    ofx_contract_ready = (
        artifact_state(artifacts, "ofx_route_contract", "contract_state")
        == "ofx_route_contract_ready_route_closed"
        and artifact_state(artifacts, "ofx_route_contract", "real_route_open") is False
        and artifact_state(artifacts, "ofx_route_contract", "mock_route_ready") is True
    )
    publication_local_only = artifact_state(artifacts, "publication_boundary", "publishable_now") is False

    return {
        "static_ready": static_ready,
        "pipl_catalog_ready": pipl_catalog_ready,
        "parameter_schema_plan_ready": parameter_schema_plan_ready,
        "parameter_schema_review_ready": parameter_schema_review_ready,
        "redacted_schema_verifier_ready": redacted_schema_verifier_ready,
        "synthetic_pipl_parser_selftest_ready": synthetic_pipl_parser_selftest_ready,
        "pipl_parser_gate_ready": pipl_parser_gate_ready,
        "synthetic_pipl_payload_parser_ready": synthetic_pipl_payload_parser_ready,
        "pipl_resource_consistency_audit_ready": pipl_resource_consistency_audit_ready,
        "pipl_payload_adapter_review_ready": pipl_payload_adapter_review_ready,
        "aepx_probe_ready": aepx_probe_ready,
        "aepx_edit_plan_ready": aepx_edit_plan_ready,
        "aepx_roundtrip_ready": aepx_roundtrip_ready,
        "aepx_redacted_text_inventory_ready": aepx_redacted_text_inventory_ready,
        "aepx_redacted_text_classifier_ready": aepx_redacted_text_classifier_ready,
        "matrix_ready": matrix_ready,
        "dependency_ready": dependency_ready,
        "dependency_preflight_ready": dependency_preflight_ready,
        "dependency_review_ready": dependency_review_ready,
        "sandbox_policy_ready": sandbox_policy_ready,
        "image_suite_ready": image_suite_ready,
        "image_validation_ready": image_validation_ready,
        "image_suite_selftest_ready": image_suite_selftest_ready,
        "image_input_smoke_ready": image_input_smoke_ready,
        "render_contract_ready": render_contract_ready,
        "fixture_ready": fixture_ready,
        "fixture_manual_review_ready": fixture_manual_review_ready,
        "fixture_provenance_review_ready": fixture_provenance_review_ready,
        "fixture_provenance_answer_template_ready": fixture_provenance_answer_template_ready,
        "fixture_provenance_answer_validator_selftest_ready": fixture_provenance_answer_validator_selftest_ready,
        "fixture_approval_verifier_ready": fixture_approval_verifier_ready,
        "fixture_approval_request_ready": fixture_approval_request_ready,
        "candidate_test_handoff_ready": candidate_test_handoff_ready,
        "candidate_test_runner_dryrun_ready": candidate_test_runner_dryrun_ready,
        "candidate_test_runner_ready": candidate_test_runner_ready,
        "candidate_compatibility_card_ready": candidate_compatibility_card_ready,
        "candidate_image_compat_mock_check": candidate_image_compat_mock_check,
        "candidate_image_compat_mock_ready": candidate_image_compat_mock_ready,
        "candidate_ofx_bridge_ready": candidate_ofx_bridge_ready,
        "candidate_ofx_host_harness_dryrun_ready": candidate_ofx_host_harness_dryrun_ready,
        "candidate_ofx_host_harness_selftest_ready": candidate_ofx_host_harness_selftest_ready,
        "candidate_ofx_runtime_boundary_contract_ready": candidate_ofx_runtime_boundary_contract_ready,
        "candidate_ofx_runtime_approval_request_ready": candidate_ofx_runtime_approval_request_ready,
        "candidate_ofx_runtime_approval_verifier_ready": candidate_ofx_runtime_approval_verifier_ready,
        "candidate_ofx_runtime_prerequisite_audit_ready": candidate_ofx_runtime_prerequisite_audit_ready,
        "candidate_ofx_host_binary_review_request_ready": candidate_ofx_host_binary_review_request_ready,
        "candidate_dependency_scope_ready": candidate_dependency_scope_ready,
        "candidate_load_gate_dryrun_ready": candidate_load_gate_dryrun_ready,
        "worker_ready": worker_ready,
        "gate_closed": gate_closed,
        "loader_refused": loader_refused,
        "native_loader_design_ready": native_loader_design_ready,
        "native_loader_broker_selftest_ready": native_loader_broker_selftest_ready,
        "native_loader_runtime_contract_ready": native_loader_runtime_contract_ready,
        "native_loader_runtime_selftest_ready": native_loader_runtime_selftest_ready,
        "native_loader_path_policy_selftest_ready": native_loader_path_policy_selftest_ready,
        "ofx_deferred": ofx_deferred,
        "ofx_mock_ready": ofx_mock_ready,
        "ofx_suite_ready": ofx_suite_ready,
        "ofx_contract_ready": ofx_contract_ready,
        "publication_local_only": publication_local_only,
    }

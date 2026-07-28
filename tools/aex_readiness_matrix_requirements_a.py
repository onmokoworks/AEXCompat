"""Readiness requirement construction group extracted from the CLI facade."""

from __future__ import annotations

from typing import Any

try:
    from aex_readiness_matrix_core import artifact_state, requirement
except ModuleNotFoundError:
    from tools.aex_readiness_matrix_core import artifact_state, requirement


def build_requirement_group_a(state: dict[str, Any]) -> list[dict[str, Any]]:
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
            "workspace_tooling_lab",
            "Tooling lab and canonical artifact chain",
            "satisfied" if clean_evidence else "failed",
            ["artifact_index"],
            {"index_state": "canonical_chain_indexed" if clean_evidence else "invalid_or_incomplete"},
            "The lab has a canonical local JSON chain when evidence validation is clean.",
        ),
        requirement(
            "ae_project_static_edit_surface",
            "AEPX static project edit surface",
            "satisfied_deferred"
            if clean_evidence
            and aepx_probe_ready
            and aepx_edit_plan_ready
            and aepx_roundtrip_ready
            and aepx_redacted_text_inventory_ready
            and aepx_redacted_text_classifier_ready
            else "failed",
            [
                "aepx_static_probe",
                "aepx_edit_plan",
                "aepx_roundtrip_validator",
                "aepx_redacted_text_inventory",
                "aepx_redacted_text_classifier",
            ],
            {
                "probe_state": artifact_state(artifacts, "aepx_static_probe", "probe_state"),
                "xml_parse_state": artifact_state(artifacts, "aepx_static_probe", "xml_parse_state"),
                "edit_readiness_state": artifact_state(artifacts, "aepx_static_probe", "edit_readiness_state"),
                "edit_plan_state": artifact_state(artifacts, "aepx_edit_plan", "edit_plan_state"),
                "write_recommendation": artifact_state(artifacts, "aepx_edit_plan", "write_recommendation"),
                "roundtrip_state": artifact_state(artifacts, "aepx_roundtrip_validator", "roundtrip_state"),
                "validator_ready": artifact_state(artifacts, "aepx_roundtrip_validator", "validator_ready"),
                "source_structure_match": artifact_state(
                    artifacts, "aepx_roundtrip_validator", "source_structure_match"
                ),
                "roundtrip_structure_match": artifact_state(
                    artifacts, "aepx_roundtrip_validator", "roundtrip_structure_match"
                ),
                "roundtrip_xml_serialized_to_disk": artifact_state(
                    artifacts, "aepx_roundtrip_validator", "roundtrip_xml_serialized_to_disk"
                ),
                "inventory_state": artifact_state(artifacts, "aepx_redacted_text_inventory", "inventory_state"),
                "inventory_ready": artifact_state(artifacts, "aepx_redacted_text_inventory", "inventory_ready"),
                "raw_text_fields_present": artifact_state(
                    artifacts, "aepx_redacted_text_inventory", "raw_text_fields_present"
                ),
                "value_hashes_emitted": artifact_state(
                    artifacts, "aepx_redacted_text_inventory", "value_hashes_emitted"
                ),
                "classifier_state": artifact_state(
                    artifacts, "aepx_redacted_text_classifier", "classifier_state"
                ),
                "classifier_ready": artifact_state(
                    artifacts, "aepx_redacted_text_classifier", "classifier_ready"
                ),
                "project_write_ready": artifact_state(
                    artifacts, "aepx_redacted_text_classifier", "project_write_ready"
                ),
                "classifier_approves_project_write": artifact_state(
                    artifacts, "aepx_redacted_text_classifier", "classifier_approves_project_write"
                ),
                "approved_write_candidate_count": artifact_state(
                    artifacts, "aepx_redacted_text_classifier", "approved_write_candidate_count"
                ),
            },
            "AEPX XML is parseable, has a no-write edit plan, passes in-memory structure round-trip validation, has a redacted text-node inventory, and classifies every text row without approving writes.",
            "Review classified no-write buckets before any schema-aware AEPX write proposal.",
        ),
        requirement(
            "aepx_roundtrip_validator",
            "AEPX no-write round-trip validator",
            "satisfied_deferred" if clean_evidence and aepx_roundtrip_ready else "failed",
            ["aepx_static_probe", "aepx_edit_plan", "aepx_roundtrip_validator"],
            {
                "probe_state": artifact_state(artifacts, "aepx_static_probe", "probe_state"),
                "edit_plan_state": artifact_state(artifacts, "aepx_edit_plan", "edit_plan_state"),
                "roundtrip_state": artifact_state(artifacts, "aepx_roundtrip_validator", "roundtrip_state"),
                "validator_ready": artifact_state(artifacts, "aepx_roundtrip_validator", "validator_ready"),
                "source_structure_match": artifact_state(
                    artifacts, "aepx_roundtrip_validator", "source_structure_match"
                ),
                "roundtrip_structure_match": artifact_state(
                    artifacts, "aepx_roundtrip_validator", "roundtrip_structure_match"
                ),
                "text_payload_exported": artifact_state(
                    artifacts, "aepx_roundtrip_validator", "text_payload_exported"
                ),
                "bdata_payload_exported": artifact_state(
                    artifacts, "aepx_roundtrip_validator", "bdata_payload_exported"
                ),
            },
            "The project XML can round-trip structurally in memory without writing project files or exporting payload values.",
            "Keep project writes blocked until redacted edit surfaces and AE host validation policy are reviewed.",
        ),
        requirement(
            "aepx_redacted_text_inventory",
            "AEPX redacted text-node inventory",
            "satisfied_deferred" if clean_evidence and aepx_redacted_text_inventory_ready else "failed",
            ["aepx_roundtrip_validator", "aepx_redacted_text_inventory"],
            {
                "inventory_state": artifact_state(artifacts, "aepx_redacted_text_inventory", "inventory_state"),
                "inventory_ready": artifact_state(artifacts, "aepx_redacted_text_inventory", "inventory_ready"),
                "source_roundtrip_state": artifact_state(
                    artifacts, "aepx_redacted_text_inventory", "source_roundtrip_state"
                ),
                "source_validator_ready": artifact_state(
                    artifacts, "aepx_redacted_text_inventory", "source_validator_ready"
                ),
                "text_payload_exported": artifact_state(
                    artifacts, "aepx_redacted_text_inventory", "text_payload_exported"
                ),
                "text_payload_hash_exported": artifact_state(
                    artifacts, "aepx_redacted_text_inventory", "text_payload_hash_exported"
                ),
                "bdata_payload_exported": artifact_state(
                    artifacts, "aepx_redacted_text_inventory", "bdata_payload_exported"
                ),
                "raw_text_fields_present": artifact_state(
                    artifacts, "aepx_redacted_text_inventory", "raw_text_fields_present"
                ),
                "value_hashes_emitted": artifact_state(
                    artifacts, "aepx_redacted_text_inventory", "value_hashes_emitted"
                ),
                "absolute_source_paths_in_inventory_rows": artifact_state(
                    artifacts, "aepx_redacted_text_inventory", "absolute_source_paths_in_inventory_rows"
                ),
                "raw_payload_serialized": artifact_state(
                    artifacts, "aepx_redacted_text_inventory", "raw_payload_serialized"
                ),
            },
            "AEPX text-node edit surfaces are represented as redacted metadata only: no text values, hashes, bdata values, or attribute values.",
            "Review and classify inventory rows before proposing schema-aware AEPX edits.",
        ),
        requirement(
            "aepx_redacted_text_classifier",
            "AEPX redacted text classifier",
            "satisfied_deferred" if clean_evidence and aepx_redacted_text_classifier_ready else "failed",
            ["aepx_redacted_text_inventory", "aepx_redacted_text_classifier"],
            {
                "classifier_state": artifact_state(
                    artifacts, "aepx_redacted_text_classifier", "classifier_state"
                ),
                "classifier_ready": artifact_state(
                    artifacts, "aepx_redacted_text_classifier", "classifier_ready"
                ),
                "source_chain_valid": artifact_state(
                    artifacts, "aepx_redacted_text_classifier", "source_chain_valid"
                ),
                "inventory_rows_classified": artifact_state(
                    artifacts, "aepx_redacted_text_classifier", "inventory_rows_classified"
                ),
                "row_count_matches_inventory_summary": artifact_state(
                    artifacts, "aepx_redacted_text_classifier", "row_count_matches_inventory_summary"
                ),
                "project_write_recommendation": artifact_state(
                    artifacts, "aepx_redacted_text_classifier", "project_write_recommendation"
                ),
                "project_write_ready": artifact_state(
                    artifacts, "aepx_redacted_text_classifier", "project_write_ready"
                ),
                "project_write_allowed_now": artifact_state(
                    artifacts, "aepx_redacted_text_classifier", "project_write_allowed_now"
                ),
                "classifier_approves_project_write": artifact_state(
                    artifacts, "aepx_redacted_text_classifier", "classifier_approves_project_write"
                ),
                "approved_write_candidate_count": artifact_state(
                    artifacts, "aepx_redacted_text_classifier", "approved_write_candidate_count"
                ),
                "classification_row_count": artifact_state(
                    artifacts, "aepx_redacted_text_classifier", "classification_row_count"
                ),
                "no_write_row_count": artifact_state(
                    artifacts, "aepx_redacted_text_classifier", "no_write_row_count"
                ),
                "unknown_row_count": artifact_state(
                    artifacts, "aepx_redacted_text_classifier", "unknown_row_count"
                ),
                "absolute_source_paths_in_classifier_rows": artifact_state(
                    artifacts, "aepx_redacted_text_classifier", "absolute_source_paths_in_classifier_rows"
                ),
                "raw_payload_serialized": artifact_state(
                    artifacts, "aepx_redacted_text_classifier", "raw_payload_serialized"
                ),
            },
            "AEPX text inventory rows are classified from metadata-only JSON evidence, with every row kept no-write and no payload values or hashes emitted.",
            "Use classified buckets only as review evidence; do not generate project-write schemas until separate schema and host-validation gates exist.",
        ),
        requirement(
            "aex_static_inventory",
            "AEX static PE/PiPL/resource inventory",
            "satisfied"
            if clean_evidence
            and static_ready
            and pipl_catalog_ready
            and parameter_schema_plan_ready
            and parameter_schema_review_ready
            and redacted_schema_verifier_ready
            and synthetic_pipl_parser_selftest_ready
            and pipl_parser_gate_ready
            and synthetic_pipl_payload_parser_ready
            and pipl_resource_consistency_audit_ready
            and pipl_payload_adapter_review_ready
            and matrix_ready
            and dependency_ready
            and dependency_preflight_ready
            and dependency_review_ready
            and sandbox_policy_ready
            and image_suite_ready
            and image_validation_ready
            and image_suite_selftest_ready
            and image_input_smoke_ready
            and render_contract_ready
            else "failed",
            [
                "static_report",
                "pipl_resource_catalog",
                "parameter_schema_plan",
                "parameter_schema_review",
                "redacted_schema_verifier",
                "synthetic_pipl_parser_selftest",
                "pipl_parser_gate",
                "synthetic_pipl_payload_parser",
                "pipl_resource_consistency_audit",
                "pipl_payload_adapter_review",
                "candidate_matrix",
                "dependency_matrix",
                "dependency_preflight",
                "dependency_review",
                "sandbox_policy",
                "image_fixture_suite",
                "image_fixture_validation",
                "image_suite_selftest",
                "image_input_smoke",
                "render_validation_contract",
            ],
            {
                "found": static_ready,
                "pipl_catalog_state": artifact_state(artifacts, "pipl_resource_catalog", "catalog_state"),
                "pipl_payload_policy": artifact_state(artifacts, "pipl_resource_catalog", "payload_policy"),
                "parameter_schema_plan_state": artifact_state(artifacts, "parameter_schema_plan", "plan_state"),
                "real_parameter_schema_available": artifact_state(
                    artifacts, "parameter_schema_plan", "real_parameter_schema_available"
                ),
                "payload_parser_enabled": artifact_state(artifacts, "parameter_schema_plan", "payload_parser_enabled"),
                "parameter_schema_review_state": artifact_state(
                    artifacts, "parameter_schema_review", "review_state"
                ),
                "redaction_policy_state": artifact_state(
                    artifacts, "parameter_schema_review", "redaction_policy_state"
                ),
                "ofx_describe_mapping_ready": artifact_state(
                    artifacts, "parameter_schema_review", "ofx_describe_mapping_ready"
                ),
                "redacted_schema_verifier_state": artifact_state(
                    artifacts, "redacted_schema_verifier", "verifier_state"
                ),
                "redacted_schema_verifier_ready": artifact_state(
                    artifacts, "redacted_schema_verifier", "verifier_ready"
                ),
                "real_redacted_schema_available": artifact_state(
                    artifacts, "redacted_schema_verifier", "real_redacted_schema_available"
                ),
                "synthetic_pipl_parser_selftest_state": artifact_state(
                    artifacts, "synthetic_pipl_parser_selftest", "selftest_state"
                ),
                "synthetic_parser_ready": artifact_state(
                    artifacts, "synthetic_pipl_parser_selftest", "synthetic_parser_ready"
                ),
                "real_pipl_payload_parser_enabled": artifact_state(
                    artifacts, "synthetic_pipl_parser_selftest", "real_pipl_payload_parser_enabled"
                ),
                "pipl_parser_gate_state": artifact_state(artifacts, "pipl_parser_gate", "gate_state"),
                "pipl_parser_gate_ready_for_review": artifact_state(
                    artifacts, "pipl_parser_gate", "gate_ready_for_review"
                ),
                "resource_payload_opened": artifact_state(artifacts, "pipl_parser_gate", "resource_payload_opened"),
                "synthetic_payload_parser_state": artifact_state(
                    artifacts, "synthetic_pipl_payload_parser", "synthetic_payload_parser_state"
                ),
                "synthetic_payload_parser_ready": artifact_state(
                    artifacts, "synthetic_pipl_payload_parser", "synthetic_payload_parser_ready"
                ),
                "real_payload_input_allowed_now": artifact_state(
                    artifacts, "synthetic_pipl_payload_parser", "real_payload_input_allowed_now"
                ),
                "pipl_resource_consistency_audit_state": artifact_state(
                    artifacts, "pipl_resource_consistency_audit", "audit_state"
                ),
                "metadata_consistency_ready": artifact_state(
                    artifacts, "pipl_resource_consistency_audit", "metadata_consistency_ready"
                ),
                "pipl_payload_adapter_review_state": artifact_state(
                    artifacts, "pipl_payload_adapter_review", "adapter_review_state"
                ),
                "real_payload_adapter_allowed_now": artifact_state(
                    artifacts, "pipl_payload_adapter_review", "real_payload_adapter_allowed_now"
                ),
                "parameter_schema_emission_allowed_now": artifact_state(
                    artifacts, "pipl_payload_adapter_review", "parameter_schema_emission_allowed_now"
                ),
                "matrix_state": artifact_state(artifacts, "candidate_matrix", "matrix_state"),
                "dependency_matrix_state": artifact_state(artifacts, "dependency_matrix", "dependency_matrix_state"),
                "dependency_preflight_state": artifact_state(artifacts, "dependency_preflight", "preflight_state"),
                "dependency_review_state": artifact_state(artifacts, "dependency_review", "review_state"),
                "native_load_recommendation": artifact_state(
                    artifacts, "dependency_review", "native_load_recommendation"
                ),
                "sandbox_policy_state": artifact_state(artifacts, "sandbox_policy", "sandbox_policy_state"),
                "image_fixture_suite_state": artifact_state(artifacts, "image_fixture_suite", "suite_state"),
                "image_fixture_validation_state": artifact_state(
                    artifacts, "image_fixture_validation", "validation_state"
                ),
                "image_suite_selftest_state": artifact_state(
                    artifacts, "image_suite_selftest", "suite_selftest_state"
                ),
                "image_input_smoke_state": artifact_state(artifacts, "image_input_smoke", "smoke_state"),
                "render_validation_contract_state": artifact_state(
                    artifacts, "render_validation_contract", "contract_state"
                ),
            },
            "Static inventory, PiPL/resource catalog, no-payload parameter schema plan, PiPL adapter review packet, candidate, dependency, dependency preflight/review, sandbox policy, validated image fixture evidence, a single-image smoke tool, and a closed render contract exist without loading plug-ins.",
        ),
        requirement(
            "pipl_parser_gate",
            "PiPL parser metadata budget gate",
            "satisfied_deferred" if clean_evidence and pipl_parser_gate_ready else "failed",
            ["pipl_resource_catalog", "synthetic_pipl_parser_selftest", "pipl_parser_gate"],
            {
                "pipl_catalog_state": artifact_state(artifacts, "pipl_resource_catalog", "catalog_state"),
                "synthetic_selftest_state": artifact_state(
                    artifacts, "synthetic_pipl_parser_selftest", "selftest_state"
                ),
                "gate_state": artifact_state(artifacts, "pipl_parser_gate", "gate_state"),
                "gate_ready_for_review": artifact_state(artifacts, "pipl_parser_gate", "gate_ready_for_review"),
                "metadata_budget_ready": artifact_state(artifacts, "pipl_parser_gate", "metadata_budget_ready"),
                "real_pipl_payload_parser_enabled": artifact_state(
                    artifacts, "pipl_parser_gate", "real_pipl_payload_parser_enabled"
                ),
                "real_pipl_payload_parsed": artifact_state(
                    artifacts, "pipl_parser_gate", "real_pipl_payload_parsed"
                ),
                "resource_payload_opened": artifact_state(artifacts, "pipl_parser_gate", "resource_payload_opened"),
                "raw_payload_serialized": artifact_state(artifacts, "pipl_parser_gate", "raw_payload_serialized"),
            },
            "PiPL parser input budgets and future parser candidate rows are derived from metadata only while the real parser gate stays closed.",
            "Use this gate before any reviewed real PiPL payload parser accepts bytes.",
        ),
        requirement(
            "pipl_resource_consistency_audit",
            "PiPL static/catalog/gate consistency audit",
            "satisfied_deferred" if clean_evidence and pipl_resource_consistency_audit_ready else "failed",
            ["static_report", "pipl_resource_catalog", "pipl_parser_gate", "pipl_resource_consistency_audit"],
            {
                "audit_state": artifact_state(artifacts, "pipl_resource_consistency_audit", "audit_state"),
                "audit_passed": artifact_state(artifacts, "pipl_resource_consistency_audit", "audit_passed"),
                "metadata_consistency_ready": artifact_state(
                    artifacts, "pipl_resource_consistency_audit", "metadata_consistency_ready"
                ),
                "source_chain_valid": artifact_state(
                    artifacts, "pipl_resource_consistency_audit", "source_chain_valid"
                ),
                "catalog_summary_recomputed": artifact_state(
                    artifacts, "pipl_resource_consistency_audit", "catalog_summary_recomputed"
                ),
                "catalog_rows_recomputed": artifact_state(
                    artifacts, "pipl_resource_consistency_audit", "catalog_rows_recomputed"
                ),
                "gate_budget_rows_recomputed": artifact_state(
                    artifacts, "pipl_resource_consistency_audit", "gate_budget_rows_recomputed"
                ),
                "gate_summary_recomputed": artifact_state(
                    artifacts, "pipl_resource_consistency_audit", "gate_summary_recomputed"
                ),
                "real_payload_input_allowed_now": artifact_state(
                    artifacts, "pipl_resource_consistency_audit", "real_payload_input_allowed_now"
                ),
                "real_pipl_payload_parser_enabled": artifact_state(
                    artifacts, "pipl_resource_consistency_audit", "real_pipl_payload_parser_enabled"
                ),
                "real_pipl_payload_parsed": artifact_state(
                    artifacts, "pipl_resource_consistency_audit", "real_pipl_payload_parsed"
                ),
                "resource_payload_opened": artifact_state(
                    artifacts, "pipl_resource_consistency_audit", "resource_payload_opened"
                ),
                "resource_payload_extracted": artifact_state(
                    artifacts, "pipl_resource_consistency_audit", "resource_payload_extracted"
                ),
                "raw_payload_serialized": artifact_state(
                    artifacts, "pipl_resource_consistency_audit", "raw_payload_serialized"
                ),
            },
            "The static report, PiPL catalog, and parser gate are cross-checked by recomputing catalog rows/summaries and gate budget/action rows from JSON metadata only.",
            "Use this audit before reviewing any real-payload adapter so source-chain drift is caught first.",
        ),
        requirement(
            "synthetic_pipl_payload_parser",
            "Synthetic PiPL payload parser implementation",
            "satisfied_deferred" if clean_evidence and synthetic_pipl_payload_parser_ready else "failed",
            ["synthetic_pipl_parser_selftest", "pipl_parser_gate", "synthetic_pipl_payload_parser"],
            {
                "synthetic_payload_parser_state": artifact_state(
                    artifacts, "synthetic_pipl_payload_parser", "synthetic_payload_parser_state"
                ),
                "synthetic_payload_parser_ready": artifact_state(
                    artifacts, "synthetic_pipl_payload_parser", "synthetic_payload_parser_ready"
                ),
                "synthetic_parser_implemented": artifact_state(
                    artifacts, "synthetic_pipl_payload_parser", "synthetic_parser_implemented"
                ),
                "synthetic_bounds_harness_reused": artifact_state(
                    artifacts, "synthetic_pipl_payload_parser", "synthetic_bounds_harness_reused"
                ),
                "synthetic_payload_cases_passed": artifact_state(
                    artifacts, "synthetic_pipl_payload_parser", "synthetic_payload_cases_passed"
                ),
                "real_payload_input_allowed_now": artifact_state(
                    artifacts, "synthetic_pipl_payload_parser", "real_payload_input_allowed_now"
                ),
                "output_metadata_only": artifact_state(
                    artifacts, "synthetic_pipl_payload_parser", "output_metadata_only"
                ),
                "real_pipl_payload_parser_enabled": artifact_state(
                    artifacts, "synthetic_pipl_payload_parser", "real_pipl_payload_parser_enabled"
                ),
                "real_pipl_payload_parsed": artifact_state(
                    artifacts, "synthetic_pipl_payload_parser", "real_pipl_payload_parsed"
                ),
                "resource_payload_opened": artifact_state(
                    artifacts, "synthetic_pipl_payload_parser", "resource_payload_opened"
                ),
                "raw_payload_serialized": artifact_state(
                    artifacts, "synthetic_pipl_payload_parser", "raw_payload_serialized"
                ),
                "parser_case_count": artifact_state(
                    artifacts, "synthetic_pipl_payload_parser", "parser_case_count"
                ),
                "parser_case_failed_count": artifact_state(
                    artifacts, "synthetic_pipl_payload_parser", "parser_case_failed_count"
                ),
            },
            "A reusable synthetic parser implementation now exercises metadata-only TLV parsing, bounds rejection, unknown-tag counting, and no-raw-output checks before any real PiPL bytes are accepted.",
            "Review a real-payload adapter separately and keep it closed until fixture approval and parser review exist.",
        ),
        requirement(
            "pipl_payload_adapter_review",
            "Real PiPL payload adapter review packet",
            "satisfied_deferred" if clean_evidence and pipl_payload_adapter_review_ready else "failed",
            [
                "pipl_parser_gate",
                "synthetic_pipl_payload_parser",
                "pipl_resource_consistency_audit",
                "parameter_schema_review",
                "pipl_payload_adapter_review",
            ],
            {
                "adapter_review_state": artifact_state(
                    artifacts, "pipl_payload_adapter_review", "adapter_review_state"
                ),
                "adapter_review_ready": artifact_state(
                    artifacts, "pipl_payload_adapter_review", "adapter_review_ready"
                ),
                "source_chain_valid": artifact_state(
                    artifacts, "pipl_payload_adapter_review", "source_chain_valid"
                ),
                "synthetic_parser_contract_reviewed": artifact_state(
                    artifacts, "pipl_payload_adapter_review", "synthetic_parser_contract_reviewed"
                ),
                "metadata_consistency_reviewed": artifact_state(
                    artifacts, "pipl_payload_adapter_review", "metadata_consistency_reviewed"
                ),
                "metadata_budget_reviewed": artifact_state(
                    artifacts, "pipl_payload_adapter_review", "metadata_budget_reviewed"
                ),
                "parameter_schema_reviewed": artifact_state(
                    artifacts, "pipl_payload_adapter_review", "parameter_schema_reviewed"
                ),
                "redaction_policy_reviewed": artifact_state(
                    artifacts, "pipl_payload_adapter_review", "redaction_policy_reviewed"
                ),
                "ofx_describe_policy_reviewed": artifact_state(
                    artifacts, "pipl_payload_adapter_review", "ofx_describe_policy_reviewed"
                ),
                "real_payload_adapter_allowed_now": artifact_state(
                    artifacts, "pipl_payload_adapter_review", "real_payload_adapter_allowed_now"
                ),
                "real_payload_input_allowed_now": artifact_state(
                    artifacts, "pipl_payload_adapter_review", "real_payload_input_allowed_now"
                ),
                "real_pipl_payload_parser_enabled": artifact_state(
                    artifacts, "pipl_payload_adapter_review", "real_pipl_payload_parser_enabled"
                ),
                "resource_payload_opened": artifact_state(
                    artifacts, "pipl_payload_adapter_review", "resource_payload_opened"
                ),
                "resource_payload_extracted": artifact_state(
                    artifacts, "pipl_payload_adapter_review", "resource_payload_extracted"
                ),
                "raw_payload_serialized": artifact_state(
                    artifacts, "pipl_payload_adapter_review", "raw_payload_serialized"
                ),
                "output_metadata_only": artifact_state(
                    artifacts, "pipl_payload_adapter_review", "output_metadata_only"
                ),
                "parameter_schema_emission_allowed_now": artifact_state(
                    artifacts, "pipl_payload_adapter_review", "parameter_schema_emission_allowed_now"
                ),
                "review_item_count": artifact_state(
                    artifacts, "pipl_payload_adapter_review", "review_item_count"
                ),
                "blocking_review_item_count": artifact_state(
                    artifacts, "pipl_payload_adapter_review", "blocking_review_item_count"
                ),
            },
            "A real PiPL payload adapter review packet exists, sourced from closed gate/synthetic/audit/schema-review evidence, while real payload access and schema emission remain disabled.",
            "Only start real-payload adapter implementation after explicit payload-access approval, fixture scope, worker containment, and redacted-schema gates are approved.",
        ),
        requirement(
            "synthetic_pipl_parser_selftest",
            "Synthetic PiPL parser bounds selftest",
            "satisfied_deferred" if clean_evidence and synthetic_pipl_parser_selftest_ready else "failed",
            ["redacted_schema_verifier", "synthetic_pipl_parser_selftest"],
            {
                "verifier_state": artifact_state(artifacts, "redacted_schema_verifier", "verifier_state"),
                "selftest_state": artifact_state(artifacts, "synthetic_pipl_parser_selftest", "selftest_state"),
                "synthetic_parser_ready": artifact_state(
                    artifacts, "synthetic_pipl_parser_selftest", "synthetic_parser_ready"
                ),
                "synthetic_payloads_used": artifact_state(
                    artifacts, "synthetic_pipl_parser_selftest", "synthetic_payloads_used"
                ),
                "real_pipl_payload_parser_enabled": artifact_state(
                    artifacts, "synthetic_pipl_parser_selftest", "real_pipl_payload_parser_enabled"
                ),
                "real_pipl_payload_parsed": artifact_state(
                    artifacts, "synthetic_pipl_parser_selftest", "real_pipl_payload_parsed"
                ),
                "raw_payload_serialized": artifact_state(
                    artifacts, "synthetic_pipl_parser_selftest", "raw_payload_serialized"
                ),
            },
            "Synthetic parser bounds and no-raw-payload reporting checks pass without touching real PiPL payloads.",
            "Use this contract as the harness before reviewing any real PiPL payload parser implementation.",
        ),
        requirement(
            "redacted_schema_verifier",
            "Redacted schema verifier",
            "satisfied_deferred" if clean_evidence and redacted_schema_verifier_ready else "failed",
            ["parameter_schema_review", "redacted_schema_verifier"],
            {
                "review_state": artifact_state(artifacts, "parameter_schema_review", "review_state"),
                "verifier_state": artifact_state(artifacts, "redacted_schema_verifier", "verifier_state"),
                "verifier_ready": artifact_state(artifacts, "redacted_schema_verifier", "verifier_ready"),
                "real_redacted_schema_available": artifact_state(
                    artifacts, "redacted_schema_verifier", "real_redacted_schema_available"
                ),
                "payload_parser_enabled": artifact_state(
                    artifacts, "redacted_schema_verifier", "payload_parser_enabled"
                ),
                "synthetic_schema_fixture_used": artifact_state(
                    artifacts, "redacted_schema_verifier", "synthetic_schema_fixture_used"
                ),
            },
            "A verifier for future redacted schema output exists, using synthetic in-memory schema checks only.",
            "Feed reviewed parser output through this verifier only after explicit schema emission approval.",
        ),
        requirement(
            "parameter_schema_review_packet",
            "AEX parameter schema parser/redaction review packet",
            "satisfied_deferred" if clean_evidence and parameter_schema_review_ready else "failed",
            ["parameter_schema_plan", "publication_boundary", "ofx_route_contract", "parameter_schema_review"],
            {
                "parameter_schema_plan_state": artifact_state(artifacts, "parameter_schema_plan", "plan_state"),
                "review_state": artifact_state(artifacts, "parameter_schema_review", "review_state"),
                "parser_design_state": artifact_state(
                    artifacts, "parameter_schema_review", "parser_design_state"
                ),
                "redaction_policy_state": artifact_state(
                    artifacts, "parameter_schema_review", "redaction_policy_state"
                ),
                "ofx_describe_policy_state": artifact_state(
                    artifacts, "parameter_schema_review", "ofx_describe_policy_state"
                ),
                "payload_parser_enabled": artifact_state(
                    artifacts, "parameter_schema_review", "payload_parser_enabled"
                ),
                "redacted_schema_available": artifact_state(
                    artifacts, "parameter_schema_review", "redacted_schema_available"
                ),
                "ofx_describe_mapping_ready": artifact_state(
                    artifacts, "parameter_schema_review", "ofx_describe_mapping_ready"
                ),
            },
            "Parser design, redaction policy, and OFX describe deferral are now explicit, but payload parsing and schema output remain closed.",
            "Implement a reviewed parser and pass its proposed redacted output through the verifier before schema emission or OFX describe mapping.",
        ),
        requirement(
            "parameter_schema_plan",
            "AEX parameter schema mapping plan",
            "satisfied_deferred" if clean_evidence and parameter_schema_plan_ready else "failed",
            ["pipl_resource_catalog", "candidate_matrix", "render_validation_contract", "parameter_schema_plan"],
            {
                "pipl_catalog_state": artifact_state(artifacts, "pipl_resource_catalog", "catalog_state"),
                "candidate_matrix_state": artifact_state(artifacts, "candidate_matrix", "matrix_state"),
                "render_contract_state": artifact_state(artifacts, "render_validation_contract", "contract_state"),
                "parameter_schema_plan_state": artifact_state(artifacts, "parameter_schema_plan", "plan_state"),
                "schema_plan_ready": artifact_state(artifacts, "parameter_schema_plan", "schema_plan_ready"),
                "real_parameter_schema_available": artifact_state(
                    artifacts, "parameter_schema_plan", "real_parameter_schema_available"
                ),
                "payload_parser_enabled": artifact_state(artifacts, "parameter_schema_plan", "payload_parser_enabled"),
            },
            "A no-payload schema mapping plan exists, but actual PiPL payload parsing and parameter schema output remain closed.",
            "Create a reviewed payload parser and redaction policy before emitting parameter names, defaults, ranges, or OFX describe data.",
        ),
        requirement(
            "fixture_candidate_review",
            "Fixture candidate selection and hold decision",
            "satisfied_local_only"
            if clean_evidence and fixture_ready and fixture_manual_review_ready and candidate_dependency_scope_ready
            else "failed",
            [
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
            ],
            {
                "matrix_state": artifact_state(artifacts, "candidate_matrix", "matrix_state"),
                "dependency_matrix_state": artifact_state(artifacts, "dependency_matrix", "dependency_matrix_state"),
                "dependency_preflight_state": artifact_state(artifacts, "dependency_preflight", "preflight_state"),
                "dependency_review_state": artifact_state(artifacts, "dependency_review", "review_state"),
                "sandbox_policy_state": artifact_state(artifacts, "sandbox_policy", "sandbox_policy_state"),
                "decision_state": artifact_state(artifacts, "fixture_decision", "decision_state"),
                "approval_state": artifact_state(artifacts, "fixture_decision", "approval_state"),
                "dossier_state": artifact_state(artifacts, "fixture_dossier", "dossier_state"),
                "manual_review_packet_state": artifact_state(
                    artifacts, "fixture_manual_review_packet", "review_packet_state"
                ),
                "manual_review_ready": artifact_state(
                    artifacts, "fixture_manual_review_packet", "manual_review_ready"
                ),
                "approval_ready": artifact_state(artifacts, "fixture_manual_review_packet", "approval_ready"),
                "candidate_dependency_scope_state": artifact_state(
                    artifacts, "candidate_dependency_scope", "candidate_dependency_scope_state"
                ),
                "candidate_dependency_blockers_present": artifact_state(
                    artifacts, "candidate_dependency_scope", "candidate_dependency_blockers_present"
                ),
                "candidate_dependency_blocker_count": artifact_state(
                    artifacts, "candidate_dependency_scope", "candidate_dependency_blocker_count"
                ),
                "candidate_dependency_missing_or_api_set_review_count": artifact_state(
                    artifacts,
                    "candidate_dependency_scope",
                    "candidate_dependency_missing_or_api_set_review_count",
                ),
            },
            "A first fixture candidate has static review evidence and a no-load manual-review packet, but its decision remains local review/hold.",
            "Complete manual provenance, license, dependency, and safety review before any approval artifact.",
        ),
        requirement(
            "fixture_manual_review_packet",
            "Fixture manual-review decision packet",
            "satisfied_deferred" if clean_evidence and fixture_manual_review_ready else "failed",
            ["fixture_dossier", "dependency_review", "load_gate", "fixture_manual_review_packet"],
            {
                "dossier_state": artifact_state(artifacts, "fixture_dossier", "dossier_state"),
                "dependency_review_state": artifact_state(artifacts, "dependency_review", "review_state"),
                "load_gate_state": artifact_state(artifacts, "load_gate", "gate_state"),
                "review_packet_state": artifact_state(
                    artifacts, "fixture_manual_review_packet", "review_packet_state"
                ),
                "manual_review_ready": artifact_state(
                    artifacts, "fixture_manual_review_packet", "manual_review_ready"
                ),
                "approval_ready": artifact_state(artifacts, "fixture_manual_review_packet", "approval_ready"),
                "approval_blocker_count": artifact_state(
                    artifacts, "fixture_manual_review_packet", "approval_blocker_count"
                ),
                "recommended_next_decision": artifact_state(
                    artifacts, "fixture_manual_review_packet", "recommended_next_decision"
                ),
            },
            "The pre-approval decision surface is consolidated from fixture dossier, dependency review, load gate, and optional WizTree inventory metadata.",
            "Keep the decision on hold until manual review and dependency blockers are resolved.",
        ),
        requirement(
            "fixture_provenance_review",
            "Fixture provenance, license, and safety review aid",
            "satisfied_deferred" if clean_evidence and fixture_provenance_review_ready else "failed",
            ["fixture_manual_review_packet", "fixture_approval_request", "fixture_provenance_review"],
            {
                "provenance_review_state": artifact_state(
                    artifacts, "fixture_provenance_review", "provenance_review_state"
                ),
                "provenance_review_ready": artifact_state(
                    artifacts, "fixture_provenance_review", "provenance_review_ready"
                ),
                "manual_review_source_ready": artifact_state(
                    artifacts, "fixture_provenance_review", "manual_review_source_ready"
                ),
                "approval_request_source_ready": artifact_state(
                    artifacts, "fixture_provenance_review", "approval_request_source_ready"
                ),
                "provenance_status": artifact_state(artifacts, "fixture_provenance_review", "provenance_status"),
                "license_status": artifact_state(artifacts, "fixture_provenance_review", "license_status"),
                "local_fixture_safety_status": artifact_state(
                    artifacts, "fixture_provenance_review", "local_fixture_safety_status"
                ),
                "approval_can_be_issued_now": artifact_state(
                    artifacts, "fixture_provenance_review", "approval_can_be_issued_now"
                ),
                "approval_manifest_created": artifact_state(
                    artifacts, "fixture_provenance_review", "approval_manifest_created"
                ),
                "current_fixture_approval_valid": artifact_state(
                    artifacts, "fixture_provenance_review", "current_fixture_approval_valid"
                ),
                "fixture_approval_satisfied": artifact_state(
                    artifacts, "fixture_provenance_review", "fixture_approval_satisfied"
                ),
                "approval_gate_stays_closed": artifact_state(
                    artifacts, "fixture_provenance_review", "approval_gate_stays_closed"
                ),
                "native_load_gate": artifact_state(artifacts, "fixture_provenance_review", "native_load_gate"),
                "native_load_gate_stays_closed": artifact_state(
                    artifacts, "fixture_provenance_review", "native_load_gate_stays_closed"
                ),
                "accepted_aex_path": artifact_state(
                    artifacts, "fixture_provenance_review", "accepted_aex_path"
                ),
                "raw_input_paths_serialized": artifact_state(
                    artifacts, "fixture_provenance_review", "raw_input_paths_serialized"
                ),
                "aex_file_hashed": artifact_state(artifacts, "fixture_provenance_review", "aex_file_hashed"),
                "aex_file_copied": artifact_state(artifacts, "fixture_provenance_review", "aex_file_copied"),
                "review_question_count": artifact_state(
                    artifacts, "fixture_provenance_review", "review_question_count"
                ),
                "unanswered_review_question_count": artifact_state(
                    artifacts, "fixture_provenance_review", "unanswered_review_question_count"
                ),
            },
            "A no-load review aid now records the provenance/license/safety questions that must be answered before any fixture approval can be considered.",
            "Record user-confirmed provenance/license/safety answers in a future hold/reject/approval decision; this packet is not approval.",
        ),
        requirement(
            "fixture_provenance_answer_template",
            "Fixture provenance answer template",
            "satisfied_deferred" if clean_evidence and fixture_provenance_answer_template_ready else "failed",
            ["fixture_provenance_review", "fixture_provenance_answer_template"],
            {
                "template_state": artifact_state(
                    artifacts, "fixture_provenance_answer_template", "template_state"
                ),
                "template_ready": artifact_state(
                    artifacts, "fixture_provenance_answer_template", "template_ready"
                ),
                "answer_template_only": artifact_state(
                    artifacts, "fixture_provenance_answer_template", "answer_template_only"
                ),
                "source_provenance_review_ready": artifact_state(
                    artifacts, "fixture_provenance_answer_template", "source_provenance_review_ready"
                ),
                "provenance_status": artifact_state(
                    artifacts, "fixture_provenance_answer_template", "provenance_status"
                ),
                "license_status": artifact_state(
                    artifacts, "fixture_provenance_answer_template", "license_status"
                ),
                "answers_present": artifact_state(
                    artifacts, "fixture_provenance_answer_template", "answers_present"
                ),
                "answered_question_count": artifact_state(
                    artifacts, "fixture_provenance_answer_template", "answered_question_count"
                ),
                "pending_answer_count": artifact_state(
                    artifacts, "fixture_provenance_answer_template", "pending_answer_count"
                ),
                "all_answers_pending": artifact_state(
                    artifacts, "fixture_provenance_answer_template", "all_answers_pending"
                ),
                "user_answer_artifact_required": artifact_state(
                    artifacts, "fixture_provenance_answer_template", "user_answer_artifact_required"
                ),
                "answer_template_approves_fixture": artifact_state(
                    artifacts, "fixture_provenance_answer_template", "answer_template_approves_fixture"
                ),
                "answer_template_approves_native_load": artifact_state(
                    artifacts, "fixture_provenance_answer_template", "answer_template_approves_native_load"
                ),
                "approval_can_be_issued_now": artifact_state(
                    artifacts, "fixture_provenance_answer_template", "approval_can_be_issued_now"
                ),
                "fixture_approval_satisfied": artifact_state(
                    artifacts, "fixture_provenance_answer_template", "fixture_approval_satisfied"
                ),
                "native_load_gate": artifact_state(
                    artifacts, "fixture_provenance_answer_template", "native_load_gate"
                ),
                "accepted_aex_path": artifact_state(
                    artifacts, "fixture_provenance_answer_template", "accepted_aex_path"
                ),
                "aex_file_hashed": artifact_state(
                    artifacts, "fixture_provenance_answer_template", "aex_file_hashed"
                ),
                "aex_file_copied": artifact_state(
                    artifacts, "fixture_provenance_answer_template", "aex_file_copied"
                ),
            },
            "A local-only answer scaffold exists so the human provenance/license/safety review can be filled later without creating approval or native-load evidence.",
            "Use a separate validated user-answer artifact before changing fixture decision state.",
        ),
        requirement(
            "fixture_provenance_answer_validator_selftest",
            "Fixture provenance answer validator selftest",
            "satisfied_deferred"
            if clean_evidence and fixture_provenance_answer_validator_selftest_ready
            else "failed",
            ["fixture_provenance_answer_template", "fixture_provenance_answer_validator_selftest"],
            {
                "validator_selftest_state": artifact_state(
                    artifacts, "fixture_provenance_answer_validator_selftest", "validator_selftest_state"
                ),
                "validator_ready": artifact_state(
                    artifacts, "fixture_provenance_answer_validator_selftest", "validator_ready"
                ),
                "source_answer_template_ready": artifact_state(
                    artifacts, "fixture_provenance_answer_validator_selftest", "source_answer_template_ready"
                ),
                "real_user_answer_artifact_consumed": artifact_state(
                    artifacts,
                    "fixture_provenance_answer_validator_selftest",
                    "real_user_answer_artifact_consumed",
                ),
                "synthetic_user_answers_used": artifact_state(
                    artifacts, "fixture_provenance_answer_validator_selftest", "synthetic_user_answers_used"
                ),
                "synthetic_payloads_serialized": artifact_state(
                    artifacts, "fixture_provenance_answer_validator_selftest", "synthetic_payloads_serialized"
                ),
                "answer_schema_validated": artifact_state(
                    artifacts, "fixture_provenance_answer_validator_selftest", "answer_schema_validated"
                ),
                "synthetic_case_count": artifact_state(
                    artifacts, "fixture_provenance_answer_validator_selftest", "synthetic_case_count"
                ),
                "synthetic_case_passed_count": artifact_state(
                    artifacts, "fixture_provenance_answer_validator_selftest", "synthetic_case_passed_count"
                ),
                "synthetic_case_failed_count": artifact_state(
                    artifacts, "fixture_provenance_answer_validator_selftest", "synthetic_case_failed_count"
                ),
                "synthetic_valid_case_count": artifact_state(
                    artifacts, "fixture_provenance_answer_validator_selftest", "synthetic_valid_case_count"
                ),
                "synthetic_rejected_case_count": artifact_state(
                    artifacts, "fixture_provenance_answer_validator_selftest", "synthetic_rejected_case_count"
                ),
                "answers_present": artifact_state(
                    artifacts, "fixture_provenance_answer_validator_selftest", "answers_present"
                ),
                "answers_validated_for_manual_review": artifact_state(
                    artifacts,
                    "fixture_provenance_answer_validator_selftest",
                    "answers_validated_for_manual_review",
                ),
                "approval_can_be_issued_now": artifact_state(
                    artifacts, "fixture_provenance_answer_validator_selftest", "approval_can_be_issued_now"
                ),
                "fixture_approval_satisfied": artifact_state(
                    artifacts, "fixture_provenance_answer_validator_selftest", "fixture_approval_satisfied"
                ),
                "native_load_gate": artifact_state(
                    artifacts, "fixture_provenance_answer_validator_selftest", "native_load_gate"
                ),
                "accepted_aex_path": artifact_state(
                    artifacts, "fixture_provenance_answer_validator_selftest", "accepted_aex_path"
                ),
            },
            "The user-answer validation rules are now selftested with synthetic accept/reject cases, while no real answer artifact has been consumed.",
            "Create and validate a separate local-only user-answer artifact before any fixture decision changes.",
        ),
        requirement(
            "fixture_approval_verifier",
            "Fixture approval manifest verifier",
            "satisfied_deferred" if clean_evidence and fixture_approval_verifier_ready else "failed",
            [
                "fixture_decision",
                "fixture_manual_review_packet",
                "candidate_dependency_scope",
                "native_loader_path_policy_selftest",
                "candidate_load_gate_dryrun",
                "fixture_approval_verifier",
            ],
            {
                "approval_verifier_state": artifact_state(
                    artifacts, "fixture_approval_verifier", "approval_verifier_state"
                ),
                "approval_verifier_ready": artifact_state(
                    artifacts, "fixture_approval_verifier", "approval_verifier_ready"
                ),
                "current_fixture_approval_valid": artifact_state(
                    artifacts, "fixture_approval_verifier", "current_fixture_approval_valid"
                ),
                "fixture_approval_satisfied": artifact_state(
                    artifacts, "fixture_approval_verifier", "fixture_approval_satisfied"
                ),
                "approval_gate_stays_closed": artifact_state(
                    artifacts, "fixture_approval_verifier", "approval_gate_stays_closed"
                ),
                "manual_review_approval_ready": artifact_state(
                    artifacts, "fixture_approval_verifier", "manual_review_approval_ready"
                ),
                "candidate_dependencies_clear": artifact_state(
                    artifacts, "fixture_approval_verifier", "candidate_dependencies_clear"
                ),
                "path_policy_closed": artifact_state(
                    artifacts, "fixture_approval_verifier", "path_policy_closed"
                ),
                "candidate_load_gate_closed": artifact_state(
                    artifacts, "fixture_approval_verifier", "candidate_load_gate_closed"
                ),
                "synthetic_approval_checks_passed": artifact_state(
                    artifacts, "fixture_approval_verifier", "synthetic_approval_checks_passed"
                ),
                "required_approval_token_name": artifact_state(
                    artifacts, "fixture_approval_verifier", "required_approval_token_name"
                ),
                "approval_only_prepares_next_gate": artifact_state(
                    artifacts, "fixture_approval_verifier", "approval_only_prepares_next_gate"
                ),
            },
            "The verifier rejects the current hold decision as approval, checks synthetic invalid/valid approval shapes, and keeps the approval gate closed.",
            "Only a separate explicit user approval manifest can change the manual approval requirement.",
        ),
        requirement(
            "fixture_approval_request",
            "Fixture approval request packet",
            "satisfied_deferred" if clean_evidence and fixture_approval_request_ready else "failed",
            [
                "fixture_manual_review_packet",
                "fixture_approval_verifier",
                "native_loader_path_policy_selftest",
                "candidate_load_gate_dryrun",
                "fixture_approval_request",
            ],
            {
                "approval_request_state": artifact_state(
                    artifacts, "fixture_approval_request", "approval_request_state"
                ),
                "approval_request_ready": artifact_state(
                    artifacts, "fixture_approval_request", "approval_request_ready"
                ),
                "approval_can_be_issued_now": artifact_state(
                    artifacts, "fixture_approval_request", "approval_can_be_issued_now"
                ),
                "approval_manifest_created": artifact_state(
                    artifacts, "fixture_approval_request", "approval_manifest_created"
                ),
                "requires_explicit_user_approval": artifact_state(
                    artifacts, "fixture_approval_request", "requires_explicit_user_approval"
                ),
                "current_fixture_approval_valid": artifact_state(
                    artifacts, "fixture_approval_request", "current_fixture_approval_valid"
                ),
                "fixture_approval_satisfied": artifact_state(
                    artifacts, "fixture_approval_request", "fixture_approval_satisfied"
                ),
                "manual_review_approval_ready": artifact_state(
                    artifacts, "fixture_approval_request", "manual_review_approval_ready"
                ),
                "approval_gate_stays_closed": artifact_state(
                    artifacts, "fixture_approval_request", "approval_gate_stays_closed"
                ),
                "native_load_gate": artifact_state(artifacts, "fixture_approval_request", "native_load_gate"),
                "candidate_dependencies_clear": artifact_state(
                    artifacts, "fixture_approval_request", "candidate_dependencies_clear"
                ),
                "path_policy_closed": artifact_state(artifacts, "fixture_approval_request", "path_policy_closed"),
                "candidate_load_gate_closed": artifact_state(
                    artifacts, "fixture_approval_request", "candidate_load_gate_closed"
                ),
                "required_approval_token_name": artifact_state(
                    artifacts, "fixture_approval_request", "required_approval_token_name"
                ),
                "approval_only_prepares_next_gate": artifact_state(
                    artifacts, "fixture_approval_request", "approval_only_prepares_next_gate"
                ),
            },
            "The request packet summarizes blockers and the exact approval shape a human would need to review, without creating approval or opening any AEX path.",
            "Use this packet as the checklist before any explicit approval manifest is considered.",
        ),
        requirement(
            "candidate_test_handoff",
            "No-load candidate test handoff",
            "satisfied_deferred" if clean_evidence and candidate_test_handoff_ready else "failed",
            [
                "fixture_approval_request",
                "candidate_load_gate_dryrun",
                "native_loader_design_contract",
                "native_loader_runtime_contract",
                "native_loader_path_policy_selftest",
                "image_input_smoke",
                "render_validation_contract",
                "ofx_route_contract",
                "candidate_test_handoff",
            ],
            {
                "handoff_state": artifact_state(artifacts, "candidate_test_handoff", "handoff_state"),
                "handoff_packet_ready": artifact_state(
                    artifacts, "candidate_test_handoff", "handoff_packet_ready"
                ),
                "no_load_test_handoff_ready": artifact_state(
                    artifacts, "candidate_test_handoff", "no_load_test_handoff_ready"
                ),
                "native_test_handoff_ready": artifact_state(
                    artifacts, "candidate_test_handoff", "native_test_handoff_ready"
                ),
                "approval_request_ready": artifact_state(
                    artifacts, "candidate_test_handoff", "approval_request_ready"
                ),
                "approval_manifest_created": artifact_state(
                    artifacts, "candidate_test_handoff", "approval_manifest_created"
                ),
                "fixture_approval_satisfied": artifact_state(
                    artifacts, "candidate_test_handoff", "fixture_approval_satisfied"
                ),
                "native_load_gate": artifact_state(artifacts, "candidate_test_handoff", "native_load_gate"),
                "path_acceptance_ready": artifact_state(
                    artifacts, "candidate_test_handoff", "path_acceptance_ready"
                ),
                "aex_path_acceptance_enabled": artifact_state(
                    artifacts, "candidate_test_handoff", "aex_path_acceptance_enabled"
                ),
                "no_load_image_test_ready": artifact_state(
                    artifacts, "candidate_test_handoff", "no_load_image_test_ready"
                ),
                "image_fixture_validation_passed": artifact_state(
                    artifacts, "candidate_test_handoff", "image_fixture_validation_passed"
                ),
                "runtime_containment_selftest_passed": artifact_state(
                    artifacts, "candidate_test_handoff", "runtime_containment_selftest_passed"
                ),
                "synthetic_subprocess_only": artifact_state(
                    artifacts, "candidate_test_handoff", "synthetic_subprocess_only"
                ),
                "no_load_render_contract_ready": artifact_state(
                    artifacts, "candidate_test_handoff", "no_load_render_contract_ready"
                ),
                "no_load_ofx_mock_ready": artifact_state(
                    artifacts, "candidate_test_handoff", "no_load_ofx_mock_ready"
                ),
                "real_render_open": artifact_state(artifacts, "candidate_test_handoff", "real_render_open"),
                "real_route_open": artifact_state(artifacts, "candidate_test_handoff", "real_route_open"),
                "handoff_blocker_count": artifact_state(
                    artifacts, "candidate_test_handoff", "handoff_blocker_count"
                ),
            },
            "The selected candidate now has a no-load handoff packet for image/OFX mock planning, while approval, path acceptance, native load, real render, and real OFX routing remain closed.",
            "Use this packet as the input checklist for later test-runner or loader work; do not treat it as native-load approval.",
        ),
        requirement(
            "candidate_test_runner_dryrun",
            "Candidate no-load test runner dry-run manifest",
            "satisfied_deferred" if clean_evidence and candidate_test_runner_dryrun_ready else "failed",
            [
                "candidate_test_handoff",
                "image_fixture_suite",
                "image_fixture_validation",
                "image_suite_selftest",
                "ofx_suite_selftest",
                "image_input_smoke",
                "render_validation_contract",
                "ofx_route_contract",
                "candidate_test_runner_dryrun",
            ],
            {
                "runner_dryrun_state": artifact_state(
                    artifacts, "candidate_test_runner_dryrun", "runner_dryrun_state"
                ),
                "runner_dryrun_ready": artifact_state(
                    artifacts, "candidate_test_runner_dryrun", "runner_dryrun_ready"
                ),
                "dry_run_only": artifact_state(artifacts, "candidate_test_runner_dryrun", "dry_run_only"),
                "would_execute": artifact_state(artifacts, "candidate_test_runner_dryrun", "would_execute"),
                "execution_performed": artifact_state(
                    artifacts, "candidate_test_runner_dryrun", "execution_performed"
                ),
                "no_load_test_plan_ready": artifact_state(
                    artifacts, "candidate_test_runner_dryrun", "no_load_test_plan_ready"
                ),
                "native_test_plan_ready": artifact_state(
                    artifacts, "candidate_test_runner_dryrun", "native_test_plan_ready"
                ),
                "image_fixture_case_count": artifact_state(
                    artifacts, "candidate_test_runner_dryrun", "image_fixture_case_count"
                ),
                "planned_no_load_case_count": artifact_state(
                    artifacts, "candidate_test_runner_dryrun", "planned_no_load_case_count"
                ),
                "planned_native_case_count": artifact_state(
                    artifacts, "candidate_test_runner_dryrun", "planned_native_case_count"
                ),
                "blocked_case_count": artifact_state(
                    artifacts, "candidate_test_runner_dryrun", "blocked_case_count"
                ),
                "worker_suite_identity_passed": artifact_state(
                    artifacts, "candidate_test_runner_dryrun", "worker_suite_identity_passed"
                ),
                "ofx_suite_identity_passed": artifact_state(
                    artifacts, "candidate_test_runner_dryrun", "ofx_suite_identity_passed"
                ),
                "real_render_open": artifact_state(
                    artifacts, "candidate_test_runner_dryrun", "real_render_open"
                ),
                "real_route_open": artifact_state(artifacts, "candidate_test_runner_dryrun", "real_route_open"),
            },
            "A runner-shaped dry-run manifest now enumerates rerunnable no-load image/worker/OFX mock cases without executing them or admitting native/render/real OFX cases.",
            "Use this manifest to drive a later explicit no-load runner; keep native and real render routes closed.",
        ),
        requirement(
            "candidate_test_runner",
            "Candidate no-load test runner execution",
            "satisfied_deferred" if clean_evidence and candidate_test_runner_ready else "failed",
            [
                "candidate_test_runner_dryrun",
                "image_fixture_suite",
                "image_fixture_validation",
                "image_suite_selftest",
                "ofx_suite_selftest",
                "image_input_smoke",
                "render_validation_contract",
                "ofx_route_contract",
                "ofx_facade",
                "candidate_test_runner",
            ],
            {
                "runner_state": artifact_state(artifacts, "candidate_test_runner", "runner_state"),
                "runner_ready": artifact_state(artifacts, "candidate_test_runner", "runner_ready"),
                "dry_run_only": artifact_state(artifacts, "candidate_test_runner", "dry_run_only"),
                "execution_performed": artifact_state(
                    artifacts, "candidate_test_runner", "execution_performed"
                ),
                "no_load_execution_performed": artifact_state(
                    artifacts, "candidate_test_runner", "no_load_execution_performed"
                ),
                "native_execution_performed": artifact_state(
                    artifacts, "candidate_test_runner", "native_execution_performed"
                ),
                "worker_invoked": artifact_state(artifacts, "candidate_test_runner", "worker_invoked"),
                "ofx_mock_invoked": artifact_state(artifacts, "candidate_test_runner", "ofx_mock_invoked"),
                "ofx_runtime_invoked": artifact_state(
                    artifacts, "candidate_test_runner", "ofx_runtime_invoked"
                ),
                "worker_identity_passed": artifact_state(
                    artifacts, "candidate_test_runner", "worker_identity_passed"
                ),
                "ofx_noop_identity_passed": artifact_state(
                    artifacts, "candidate_test_runner", "ofx_noop_identity_passed"
                ),
                "blocked_load_aex_verified": artifact_state(
                    artifacts, "candidate_test_runner", "blocked_load_aex_verified"
                ),
                "executed_worker_case_count": artifact_state(
                    artifacts, "candidate_test_runner", "executed_worker_case_count"
                ),
                "executed_ofx_noop_case_count": artifact_state(
                    artifacts, "candidate_test_runner", "executed_ofx_noop_case_count"
                ),
                "executed_native_case_count": artifact_state(
                    artifacts, "candidate_test_runner", "executed_native_case_count"
                ),
                "real_render_open": artifact_state(artifacts, "candidate_test_runner", "real_render_open"),
                "real_route_open": artifact_state(artifacts, "candidate_test_runner", "real_route_open"),
            },
            "The no-load runner now re-executes worker identity and OFX no-op identity over approved generated image fixtures, and verifies the worker rejects load_aex.",
            "Keep using this runner only for no-load fixture/tooling checks until explicit fixture approval and path acceptance exist.",
        ),
        requirement(
            "candidate_compatibility_card",
            "Candidate compatibility card",
            "satisfied_deferred" if clean_evidence and candidate_compatibility_card_ready else "failed",
            [
                "candidate_matrix",
                "pipl_resource_catalog",
                "candidate_test_runner",
                "fixture_provenance_answer_validator_selftest",
                "candidate_compatibility_card",
            ],
            {
                "compatibility_card_state": artifact_state(
                    artifacts, "candidate_compatibility_card", "compatibility_card_state"
                ),
                "compatibility_card_ready": artifact_state(
                    artifacts, "candidate_compatibility_card", "compatibility_card_ready"
                ),
                "unsafe_exports_present": artifact_state(
                    artifacts, "candidate_compatibility_card", "unsafe_exports_present"
                ),
                "absolute_ppm_paths_exported": artifact_state(
                    artifacts, "candidate_compatibility_card", "absolute_ppm_paths_exported"
                ),
                "absolute_aex_paths_exported": artifact_state(
                    artifacts, "candidate_compatibility_card", "absolute_aex_paths_exported"
                ),
                "approval_can_be_issued_now": artifact_state(
                    artifacts, "candidate_compatibility_card", "approval_can_be_issued_now"
                ),
                "current_fixture_approval_valid": artifact_state(
                    artifacts, "candidate_compatibility_card", "current_fixture_approval_valid"
                ),
                "fixture_approval_satisfied": artifact_state(
                    artifacts, "candidate_compatibility_card", "fixture_approval_satisfied"
                ),
                "native_load_gate": artifact_state(
                    artifacts, "candidate_compatibility_card", "native_load_gate"
                ),
                "native_load_gate_stays_closed": artifact_state(
                    artifacts, "candidate_compatibility_card", "native_load_gate_stays_closed"
                ),
                "path_acceptance_ready": artifact_state(
                    artifacts, "candidate_compatibility_card", "path_acceptance_ready"
                ),
                "aex_path_acceptance_enabled": artifact_state(
                    artifacts, "candidate_compatibility_card", "aex_path_acceptance_enabled"
                ),
                "real_render_open": artifact_state(
                    artifacts, "candidate_compatibility_card", "real_render_open"
                ),
                "real_route_open": artifact_state(
                    artifacts, "candidate_compatibility_card", "real_route_open"
                ),
            },
            "The selected AEX candidate now has a compact no-load compatibility card joining metadata-only PiPL evidence, no-load image/OFX mock results, and closed approval gates.",
            "Use this card as the bridge into future image compatibility or OFX mock tooling; do not treat it as fixture approval or native-load permission.",
        ),
        requirement(
            "candidate_image_compat_mock",
            "Candidate image compatibility mock",
            "satisfied_deferred" if clean_evidence and candidate_image_compat_mock_ready else "failed",
            [
                "candidate_compatibility_card",
                "image_fixture_suite",
                "image_fixture_validation",
                "candidate_image_compat_mock",
            ],
            {
                "mock_state": artifact_state(artifacts, "candidate_image_compat_mock", "mock_state"),
                "mock_ready": artifact_state(artifacts, "candidate_image_compat_mock", "mock_ready"),
                "operation": artifact_state(artifacts, "candidate_image_compat_mock", "operation"),
                "source_compatibility_card_state": artifact_state(
                    artifacts, "candidate_image_compat_mock", "source_compatibility_card_state"
                ),
                "source_compatibility_card_ready": artifact_state(
                    artifacts, "candidate_image_compat_mock", "source_compatibility_card_ready"
                ),
                "source_native_load_gate": artifact_state(
                    artifacts, "candidate_image_compat_mock", "source_native_load_gate"
                ),
                "source_real_render_open": artifact_state(
                    artifacts, "candidate_image_compat_mock", "source_real_render_open"
                ),
                "source_real_route_open": artifact_state(
                    artifacts, "candidate_image_compat_mock", "source_real_route_open"
                ),
                "input_ppm_absolute_path_exported": artifact_state(
                    artifacts, "candidate_image_compat_mock", "input_ppm_absolute_path_exported"
                ),
                "output_ppm_absolute_path_exported": artifact_state(
                    artifacts, "candidate_image_compat_mock", "output_ppm_absolute_path_exported"
                ),
                "transform_check": artifact_state(
                    artifacts, "candidate_image_compat_mock", "transform_check"
                ),
                "candidate_image_mock_performed": artifact_state(
                    artifacts, "candidate_image_compat_mock", "candidate_image_mock_performed"
                ),
                "mock_transform_performed": artifact_state(
                    artifacts, "candidate_image_compat_mock", "mock_transform_performed"
                ),
            },
            "The selected candidate now has a tiny no-load image-facing mock transform driven by the compatibility card and generated PPM fixtures.",
            "Use this as a placeholder image tool only; it is not an AEX render and does not open native, AE, or real OFX routes.",
        ),
    ]

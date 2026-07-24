#!/usr/bin/env python3
"""Build a readiness matrix from the canonical AEX Compat Lab artifact index.

This tool reads JSON index evidence only. It never opens AEX files, starts
After Effects, invokes OFX, or touches image runtimes.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
INDEX_ROOT = TARGET_ROOT / "artifact-index"
MATRIX_ROOT = TARGET_ROOT / "readiness-matrix"

REQUIRED_ARTIFACT_LABELS = (
    "static_report",
    "pipl_resource_catalog",
    "parameter_schema_plan",
    "parameter_schema_review",
    "redacted_schema_verifier",
    "synthetic_pipl_parser_selftest",
    "synthetic_pipl_payload_parser",
    "pipl_parser_gate",
    "pipl_resource_consistency_audit",
    "pipl_payload_adapter_review",
    "aepx_static_probe",
    "aepx_edit_plan",
    "aepx_roundtrip_validator",
    "aepx_redacted_text_inventory",
    "aepx_redacted_text_classifier",
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
    "fixture_manifest",
    "fixture_decision",
    "fixture_dossier",
    "fixture_manual_review_packet",
    "fixture_provenance_review",
    "fixture_provenance_answer_template",
    "fixture_provenance_answer_validator_selftest",
    "fixture_approval_verifier",
    "fixture_approval_request",
    "candidate_test_handoff",
    "candidate_test_runner_dryrun",
    "candidate_test_runner",
    "candidate_compatibility_card",
    "candidate_image_compat_mock",
    "candidate_ofx_bridge",
    "candidate_ofx_host_harness_dryrun",
    "candidate_ofx_host_harness_selftest",
    "candidate_ofx_runtime_boundary_contract",
    "candidate_ofx_runtime_approval_request",
    "candidate_ofx_runtime_approval_verifier",
    "candidate_ofx_runtime_prerequisite_audit",
    "candidate_ofx_host_binary_review_request",
    "candidate_dependency_scope",
    "candidate_load_gate_dryrun",
    "worker_design",
    "worker_selftest",
    "load_gate",
    "native_loader_stub",
    "native_loader_design_contract",
    "native_loader_broker_selftest",
    "native_loader_runtime_contract",
    "native_loader_runtime_selftest",
    "native_loader_path_policy_selftest",
    "ofx_facade",
    "ofx_noop_mock",
    "ofx_suite_selftest",
    "ofx_route_contract",
    "safety_audit",
    "publication_boundary",
)

SAFETY_FLAGS = (
    "native_load_enabled",
    "native_load_performed",
    "dll_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
    "aex_file_opened",
    "aepx_file_modified",
    "aep_binary_modified",
    "ae_project_write_performed",
    "ofx_plugin_built",
    "ofx_describe_performed",
    "ofx_render_performed",
    "aex_render_performed",
    "render_validation_performed",
    "aex_file_hashed",
    "aex_file_copied",
    "pipl_payload_parsed",
    "parameter_schema_emitted",
    "redacted_schema_emitted",
    "real_pipl_payload_parser_enabled",
    "real_pipl_payload_parsed",
    "resource_payload_opened",
    "raw_payload_serialized",
    "ofx_host_binary_opened",
    "ofx_host_binary_hashed",
    "ofx_host_binary_copied",
    "ofx_host_binary_executed",
    "ofx_plugin_binary_opened",
    "ofx_plugin_binary_hashed",
    "ofx_plugin_binary_copied",
)


def path_has_traversal(path: Path) -> bool:
    return any(part in ("..", ".") for part in path.parts)


def resolve_under_root(path: Path, root: Path, *, must_exist: bool) -> Path:
    if path_has_traversal(path):
        raise ValueError("path must not contain traversal components")
    absolute = path if path.is_absolute() else LAB_ROOT / path
    resolved_root = root.resolve(strict=True)
    resolved = absolute.resolve(strict=must_exist)
    if not resolved.is_relative_to(resolved_root):
        raise ValueError(f"path must stay under {root}")
    return resolved


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("readiness matrix report must have .json extension")
    MATRIX_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, MATRIX_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(MATRIX_ROOT.resolve(strict=True)):
        raise ValueError(f"readiness matrix parent must stay under {MATRIX_ROOT}")
    return resolved


def read_json(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("artifact index must be a JSON object")
    return payload


def load_artifact_index(path: Path) -> tuple[dict[str, Any], Path]:
    if path.suffix.lower() != ".json":
        raise ValueError("artifact index must have .json extension")
    resolved = resolve_under_root(path, INDEX_ROOT, must_exist=True)
    payload = read_json(resolved)
    return payload, resolved


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def artifacts_by_label(index: dict[str, Any]) -> dict[str, dict[str, Any]]:
    artifacts = index.get("artifacts")
    if not isinstance(artifacts, list):
        return {}
    by_label: dict[str, dict[str, Any]] = {}
    for artifact in artifacts:
        if isinstance(artifact, dict) and isinstance(artifact.get("label"), str):
            by_label[artifact["label"]] = artifact
    return by_label


def artifact_state(artifacts: dict[str, dict[str, Any]], label: str, key: str) -> Any:
    states = artifacts.get(label, {}).get("states")
    if not isinstance(states, dict):
        return None
    return states.get(key)


def artifact_found(artifacts: dict[str, dict[str, Any]], label: str) -> bool:
    return artifacts.get(label, {}).get("found") is True


def labels_found(artifacts: dict[str, dict[str, Any]], labels: tuple[str, ...]) -> bool:
    return all(artifact_found(artifacts, label) for label in labels)


def safety_errors(index: dict[str, Any], artifacts: dict[str, dict[str, Any]]) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in index and index.get(flag) is not False:
            errors.append(f"artifact index {flag} must be false")
    for label, artifact in artifacts.items():
        safety_flags = artifact.get("safety_flags")
        if not isinstance(safety_flags, dict):
            continue
        for flag, value in safety_flags.items():
            if flag in SAFETY_FLAGS and value is not False:
                errors.append(f"{label} {flag} must be false")
    return errors


def evidence_errors(index: dict[str, Any], artifacts: dict[str, dict[str, Any]]) -> list[str]:
    errors: list[str] = []
    if index.get("report_kind") != "aex_artifact_index":
        errors.append("source artifact must have report_kind=aex_artifact_index")
    if index.get("index_state") != "canonical_chain_indexed":
        errors.append("artifact index must have index_state=canonical_chain_indexed")
    source_errors = index.get("errors")
    if source_errors:
        errors.append("artifact index errors must be empty")
    for label in REQUIRED_ARTIFACT_LABELS:
        if not artifact_found(artifacts, label):
            errors.append(f"{label} artifact must be present")
    if artifact_state(artifacts, "safety_audit", "audit_passed") is not True:
        errors.append("safety_audit audit_passed must be true")
    if artifact_state(artifacts, "publication_boundary", "publishable_now") is not False:
        errors.append("publication_boundary publishable_now must be false")
    errors.extend(safety_errors(index, artifacts))
    return errors


def requirement(
    requirement_id: str,
    title: str,
    status: str,
    evidence_artifacts: list[str],
    evidence_states: dict[str, Any],
    note: str,
    next_action: str | None = None,
) -> dict[str, Any]:
    item: dict[str, Any] = {
        "id": requirement_id,
        "title": title,
        "status": status,
        "evidence_artifacts": evidence_artifacts,
        "evidence_states": evidence_states,
        "note": note,
    }
    if next_action:
        item["next_action"] = next_action
    return item


def build_requirements(artifacts: dict[str, dict[str, Any]], clean_evidence: bool) -> list[dict[str, Any]]:
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


def summarize_requirements(requirements: list[dict[str, Any]]) -> dict[str, int]:
    counts = {
        "satisfied_count": 0,
        "pending_count": 0,
        "intentionally_closed_count": 0,
        "failed_count": 0,
    }
    for item in requirements:
        status = item["status"]
        if status.startswith("satisfied"):
            counts["satisfied_count"] += 1
        elif status.startswith("pending"):
            counts["pending_count"] += 1
        elif status == "intentionally_closed":
            counts["intentionally_closed_count"] += 1
        elif status == "failed":
            counts["failed_count"] += 1
    return counts


def build_readiness_matrix(index: dict[str, Any], source_path: Path) -> dict[str, Any]:
    artifacts = artifacts_by_label(index)
    errors = evidence_errors(index, artifacts)
    clean_evidence = not errors
    requirements = build_requirements(artifacts, clean_evidence)
    counts = summarize_requirements(requirements)
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_readiness_matrix",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_artifact_index": str(source_path),
        "readiness_state": (
            "no_load_foundation_ready_pending_manual_approval"
            if clean_evidence
            else "evidence_invalid_no_load_foundation_not_ready"
        ),
        "overall_ready_for_no_load_tooling": clean_evidence,
        "overall_ready_for_native_load": False,
        "overall_ready_for_publication": False,
        "requirements": requirements,
        "summary": counts,
        "evidence_errors": errors,
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
        "raw_payload_serialized": False,
        "next_required_actions": [
            "Author a separate local-only user-answer artifact and run it through the provenance answer validator rules.",
            "Explicit user approval artifact before any native loader accepts an AEX path.",
            "Keep the native-loader broker pathless until fixture approval and an explicit path-acceptance approval exist.",
            "Review dependency availability preflight misses/default-deny rows before native loader work.",
            "Use the PiPL payload adapter review packet before any approved real-payload adapter implementation.",
            "Feed proposed redacted output through the verifier before schema emission.",
            "Review classified AEPX text inventory buckets before any project write tooling.",
            "Keep OFX work on the deferred/no-op path until native loader evidence exists.",
            "Create a redacted public summary only after publication review is complete.",
        ],
        "notes": [
            "Readiness matrix reads the artifact index JSON only.",
            "No AEX, AE, OFX, image runtime, or binary-payload operation is performed.",
            "Native load and publication readiness are intentionally false in this slice.",
        ],
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build local AEX Compat Lab readiness matrix")
    parser.add_argument("--artifact-index", required=True, help="Artifact index JSON under target/artifact-index")
    parser.add_argument("--out", required=True, help="Create-new readiness matrix under target/readiness-matrix")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    index, source_path = load_artifact_index(Path(args.artifact_index))
    matrix = build_readiness_matrix(index, source_path)
    written = write_json_create_new(Path(args.out), matrix)
    print(written)
    return 0 if matrix["overall_ready_for_no_load_tooling"] else 1


if __name__ == "__main__":
    raise SystemExit(main())

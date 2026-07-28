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

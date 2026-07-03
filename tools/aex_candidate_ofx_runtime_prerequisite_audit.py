#!/usr/bin/env python3
"""Audit candidate OFX runtime prerequisites without opening runtime gates.

The audit reads JSON evidence only. It joins the candidate OFX runtime approval
verifier, native runtime containment selftest, render validation contract, and
parameter schema review packet to show which prerequisites are ready as
no-load evidence and which still block any real runtime invocation.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
RUNTIME_APPROVAL_VERIFIER_ROOT = TARGET_ROOT / "candidate-ofx-runtime-approval-verifier"
RUNTIME_SELFTEST_ROOT = TARGET_ROOT / "native-loader-runtime-selftest"
RENDER_CONTRACT_ROOT = TARGET_ROOT / "render-validation-contract"
PARAMETER_SCHEMA_REVIEW_ROOT = TARGET_ROOT / "parameter-schema-review"
PREREQUISITE_AUDIT_ROOT = TARGET_ROOT / "candidate-ofx-runtime-prerequisite-audit"

SAFETY_FLAGS = (
    "native_load_enabled",
    "native_load_performed",
    "dll_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "ofx_runtime_invoked",
    "host_process_launch_enabled",
    "private_payload_copied",
    "aex_file_opened",
    "aex_file_hashed",
    "aex_file_copied",
    "aepx_file_modified",
    "aep_binary_modified",
    "ae_project_write_performed",
    "ofx_plugin_built",
    "ofx_describe_performed",
    "ofx_render_performed",
    "aex_render_performed",
    "render_validation_performed",
    "ppm_pixel_read_performed",
    "pipl_payload_parsed",
    "parameter_schema_emitted",
    "redacted_schema_emitted",
    "real_pipl_payload_parser_enabled",
    "real_pipl_payload_parsed",
    "resource_payload_opened",
    "resource_payload_extracted",
    "raw_payload_serialized",
)

BLOCKED_ACTIONS = (
    "accept_aex_path",
    "open_aex_file",
    "hash_aex_file",
    "copy_selected_aex_fixture",
    "load_aex_dll",
    "load_dependency_dll",
    "call_EffectMain",
    "dispatch_PF_Cmd",
    "start_after_effects",
    "render_with_aex",
    "build_ofx_binary",
    "accept_ofx_host_path",
    "accept_ofx_plugin_binary_path",
    "launch_ofx_host_process",
    "route_through_real_ofx",
    "route_through_ofx",
    "instantiate_ofx_runtime",
    "load_ofx_plugin",
    "ofx_describe_from_aex",
    "ofx_render_with_aex",
    "read_ppm_pixels",
    "open_candidate_mock_ppm",
    "compare_aex_render_pixels",
    "claim_render_equivalence",
    "parse_real_pipl_payload",
    "emit_parameter_schema",
    "emit_redacted_schema",
)


def path_has_traversal(path: Path) -> bool:
    return any(part in ("..", ".") for part in path.parts)


def resolve_under_root(path: Path, root: Path, *, must_exist: bool) -> Path:
    if path_has_traversal(path):
        raise ValueError("path must not contain traversal components")
    absolute = path if path.is_absolute() else LAB_ROOT / path
    resolved_root = root.resolve(strict=True)
    try:
        resolved = absolute.resolve(strict=must_exist)
    except FileNotFoundError as exc:
        raise ValueError(f"path does not exist: {absolute}") from exc
    if not resolved.is_relative_to(resolved_root):
        raise ValueError(f"path must stay under {root}")
    return resolved


def validate_json_input(path: Path, root: Path, label: str) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError(f"{label} must have .json extension")
    return resolve_under_root(path, root, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("candidate OFX runtime prerequisite audit must have .json extension")
    PREREQUISITE_AUDIT_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, PREREQUISITE_AUDIT_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(PREREQUISITE_AUDIT_ROOT.resolve(strict=True)):
        raise ValueError(f"candidate OFX runtime prerequisite audit parent must stay under {PREREQUISITE_AUDIT_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_runtime_approval_verifier(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, RUNTIME_APPROVAL_VERIFIER_ROOT, "candidate OFX runtime approval verifier")
    return read_json_object(resolved), resolved


def load_runtime_selftest(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, RUNTIME_SELFTEST_ROOT, "native runtime containment selftest")
    return read_json_object(resolved), resolved


def load_render_validation_contract(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, RENDER_CONTRACT_ROOT, "render validation contract")
    return read_json_object(resolved), resolved


def load_parameter_schema_review(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, PARAMETER_SCHEMA_REVIEW_ROOT, "parameter schema review")
    return read_json_object(resolved), resolved


def relative_to_lab(path: Path) -> str:
    return path.resolve(strict=False).relative_to(LAB_ROOT.resolve(strict=True)).as_posix()


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def validate_runtime_approval_verifier(verifier: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if verifier.get("publication_status") != "local-only":
        errors.append("runtime approval verifier publication_status must be local-only")
    if verifier.get("report_kind") != "aex_candidate_ofx_runtime_approval_verifier":
        errors.append("runtime approval verifier report_kind must be aex_candidate_ofx_runtime_approval_verifier")
    if verifier.get("runtime_approval_verifier_state") != "candidate_ofx_runtime_approval_verifier_ready_no_approval":
        errors.append("runtime approval verifier state must be ready no approval")
    for key in (
        "runtime_approval_verifier_ready",
        "runtime_approval_verified_not_approved",
        "runtime_approval_gate_stays_closed",
        "boundary_contract_cross_checked",
        "boundary_contract_matches_request",
        "path_acceptance_closed",
        "synthetic_runtime_approval_checks_passed",
    ):
        if verifier.get(key) is not True:
            errors.append(f"runtime approval verifier {key} must be true")
    for key in (
        "current_runtime_approval_valid",
        "runtime_approval_satisfied",
        "runtime_approval_manifest_created",
        "runtime_approval_can_be_issued_now",
        "explicit_runtime_approval_present",
        "request_blockers_clear",
        "fixture_approval_satisfied",
        "ofx_host_binary_review_ready",
        "runtime_containment_selftest_ready",
        "schema_and_render_validation_ready",
        "ofx_runtime_invocation_ready",
        "host_process_launch_enabled",
        "path_acceptance_ready",
        "real_route_open",
        "ofx_runtime_invoked",
        "ppm_pixel_read_performed",
        "runtime_approval_verifier_path_payload_exported",
    ):
        if verifier.get(key) is not False:
            errors.append(f"runtime approval verifier {key} must be false")
    if verifier.get("accepted_aex_path") is not None:
        errors.append("runtime approval verifier accepted_aex_path must be null")
    if int(verifier.get("approval_blocker_count") or 0) < 1:
        errors.append("runtime approval verifier approval_blocker_count must be positive")
    errors.extend(safety_errors(verifier, "runtime approval verifier"))
    return errors


def validate_runtime_selftest(selftest: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if selftest.get("publication_status") != "local-only":
        errors.append("runtime selftest publication_status must be local-only")
    if selftest.get("report_kind") != "aex_native_loader_runtime_selftest":
        errors.append("runtime selftest report_kind must be aex_native_loader_runtime_selftest")
    if selftest.get("runtime_selftest_state") != "runtime_containment_selftest_passed_no_load":
        errors.append("runtime selftest state must be runtime_containment_selftest_passed_no_load")
    for key in (
        "runtime_containment_selftest_passed",
        "runtime_containment_ready",
        "synthetic_subprocess_only",
        "normal_exit_case_passed",
        "stderr_capture_passed",
        "timeout_case_passed",
        "child_cleanup_passed",
    ):
        if selftest.get(key) is not True:
            errors.append(f"runtime selftest {key} must be true")
    for key in ("path_acceptance_ready", "aex_path_acceptance_enabled", "path_payload_supplied"):
        if selftest.get(key) is not False:
            errors.append(f"runtime selftest {key} must be false")
    if selftest.get("accepted_aex_path") is not None:
        errors.append("runtime selftest accepted_aex_path must be null")
    errors.extend(safety_errors(selftest, "runtime selftest"))
    return errors


def validate_render_contract(contract: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if contract.get("publication_status") != "local-only":
        errors.append("render validation contract publication_status must be local-only")
    if contract.get("report_kind") != "aex_render_validation_contract":
        errors.append("render validation contract report_kind must be aex_render_validation_contract")
    if contract.get("contract_state") != "render_validation_contract_ready_render_closed":
        errors.append("render validation contract state must be ready render closed")
    if contract.get("no_load_validation_ready") is not True:
        errors.append("render validation contract no_load_validation_ready must be true")
    if contract.get("real_render_open") is not False:
        errors.append("render validation contract real_render_open must be false")
    if contract.get("render_validation_performed") is not False:
        errors.append("render validation contract render_validation_performed must be false")
    render_contract = contract.get("render_contract")
    if not isinstance(render_contract, dict):
        errors.append("render validation contract render_contract must be an object")
    elif render_contract.get("real_render_open") is not False:
        errors.append("render validation contract render_contract.real_render_open must be false")
    errors.extend(safety_errors(contract, "render validation contract"))
    return errors


def validate_parameter_schema_review(review: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if review.get("publication_status") != "local-only":
        errors.append("parameter schema review publication_status must be local-only")
    if review.get("packet_kind") != "aex_parameter_schema_review_packet":
        errors.append("parameter schema review packet_kind must be aex_parameter_schema_review_packet")
    if review.get("review_state") != "parameter_schema_review_ready_no_payload":
        errors.append("parameter schema review state must be ready no payload")
    if review.get("parser_design_state") != "payload_parser_design_review_ready_parser_disabled":
        errors.append("parameter schema review parser design must remain parser disabled")
    if review.get("redaction_policy_state") != "redaction_policy_ready_no_schema_output":
        errors.append("parameter schema review redaction policy must be no schema output")
    if review.get("ofx_describe_policy_state") != "ofx_describe_mapping_deferred_until_redacted_schema":
        errors.append("parameter schema review OFX describe policy must be deferred")
    for key in (
        "payload_parser_enabled",
        "redacted_schema_available",
        "ofx_describe_mapping_ready",
        "pipl_payload_parsed",
        "parameter_schema_emitted",
        "redacted_schema_emitted",
        "ofx_describe_performed",
        "ofx_render_performed",
        "render_validation_performed",
    ):
        if review.get(key) is not False:
            errors.append(f"parameter schema review {key} must be false")
    errors.extend(safety_errors(review, "parameter schema review"))
    return errors


def build_prerequisite_rows(
    *,
    verifier: dict[str, Any],
    runtime_selftest: dict[str, Any],
    render_contract: dict[str, Any],
    parameter_schema_review: dict[str, Any],
) -> list[dict[str, Any]]:
    return [
        {
            "id": "runtime_approval_request_verified_not_approved",
            "satisfied": verifier.get("runtime_approval_verified_not_approved") is True,
            "current_evidence": verifier.get("runtime_approval_verifier_state"),
            "next_action": "Keep this as no-approval evidence until explicit runtime approval exists.",
        },
        {
            "id": "explicit_runtime_approval",
            "satisfied": False,
            "current_evidence": verifier.get("explicit_runtime_approval_present"),
            "next_action": "Collect explicit user approval outside this audit before runtime work can proceed.",
        },
        {
            "id": "fixture_approval",
            "satisfied": False,
            "current_evidence": verifier.get("fixture_approval_satisfied"),
            "next_action": "Resolve fixture provenance/license/manual approval before accepting any AEX path.",
        },
        {
            "id": "ofx_host_binary_review",
            "satisfied": False,
            "current_evidence": verifier.get("ofx_host_binary_review_ready"),
            "next_action": "Review OFX host/shim provenance and containment before accepting host paths.",
        },
        {
            "id": "runtime_containment_selftest",
            "satisfied": runtime_selftest.get("runtime_containment_selftest_passed") is True,
            "current_evidence": runtime_selftest.get("runtime_selftest_state"),
            "next_action": "Keep this synthetic subprocess evidence current when runtime policy changes.",
        },
        {
            "id": "parameter_schema_review_policy",
            "satisfied": parameter_schema_review.get("review_state") == "parameter_schema_review_ready_no_payload",
            "current_evidence": parameter_schema_review.get("review_state"),
            "next_action": "Real PiPL payload parsing and schema output remain blocked pending review.",
        },
        {
            "id": "render_validation_contract",
            "satisfied": render_contract.get("no_load_validation_ready") is True,
            "current_evidence": render_contract.get("contract_state"),
            "next_action": "Real AEX/OFX render validation remains blocked pending fixture/runtime/schema review.",
        },
        {
            "id": "real_schema_and_render_validation",
            "satisfied": False,
            "current_evidence": "not_present_real_routes_closed",
            "next_action": "Only add after approved fixture, schema, runtime, and render evidence exists.",
        },
    ]


def prerequisite_blockers(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [
        {
            "id": row["id"],
            "current_evidence": row["current_evidence"],
            "next_action": row["next_action"],
        }
        for row in rows
        if row.get("satisfied") is not True
    ]


def build_runtime_prerequisite_audit(
    *,
    runtime_approval_verifier: dict[str, Any],
    runtime_approval_verifier_path: Path,
    runtime_selftest: dict[str, Any],
    runtime_selftest_path: Path,
    render_validation_contract: dict[str, Any],
    render_validation_contract_path: Path,
    parameter_schema_review: dict[str, Any],
    parameter_schema_review_path: Path,
) -> dict[str, Any]:
    errors = (
        validate_runtime_approval_verifier(runtime_approval_verifier)
        + validate_runtime_selftest(runtime_selftest)
        + validate_render_contract(render_validation_contract)
        + validate_parameter_schema_review(parameter_schema_review)
    )
    if errors:
        raise ValueError("; ".join(errors))

    rows = build_prerequisite_rows(
        verifier=runtime_approval_verifier,
        runtime_selftest=runtime_selftest,
        render_contract=render_validation_contract,
        parameter_schema_review=parameter_schema_review,
    )
    blockers = prerequisite_blockers(rows)
    satisfied_count = len(rows) - len(blockers)
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_ofx_runtime_prerequisite_audit",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_candidate_ofx_runtime_approval_verifier": relative_to_lab(runtime_approval_verifier_path),
        "source_native_loader_runtime_selftest": relative_to_lab(runtime_selftest_path),
        "source_render_validation_contract": relative_to_lab(render_validation_contract_path),
        "source_parameter_schema_review": relative_to_lab(parameter_schema_review_path),
        "candidate_relative_path": runtime_approval_verifier.get("candidate_relative_path"),
        "runtime_prerequisite_audit_state": "candidate_ofx_runtime_prerequisite_audit_ready_gates_closed",
        "runtime_prerequisite_audit_ready": True,
        "runtime_prerequisites_complete": False,
        "runtime_prerequisites_all_satisfied": False,
        "runtime_invocation_prerequisites_ready": False,
        "approval_can_be_issued_now": False,
        "failed_evidence_count": 0,
        "blocking_prerequisite_count": len(blockers),
        "runtime_prerequisite_count": len(rows),
        "runtime_prerequisite_satisfied_count": satisfied_count,
        "runtime_prerequisite_blocker_count": len(blockers),
        "runtime_prerequisite_rows": rows,
        "runtime_prerequisite_blockers": blockers,
        "prerequisite_audit_checks": rows,
        "prerequisite_gaps": blockers,
        "audit_summary": {
            "runtime_prerequisite_count": len(rows),
            "runtime_prerequisite_satisfied_count": satisfied_count,
            "blocking_prerequisite_count": len(blockers),
            "failed_evidence_count": 0,
            "runtime_invocation_prerequisites_ready": False,
            "gates_closed": True,
        },
        "runtime_approval_verifier_ready": runtime_approval_verifier.get("runtime_approval_verifier_ready"),
        "runtime_approval_verified_not_approved": runtime_approval_verifier.get(
            "runtime_approval_verified_not_approved"
        ),
        "current_runtime_approval_valid": runtime_approval_verifier.get("current_runtime_approval_valid"),
        "runtime_approval_satisfied": runtime_approval_verifier.get("runtime_approval_satisfied"),
        "runtime_approval_gate_stays_closed": True,
        "boundary_contract_cross_checked": runtime_approval_verifier.get("boundary_contract_cross_checked"),
        "boundary_contract_matches_request": runtime_approval_verifier.get("boundary_contract_matches_request"),
        "approval_blocker_count": runtime_approval_verifier.get("approval_blocker_count"),
        "request_blockers_clear": runtime_approval_verifier.get("request_blockers_clear"),
        "explicit_runtime_approval_present": False,
        "fixture_approval_satisfied": False,
        "ofx_host_binary_review_ready": False,
        "runtime_containment_contract_ready": runtime_selftest.get("runtime_containment_ready"),
        "runtime_containment_selftest_ready": False,
        "runtime_containment_selftest_synthetic_passed": runtime_selftest.get(
            "runtime_containment_selftest_passed"
        ),
        "runtime_containment_selftest_passed": runtime_selftest.get("runtime_containment_selftest_passed"),
        "runtime_selftest_state": runtime_selftest.get("runtime_selftest_state"),
        "synthetic_subprocess_only": runtime_selftest.get("synthetic_subprocess_only"),
        "normal_exit_case_passed": runtime_selftest.get("normal_exit_case_passed"),
        "stderr_capture_passed": runtime_selftest.get("stderr_capture_passed"),
        "timeout_case_passed": runtime_selftest.get("timeout_case_passed"),
        "child_cleanup_passed": runtime_selftest.get("child_cleanup_passed"),
        "parameter_schema_review_policy_ready": True,
        "parameter_schema_review_state": parameter_schema_review.get("review_state"),
        "payload_parser_enabled": False,
        "redacted_schema_available": False,
        "ofx_describe_mapping_ready": False,
        "schema_and_render_validation_ready": False,
        "render_validation_contract_ready": True,
        "render_validation_contract_state": render_validation_contract.get("contract_state"),
        "no_load_validation_ready": render_validation_contract.get("no_load_validation_ready"),
        "real_render_open": False,
        "ofx_route_contract_closed": True,
        "real_route_open": False,
        "real_ofx_route_ready": False,
        "mock_route_ready": True,
        "path_acceptance_closed": True,
        "ofx_runtime_allowed_now": False,
        "ofx_runtime_invocation_ready": False,
        "ofx_runtime_instantiation_ready": False,
        "ofx_runtime_instantiation_performed": False,
        "host_process_launch_enabled": False,
        "ofx_host_path_payload_supplied": False,
        "ofx_plugin_binary_path_payload_supplied": False,
        "path_acceptance_ready": False,
        "aex_path_acceptance_enabled": False,
        "accepted_aex_path": None,
        "ofx_runtime_invoked": False,
        "aex_runtime_invoked": False,
        "ofx_describe_ready": False,
        "ofx_render_ready": False,
        "render_equivalence_claim_ready": False,
        "ppm_pixel_read_performed": False,
        "absolute_ppm_paths_exported": False,
        "absolute_aex_paths_exported": False,
        "runtime_prerequisite_audit_path_payload_exported": False,
        "blocked_actions_after_audit": list(BLOCKED_ACTIONS),
        "blocked_actions": list(BLOCKED_ACTIONS),
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
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
        "pipl_payload_parsed": False,
        "parameter_schema_emitted": False,
        "redacted_schema_emitted": False,
        "real_pipl_payload_parser_enabled": False,
        "real_pipl_payload_parsed": False,
        "resource_payload_opened": False,
        "resource_payload_extracted": False,
        "raw_payload_serialized": False,
        "next_required_actions": [
            "Resolve explicit runtime approval, fixture approval, and OFX host binary review before any runtime path opens.",
            "Keep runtime containment, schema review, and render validation evidence current as policies change.",
            "Regenerate approval verifier and this audit after any prerequisite evidence changes.",
        ],
        "notes": [
            "This audit reads JSON evidence only.",
            "It does not approve or invoke any runtime.",
            "No AEX, OFX host, OFX plugin, PPM path payload, DLL, AE, render, or PiPL payload route is opened.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Audit candidate OFX runtime prerequisites without opening runtime")
    parser.add_argument(
        "--runtime-approval-verifier",
        required=True,
        help="Candidate OFX runtime approval verifier under target/candidate-ofx-runtime-approval-verifier",
    )
    parser.add_argument(
        "--runtime-selftest",
        required=True,
        help="Native runtime containment selftest under target/native-loader-runtime-selftest",
    )
    parser.add_argument(
        "--render-validation-contract",
        required=True,
        help="Render validation contract under target/render-validation-contract",
    )
    parser.add_argument(
        "--parameter-schema-review",
        required=True,
        help="Parameter schema review under target/parameter-schema-review",
    )
    parser.add_argument(
        "--out",
        required=True,
        help="Create-new audit JSON under target/candidate-ofx-runtime-prerequisite-audit",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    verifier, verifier_path = load_runtime_approval_verifier(Path(args.runtime_approval_verifier))
    runtime_selftest, runtime_selftest_path = load_runtime_selftest(Path(args.runtime_selftest))
    render_contract, render_contract_path = load_render_validation_contract(Path(args.render_validation_contract))
    schema_review, schema_review_path = load_parameter_schema_review(Path(args.parameter_schema_review))
    audit = build_runtime_prerequisite_audit(
        runtime_approval_verifier=verifier,
        runtime_approval_verifier_path=verifier_path,
        runtime_selftest=runtime_selftest,
        runtime_selftest_path=runtime_selftest_path,
        render_validation_contract=render_contract,
        render_validation_contract_path=render_contract_path,
        parameter_schema_review=schema_review,
        parameter_schema_review_path=schema_review_path,
    )
    written = write_json_create_new(Path(args.out), audit)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

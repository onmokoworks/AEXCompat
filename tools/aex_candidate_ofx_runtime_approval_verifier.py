#!/usr/bin/env python3
"""Verify candidate OFX runtime approval evidence without opening runtime gates.

The verifier reads the candidate OFX runtime approval request JSON only. It
checks that the current request is not approval, synthetic future approval
shapes are constrained to review preparation, and runtime/host/path/render
routes remain closed.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
RUNTIME_APPROVAL_REQUEST_ROOT = TARGET_ROOT / "candidate-ofx-runtime-approval-request"
RUNTIME_BOUNDARY_ROOT = TARGET_ROOT / "candidate-ofx-runtime-boundary-contract"
RUNTIME_APPROVAL_VERIFIER_ROOT = TARGET_ROOT / "candidate-ofx-runtime-approval-verifier"

APPROVAL_TOKEN_NAME = "APPROVE_OFX_RUNTIME_INVOCATION"
PREPARE_RUNTIME_REVIEW_ACTION = "prepare_ofx_runtime_review"

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

FORBIDDEN_APPROVED_ACTIONS = {
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
}


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
        raise ValueError("candidate OFX runtime approval verifier must have .json extension")
    RUNTIME_APPROVAL_VERIFIER_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, RUNTIME_APPROVAL_VERIFIER_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(RUNTIME_APPROVAL_VERIFIER_ROOT.resolve(strict=True)):
        raise ValueError(f"candidate OFX runtime approval verifier parent must stay under {RUNTIME_APPROVAL_VERIFIER_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_runtime_approval_request(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, RUNTIME_APPROVAL_REQUEST_ROOT, "candidate OFX runtime approval request")
    return read_json_object(resolved), resolved


def load_runtime_boundary_contract(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, RUNTIME_BOUNDARY_ROOT, "candidate OFX runtime boundary contract")
    return read_json_object(resolved), resolved


def relative_to_lab(path: Path) -> str:
    return path.resolve(strict=False).relative_to(LAB_ROOT.resolve(strict=True)).as_posix()


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def validate_runtime_approval_request(request: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if request.get("publication_status") != "local-only":
        errors.append("runtime approval request publication_status must be local-only")
    if request.get("report_kind") != "aex_candidate_ofx_runtime_approval_request_packet":
        errors.append("runtime approval request report_kind must be aex_candidate_ofx_runtime_approval_request_packet")
    if request.get("runtime_approval_request_state") != "candidate_ofx_runtime_approval_request_ready_pending_manual_approval":
        errors.append("runtime approval request state must be ready pending manual approval")
    for key in (
        "runtime_approval_request_ready",
        "runtime_approval_request_created",
        "runtime_approval_gate_stays_closed",
        "requires_explicit_user_approval",
        "approval_token_not_stored_in_manifest",
        "approval_only_prepares_runtime_review",
        "source_boundary_contract_ready",
        "source_mock_route_ready",
        "source_requires_future_runtime_approval",
        "source_requires_future_fixture_approval",
        "source_requires_future_render_validation_approval",
    ):
        if request.get(key) is not True:
            errors.append(f"runtime approval request {key} must be true")
    if request.get("required_approval_token_name") != APPROVAL_TOKEN_NAME:
        errors.append(f"required_approval_token_name must be {APPROVAL_TOKEN_NAME}")
    if request.get("source_boundary_contract_state") != "candidate_ofx_runtime_boundary_contract_ready_no_runtime_route_closed":
        errors.append("source boundary contract state must be ready no-runtime route closed")
    if request.get("source_contract_state") != "candidate_ofx_runtime_boundary_contract_ready_runtime_closed":
        errors.append("source contract state must be ready runtime closed")
    for key in (
        "runtime_approval_can_be_issued_now",
        "runtime_approval_manifest_created",
        "source_ofx_runtime_allowed_now",
        "source_ofx_runtime_invocation_ready",
        "source_host_process_launch_enabled",
        "source_path_acceptance_ready",
        "source_real_route_open",
        "source_ofx_runtime_invoked",
        "source_ppm_pixel_read_performed",
        "source_fixture_approval_satisfied",
        "ofx_runtime_allowed_now",
        "ofx_runtime_invocation_ready",
        "ofx_runtime_instantiation_performed",
        "host_process_launch_enabled",
        "ofx_host_path_payload_supplied",
        "ofx_plugin_binary_path_payload_supplied",
        "path_acceptance_ready",
        "aex_path_acceptance_enabled",
        "real_route_open",
        "real_ofx_route_ready",
        "ofx_runtime_invoked",
        "aex_runtime_invoked",
        "ofx_describe_ready",
        "ofx_render_ready",
        "render_equivalence_claim_ready",
        "ppm_pixel_read_performed",
        "absolute_ppm_paths_exported",
        "absolute_aex_paths_exported",
        "runtime_approval_path_payload_exported",
    ):
        if request.get(key) is not False:
            errors.append(f"runtime approval request {key} must be false")
    if request.get("accepted_aex_path") is not None:
        errors.append("runtime approval request accepted_aex_path must be null")
    if request.get("mock_route_ready") is not True:
        errors.append("runtime approval request mock_route_ready must be true")
    if int(request.get("review_checklist_count") or 0) != 6:
        errors.append("review_checklist_count must be 6")
    if int(request.get("approval_blocker_count") or 0) < 1:
        errors.append("approval_blocker_count must be positive")
    for field in ("review_checklist", "approval_blockers", "blocked_actions_after_request", "blocked_actions"):
        if not isinstance(request.get(field), list):
            errors.append(f"{field} must be a list")
    blocked = request.get("blocked_actions_after_request")
    if isinstance(blocked, list):
        for action in (
            "instantiate_ofx_runtime",
            "launch_ofx_host_process",
            "accept_ofx_host_path",
            "accept_ofx_plugin_binary_path",
            "load_ofx_plugin",
            "ofx_describe_from_aex",
            "ofx_render_with_aex",
        ):
            if action not in blocked:
                errors.append(f"runtime approval request must keep {action} blocked")
    errors.extend(safety_errors(request, "runtime approval request"))
    return errors


def validate_runtime_boundary_cross_check(boundary: dict[str, Any], request: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if boundary.get("publication_status") != "local-only":
        errors.append("runtime boundary publication_status must be local-only")
    if boundary.get("report_kind") != "aex_candidate_ofx_runtime_boundary_contract":
        errors.append("runtime boundary report_kind must be aex_candidate_ofx_runtime_boundary_contract")
    checks = {
        "candidate_relative_path": request.get("candidate_relative_path"),
        "candidate_ofx_runtime_boundary_contract_state": request.get("source_boundary_contract_state"),
        "contract_state": request.get("source_contract_state"),
        "runtime_boundary_ready": request.get("source_boundary_contract_ready"),
        "ofx_runtime_allowed_now": request.get("source_ofx_runtime_allowed_now"),
        "ofx_runtime_invocation_ready": request.get("source_ofx_runtime_invocation_ready"),
        "host_process_launch_enabled": request.get("source_host_process_launch_enabled"),
        "path_acceptance_ready": request.get("source_path_acceptance_ready"),
        "real_route_open": request.get("source_real_route_open"),
        "mock_route_ready": request.get("source_mock_route_ready"),
        "ofx_runtime_invoked": request.get("source_ofx_runtime_invoked"),
        "ppm_pixel_read_performed": request.get("source_ppm_pixel_read_performed"),
        "source_fixture_approval_satisfied": request.get("source_fixture_approval_satisfied"),
        "requires_future_runtime_approval": request.get("source_requires_future_runtime_approval"),
        "requires_future_fixture_approval": request.get("source_requires_future_fixture_approval"),
        "requires_future_render_validation_approval": request.get(
            "source_requires_future_render_validation_approval"
        ),
    }
    for key, expected in checks.items():
        if boundary.get(key) != expected:
            errors.append(f"runtime boundary {key} must match approval request")
    if boundary.get("runtime_boundary_path_payload_exported") is not False:
        errors.append("runtime boundary path payload export must remain false")
    if boundary.get("accepted_aex_path") is not None:
        errors.append("runtime boundary accepted_aex_path must be null")
    blocked = boundary.get("blocked_actions")
    if not isinstance(blocked, list):
        errors.append("runtime boundary blocked_actions must be a list")
    else:
        for action in (
            "instantiate_ofx_runtime",
            "launch_ofx_host_process",
            "accept_ofx_host_path",
            "accept_ofx_plugin_binary_path",
            "ofx_describe_from_aex",
            "ofx_render_with_aex",
        ):
            if action not in blocked:
                errors.append(f"runtime boundary must block {action}")
    errors.extend(safety_errors(boundary, "runtime boundary"))
    return errors


def evaluate_runtime_approval_manifest(
    manifest: dict[str, Any],
    *,
    candidate_relative_path: str | None,
    request_ready: bool,
    request_blockers_clear: bool,
    fixture_approval_satisfied: bool,
    runtime_boundary_ready: bool,
    host_binary_review_ready: bool,
    runtime_containment_selftest_ready: bool,
    schema_and_render_validation_ready: bool,
    path_acceptance_closed: bool,
) -> dict[str, Any]:
    reasons: list[str] = []
    if manifest.get("manifest_kind") != "aex_candidate_ofx_runtime_approval_manifest":
        reasons.append("manifest_kind_not_runtime_approval")
    if manifest.get("publication_status") != "local-only":
        reasons.append("publication_status_not_local_only")
    if manifest.get("approval_state") != "user_approved_for_ofx_runtime_review":
        reasons.append("approval_state_not_user_approved_for_ofx_runtime_review")
    if manifest.get("explicit_user_approval") is not True:
        reasons.append("explicit_user_approval_missing")
    if manifest.get("candidate_relative_path") != candidate_relative_path:
        reasons.append("candidate_relative_path_mismatch")
    approved_actions = manifest.get("approved_actions", [])
    if not isinstance(approved_actions, list):
        approved_actions = []
        reasons.append("approved_actions_not_list")
    if PREPARE_RUNTIME_REVIEW_ACTION not in approved_actions:
        reasons.append("prepare_ofx_runtime_review_not_approved")
    forbidden = sorted(action for action in approved_actions if action in FORBIDDEN_APPROVED_ACTIONS)
    if forbidden:
        reasons.append("forbidden_runtime_action_approved")
    if not request_ready:
        reasons.append("runtime_approval_request_not_ready")
    if not request_blockers_clear:
        reasons.append("runtime_approval_request_has_blockers")
    if not fixture_approval_satisfied:
        reasons.append("fixture_approval_not_satisfied")
    if not runtime_boundary_ready:
        reasons.append("runtime_boundary_not_ready")
    if not host_binary_review_ready:
        reasons.append("ofx_host_binary_review_not_ready")
    if not runtime_containment_selftest_ready:
        reasons.append("runtime_containment_selftest_not_ready")
    if not schema_and_render_validation_ready:
        reasons.append("schema_and_render_validation_not_ready")
    if not path_acceptance_closed:
        reasons.append("path_acceptance_not_closed")
    for error in safety_errors(manifest, "runtime approval"):
        reasons.append(error.replace(" ", "_"))
    return {
        "valid": not reasons,
        "reasons": reasons,
        "approved_actions": approved_actions,
        "forbidden_approved_actions": forbidden,
        "approval_only_prepares_runtime_review": not forbidden and PREPARE_RUNTIME_REVIEW_ACTION in approved_actions,
    }


def synthetic_runtime_approval_checks(request: dict[str, Any]) -> list[dict[str, Any]]:
    candidate_relative_path = request.get("candidate_relative_path")
    base = {
        "schema_version": 1,
        "publication_status": "local-only",
        "manifest_kind": "aex_candidate_ofx_runtime_approval_manifest",
        "approval_state": "user_approved_for_ofx_runtime_review",
        "explicit_user_approval": True,
        "candidate_relative_path": candidate_relative_path,
        "approved_actions": [PREPARE_RUNTIME_REVIEW_ACTION],
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
        "ofx_plugin_built": False,
        "ofx_describe_performed": False,
        "ofx_render_performed": False,
        "aex_render_performed": False,
        "render_validation_performed": False,
        "ppm_pixel_read_performed": False,
    }
    ready_context = {
        "candidate_relative_path": candidate_relative_path,
        "request_ready": True,
        "request_blockers_clear": True,
        "fixture_approval_satisfied": True,
        "runtime_boundary_ready": True,
        "host_binary_review_ready": True,
        "runtime_containment_selftest_ready": True,
        "schema_and_render_validation_ready": True,
        "path_acceptance_closed": True,
    }
    current_context = {
        "candidate_relative_path": candidate_relative_path,
        "request_ready": request.get("runtime_approval_request_ready") is True,
        "request_blockers_clear": int(request.get("approval_blocker_count") or 0) == 0,
        "fixture_approval_satisfied": request.get("source_fixture_approval_satisfied") is True,
        "runtime_boundary_ready": request.get("source_boundary_contract_ready") is True,
        "host_binary_review_ready": False,
        "runtime_containment_selftest_ready": False,
        "schema_and_render_validation_ready": False,
        "path_acceptance_closed": request.get("path_acceptance_ready") is False and request.get("accepted_aex_path") is None,
    }
    cases = [
        {
            "case": "current_request_packet_rejected",
            "manifest": request,
            "context": current_context,
            "expect_valid": False,
            "expect_reason": "manifest_kind_not_runtime_approval",
        },
        {
            "case": "missing_explicit_user_approval_rejected",
            "manifest": {**base, "explicit_user_approval": False},
            "context": ready_context,
            "expect_valid": False,
            "expect_reason": "explicit_user_approval_missing",
        },
        {
            "case": "forbidden_runtime_action_rejected",
            "manifest": {**base, "approved_actions": [PREPARE_RUNTIME_REVIEW_ACTION, "instantiate_ofx_runtime"]},
            "context": ready_context,
            "expect_valid": False,
            "expect_reason": "forbidden_runtime_action_approved",
        },
        {
            "case": "fixture_approval_missing_rejected",
            "manifest": base,
            "context": {**ready_context, "fixture_approval_satisfied": False},
            "expect_valid": False,
            "expect_reason": "fixture_approval_not_satisfied",
        },
        {
            "case": "request_blockers_rejected",
            "manifest": base,
            "context": {**ready_context, "request_blockers_clear": False},
            "expect_valid": False,
            "expect_reason": "runtime_approval_request_has_blockers",
        },
        {
            "case": "future_valid_shape_only_prepares_runtime_review",
            "manifest": base,
            "context": ready_context,
            "expect_valid": True,
            "expect_reason": None,
        },
    ]
    results: list[dict[str, Any]] = []
    for case in cases:
        evaluation = evaluate_runtime_approval_manifest(case["manifest"], **case["context"])
        if evaluation["valid"] != case["expect_valid"]:
            raise RuntimeError(f"synthetic runtime approval case failed: {case['case']}")
        if case["expect_reason"] and case["expect_reason"] not in evaluation["reasons"]:
            raise RuntimeError(f"synthetic runtime approval case reason mismatch: {case['case']}")
        results.append(
            {
                "case": case["case"],
                "valid": evaluation["valid"],
                "reasons": evaluation["reasons"],
                "approval_only_prepares_runtime_review": evaluation["approval_only_prepares_runtime_review"],
            }
        )
    return results


def build_runtime_approval_verifier(
    *,
    runtime_approval_request: dict[str, Any],
    runtime_approval_request_path: Path,
    runtime_boundary_contract: dict[str, Any] | None = None,
    runtime_boundary_contract_path: Path | None = None,
) -> dict[str, Any]:
    errors = validate_runtime_approval_request(runtime_approval_request)
    if runtime_boundary_contract is not None:
        errors.extend(validate_runtime_boundary_cross_check(runtime_boundary_contract, runtime_approval_request))
    if errors:
        raise ValueError("; ".join(errors))

    request_ready = runtime_approval_request.get("runtime_approval_request_ready") is True
    request_blockers_clear = int(runtime_approval_request.get("approval_blocker_count") or 0) == 0
    fixture_approval_satisfied = runtime_approval_request.get("source_fixture_approval_satisfied") is True
    path_acceptance_closed = (
        runtime_approval_request.get("path_acceptance_ready") is False
        and runtime_approval_request.get("aex_path_acceptance_enabled") is False
        and runtime_approval_request.get("accepted_aex_path") is None
        and runtime_approval_request.get("runtime_approval_path_payload_exported") is False
    )
    current_evaluation = evaluate_runtime_approval_manifest(
        runtime_approval_request,
        candidate_relative_path=runtime_approval_request.get("candidate_relative_path"),
        request_ready=request_ready,
        request_blockers_clear=request_blockers_clear,
        fixture_approval_satisfied=fixture_approval_satisfied,
        runtime_boundary_ready=runtime_approval_request.get("source_boundary_contract_ready") is True,
        host_binary_review_ready=False,
        runtime_containment_selftest_ready=False,
        schema_and_render_validation_ready=False,
        path_acceptance_closed=path_acceptance_closed,
    )
    synthetic_checks = synthetic_runtime_approval_checks(runtime_approval_request)
    boundary_cross_checked = runtime_boundary_contract is not None
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_ofx_runtime_approval_verifier",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_candidate_ofx_runtime_approval_request": relative_to_lab(runtime_approval_request_path),
        "source_candidate_ofx_runtime_boundary_contract": (
            relative_to_lab(runtime_boundary_contract_path)
            if runtime_boundary_contract is not None and runtime_boundary_contract_path is not None
            else None
        ),
        "candidate_relative_path": runtime_approval_request.get("candidate_relative_path"),
        "runtime_approval_verifier_state": "candidate_ofx_runtime_approval_verifier_ready_no_approval",
        "runtime_approval_verifier_ready": True,
        "runtime_approval_verified_not_approved": True,
        "source_runtime_approval_request_state": runtime_approval_request.get("runtime_approval_request_state"),
        "source_runtime_approval_request_ready": request_ready,
        "runtime_approval_request_state": runtime_approval_request.get("runtime_approval_request_state"),
        "runtime_approval_request_ready": request_ready,
        "runtime_approval_request_created": runtime_approval_request.get("runtime_approval_request_created"),
        "runtime_approval_gate_stays_closed": True,
        "approval_request_kind": runtime_approval_request.get("approval_request_kind"),
        "runtime_approval_manifest_kind": runtime_approval_request.get("manifest_kind"),
        "current_runtime_approval_valid": current_evaluation["valid"],
        "runtime_approval_satisfied": False,
        "runtime_approval_manifest_created": False,
        "runtime_approval_can_be_issued_now": False,
        "runtime_approval_gate_closed": True,
        "boundary_contract_cross_checked": boundary_cross_checked,
        "boundary_contract_matches_request": boundary_cross_checked,
        "explicit_runtime_approval_present": False,
        "requires_explicit_user_approval": runtime_approval_request.get("requires_explicit_user_approval"),
        "required_approval_token_name": APPROVAL_TOKEN_NAME,
        "approval_token_not_stored_in_manifest": True,
        "approval_only_prepares_runtime_review": True,
        "review_checklist_count": runtime_approval_request.get("review_checklist_count"),
        "approval_blocker_count": runtime_approval_request.get("approval_blocker_count"),
        "request_blockers_clear": request_blockers_clear,
        "source_boundary_contract_state": runtime_approval_request.get("source_boundary_contract_state"),
        "source_boundary_contract_ready": runtime_approval_request.get("source_boundary_contract_ready"),
        "source_contract_state": runtime_approval_request.get("source_contract_state"),
        "source_ofx_runtime_allowed_now": runtime_approval_request.get("source_ofx_runtime_allowed_now"),
        "source_ofx_runtime_invocation_ready": runtime_approval_request.get(
            "source_ofx_runtime_invocation_ready"
        ),
        "source_host_process_launch_enabled": runtime_approval_request.get(
            "source_host_process_launch_enabled"
        ),
        "source_path_acceptance_ready": runtime_approval_request.get("source_path_acceptance_ready"),
        "source_real_route_open": runtime_approval_request.get("source_real_route_open"),
        "source_mock_route_ready": runtime_approval_request.get("source_mock_route_ready"),
        "source_ofx_runtime_invoked": runtime_approval_request.get("source_ofx_runtime_invoked"),
        "source_ppm_pixel_read_performed": runtime_approval_request.get("source_ppm_pixel_read_performed"),
        "source_fixture_approval_satisfied": runtime_approval_request.get("source_fixture_approval_satisfied"),
        "source_requires_future_runtime_approval": runtime_approval_request.get(
            "source_requires_future_runtime_approval"
        ),
        "source_requires_future_fixture_approval": runtime_approval_request.get(
            "source_requires_future_fixture_approval"
        ),
        "source_requires_future_render_validation_approval": runtime_approval_request.get(
            "source_requires_future_render_validation_approval"
        ),
        "runtime_boundary_ready": runtime_approval_request.get("source_boundary_contract_ready"),
        "fixture_approval_satisfied": fixture_approval_satisfied,
        "ofx_host_binary_review_ready": False,
        "runtime_containment_selftest_ready": False,
        "schema_and_render_validation_ready": False,
        "path_acceptance_closed": path_acceptance_closed,
        "current_runtime_approval_evaluation": current_evaluation,
        "synthetic_runtime_approval_checks_passed": True,
        "synthetic_runtime_approval_checks": synthetic_checks,
        "forbidden_actions_after_runtime_approval": sorted(FORBIDDEN_APPROVED_ACTIONS),
        "blocked_actions_after_verification": sorted(FORBIDDEN_APPROVED_ACTIONS),
        "allowed_future_approval_actions": [PREPARE_RUNTIME_REVIEW_ACTION],
        "blocked_actions": sorted(FORBIDDEN_APPROVED_ACTIONS),
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
        "runtime_approval_verifier_path_payload_exported": False,
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
            "Treat this verifier as no-approval evidence only.",
            "Resolve fixture approval, host binary review, containment selftest, schema, and render evidence before any runtime approval can be valid.",
            "Regenerate the approval request and verifier after any boundary, path, or runtime evidence changes.",
        ],
        "notes": [
            "This verifier reads the runtime approval request JSON only.",
            "The current request packet is not converted into approval.",
            "A future approval manifest would only prepare reviewed runtime work, not invoke a runtime.",
            "Runtime invocation, host process launch, path acceptance, OFX describe/render, and AEX-backed routes remain closed.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Verify candidate OFX runtime approval evidence without runtime load")
    parser.add_argument(
        "--runtime-approval-request",
        required=True,
        help="Candidate OFX runtime approval request under target/candidate-ofx-runtime-approval-request",
    )
    parser.add_argument(
        "--runtime-boundary-contract",
        help="Optional candidate OFX runtime boundary contract under target/candidate-ofx-runtime-boundary-contract for cross-check",
    )
    parser.add_argument(
        "--out",
        required=True,
        help="Create-new verifier JSON under target/candidate-ofx-runtime-approval-verifier",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    request, request_path = load_runtime_approval_request(Path(args.runtime_approval_request))
    boundary = None
    boundary_path = None
    if args.runtime_boundary_contract:
        boundary, boundary_path = load_runtime_boundary_contract(Path(args.runtime_boundary_contract))
    report = build_runtime_approval_verifier(
        runtime_approval_request=request,
        runtime_approval_request_path=request_path,
        runtime_boundary_contract=boundary,
        runtime_boundary_contract_path=boundary_path,
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Build an OFX host binary review request without accepting host paths.

The request reads JSON evidence only. It turns the prerequisite audit's
`ofx_host_binary_review` blocker into a manual review checklist while keeping
host/plugin path acceptance, host process launch, runtime invocation, describe,
render, and AEX-backed routes closed.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
PREREQUISITE_AUDIT_ROOT = TARGET_ROOT / "candidate-ofx-runtime-prerequisite-audit"
HOST_HARNESS_DRYRUN_ROOT = TARGET_ROOT / "candidate-ofx-host-harness-dryrun"
HOST_HARNESS_SELFTEST_ROOT = TARGET_ROOT / "candidate-ofx-host-harness-selftest"
RUNTIME_BOUNDARY_ROOT = TARGET_ROOT / "candidate-ofx-runtime-boundary-contract"
HOST_BINARY_REVIEW_ROOT = TARGET_ROOT / "candidate-ofx-host-binary-review-request"

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
    "ofx_host_binary_opened",
    "ofx_host_binary_hashed",
    "ofx_host_binary_copied",
    "ofx_host_binary_executed",
    "ofx_plugin_binary_opened",
    "ofx_plugin_binary_hashed",
    "ofx_plugin_binary_copied",
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
    "open_ofx_host_binary",
    "hash_ofx_host_binary",
    "copy_ofx_host_binary",
    "execute_ofx_host_binary",
    "open_ofx_plugin_binary",
    "hash_ofx_plugin_binary",
    "copy_ofx_plugin_binary",
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
        raise ValueError("candidate OFX host binary review request must have .json extension")
    HOST_BINARY_REVIEW_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, HOST_BINARY_REVIEW_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(HOST_BINARY_REVIEW_ROOT.resolve(strict=True)):
        raise ValueError(f"candidate OFX host binary review request parent must stay under {HOST_BINARY_REVIEW_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_prerequisite_audit(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, PREREQUISITE_AUDIT_ROOT, "candidate OFX runtime prerequisite audit")
    return read_json_object(resolved), resolved


def load_host_harness_dryrun(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, HOST_HARNESS_DRYRUN_ROOT, "candidate OFX host harness dry-run")
    return read_json_object(resolved), resolved


def load_host_harness_selftest(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, HOST_HARNESS_SELFTEST_ROOT, "candidate OFX host harness selftest")
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


def validate_prerequisite_audit(audit: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if audit.get("publication_status") != "local-only":
        errors.append("prerequisite audit publication_status must be local-only")
    if audit.get("report_kind") != "aex_candidate_ofx_runtime_prerequisite_audit":
        errors.append("prerequisite audit report_kind must be aex_candidate_ofx_runtime_prerequisite_audit")
    if audit.get("runtime_prerequisite_audit_state") != "candidate_ofx_runtime_prerequisite_audit_ready_gates_closed":
        errors.append("prerequisite audit state must be ready gates closed")
    for key in (
        "runtime_prerequisite_audit_ready",
        "runtime_approval_verified_not_approved",
        "runtime_approval_gate_stays_closed",
        "runtime_containment_contract_ready",
        "runtime_containment_selftest_synthetic_passed",
        "parameter_schema_review_policy_ready",
        "render_validation_contract_ready",
        "ofx_route_contract_closed",
        "mock_route_ready",
        "path_acceptance_closed",
    ):
        if audit.get(key) is not True:
            errors.append(f"prerequisite audit {key} must be true")
    for key in (
        "runtime_invocation_prerequisites_ready",
        "approval_can_be_issued_now",
        "runtime_approval_satisfied",
        "explicit_runtime_approval_present",
        "fixture_approval_satisfied",
        "ofx_host_binary_review_ready",
        "runtime_containment_selftest_ready",
        "schema_and_render_validation_ready",
        "real_render_open",
        "real_route_open",
        "ofx_runtime_invoked",
        "host_process_launch_enabled",
        "path_acceptance_ready",
        "ppm_pixel_read_performed",
        "runtime_prerequisite_audit_path_payload_exported",
    ):
        if audit.get(key) is not False:
            errors.append(f"prerequisite audit {key} must be false")
    if int(audit.get("failed_evidence_count") or 0) != 0:
        errors.append("prerequisite audit failed_evidence_count must be 0")
    gaps = audit.get("prerequisite_gaps")
    if not isinstance(gaps, list):
        errors.append("prerequisite audit prerequisite_gaps must be a list")
    elif "ofx_host_binary_review" not in {item.get("id") for item in gaps if isinstance(item, dict)}:
        errors.append("prerequisite audit must include ofx_host_binary_review gap")
    errors.extend(safety_errors(audit, "prerequisite audit"))
    return errors


def validate_host_harness_dryrun(dryrun: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if dryrun.get("publication_status") != "local-only":
        errors.append("host harness dry-run publication_status must be local-only")
    if dryrun.get("report_kind") != "aex_candidate_ofx_host_harness_dryrun":
        errors.append("host harness dry-run report_kind must be aex_candidate_ofx_host_harness_dryrun")
    if dryrun.get("harness_dryrun_state") != "candidate_ofx_host_harness_dryrun_ready_route_closed":
        errors.append("host harness dry-run state must be ready route closed")
    for key in ("harness_dryrun_ready", "dry_run_only", "source_mock_route_ready", "requires_future_runtime_approval"):
        if dryrun.get(key) is not True:
            errors.append(f"host harness dry-run {key} must be true")
    for key in (
        "source_real_route_open",
        "source_ofx_runtime_invoked",
        "source_ofx_describe_ready",
        "source_ofx_render_ready",
        "host_harness_path_payload_exported",
        "ofx_runtime_invoked",
        "ofx_describe_performed",
        "ofx_render_performed",
    ):
        if dryrun.get(key) is not False:
            errors.append(f"host harness dry-run {key} must be false")
    if int(dryrun.get("planned_case_count") or 0) < 1:
        errors.append("host harness dry-run planned_case_count must be positive")
    errors.extend(safety_errors(dryrun, "host harness dry-run"))
    return errors


def validate_host_harness_selftest(selftest: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if selftest.get("publication_status") != "local-only":
        errors.append("host harness selftest publication_status must be local-only")
    if selftest.get("report_kind") != "aex_candidate_ofx_host_harness_selftest":
        errors.append("host harness selftest report_kind must be aex_candidate_ofx_host_harness_selftest")
    if selftest.get("host_harness_selftest_state") != "candidate_ofx_host_harness_selftest_passed_synthetic_route_closed":
        errors.append("host harness selftest state must be passed synthetic route closed")
    for key in (
        "host_harness_selftest_ready",
        "synthetic_only",
        "synthetic_contract_checks_performed",
        "descriptor_contract_checked",
        "render_identity_contract_checked",
        "requires_future_runtime_approval",
    ):
        if selftest.get(key) is not True:
            errors.append(f"host harness selftest {key} must be true")
    for key in (
        "ppm_pixel_read_performed",
        "ofx_runtime_invoked",
        "ofx_describe_performed",
        "ofx_render_performed",
        "host_harness_path_payload_exported",
    ):
        if selftest.get(key) is not False:
            errors.append(f"host harness selftest {key} must be false")
    if int(selftest.get("checked_case_count") or 0) < 1:
        errors.append("host harness selftest checked_case_count must be positive")
    errors.extend(safety_errors(selftest, "host harness selftest"))
    return errors


def validate_runtime_boundary(boundary: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if boundary.get("publication_status") != "local-only":
        errors.append("runtime boundary publication_status must be local-only")
    if boundary.get("report_kind") != "aex_candidate_ofx_runtime_boundary_contract":
        errors.append("runtime boundary report_kind must be aex_candidate_ofx_runtime_boundary_contract")
    if boundary.get("candidate_ofx_runtime_boundary_contract_state") != "candidate_ofx_runtime_boundary_contract_ready_no_runtime_route_closed":
        errors.append("runtime boundary state must be ready no-runtime route closed")
    if boundary.get("contract_state") != "candidate_ofx_runtime_boundary_contract_ready_runtime_closed":
        errors.append("runtime boundary contract_state must be ready runtime closed")
    for key in ("runtime_boundary_ready", "mock_route_ready", "requires_future_runtime_approval"):
        if boundary.get(key) is not True:
            errors.append(f"runtime boundary {key} must be true")
    for key in (
        "source_fixture_approval_satisfied",
        "ofx_runtime_allowed_now",
        "ofx_runtime_invocation_ready",
        "host_process_launch_enabled",
        "path_acceptance_ready",
        "real_route_open",
        "ofx_runtime_invoked",
        "ppm_pixel_read_performed",
        "ofx_host_path_payload_supplied",
        "ofx_plugin_binary_path_payload_supplied",
        "runtime_boundary_path_payload_exported",
    ):
        if boundary.get(key) is not False:
            errors.append(f"runtime boundary {key} must be false")
    errors.extend(safety_errors(boundary, "runtime boundary"))
    return errors


def build_review_checklist(
    *,
    audit: dict[str, Any],
    dryrun: dict[str, Any],
    selftest: dict[str, Any],
    boundary: dict[str, Any],
) -> list[dict[str, Any]]:
    runtime_approval_still_required = (
        audit.get("runtime_invocation_prerequisites_ready") is False
        and audit.get("approval_can_be_issued_now") is False
        and audit.get("runtime_approval_satisfied") is False
    )
    return [
        {
            "id": "host_binary_identity_required",
            "required": True,
            "satisfied": False,
            "current_evidence": "not_present",
            "next_action": "Record host/shim name, version, source, and role in a separate reviewed manifest.",
        },
        {
            "id": "host_binary_provenance_required",
            "required": True,
            "satisfied": False,
            "current_evidence": "not_present",
            "next_action": "Document where the host/shim binary or source came from before any path is accepted.",
        },
        {
            "id": "host_binary_license_required",
            "required": True,
            "satisfied": False,
            "current_evidence": "not_present",
            "next_action": "Confirm the host/shim license is compatible with local testing before runtime use.",
        },
        {
            "id": "host_binary_integrity_review_required",
            "required": True,
            "satisfied": False,
            "current_evidence": "not_present",
            "next_action": "Define hash/signature review policy in a separate artifact before any binary path is accepted.",
        },
        {
            "id": "host_shim_build_policy_required",
            "required": True,
            "satisfied": False,
            "current_evidence": "not_present",
            "next_action": "Review how the host/shim is built and isolated before launch becomes possible.",
        },
        {
            "id": "process_containment_required",
            "required": True,
            "satisfied": False,
            "current_evidence": {
                "runtime_containment_contract_ready": audit.get("runtime_containment_contract_ready"),
                "runtime_containment_selftest_synthetic_passed": audit.get(
                    "runtime_containment_selftest_synthetic_passed"
                ),
                "runtime_boundary_state": boundary.get("candidate_ofx_runtime_boundary_contract_state"),
                "host_harness_dryrun_state": dryrun.get("harness_dryrun_state"),
                "host_harness_selftest_state": selftest.get("host_harness_selftest_state"),
            },
            "next_action": "Promote synthetic containment evidence to a manual host launch containment review.",
        },
        {
            "id": "log_redaction_required",
            "required": True,
            "satisfied": False,
            "current_evidence": "not_present",
            "next_action": "Define how host/runtime logs are redacted before any process is launched.",
        },
        {
            "id": "runtime_approval_still_required",
            "required": True,
            "satisfied": runtime_approval_still_required,
            "current_evidence": {
                "runtime_invocation_prerequisites_ready": audit.get("runtime_invocation_prerequisites_ready"),
                "approval_can_be_issued_now": audit.get("approval_can_be_issued_now"),
                "runtime_approval_satisfied": audit.get("runtime_approval_satisfied"),
            },
            "next_action": "Keep runtime approval separate from host binary review; this request is not approval.",
        },
    ]


def review_blockers(checklist: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [
        {
            "id": item["id"],
            "current_evidence": item["current_evidence"],
            "next_action": item["next_action"],
        }
        for item in checklist
        if item.get("required") is True and item.get("satisfied") is not True
    ]


def build_host_binary_review_request(
    *,
    prerequisite_audit: dict[str, Any],
    prerequisite_audit_path: Path,
    host_harness_dryrun: dict[str, Any],
    host_harness_dryrun_path: Path,
    host_harness_selftest: dict[str, Any],
    host_harness_selftest_path: Path,
    runtime_boundary_contract: dict[str, Any],
    runtime_boundary_contract_path: Path,
) -> dict[str, Any]:
    errors = (
        validate_prerequisite_audit(prerequisite_audit)
        + validate_host_harness_dryrun(host_harness_dryrun)
        + validate_host_harness_selftest(host_harness_selftest)
        + validate_runtime_boundary(runtime_boundary_contract)
    )
    if errors:
        raise ValueError("; ".join(errors))

    checklist = build_review_checklist(
        audit=prerequisite_audit,
        dryrun=host_harness_dryrun,
        selftest=host_harness_selftest,
        boundary=runtime_boundary_contract,
    )
    blockers = review_blockers(checklist)
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_ofx_host_binary_review_request",
        "review_request_kind": "ofx_host_binary_provenance_manual_review_request",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_candidate_ofx_runtime_prerequisite_audit": relative_to_lab(prerequisite_audit_path),
        "source_candidate_ofx_host_harness_dryrun": relative_to_lab(host_harness_dryrun_path),
        "source_candidate_ofx_host_harness_selftest": relative_to_lab(host_harness_selftest_path),
        "source_candidate_ofx_runtime_boundary_contract": relative_to_lab(runtime_boundary_contract_path),
        "candidate_relative_path": prerequisite_audit.get("candidate_relative_path"),
        "host_binary_review_request_state": "candidate_ofx_host_binary_review_request_ready_pending_manual_review",
        "host_binary_review_request_ready": True,
        "host_binary_review_can_be_approved_now": False,
        "host_binary_review_ready": False,
        "host_binary_review_satisfied": False,
        "host_binary_review_manifest_created": False,
        "host_binary_review_gate_stays_closed": True,
        "ofx_host_binary_review_request_state": "candidate_ofx_host_binary_review_request_ready_pending_manual_review",
        "ofx_host_binary_review_request_ready": True,
        "ofx_host_binary_review_ready": False,
        "ofx_host_binary_review_satisfied": False,
        "ofx_host_binary_review_can_be_approved_now": False,
        "ofx_host_binary_review_manifest_created": False,
        "ofx_host_binary_review_gate_stays_closed": True,
        "requires_explicit_host_binary_review": True,
        "host_binary_review_request_created": True,
        "host_binary_path_acceptance_ready": False,
        "host_binary_path_payload_exported": False,
        "ofx_host_path_payload_supplied": False,
        "ofx_plugin_binary_path_payload_supplied": False,
        "host_process_launch_enabled": False,
        "runtime_invocation_prerequisites_ready": False,
        "source_prerequisite_audit_state": prerequisite_audit.get("runtime_prerequisite_audit_state"),
        "source_failed_evidence_count": prerequisite_audit.get("failed_evidence_count"),
        "source_blocking_prerequisite_count": prerequisite_audit.get("blocking_prerequisite_count"),
        "source_runtime_invocation_prerequisites_ready": prerequisite_audit.get(
            "runtime_invocation_prerequisites_ready"
        ),
        "source_ofx_host_binary_review_ready": prerequisite_audit.get("ofx_host_binary_review_ready"),
        "source_runtime_containment_selftest_synthetic_passed": prerequisite_audit.get(
            "runtime_containment_selftest_synthetic_passed"
        ),
        "source_schema_and_render_validation_ready": prerequisite_audit.get(
            "schema_and_render_validation_ready"
        ),
        "source_harness_dryrun_state": host_harness_dryrun.get("harness_dryrun_state"),
        "source_harness_dryrun_ready": host_harness_dryrun.get("harness_dryrun_ready"),
        "source_host_harness_selftest_state": host_harness_selftest.get("host_harness_selftest_state"),
        "source_host_harness_selftest_ready": host_harness_selftest.get("host_harness_selftest_ready"),
        "source_boundary_contract_state": runtime_boundary_contract.get(
            "candidate_ofx_runtime_boundary_contract_state"
        ),
        "source_boundary_contract_ready": runtime_boundary_contract.get("runtime_boundary_ready"),
        "source_boundary_host_process_launch_enabled": runtime_boundary_contract.get(
            "host_process_launch_enabled"
        ),
        "source_boundary_ofx_host_path_payload_supplied": runtime_boundary_contract.get(
            "ofx_host_path_payload_supplied"
        ),
        "source_boundary_ofx_plugin_binary_path_payload_supplied": runtime_boundary_contract.get(
            "ofx_plugin_binary_path_payload_supplied"
        ),
        "review_checklist": checklist,
        "review_checklist_count": len(checklist),
        "review_blockers": blockers,
        "review_blocker_count": len(blockers),
        "host_binary_review_blockers": blockers,
        "host_binary_review_blocker_count": len(blockers),
        "ready_no_load_evidence_count": 4,
        "source_no_load_evidence": [
            "candidate_ofx_runtime_prerequisite_audit",
            "candidate_ofx_host_harness_dryrun",
            "candidate_ofx_host_harness_selftest",
            "candidate_ofx_runtime_boundary_contract",
        ],
        "source_no_load_evidence_count": 4,
        "manual_review_required": True,
        "explicit_user_review_required": True,
        "approval_only_prepares_host_binary_review": True,
        "review_does_not_accept_paths": True,
        "review_does_not_launch_host": True,
        "review_does_not_invoke_runtime": True,
        "ofx_runtime_allowed_now": False,
        "ofx_runtime_invocation_ready": False,
        "ofx_runtime_instantiation_ready": False,
        "ofx_runtime_instantiation_performed": False,
        "path_acceptance_ready": False,
        "aex_path_acceptance_enabled": False,
        "accepted_aex_path": None,
        "accepted_ofx_host_path": None,
        "accepted_ofx_plugin_binary_path": None,
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
        "ofx_host_binary_review_path_payload_exported": False,
        "host_binary_review_path_payload_exported": False,
        "blocked_actions_after_request": list(BLOCKED_ACTIONS),
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
        "ofx_host_binary_opened": False,
        "ofx_host_binary_hashed": False,
        "ofx_host_binary_copied": False,
        "ofx_host_binary_executed": False,
        "ofx_plugin_binary_opened": False,
        "ofx_plugin_binary_hashed": False,
        "ofx_plugin_binary_copied": False,
        "next_required_actions": [
            "Collect host/shim identity, provenance, license, integrity, and containment answers without storing paths here.",
            "Keep host/plugin path acceptance and host process launch closed until a separate reviewed manifest exists.",
            "Regenerate prerequisite audit and this request after any runtime boundary or host harness evidence changes.",
        ],
        "notes": [
            "This request reads JSON evidence only.",
            "It creates no host binary approval manifest.",
            "No OFX host path, OFX plugin path, AEX path, PPM path, DLL, AE, render, or runtime route is accepted.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build candidate OFX host binary review request without host paths")
    parser.add_argument(
        "--prerequisite-audit",
        required=True,
        help="Candidate OFX runtime prerequisite audit under target/candidate-ofx-runtime-prerequisite-audit",
    )
    parser.add_argument(
        "--host-harness-dryrun",
        required=True,
        help="Candidate OFX host harness dry-run under target/candidate-ofx-host-harness-dryrun",
    )
    parser.add_argument(
        "--host-harness-selftest",
        required=True,
        help="Candidate OFX host harness selftest under target/candidate-ofx-host-harness-selftest",
    )
    parser.add_argument(
        "--runtime-boundary-contract",
        required=True,
        help="Candidate OFX runtime boundary contract under target/candidate-ofx-runtime-boundary-contract",
    )
    parser.add_argument(
        "--out",
        required=True,
        help="Create-new review request JSON under target/candidate-ofx-host-binary-review-request",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    audit, audit_path = load_prerequisite_audit(Path(args.prerequisite_audit))
    dryrun, dryrun_path = load_host_harness_dryrun(Path(args.host_harness_dryrun))
    selftest, selftest_path = load_host_harness_selftest(Path(args.host_harness_selftest))
    boundary, boundary_path = load_runtime_boundary_contract(Path(args.runtime_boundary_contract))
    request = build_host_binary_review_request(
        prerequisite_audit=audit,
        prerequisite_audit_path=audit_path,
        host_harness_dryrun=dryrun,
        host_harness_dryrun_path=dryrun_path,
        host_harness_selftest=selftest,
        host_harness_selftest_path=selftest_path,
        runtime_boundary_contract=boundary,
        runtime_boundary_contract_path=boundary_path,
    )
    written = write_json_create_new(Path(args.out), request)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

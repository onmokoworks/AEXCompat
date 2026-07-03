#!/usr/bin/env python3
"""Build a candidate OFX runtime approval request packet without approval.

The packet consumes the candidate OFX runtime boundary contract JSON only. It
does not create approval, instantiate an OFX runtime, launch a host process,
accept path payloads, open AEX files, read PPM pixels, describe, render, or
route pixels.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
RUNTIME_BOUNDARY_ROOT = TARGET_ROOT / "candidate-ofx-runtime-boundary-contract"
RUNTIME_APPROVAL_REQUEST_ROOT = TARGET_ROOT / "candidate-ofx-runtime-approval-request"

APPROVAL_TOKEN_NAME = "APPROVE_OFX_RUNTIME_INVOCATION"

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
        raise ValueError("candidate OFX runtime approval request must have .json extension")
    RUNTIME_APPROVAL_REQUEST_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, RUNTIME_APPROVAL_REQUEST_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(RUNTIME_APPROVAL_REQUEST_ROOT.resolve(strict=True)):
        raise ValueError(f"candidate OFX runtime approval request parent must stay under {RUNTIME_APPROVAL_REQUEST_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


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


def validate_runtime_boundary(boundary: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if boundary.get("publication_status") != "local-only":
        errors.append("runtime boundary publication_status must be local-only")
    if boundary.get("report_kind") != "aex_candidate_ofx_runtime_boundary_contract":
        errors.append("runtime boundary report_kind must be aex_candidate_ofx_runtime_boundary_contract")
    if (
        boundary.get("candidate_ofx_runtime_boundary_contract_state")
        != "candidate_ofx_runtime_boundary_contract_ready_no_runtime_route_closed"
    ):
        errors.append("runtime boundary contract state must be ready no-runtime route closed")
    if boundary.get("contract_state") != "candidate_ofx_runtime_boundary_contract_ready_runtime_closed":
        errors.append("runtime boundary contract_state must be ready runtime closed")
    if boundary.get("runtime_boundary_ready") is not True:
        errors.append("runtime_boundary_ready must be true")
    if boundary.get("boundary_contract_ready") is not True:
        errors.append("boundary_contract_ready must be true")
    if boundary.get("source_bridge_state") != "candidate_ofx_bridge_ready_no_load_route_closed":
        errors.append("source_bridge_state must be candidate_ofx_bridge_ready_no_load_route_closed")
    if boundary.get("source_harness_dryrun_state") != "candidate_ofx_host_harness_dryrun_ready_route_closed":
        errors.append("source_harness_dryrun_state must be candidate_ofx_host_harness_dryrun_ready_route_closed")
    if (
        boundary.get("source_host_harness_selftest_state")
        != "candidate_ofx_host_harness_selftest_passed_synthetic_route_closed"
    ):
        errors.append("source_host_harness_selftest_state must be passed synthetic route closed")
    if boundary.get("source_ofx_route_contract_state") != "ofx_route_contract_ready_route_closed":
        errors.append("source_ofx_route_contract_state must be ofx_route_contract_ready_route_closed")
    if boundary.get("source_native_runtime_contract_state") != "runtime_containment_contract_ready_no_load":
        errors.append("source_native_runtime_contract_state must be runtime_containment_contract_ready_no_load")
    if boundary.get("source_fixture_approval_satisfied") is not False:
        errors.append("source fixture approval must remain unsatisfied")
    if boundary.get("no_load_boundary_contract_created") is not True:
        errors.append("no_load_boundary_contract_created must be true")
    if int(boundary.get("approval_gate_count") or 0) < 1:
        errors.append("approval_gate_count must be positive")
    if int(boundary.get("required_runtime_evidence_count") or 0) < 1:
        errors.append("required_runtime_evidence_count must be positive")
    for key in (
        "ofx_runtime_allowed_now",
        "ofx_runtime_invocation_ready",
        "ofx_runtime_instantiation_ready",
        "ofx_runtime_instantiation_performed",
        "ofx_binary_build_allowed_now",
        "ofx_binary_built",
        "real_ofx_describe_allowed_now",
        "real_ofx_render_allowed_now",
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
        "runtime_boundary_path_payload_exported",
        "ofx_host_path_payload_supplied",
        "ofx_plugin_binary_path_payload_supplied",
        "path_acceptance_ready",
        "aex_path_acceptance_enabled",
        "host_process_launch_enabled",
    ):
        if boundary.get(key) is not False:
            errors.append(f"runtime boundary {key} must be false")
    if boundary.get("accepted_aex_path") is not None:
        errors.append("runtime boundary accepted_aex_path must be null")
    for key in (
        "requires_future_runtime_approval",
        "runtime_approval_required_before_invocation",
        "requires_future_fixture_approval",
        "requires_future_render_validation_approval",
    ):
        if boundary.get(key) is not True:
            errors.append(f"runtime boundary {key} must be true")
    required = boundary.get("required_before_runtime_invocation")
    if not isinstance(required, list) or not required:
        errors.append("required_before_runtime_invocation must be a non-empty list")
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


def build_review_checklist(boundary: dict[str, Any]) -> list[dict[str, Any]]:
    return [
        {
            "id": "explicit_runtime_approval",
            "required": True,
            "satisfied": False,
            "current_evidence": "not_present",
            "next_action": f"Only create runtime approval after the user provides {APPROVAL_TOKEN_NAME}.",
        },
        {
            "id": "fixture_approval",
            "required": True,
            "satisfied": boundary.get("source_fixture_approval_satisfied") is True,
            "current_evidence": boundary.get("source_fixture_approval_satisfied"),
            "next_action": "Resolve fixture provenance/license/manual approval before any AEX path is accepted.",
        },
        {
            "id": "runtime_boundary_contract",
            "required": True,
            "satisfied": boundary.get("runtime_boundary_ready") is True,
            "current_evidence": boundary.get("candidate_ofx_runtime_boundary_contract_state"),
            "next_action": "Keep this contract current whenever route/runtime evidence changes.",
        },
        {
            "id": "ofx_host_binary_provenance",
            "required": True,
            "satisfied": False,
            "current_evidence": "not_present",
            "next_action": "Review OFX host/shim provenance before accepting any host process path.",
        },
        {
            "id": "runtime_containment_selftest",
            "required": True,
            "satisfied": False,
            "current_evidence": "not_present",
            "next_action": "Add reviewed timeout, crash cleanup, and local-only log selftests before runtime launch.",
        },
        {
            "id": "schema_and_render_validation",
            "required": True,
            "satisfied": False,
            "current_evidence": "not_present",
            "next_action": "Review parameter schema and render validation evidence before real describe/render.",
        },
    ]


def approval_blockers(checklist: list[dict[str, Any]]) -> list[dict[str, Any]]:
    blockers: list[dict[str, Any]] = []
    for item in checklist:
        if item.get("required") is True and item.get("satisfied") is not True:
            blockers.append(
                {
                    "id": item.get("id"),
                    "current_evidence": item.get("current_evidence"),
                    "next_action": item.get("next_action"),
                }
            )
    return blockers


def build_runtime_approval_request_packet(
    *,
    runtime_boundary_contract: dict[str, Any],
    runtime_boundary_contract_path: Path,
) -> dict[str, Any]:
    errors = validate_runtime_boundary(runtime_boundary_contract)
    if errors:
        raise ValueError("; ".join(errors))

    checklist = build_review_checklist(runtime_boundary_contract)
    blockers = approval_blockers(checklist)
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_ofx_runtime_approval_request_packet",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_candidate_ofx_runtime_boundary_contract": relative_to_lab(runtime_boundary_contract_path),
        "candidate_relative_path": runtime_boundary_contract.get("candidate_relative_path"),
        "runtime_approval_request_state": "candidate_ofx_runtime_approval_request_ready_pending_manual_approval",
        "runtime_approval_request_ready": True,
        "runtime_approval_request_created": True,
        "runtime_approval_can_be_issued_now": False,
        "runtime_approval_manifest_created": False,
        "runtime_approval_gate_stays_closed": True,
        "approval_request_kind": "ofx_runtime_invocation_manual_approval_request",
        "requires_explicit_user_approval": True,
        "required_approval_token_name": APPROVAL_TOKEN_NAME,
        "approval_token_not_stored_in_manifest": True,
        "approval_only_prepares_runtime_review": True,
        "source_boundary_contract_state": runtime_boundary_contract.get(
            "candidate_ofx_runtime_boundary_contract_state"
        ),
        "source_boundary_contract_ready": runtime_boundary_contract.get("runtime_boundary_ready"),
        "source_contract_state": runtime_boundary_contract.get("contract_state"),
        "source_ofx_runtime_allowed_now": runtime_boundary_contract.get("ofx_runtime_allowed_now"),
        "source_ofx_runtime_invocation_ready": runtime_boundary_contract.get("ofx_runtime_invocation_ready"),
        "source_host_process_launch_enabled": runtime_boundary_contract.get("host_process_launch_enabled"),
        "source_path_acceptance_ready": runtime_boundary_contract.get("path_acceptance_ready"),
        "source_real_route_open": runtime_boundary_contract.get("real_route_open"),
        "source_mock_route_ready": runtime_boundary_contract.get("mock_route_ready"),
        "source_ofx_runtime_invoked": runtime_boundary_contract.get("ofx_runtime_invoked"),
        "source_ppm_pixel_read_performed": runtime_boundary_contract.get("ppm_pixel_read_performed"),
        "source_fixture_approval_satisfied": runtime_boundary_contract.get("source_fixture_approval_satisfied"),
        "source_requires_future_runtime_approval": runtime_boundary_contract.get("requires_future_runtime_approval"),
        "source_requires_future_fixture_approval": runtime_boundary_contract.get("requires_future_fixture_approval"),
        "source_requires_future_render_validation_approval": runtime_boundary_contract.get(
            "requires_future_render_validation_approval"
        ),
        "review_checklist": checklist,
        "review_checklist_count": len(checklist),
        "approval_blockers": blockers,
        "approval_blocker_count": len(blockers),
        "required_before_runtime_invocation": runtime_boundary_contract.get("required_before_runtime_invocation", []),
        "blocked_actions_after_request": list(BLOCKED_ACTIONS),
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
        "blocked_actions": list(BLOCKED_ACTIONS),
        "next_required_actions": [
            "Collect explicit user approval separately; this packet stores no approval token.",
            "Keep OFX runtime invocation closed until fixture, host binary, containment, schema, and render evidence are reviewed.",
            "Regenerate the boundary contract and this request after any runtime/path/render evidence changes.",
        ],
        "notes": [
            "This packet requests review only and creates no approval manifest.",
            "It reads boundary JSON evidence only and accepts no AEX/OFX/PPM path payload.",
            "Runtime invocation, host process launch, real describe/render, native loading, and AE startup remain closed.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build candidate OFX runtime approval request packet")
    parser.add_argument(
        "--runtime-boundary-contract",
        required=True,
        help="Candidate OFX runtime boundary contract under target/candidate-ofx-runtime-boundary-contract",
    )
    parser.add_argument(
        "--out",
        required=True,
        help="Create-new approval request JSON under target/candidate-ofx-runtime-approval-request",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    boundary, boundary_path = load_runtime_boundary_contract(Path(args.runtime_boundary_contract))
    packet = build_runtime_approval_request_packet(
        runtime_boundary_contract=boundary,
        runtime_boundary_contract_path=boundary_path,
    )
    written = write_json_create_new(Path(args.out), packet)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

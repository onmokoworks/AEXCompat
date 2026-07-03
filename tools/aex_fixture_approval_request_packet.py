#!/usr/bin/env python3
"""Build a fixture approval request packet without creating approval.

The request packet is the human-facing bridge between the no-load approval
verifier and any future explicit approval manifest. It reads JSON evidence only
and keeps the native AEX load gate closed.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
APPROVAL_VERIFIER_ROOT = TARGET_ROOT / "fixture-approval-verifier"
FIXTURE_MANUAL_REVIEW_ROOT = TARGET_ROOT / "fixture-manual-review"
APPROVAL_REQUEST_ROOT = TARGET_ROOT / "fixture-approval-request"

APPROVAL_TOKEN_NAME = "APPROVE_AEX_LOAD_GATE"
SAFETY_FLAGS = (
    "native_load_enabled",
    "native_load_performed",
    "dll_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
    "aex_file_opened",
)
DEFAULT_FORBIDDEN_AFTER_APPROVAL = (
    "accept_aex_path",
    "open_aex_file",
    "hash_aex_file",
    "copy_selected_aex_fixture",
    "load_aex_dll",
    "load_dependency_dll",
    "call_EffectMain",
    "dispatch_PF_Cmd",
    "render_with_aex",
    "route_through_ofx",
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


def validate_json_input(path: Path, root: Path, label: str) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError(f"{label} must have .json extension")
    return resolve_under_root(path, root, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("fixture approval request packet must have .json extension")
    APPROVAL_REQUEST_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, APPROVAL_REQUEST_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(APPROVAL_REQUEST_ROOT.resolve(strict=True)):
        raise ValueError(f"fixture approval request parent must stay under {APPROVAL_REQUEST_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_approval_verifier(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, APPROVAL_VERIFIER_ROOT, "fixture approval verifier")
    return read_json_object(resolved), resolved


def load_fixture_manual_review(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, FIXTURE_MANUAL_REVIEW_ROOT, "fixture manual-review packet")
    return read_json_object(resolved), resolved


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def future_valid_shape_only_prepares_next_gate(verifier: dict[str, Any]) -> bool:
    checks = verifier.get("synthetic_approval_checks")
    if not isinstance(checks, list):
        return False
    for check in checks:
        if not isinstance(check, dict):
            continue
        if check.get("case") != "future_valid_shape_only_prepares_next_gate":
            continue
        return check.get("valid") is True and check.get("approval_only_prepares_next_gate") is True
    return False


def validate_approval_verifier(verifier: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if verifier.get("publication_status") != "local-only":
        errors.append("fixture approval verifier publication_status must be local-only")
    if verifier.get("report_kind") != "aex_fixture_approval_verifier":
        errors.append("fixture approval verifier report_kind must be aex_fixture_approval_verifier")
    if verifier.get("approval_verifier_state") != "fixture_approval_verifier_ready_no_approval":
        errors.append("fixture approval verifier must be ready no-approval")
    if verifier.get("approval_verifier_ready") is not True:
        errors.append("fixture approval verifier approval_verifier_ready must be true")
    if verifier.get("current_fixture_approval_valid") is not False:
        errors.append("current fixture approval must be invalid before request packet")
    if verifier.get("fixture_approval_satisfied") is not False:
        errors.append("fixture approval must remain unsatisfied before request packet")
    if verifier.get("approval_gate_stays_closed") is not True:
        errors.append("approval gate must stay closed")
    if verifier.get("manual_review_ready") is not True:
        errors.append("manual review must be ready")
    if verifier.get("manual_review_approval_ready") is not False:
        errors.append("manual review approval_ready must be false")
    if verifier.get("candidate_dependencies_clear") is not True:
        errors.append("candidate dependencies must be clear before request packet")
    if verifier.get("path_policy_closed") is not True:
        errors.append("path policy must remain closed")
    if verifier.get("candidate_load_gate_closed") is not True:
        errors.append("candidate load gate must remain closed")
    if verifier.get("synthetic_approval_checks_passed") is not True:
        errors.append("synthetic approval checks must pass")
    if verifier.get("required_approval_token_name") != APPROVAL_TOKEN_NAME:
        errors.append("approval token name mismatch")
    if verifier.get("approval_token_not_stored_in_manifest") is not True:
        errors.append("approval token must not be stored in manifest")
    if verifier.get("approval_only_prepares_next_gate") is not True:
        errors.append("approval must only prepare the next gate")
    if not future_valid_shape_only_prepares_next_gate(verifier):
        errors.append("future valid approval shape must only prepare the next gate")
    errors.extend(safety_errors(verifier, "fixture approval verifier"))
    return errors


def validate_manual_review(packet: dict[str, Any], candidate_relative_path: str | None) -> list[str]:
    errors: list[str] = []
    if packet.get("publication_status") != "local-only":
        errors.append("fixture manual-review publication_status must be local-only")
    if packet.get("report_kind") != "aex_fixture_manual_review_packet":
        errors.append("fixture manual-review report_kind must be aex_fixture_manual_review_packet")
    if packet.get("review_packet_state") != "fixture_manual_review_packet_ready_no_load":
        errors.append("fixture manual-review must be ready no-load")
    if packet.get("manual_review_ready") is not True:
        errors.append("fixture manual-review manual_review_ready must be true")
    if packet.get("approval_ready") is not False:
        errors.append("fixture manual-review approval_ready must be false")
    if packet.get("candidate_relative_path") != candidate_relative_path:
        errors.append("fixture manual-review candidate_relative_path must match verifier")
    if int(packet.get("approval_blocker_count") or 0) <= 0:
        errors.append("fixture manual-review must still expose approval blockers")
    errors.extend(safety_errors(packet, "fixture manual-review"))
    return errors


def approval_blockers(packet: dict[str, Any]) -> list[dict[str, Any]]:
    blockers = packet.get("approval_blockers")
    if not isinstance(blockers, list):
        return []
    return [item for item in blockers if isinstance(item, dict)]


def synthetic_checks_summary(verifier: dict[str, Any]) -> dict[str, Any]:
    checks = verifier.get("synthetic_approval_checks")
    if not isinstance(checks, list):
        checks = []
    valid_cases = [
        item.get("case")
        for item in checks
        if isinstance(item, dict) and item.get("valid") is True and isinstance(item.get("case"), str)
    ]
    rejected_cases = [
        item.get("case")
        for item in checks
        if isinstance(item, dict) and item.get("valid") is False and isinstance(item.get("case"), str)
    ]
    return {
        "check_count": len(checks),
        "valid_cases": valid_cases,
        "rejected_cases": rejected_cases,
        "future_valid_shape_only_prepares_next_gate": future_valid_shape_only_prepares_next_gate(verifier),
    }


def build_review_checklist(verifier: dict[str, Any], manual_review: dict[str, Any]) -> list[dict[str, Any]]:
    return [
        {
            "id": "manual_fixture_review",
            "required": True,
            "satisfied": manual_review.get("approval_ready") is True,
            "current_evidence": manual_review.get("recommended_next_decision"),
            "next_action": "Resolve provenance, license, dependency, and safety blockers before approval.",
        },
        {
            "id": "explicit_user_approval",
            "required": True,
            "satisfied": False,
            "current_evidence": "not_present",
            "next_action": f"Only create an approval manifest after the user provides {APPROVAL_TOKEN_NAME}.",
        },
        {
            "id": "approval_manifest_shape",
            "required": True,
            "satisfied": False,
            "current_evidence": verifier.get("approval_manifest_kind"),
            "next_action": "Future approval must be a local-only approval manifest with prepare_native_load_gate only.",
        },
        {
            "id": "candidate_dependency_scope",
            "required": True,
            "satisfied": verifier.get("candidate_dependencies_clear") is True,
            "current_evidence": "candidate_dependencies_clear",
            "next_action": "Keep global/default-deny dependency blockers out of the native gate until reviewed.",
        },
        {
            "id": "closed_path_policy",
            "required": True,
            "satisfied": verifier.get("path_policy_closed") is True,
            "current_evidence": "path_policy_closed",
            "next_action": "Do not accept or serialize real AEX paths before a separate path-acceptance approval.",
        },
        {
            "id": "candidate_load_gate_dryrun",
            "required": True,
            "satisfied": verifier.get("candidate_load_gate_closed") is True,
            "current_evidence": verifier.get("source_candidate_load_gate_state"),
            "next_action": "Rerun the dry-run gate after approval evidence changes; do not perform native load.",
        },
    ]


def build_fixture_approval_request_packet(
    *,
    approval_verifier: dict[str, Any],
    approval_verifier_path: Path,
    fixture_manual_review: dict[str, Any],
    fixture_manual_review_path: Path,
) -> dict[str, Any]:
    candidate_relative_path = approval_verifier.get("candidate_relative_path")
    if not isinstance(candidate_relative_path, str):
        candidate_relative_path = None
    errors = validate_approval_verifier(approval_verifier) + validate_manual_review(
        fixture_manual_review, candidate_relative_path
    )
    if errors:
        raise ValueError("; ".join(errors))

    blockers = approval_blockers(fixture_manual_review)
    forbidden_actions = approval_verifier.get("blocked_actions")
    if not isinstance(forbidden_actions, list):
        forbidden_actions = list(DEFAULT_FORBIDDEN_AFTER_APPROVAL)
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_fixture_approval_request_packet",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_approval_verifier": str(approval_verifier_path),
        "source_fixture_manual_review": str(fixture_manual_review_path),
        "approval_request_state": "fixture_approval_request_ready_pending_manual_approval",
        "approval_request_ready": True,
        "approval_can_be_issued_now": False,
        "approval_manifest_created": False,
        "requires_explicit_user_approval": True,
        "required_approval_token_name": APPROVAL_TOKEN_NAME,
        "approval_token_not_stored_in_manifest": True,
        "approval_only_prepares_next_gate": True,
        "candidate_relative_path": candidate_relative_path,
        "approval_manifest_kind": approval_verifier.get("approval_manifest_kind"),
        "decision_state": approval_verifier.get("decision_state"),
        "approval_state": approval_verifier.get("approval_state"),
        "current_fixture_approval_valid": False,
        "fixture_approval_satisfied": False,
        "manual_review_ready": fixture_manual_review.get("manual_review_ready"),
        "manual_review_approval_ready": False,
        "approval_blocker_count": fixture_manual_review.get("approval_blocker_count"),
        "approval_blockers": blockers,
        "approval_gate_stays_closed": True,
        "native_load_gate": "closed",
        "candidate_dependencies_clear": approval_verifier.get("candidate_dependencies_clear"),
        "path_policy_closed": approval_verifier.get("path_policy_closed"),
        "candidate_load_gate_closed": approval_verifier.get("candidate_load_gate_closed"),
        "current_approval_evaluation": approval_verifier.get("current_approval_evaluation"),
        "synthetic_approval_checks_summary": synthetic_checks_summary(approval_verifier),
        "review_checklist": build_review_checklist(approval_verifier, fixture_manual_review),
        "request_sections": [
            "current_hold_status",
            "manual_review_blockers",
            "approval_manifest_requirements",
            "actions_still_forbidden_after_approval",
            "next_no_load_gate",
        ],
        "forbidden_actions_after_approval": sorted(str(action) for action in forbidden_actions),
        "allowed_actions_after_approval": [
            "prepare_native_load_gate",
            "rerun_candidate_load_gate_dryrun",
            "review_path_allowlist_without_accepting_real_aex_path",
        ],
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "notes": [
            "This request packet reads JSON evidence only.",
            "It does not create or store an approval token or approval manifest.",
            "It does not accept, open, hash, copy, load, or execute AEX files.",
            "Even after explicit approval, native load remains behind a separate no-load gate and path policy review.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build fixture approval request packet without native load")
    parser.add_argument(
        "--approval-verifier",
        required=True,
        help="Fixture approval verifier JSON under target/fixture-approval-verifier",
    )
    parser.add_argument(
        "--fixture-manual-review",
        required=True,
        help="Fixture manual-review packet JSON under target/fixture-manual-review",
    )
    parser.add_argument("--out", required=True, help="Create-new packet under target/fixture-approval-request")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    verifier, verifier_path = load_approval_verifier(Path(args.approval_verifier))
    manual_review, manual_review_path = load_fixture_manual_review(Path(args.fixture_manual_review))
    packet = build_fixture_approval_request_packet(
        approval_verifier=verifier,
        approval_verifier_path=verifier_path,
        fixture_manual_review=manual_review,
        fixture_manual_review_path=manual_review_path,
    )
    written = write_json_create_new(Path(args.out), packet)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

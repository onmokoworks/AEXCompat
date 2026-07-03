#!/usr/bin/env python3
"""Build a no-approval fixture provenance/license/safety review packet.

The packet reads fixture manual-review and approval-request JSON only. It does
not open, hash, copy, load, or execute AEX files, and it does not create
approval manifests.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
FIXTURE_MANUAL_REVIEW_ROOT = TARGET_ROOT / "fixture-manual-review"
FIXTURE_APPROVAL_REQUEST_ROOT = TARGET_ROOT / "fixture-approval-request"
PROVENANCE_REVIEW_ROOT = TARGET_ROOT / "fixture-provenance-review"

SAFETY_FLAGS = (
    "native_load_enabled",
    "native_load_performed",
    "dll_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
    "aex_file_opened",
    "aex_file_hashed",
    "aex_file_copied",
)

FORBIDDEN_APPROVAL_KEYS = (
    "manifest_kind",
    "approved_actions",
    "approval_token_value",
    "approval_token",
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
        raise ValueError("fixture provenance review packet must have .json extension")
    PROVENANCE_REVIEW_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, PROVENANCE_REVIEW_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(PROVENANCE_REVIEW_ROOT.resolve(strict=True)):
        raise ValueError(f"fixture provenance review parent must stay under {PROVENANCE_REVIEW_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_fixture_manual_review(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, FIXTURE_MANUAL_REVIEW_ROOT, "fixture manual-review")
    return read_json_object(resolved), resolved


def load_approval_request(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, FIXTURE_APPROVAL_REQUEST_ROOT, "fixture approval request")
    return read_json_object(resolved), resolved


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    for key in FORBIDDEN_APPROVAL_KEYS:
        if key in payload:
            errors.append(f"{label} must not contain {key}")
    return errors


def validate_manual_review(packet: dict[str, Any]) -> list[str]:
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
    if not isinstance(packet.get("candidate_relative_path"), str):
        errors.append("fixture manual-review candidate_relative_path must be a string")
    if not isinstance(packet.get("approval_blockers"), list):
        errors.append("fixture manual-review approval_blockers must be a list")
    if int(packet.get("approval_blocker_count") or 0) <= 0:
        errors.append("fixture manual-review approval_blocker_count must be positive")
    errors.extend(safety_errors(packet, "fixture manual-review"))
    return errors


def validate_approval_request(packet: dict[str, Any], candidate_relative_path: str | None) -> list[str]:
    errors: list[str] = []
    if packet.get("publication_status") != "local-only":
        errors.append("fixture approval request publication_status must be local-only")
    if packet.get("report_kind") != "aex_fixture_approval_request_packet":
        errors.append("fixture approval request report_kind must be aex_fixture_approval_request_packet")
    if packet.get("approval_request_state") != "fixture_approval_request_ready_pending_manual_approval":
        errors.append("fixture approval request must be pending manual approval")
    if packet.get("approval_request_ready") is not True:
        errors.append("fixture approval request approval_request_ready must be true")
    if packet.get("approval_can_be_issued_now") is not False:
        errors.append("fixture approval request approval_can_be_issued_now must be false")
    if packet.get("approval_manifest_created") is not False:
        errors.append("fixture approval request approval_manifest_created must be false")
    if packet.get("requires_explicit_user_approval") is not True:
        errors.append("fixture approval request requires_explicit_user_approval must be true")
    if packet.get("current_fixture_approval_valid") is not False:
        errors.append("fixture approval request current_fixture_approval_valid must be false")
    if packet.get("fixture_approval_satisfied") is not False:
        errors.append("fixture approval request fixture_approval_satisfied must be false")
    if packet.get("approval_gate_stays_closed") is not True:
        errors.append("fixture approval request approval_gate_stays_closed must be true")
    if packet.get("native_load_gate") != "closed":
        errors.append("fixture approval request native_load_gate must be closed")
    if packet.get("required_approval_token_name") != "APPROVE_AEX_LOAD_GATE":
        errors.append("fixture approval request required_approval_token_name must be APPROVE_AEX_LOAD_GATE")
    if packet.get("approval_token_not_stored_in_manifest") is not True:
        errors.append("fixture approval request approval_token_not_stored_in_manifest must be true")
    if packet.get("approval_only_prepares_next_gate") is not True:
        errors.append("fixture approval request approval_only_prepares_next_gate must be true")
    if packet.get("candidate_relative_path") != candidate_relative_path:
        errors.append("fixture approval request candidate_relative_path must match manual review")
    if int(packet.get("approval_blocker_count") or 0) <= 0:
        errors.append("fixture approval request approval_blocker_count must be positive")
    errors.extend(safety_errors(packet, "fixture approval request"))
    return errors


def blocker_ids(items: Any) -> list[str]:
    if not isinstance(items, list):
        return []
    result = []
    for item in items:
        if isinstance(item, dict) and isinstance(item.get("id"), str):
            result.append(item["id"])
    return sorted(set(result))


def review_question(question_id: str, subject: str, status: str, source: str, required_evidence: str) -> dict[str, Any]:
    return {
        "question_id": question_id,
        "subject": subject,
        "status": status,
        "source": source,
        "required_evidence": required_evidence,
        "approval_effect": "does_not_approve_native_load",
    }


def build_review_questions(manual_review: dict[str, Any], approval_request: dict[str, Any]) -> list[dict[str, Any]]:
    questions = [
        review_question(
            "provenance_confirmed",
            "Candidate provenance is known and acceptable for local fixture use",
            "requires_user_review",
            "manual_review",
            "User-confirmed source/provenance note for the selected AEX.",
        ),
        review_question(
            "license_scope_confirmed",
            "License scope is acceptable for local-only testing and future publication boundary",
            "requires_user_review",
            "manual_review",
            "User-confirmed license/provenance note; publication remains closed separately.",
        ),
        review_question(
            "safety_scope_confirmed",
            "Static no-load safety evidence is understood before any native gate changes",
            "requires_user_review",
            "manual_review",
            "Manual acknowledgement of dependency, load-gate, and runtime containment blockers.",
        ),
        review_question(
            "explicit_approval_absent",
            "Explicit approval token is absent",
            "blocked_until_explicit_user_approval",
            "approval_request",
            str(approval_request.get("required_approval_token_name") or "APPROVE_AEX_LOAD_GATE"),
        ),
    ]
    for raw in manual_review.get("manual_review_questions", []):
        if isinstance(raw, str):
            questions.append(
                review_question(
                    f"manual_question_{len(questions):02d}",
                    raw,
                    "requires_user_review",
                    "fixture_manual_review_packet",
                    "User-authored answer or documented hold/reject decision.",
                )
            )
    return questions


def build_fixture_provenance_review_packet(
    *,
    fixture_manual_review: dict[str, Any],
    fixture_manual_review_path: Path,
    approval_request: dict[str, Any],
    approval_request_path: Path,
) -> dict[str, Any]:
    candidate_relative_path = fixture_manual_review.get("candidate_relative_path")
    if not isinstance(candidate_relative_path, str):
        candidate_relative_path = None
    errors = validate_manual_review(fixture_manual_review) + validate_approval_request(
        approval_request,
        candidate_relative_path,
    )
    if errors:
        raise ValueError("; ".join(errors))

    manual_blockers = blocker_ids(fixture_manual_review.get("approval_blockers"))
    request_blockers = blocker_ids(approval_request.get("approval_blockers"))
    questions = build_review_questions(fixture_manual_review, approval_request)
    blockers = [
        "provenance_not_confirmed",
        "license_scope_not_confirmed",
        "explicit_user_approval_missing",
        "approval_blockers_present",
        "native_load_gate_closed",
        "publication_review_not_complete",
    ]
    review_items = [
        {
            "item_id": "manual_review_source_ready",
            "status": "satisfied_no_load",
            "source": "fixture_manual_review",
            "evidence": "manual_review_ready true and approval_ready false",
        },
        {
            "item_id": "approval_request_source_ready",
            "status": "satisfied_no_load",
            "source": "fixture_approval_request",
            "evidence": "approval request ready but cannot be issued now",
        },
        {
            "item_id": "candidate_identity_consistent",
            "status": "satisfied_metadata_only",
            "source": "fixture_manual_review+fixture_approval_request",
            "evidence": "candidate_relative_path matches across consumed JSON artifacts",
        },
        {
            "item_id": "provenance_confirmation",
            "status": "pending_user_review",
            "source": "user_review",
            "evidence_required": "User-confirmed source/provenance note for the selected AEX.",
        },
        {
            "item_id": "license_scope_confirmation",
            "status": "pending_user_review",
            "source": "user_review",
            "evidence_required": "User-confirmed license scope for local fixture use and publication boundary.",
        },
        {
            "item_id": "native_load_gate_closed",
            "status": "blocked_until_explicit_approval",
            "source": "approval_request",
            "evidence": "fixture_approval_satisfied false and native_load_gate closed",
        },
    ]
    candidate = fixture_manual_review.get("candidate") if isinstance(fixture_manual_review.get("candidate"), dict) else {}
    wiztree_inventory = (
        fixture_manual_review.get("wiztree_aex_inventory")
        if isinstance(fixture_manual_review.get("wiztree_aex_inventory"), dict)
        else {}
    )
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_fixture_provenance_review_packet",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_fixture_manual_review": str(fixture_manual_review_path),
        "source_fixture_approval_request": str(approval_request_path),
        "provenance_review_state": "fixture_provenance_review_packet_ready_no_load",
        "provenance_review_ready": True,
        "manual_review_source_ready": True,
        "approval_request_source_ready": True,
        "candidate_relative_path": candidate_relative_path,
        "candidate_file_name": candidate.get("file_name"),
        "candidate_size_bytes": candidate.get("size_bytes"),
        "candidate_wiztree_inventory_state": wiztree_inventory.get("inventory_state"),
        "candidate_wiztree_match_count": wiztree_inventory.get("candidate_match_count"),
        "candidate_wiztree_size_match": wiztree_inventory.get("candidate_size_match"),
        "provenance_status": "unknown_requires_user_review",
        "license_status": "unknown_requires_user_review",
        "safety_review_status": "pending_user_review_no_load",
        "publication_boundary_status": "local_only_not_publishable",
        "local_fixture_safety_status": "no_load_evidence_ready_pending_manual_review",
        "approval_request_ready": approval_request.get("approval_request_ready"),
        "approval_can_be_issued_now": False,
        "approval_manifest_created": False,
        "requires_explicit_user_approval": True,
        "current_fixture_approval_valid": False,
        "fixture_approval_satisfied": False,
        "approval_gate_stays_closed": True,
        "native_load_gate": "closed",
        "manual_review_approval_ready": False,
        "native_load_gate_stays_closed": True,
        "required_approval_token_name": "APPROVE_AEX_LOAD_GATE",
        "approval_token_not_stored_in_manifest": True,
        "approval_only_prepares_next_gate": True,
        "approval_blocker_count": fixture_manual_review.get("approval_blocker_count"),
        "manual_review_blocker_ids": manual_blockers,
        "approval_request_blocker_ids": request_blockers,
        "review_item_count": len(review_items),
        "blocking_review_item_count": sum(
            1 for item in review_items if str(item.get("status", "")).startswith(("pending", "blocked"))
        ),
        "provenance_review_items": review_items,
        "review_question_count": len(questions),
        "unanswered_review_question_count": len(questions),
        "review_questions": questions,
        "provenance_review_blockers": blockers,
        "blockers": blockers,
        "recommended_next_action": "collect_user_provenance_license_safety_review_or_keep_hold",
        "allowed_next_actions": [
            "record manual provenance/license/safety answers in a future hold/reject/approval decision",
            "keep fixture on hold",
            "reject fixture candidate",
            "rerun approval verifier after evidence changes",
        ],
        "blocked_actions": [
            "create_approval_manifest",
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
            "route_through_ofx",
        ],
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
        "accepted_aex_path": None,
        "raw_input_paths_serialized": False,
        "notes": [
            "Packet reads fixture manual-review and approval-request JSON only.",
            "It does not inspect, hash, copy, load, or execute the candidate AEX.",
            "It is not a provenance approval, license approval, fixture approval, or native-load approval.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build no-approval fixture provenance review packet")
    parser.add_argument(
        "--fixture-manual-review",
        required=True,
        help="Fixture manual-review JSON under target/fixture-manual-review",
    )
    parser.add_argument(
        "--approval-request",
        required=True,
        help="Fixture approval-request JSON under target/fixture-approval-request",
    )
    parser.add_argument("--out", required=True, help="Create-new packet under target/fixture-provenance-review")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    manual_review, manual_review_path = load_fixture_manual_review(Path(args.fixture_manual_review))
    approval_request, approval_request_path = load_approval_request(Path(args.approval_request))
    packet = build_fixture_provenance_review_packet(
        fixture_manual_review=manual_review,
        fixture_manual_review_path=manual_review_path,
        approval_request=approval_request,
        approval_request_path=approval_request_path,
    )
    written = write_json_create_new(Path(args.out), packet)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

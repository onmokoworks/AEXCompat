#!/usr/bin/env python3
"""Build a no-approval fixture provenance answer template.

The template reads the provenance review packet JSON only and emits pending
answer slots for a future human-authored review artifact. It does not record
answers, approve a fixture, open/hash/copy/load AEX files, or create approval
manifests.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
PROVENANCE_REVIEW_ROOT = TARGET_ROOT / "fixture-provenance-review"
ANSWER_TEMPLATE_ROOT = TARGET_ROOT / "fixture-provenance-answer-template"

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
    "aex_file_hashed",
    "aex_file_copied",
)
REQUIRED_BLOCKED_ACTIONS = (
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
)
FORBIDDEN_APPROVAL_KEYS = (
    "manifest_kind",
    "approved_actions",
    "approval_token",
    "approval_token_value",
)
ALLOWED_ANSWER_STATUSES = (
    "confirmed_local_only",
    "not_confirmed_keep_hold",
    "reject_fixture",
    "needs_more_information",
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
        raise ValueError("fixture provenance answer template must have .json extension")
    ANSWER_TEMPLATE_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, ANSWER_TEMPLATE_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(ANSWER_TEMPLATE_ROOT.resolve(strict=True)):
        raise ValueError(f"fixture provenance answer template parent must stay under {ANSWER_TEMPLATE_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_provenance_review(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, PROVENANCE_REVIEW_ROOT, "fixture provenance review")
    return read_json_object(resolved), resolved


def safety_errors(payload: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    for key in FORBIDDEN_APPROVAL_KEYS:
        if key in payload:
            errors.append(f"fixture provenance review must not contain {key}")
    for flag in SAFETY_FLAGS:
        if payload.get(flag) is not False:
            errors.append(f"fixture provenance review {flag} must be false")
    if payload.get("accepted_aex_path") is not None:
        errors.append("fixture provenance review accepted_aex_path must be null")
    if payload.get("raw_input_paths_serialized") is not False:
        errors.append("fixture provenance review raw_input_paths_serialized must be false")
    return errors


def validate_provenance_review(packet: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if packet.get("publication_status") != "local-only":
        errors.append("fixture provenance review publication_status must be local-only")
    if packet.get("report_kind") != "aex_fixture_provenance_review_packet":
        errors.append("fixture provenance review report_kind must be aex_fixture_provenance_review_packet")
    if packet.get("provenance_review_state") != "fixture_provenance_review_packet_ready_no_load":
        errors.append("fixture provenance review must be ready no-load")
    if packet.get("provenance_review_ready") is not True:
        errors.append("fixture provenance review provenance_review_ready must be true")
    if packet.get("manual_review_source_ready") is not True:
        errors.append("fixture provenance review manual_review_source_ready must be true")
    if packet.get("approval_request_source_ready") is not True:
        errors.append("fixture provenance review approval_request_source_ready must be true")
    if not isinstance(packet.get("candidate_relative_path"), str):
        errors.append("fixture provenance review candidate_relative_path must be a string")
    if packet.get("provenance_status") != "unknown_requires_user_review":
        errors.append("fixture provenance review provenance_status must require user review")
    if packet.get("license_status") != "unknown_requires_user_review":
        errors.append("fixture provenance review license_status must require user review")
    if packet.get("local_fixture_safety_status") != "no_load_evidence_ready_pending_manual_review":
        errors.append("fixture provenance review local fixture safety status must be pending manual review")
    if packet.get("approval_can_be_issued_now") is not False:
        errors.append("fixture provenance review approval_can_be_issued_now must be false")
    if packet.get("approval_manifest_created") is not False:
        errors.append("fixture provenance review approval_manifest_created must be false")
    if packet.get("requires_explicit_user_approval") is not True:
        errors.append("fixture provenance review requires_explicit_user_approval must be true")
    if packet.get("current_fixture_approval_valid") is not False:
        errors.append("fixture provenance review current_fixture_approval_valid must be false")
    if packet.get("fixture_approval_satisfied") is not False:
        errors.append("fixture provenance review fixture_approval_satisfied must be false")
    if packet.get("approval_gate_stays_closed") is not True:
        errors.append("fixture provenance review approval_gate_stays_closed must be true")
    if packet.get("native_load_gate") != "closed":
        errors.append("fixture provenance review native_load_gate must be closed")
    if packet.get("native_load_gate_stays_closed") is not True:
        errors.append("fixture provenance review native_load_gate_stays_closed must be true")
    if packet.get("required_approval_token_name") != APPROVAL_TOKEN_NAME:
        errors.append("fixture provenance review required_approval_token_name mismatch")
    if packet.get("approval_token_not_stored_in_manifest") is not True:
        errors.append("fixture provenance review approval_token_not_stored_in_manifest must be true")
    if packet.get("approval_only_prepares_next_gate") is not True:
        errors.append("fixture provenance review approval_only_prepares_next_gate must be true")

    questions = packet.get("review_questions")
    if not isinstance(questions, list) or not questions:
        errors.append("fixture provenance review review_questions must be a non-empty list")
    else:
        if packet.get("review_question_count") != len(questions):
            errors.append("fixture provenance review review_question_count must match review_questions")
        if packet.get("unanswered_review_question_count") != len(questions):
            errors.append("fixture provenance review questions must all remain unanswered")
        for index, question in enumerate(questions):
            if not isinstance(question, dict):
                errors.append(f"review question {index} must be an object")
                continue
            if not isinstance(question.get("question_id"), str):
                errors.append(f"review question {index} question_id must be a string")
            if not isinstance(question.get("subject"), str):
                errors.append(f"review question {index} subject must be a string")
            if question.get("approval_effect") != "does_not_approve_native_load":
                errors.append(f"review question {index} approval_effect must not approve native load")

    blocked_actions = packet.get("blocked_actions")
    if not isinstance(blocked_actions, list):
        errors.append("fixture provenance review blocked_actions must be a list")
    else:
        blocked = {str(action) for action in blocked_actions}
        for action in REQUIRED_BLOCKED_ACTIONS:
            if action not in blocked:
                errors.append(f"fixture provenance review must block {action}")
    errors.extend(safety_errors(packet))
    return errors


def answer_template_entry(question: dict[str, Any]) -> dict[str, Any]:
    return {
        "question_id": question["question_id"],
        "subject": question["subject"],
        "source": question.get("source"),
        "required_evidence": question.get("required_evidence"),
        "approval_effect": "does_not_approve_native_load",
        "answer_status": "pending_user_answer",
        "allowed_answer_statuses": list(ALLOWED_ANSWER_STATUSES),
        "answer_text_present": False,
        "answer_evidence_present": False,
        "answer_must_not_include_approval_token": True,
        "answer_does_not_approve_fixture": True,
        "answer_does_not_enable_native_load": True,
    }


def build_fixture_provenance_answer_template(
    *,
    provenance_review: dict[str, Any],
    provenance_review_path: Path,
) -> dict[str, Any]:
    errors = validate_provenance_review(provenance_review)
    if errors:
        raise ValueError("; ".join(errors))
    questions = [item for item in provenance_review["review_questions"] if isinstance(item, dict)]
    entries = [answer_template_entry(question) for question in questions]
    blocked_actions = sorted({str(action) for action in provenance_review.get("blocked_actions", [])})
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_fixture_provenance_answer_template",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_fixture_provenance_review": str(provenance_review_path),
        "template_state": "fixture_provenance_answer_template_ready_all_answers_pending_no_load",
        "template_ready": True,
        "answer_template_only": True,
        "source_provenance_review_ready": True,
        "candidate_relative_path": provenance_review.get("candidate_relative_path"),
        "candidate_file_name": provenance_review.get("candidate_file_name"),
        "candidate_size_bytes": provenance_review.get("candidate_size_bytes"),
        "provenance_status": provenance_review.get("provenance_status"),
        "license_status": provenance_review.get("license_status"),
        "safety_review_status": provenance_review.get("safety_review_status"),
        "publication_boundary_status": provenance_review.get("publication_boundary_status"),
        "local_fixture_safety_status": provenance_review.get("local_fixture_safety_status"),
        "answer_schema": {
            "schema_state": "pending_answers_template_no_approval",
            "required_fields": [
                "question_id",
                "answer_status",
                "answer_text",
                "answer_evidence_reference",
                "local_only_acknowledged",
            ],
            "allowed_answer_statuses": list(ALLOWED_ANSWER_STATUSES),
            "forbidden_fields": [
                "approval_token",
                "approval_token_value",
                "approved_actions",
                "accepted_aex_path",
                "copied_fixture_path",
                "aex_hash",
                "raw_payload",
            ],
        },
        "answer_template_entries": entries,
        "answer_template_entry_count": len(entries),
        "answers_present": False,
        "answered_question_count": 0,
        "pending_answer_count": len(entries),
        "all_answers_pending": True,
        "user_answer_artifact_required": True,
        "answer_template_approves_fixture": False,
        "answer_template_approves_publication": False,
        "answer_template_approves_native_load": False,
        "approval_can_be_issued_now": False,
        "approval_manifest_created": False,
        "requires_explicit_user_approval": True,
        "current_fixture_approval_valid": False,
        "fixture_approval_satisfied": False,
        "approval_gate_stays_closed": True,
        "native_load_gate": "closed",
        "native_load_gate_stays_closed": True,
        "required_approval_token_name": APPROVAL_TOKEN_NAME,
        "approval_token_not_stored_in_manifest": True,
        "approval_only_prepares_next_gate": True,
        "accepted_aex_path": None,
        "raw_input_paths_serialized": False,
        "blocked_actions": blocked_actions,
        "blockers": [
            "user_answers_not_recorded",
            "provenance_not_confirmed",
            "license_scope_not_confirmed",
            "fixture_not_approved",
            "native_load_gate_closed",
        ],
        "next_required_actions": [
            "Author a separate local-only user answer artifact from this template.",
            "Validate answers before changing any fixture decision state.",
            "Keep approval and native load gates closed until explicit approval and later path gate review.",
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
        "notes": [
            "Template reads fixture provenance review JSON only.",
            "It contains no human answers and does not approve any fixture action.",
            "It must not be treated as an approval, publication clearance, or native-load gate.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build no-approval fixture provenance answer template")
    parser.add_argument(
        "--provenance-review",
        required=True,
        help="Fixture provenance review JSON under target/fixture-provenance-review",
    )
    parser.add_argument("--out", required=True, help="Create-new template under target/fixture-provenance-answer-template")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    provenance_review, provenance_review_path = load_provenance_review(Path(args.provenance_review))
    template = build_fixture_provenance_answer_template(
        provenance_review=provenance_review,
        provenance_review_path=provenance_review_path,
    )
    written = write_json_create_new(Path(args.out), template)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

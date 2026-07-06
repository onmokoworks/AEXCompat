#!/usr/bin/env python3
"""Validate a human-authored fixture provenance answer artifact.

The intake reads the pending answer template JSON and a separate local-only
user answer JSON, then applies the same validation rules the answer validator
selftest pinned. It records answer statuses and length metadata only. It never
echoes answer text into the report, never creates an approval manifest, never
stores an approval token, and never opens, hashes, copies, or loads AEX files.
Accepted intake only prepares the existing manual decision flow; it is not an
approval.
"""

from __future__ import annotations

import argparse
import json
import re
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
ANSWER_TEMPLATE_ROOT = TARGET_ROOT / "fixture-provenance-answer-template"
USER_ANSWER_ROOT = TARGET_ROOT / "fixture-provenance-user-answers"
INTAKE_ROOT = TARGET_ROOT / "fixture-provenance-answer-intake"

APPROVAL_TOKEN_NAME = "APPROVE_AEX_LOAD_GATE"
USER_ANSWER_REPORT_KIND = "aex_fixture_provenance_user_answers"
WINDOWS_ABSOLUTE_PATH = re.compile(r"[A-Za-z]:[\\/]")
ALLOWED_ANSWER_STATUSES = (
    "confirmed_local_only",
    "not_confirmed_keep_hold",
    "reject_fixture",
    "needs_more_information",
)
ALLOWED_ANSWER_ITEM_KEYS = (
    "question_id",
    "answer_status",
    "answer_text",
    "answer_evidence_reference",
    "local_only_acknowledged",
    "approval_effect",
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
    "aex_file_hashed",
    "aex_file_copied",
)
FORBIDDEN_KEYS = (
    "manifest_kind",
    "approved_actions",
    "approval_token",
    "approval_token_value",
    "explicit_user_approval",
    "copied_fixture_path",
    "aex_hash",
    "raw_payload",
    "found_paths",
)
REQUIRED_TEMPLATE_FORBIDDEN_FIELDS = (
    "approval_token",
    "approval_token_value",
    "approved_actions",
    "accepted_aex_path",
    "copied_fixture_path",
    "aex_hash",
    "raw_payload",
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
        raise ValueError("fixture provenance answer intake report must have .json extension")
    INTAKE_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, INTAKE_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(INTAKE_ROOT.resolve(strict=True)):
        raise ValueError(f"fixture provenance answer intake parent must stay under {INTAKE_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_answer_template(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, ANSWER_TEMPLATE_ROOT, "fixture provenance answer template")
    return read_json_object(resolved), resolved


def load_user_answers(path: Path) -> tuple[dict[str, Any], Path]:
    USER_ANSWER_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = validate_json_input(path, USER_ANSWER_ROOT, "fixture provenance user answers")
    return read_json_object(resolved), resolved


def nested_keys(value: Any) -> list[str]:
    if isinstance(value, dict):
        result = list(value.keys())
        for item in value.values():
            result.extend(nested_keys(item))
        return result
    if isinstance(value, list):
        result: list[str] = []
        for item in value:
            result.extend(nested_keys(item))
        return result
    return []


def iter_string_values(value: Any, path: str = "$") -> list[tuple[str, str]]:
    if isinstance(value, str):
        return [(path, value)]
    if isinstance(value, dict):
        result: list[tuple[str, str]] = []
        for key, child in value.items():
            result.extend(iter_string_values(child, f"{path}.{key}"))
        return result
    if isinstance(value, list):
        result = []
        for index, child in enumerate(value):
            result.extend(iter_string_values(child, f"{path}[{index}]"))
        return result
    return []


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    if payload.get("accepted_aex_path") is not None:
        errors.append(f"{label} accepted_aex_path must be null")
    if payload.get("raw_input_paths_serialized") is not False:
        errors.append(f"{label} raw_input_paths_serialized must be false")
    return errors


def forbidden_key_errors(payload: dict[str, Any], label: str) -> list[str]:
    hits = sorted(set(nested_keys(payload)) & set(FORBIDDEN_KEYS))
    return [f"{label} contains forbidden key {key}" for key in hits]


def absolute_path_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for path, text in iter_string_values(payload):
        if WINDOWS_ABSOLUTE_PATH.search(text):
            errors.append(f"{label} contains windows absolute path value at {path}")
    return errors


def template_entries(template: dict[str, Any]) -> list[dict[str, Any]]:
    entries = template.get("answer_template_entries")
    if not isinstance(entries, list):
        return []
    return [entry for entry in entries if isinstance(entry, dict)]


def validate_answer_template(template: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if template.get("publication_status") != "local-only":
        errors.append("answer template publication_status must be local-only")
    if template.get("report_kind") != "aex_fixture_provenance_answer_template":
        errors.append("answer template report_kind must be aex_fixture_provenance_answer_template")
    if template.get("template_state") != "fixture_provenance_answer_template_ready_all_answers_pending_no_load":
        errors.append("answer template state must be ready all-answers-pending no-load")
    if template.get("template_ready") is not True:
        errors.append("answer template template_ready must be true")
    if template.get("answer_template_only") is not True:
        errors.append("answer template answer_template_only must be true")
    if not isinstance(template.get("candidate_relative_path"), str):
        errors.append("answer template candidate_relative_path must be a string")
    if template.get("answers_present") is not False:
        errors.append("answer template answers_present must be false")
    if template.get("answered_question_count") != 0:
        errors.append("answer template answered_question_count must be zero")
    if template.get("all_answers_pending") is not True:
        errors.append("answer template all_answers_pending must be true")
    if template.get("user_answer_artifact_required") is not True:
        errors.append("answer template user_answer_artifact_required must be true")
    if template.get("answer_template_approves_fixture") is not False:
        errors.append("answer template must not approve fixture")
    if template.get("answer_template_approves_native_load") is not False:
        errors.append("answer template must not approve native load")
    if template.get("approval_can_be_issued_now") is not False:
        errors.append("answer template approval_can_be_issued_now must be false")
    if template.get("approval_manifest_created") is not False:
        errors.append("answer template approval_manifest_created must be false")
    if template.get("fixture_approval_satisfied") is not False:
        errors.append("answer template fixture_approval_satisfied must be false")
    if template.get("approval_gate_stays_closed") is not True:
        errors.append("answer template approval_gate_stays_closed must be true")
    if template.get("native_load_gate") != "closed":
        errors.append("answer template native_load_gate must be closed")
    if template.get("native_load_gate_stays_closed") is not True:
        errors.append("answer template native_load_gate_stays_closed must be true")
    if template.get("required_approval_token_name") != APPROVAL_TOKEN_NAME:
        errors.append("answer template approval token name mismatch")

    entries = template_entries(template)
    if not entries:
        errors.append("answer template entries must be non-empty")
    else:
        question_ids: list[str] = []
        for index, entry in enumerate(entries):
            question_id = entry.get("question_id")
            if not isinstance(question_id, str):
                errors.append(f"answer template entry {index} question_id must be a string")
            else:
                question_ids.append(question_id)
            if entry.get("approval_effect") != "does_not_approve_native_load":
                errors.append(f"answer template entry {index} approval_effect must not approve native load")
            if entry.get("answer_status") != "pending_user_answer":
                errors.append(f"answer template entry {index} answer_status must be pending_user_answer")
        if len(question_ids) != len(set(question_ids)):
            errors.append("answer template question ids must be unique")
        if template.get("answer_template_entry_count") != len(entries):
            errors.append("answer template entry count must match entries")
        if template.get("pending_answer_count") != len(entries):
            errors.append("answer template pending_answer_count must match entries")

    schema = template.get("answer_schema")
    if not isinstance(schema, dict):
        errors.append("answer template answer_schema must be an object")
    else:
        forbidden_fields = schema.get("forbidden_fields")
        if not isinstance(forbidden_fields, list):
            errors.append("answer template answer_schema forbidden_fields must be a list")
        else:
            missing = [field for field in REQUIRED_TEMPLATE_FORBIDDEN_FIELDS if field not in forbidden_fields]
            if missing:
                errors.append(f"answer template missing forbidden fields: {', '.join(missing)}")

    blocked_actions = template.get("blocked_actions")
    if not isinstance(blocked_actions, list):
        errors.append("answer template blocked_actions must be a list")
    else:
        blocked = {str(action) for action in blocked_actions}
        for action in REQUIRED_BLOCKED_ACTIONS:
            if action not in blocked:
                errors.append(f"answer template must block {action}")
    errors.extend(safety_errors(template, "answer template"))
    errors.extend(forbidden_key_errors(template, "answer template"))
    return errors


def expected_question_ids(template: dict[str, Any]) -> list[str]:
    ids: list[str] = []
    for entry in template_entries(template):
        question_id = entry.get("question_id")
        if isinstance(question_id, str):
            ids.append(question_id)
    return ids


def answer_items(answer_artifact: dict[str, Any]) -> list[dict[str, Any]]:
    items = answer_artifact.get("answers")
    if not isinstance(items, list):
        return []
    return [item for item in items if isinstance(item, dict)]


def validate_user_answer_artifact(template: dict[str, Any], answer_artifact: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if answer_artifact.get("publication_status") != "local-only":
        errors.append("user answers publication_status must be local-only")
    if answer_artifact.get("report_kind") != USER_ANSWER_REPORT_KIND:
        errors.append(f"user answers report_kind must be {USER_ANSWER_REPORT_KIND}")
    if answer_artifact.get("answer_state") != "fixture_provenance_user_answers_recorded_no_approval":
        errors.append("user answers answer_state must be recorded no-approval")
    if answer_artifact.get("candidate_relative_path") != template.get("candidate_relative_path"):
        errors.append("user answers candidate_relative_path must match template")
    if answer_artifact.get("answers_approve_fixture") is not False:
        errors.append("user answers must not approve fixture")
    if answer_artifact.get("answers_approve_publication") is not False:
        errors.append("user answers must not approve publication")
    if answer_artifact.get("answers_approve_native_load") is not False:
        errors.append("user answers must not approve native load")
    if answer_artifact.get("approval_can_be_issued_now") is not False:
        errors.append("user answers approval_can_be_issued_now must be false")
    if answer_artifact.get("approval_manifest_created") is not False:
        errors.append("user answers approval_manifest_created must be false")
    if answer_artifact.get("fixture_approval_satisfied") is not False:
        errors.append("user answers fixture_approval_satisfied must be false")
    if answer_artifact.get("approval_gate_stays_closed") is not True:
        errors.append("user answers approval_gate_stays_closed must be true")
    if answer_artifact.get("native_load_gate") != "closed":
        errors.append("user answers native_load_gate must be closed")

    expected = expected_question_ids(template)
    expected_set = set(expected)
    answers = answer_items(answer_artifact)
    answer_ids = [item.get("question_id") for item in answers if isinstance(item.get("question_id"), str)]
    if len(answers) != len(expected):
        errors.append("user answers must contain exactly one answer per template question")
    if set(answer_ids) != expected_set:
        missing = sorted(expected_set - set(answer_ids))
        extra = sorted(set(answer_ids) - expected_set)
        if missing:
            errors.append(f"user answers missing question ids: {', '.join(missing)}")
        if extra:
            errors.append(f"user answers contain unknown question ids: {', '.join(extra)}")
    if len(answer_ids) != len(set(answer_ids)):
        errors.append("user answers question ids must be unique")
    for index, answer in enumerate(answers):
        unknown_keys = sorted(set(answer.keys()) - set(ALLOWED_ANSWER_ITEM_KEYS))
        if unknown_keys:
            errors.append(f"user answer {index} contains unknown keys: {', '.join(unknown_keys)}")
        status = answer.get("answer_status")
        if status not in ALLOWED_ANSWER_STATUSES:
            errors.append(f"user answer {index} answer_status is not allowed")
        if answer.get("approval_effect") != "does_not_approve_native_load":
            errors.append(f"user answer {index} approval_effect must not approve native load")
        if answer.get("local_only_acknowledged") is not True:
            errors.append(f"user answer {index} local_only_acknowledged must be true")
        answer_text = answer.get("answer_text")
        if not isinstance(answer_text, str) or not answer_text.strip():
            errors.append(f"user answer {index} answer_text must be a non-empty string")
        evidence = answer.get("answer_evidence_reference")
        if not isinstance(evidence, str) or not evidence.strip():
            errors.append(f"user answer {index} answer_evidence_reference must be a non-empty string")
    errors.extend(safety_errors(answer_artifact, "user answers"))
    errors.extend(forbidden_key_errors(answer_artifact, "user answers"))
    errors.extend(absolute_path_errors(answer_artifact, "user answers"))
    return errors


def answer_metadata_rows(answers: list[dict[str, Any]]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for answer in answers:
        question_id = answer.get("question_id")
        status = answer.get("answer_status")
        answer_text = answer.get("answer_text")
        evidence = answer.get("answer_evidence_reference")
        rows.append(
            {
                "question_id": question_id if isinstance(question_id, str) else None,
                "answer_status": status if status in ALLOWED_ANSWER_STATUSES else "invalid_status",
                "answer_text_chars": len(answer_text) if isinstance(answer_text, str) else 0,
                "answer_evidence_reference_present": isinstance(evidence, str) and bool(evidence.strip()),
                "answer_text_echoed": False,
            }
        )
    return rows


def answer_status_counts(answers: list[dict[str, Any]]) -> dict[str, int]:
    counts = {status: 0 for status in ALLOWED_ANSWER_STATUSES}
    for answer in answers:
        status = answer.get("answer_status")
        if status in counts:
            counts[status] += 1
    return counts


def recommended_next_decision(counts: dict[str, int], accepted: bool) -> str:
    if not accepted:
        return "fix_and_resubmit_user_answers"
    if counts["reject_fixture"] > 0:
        return "record_reject_decision_with_existing_decision_tool"
    if counts["needs_more_information"] > 0 or counts["not_confirmed_keep_hold"] > 0:
        return "record_hold_decision_with_existing_decision_tool"
    return "proceed_to_manual_review_then_explicit_decision_tool"


def build_fixture_provenance_answer_intake(
    *,
    answer_template: dict[str, Any],
    answer_template_path: Path,
    user_answers: dict[str, Any],
    user_answers_path: Path,
) -> dict[str, Any]:
    template_errors = validate_answer_template(answer_template)
    if template_errors:
        raise ValueError("; ".join(template_errors))
    rejection_reasons = validate_user_answer_artifact(answer_template, user_answers)
    accepted = not rejection_reasons
    answers = answer_items(user_answers)
    counts = answer_status_counts(answers)
    expected = expected_question_ids(answer_template)
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_fixture_provenance_answer_intake",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_fixture_provenance_answer_template": str(answer_template_path),
        "source_user_answer_artifact": str(user_answers_path),
        "intake_state": (
            "fixture_provenance_answers_accepted_for_manual_review"
            if accepted
            else "fixture_provenance_answers_rejected"
        ),
        "answers_accepted": accepted,
        "real_user_answer_artifact_consumed": True,
        "answer_text_echoed": False,
        "candidate_relative_path": answer_template.get("candidate_relative_path"),
        "question_count": len(expected),
        "answered_question_count": len(answers) if accepted else 0,
        "pending_answer_count": 0 if accepted else len(expected),
        "rejection_reason_count": len(rejection_reasons),
        "rejection_reasons": rejection_reasons,
        "answer_status_counts": counts,
        "all_answers_confirmed_local_only": accepted and counts["confirmed_local_only"] == len(expected),
        "answer_metadata_rows": answer_metadata_rows(answers),
        "answers_validated_for_manual_review": accepted,
        "recommended_next_decision": recommended_next_decision(counts, accepted),
        "intake_is_not_approval": True,
        "approval_can_be_issued_now": False,
        "approval_manifest_created": False,
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
        "blocked_actions": list(REQUIRED_BLOCKED_ACTIONS),
        "blockers": (
            [
                "fixture_not_approved",
                "explicit_decision_artifact_not_created",
                "native_load_gate_closed",
                "publication_review_not_complete",
            ]
            if accepted
            else [
                "user_answers_rejected",
                "fixture_not_approved",
                "native_load_gate_closed",
                "publication_review_not_complete",
            ]
        ),
        "next_required_actions": (
            [
                "Review the accepted answers manually before any decision change.",
                "Record hold/reject/approval only through the existing explicit decision tool.",
                "Keep approval and native load gates closed until that separate explicit step.",
            ]
            if accepted
            else [
                "Fix the rejected user answer artifact and rerun this intake.",
                "Keep approval and native load gates closed.",
            ]
        ),
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
            "Intake reads the answer template JSON and one user answer JSON only.",
            "Answer text is validated in memory and only status/length metadata is reported.",
            "Accepted intake is validation evidence, not fixture approval, publication clearance, or a native-load gate change.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Validate a human-authored fixture provenance answer artifact")
    parser.add_argument(
        "--answer-template",
        required=True,
        help="Fixture provenance answer template JSON under target/fixture-provenance-answer-template",
    )
    parser.add_argument(
        "--answers",
        required=True,
        help="Human-authored user answer JSON under target/fixture-provenance-user-answers",
    )
    parser.add_argument(
        "--out",
        required=True,
        help="Create-new intake report under target/fixture-provenance-answer-intake",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    answer_template, answer_template_path = load_answer_template(Path(args.answer_template))
    user_answers, user_answers_path = load_user_answers(Path(args.answers))
    report = build_fixture_provenance_answer_intake(
        answer_template=answer_template,
        answer_template_path=answer_template_path,
        user_answers=user_answers,
        user_answers_path=user_answers_path,
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0 if report["answers_accepted"] else 1


if __name__ == "__main__":
    raise SystemExit(main())

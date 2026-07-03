#!/usr/bin/env python3
"""Selftest the fixture provenance answer validator without user answers.

The selftest reads the pending answer template JSON only, then validates
synthetic in-memory answer artifacts. It consumes no real user answers and
keeps fixture approval, native load, AE, render, and OFX routes closed.
"""

from __future__ import annotations

import argparse
import json
from copy import deepcopy
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
ANSWER_TEMPLATE_ROOT = TARGET_ROOT / "fixture-provenance-answer-template"
VALIDATOR_SELFTEST_ROOT = TARGET_ROOT / "fixture-provenance-answer-validator-selftest"

APPROVAL_TOKEN_NAME = "APPROVE_AEX_LOAD_GATE"
USER_ANSWER_REPORT_KIND = "aex_fixture_provenance_user_answers"
ALLOWED_ANSWER_STATUSES = (
    "confirmed_local_only",
    "not_confirmed_keep_hold",
    "reject_fixture",
    "needs_more_information",
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
        raise ValueError("fixture provenance answer validator selftest must have .json extension")
    VALIDATOR_SELFTEST_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, VALIDATOR_SELFTEST_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(VALIDATOR_SELFTEST_ROOT.resolve(strict=True)):
        raise ValueError(f"validator selftest parent must stay under {VALIDATOR_SELFTEST_ROOT}")
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
    if template.get("source_provenance_review_ready") is not True:
        errors.append("answer template source_provenance_review_ready must be true")
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
    if template.get("answer_template_approves_publication") is not False:
        errors.append("answer template must not approve publication")
    if template.get("answer_template_approves_native_load") is not False:
        errors.append("answer template must not approve native load")
    if template.get("approval_can_be_issued_now") is not False:
        errors.append("answer template approval_can_be_issued_now must be false")
    if template.get("approval_manifest_created") is not False:
        errors.append("answer template approval_manifest_created must be false")
    if template.get("current_fixture_approval_valid") is not False:
        errors.append("answer template current_fixture_approval_valid must be false")
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
    if template.get("approval_token_not_stored_in_manifest") is not True:
        errors.append("answer template approval_token_not_stored_in_manifest must be true")
    if template.get("approval_only_prepares_next_gate") is not True:
        errors.append("answer template approval_only_prepares_next_gate must be true")

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
            if entry.get("answer_text_present") is not False:
                errors.append(f"answer template entry {index} answer_text_present must be false")
            if entry.get("answer_evidence_present") is not False:
                errors.append(f"answer template entry {index} answer_evidence_present must be false")
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
    errors.extend(safety_errors(answer_artifact, "user answers"))
    errors.extend(forbidden_key_errors(answer_artifact, "user answers"))
    return errors


def synthetic_answer_artifact(template: dict[str, Any], status: str) -> dict[str, Any]:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": USER_ANSWER_REPORT_KIND,
        "answer_state": "fixture_provenance_user_answers_recorded_no_approval",
        "candidate_relative_path": template.get("candidate_relative_path"),
        "answers": [
            {
                "question_id": question_id,
                "answer_status": status,
                "answer_text": f"synthetic {status} answer for {question_id}",
                "answer_evidence_reference": "synthetic-selftest",
                "local_only_acknowledged": True,
                "approval_effect": "does_not_approve_native_load",
            }
            for question_id in expected_question_ids(template)
        ],
        "answers_approve_fixture": False,
        "answers_approve_publication": False,
        "answers_approve_native_load": False,
        "approval_can_be_issued_now": False,
        "approval_manifest_created": False,
        "fixture_approval_satisfied": False,
        "approval_gate_stays_closed": True,
        "native_load_gate": "closed",
        "accepted_aex_path": None,
        "raw_input_paths_serialized": False,
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
    }


def synthetic_cases(template: dict[str, Any]) -> list[dict[str, Any]]:
    valid_hold = synthetic_answer_artifact(template, "not_confirmed_keep_hold")
    valid_confirmed = synthetic_answer_artifact(template, "confirmed_local_only")
    invalid_token = deepcopy(valid_hold)
    invalid_token["approval_token"] = APPROVAL_TOKEN_NAME
    invalid_missing = deepcopy(valid_hold)
    invalid_missing["answers"] = invalid_missing["answers"][:-1]
    invalid_unknown = deepcopy(valid_hold)
    invalid_unknown["answers"][0]["question_id"] = "unknown_question"
    invalid_status = deepcopy(valid_hold)
    invalid_status["answers"][0]["answer_status"] = "approve_native_load"
    invalid_gate = deepcopy(valid_hold)
    invalid_gate["native_load_gate"] = "open"
    invalid_actions = deepcopy(valid_hold)
    invalid_actions["approved_actions"] = ["prepare_native_load_gate"]
    return [
        {"case_id": "valid_hold_answers_no_approval", "payload": valid_hold, "expected_valid": True},
        {"case_id": "valid_confirmed_answers_no_approval", "payload": valid_confirmed, "expected_valid": True},
        {"case_id": "reject_approval_token", "payload": invalid_token, "expected_valid": False},
        {"case_id": "reject_missing_question", "payload": invalid_missing, "expected_valid": False},
        {"case_id": "reject_unknown_question", "payload": invalid_unknown, "expected_valid": False},
        {"case_id": "reject_invalid_status", "payload": invalid_status, "expected_valid": False},
        {"case_id": "reject_open_native_gate", "payload": invalid_gate, "expected_valid": False},
        {"case_id": "reject_approved_actions", "payload": invalid_actions, "expected_valid": False},
    ]


def run_synthetic_cases(template: dict[str, Any]) -> list[dict[str, Any]]:
    results: list[dict[str, Any]] = []
    for case in synthetic_cases(template):
        errors = validate_user_answer_artifact(template, case["payload"])
        actual_valid = not errors
        results.append(
            {
                "case_id": case["case_id"],
                "expected_valid": case["expected_valid"],
                "actual_valid": actual_valid,
                "passed": actual_valid == case["expected_valid"],
                "error_count": len(errors),
                "errors": errors,
                "payload_serialized": False,
            }
        )
    return results


def build_answer_validation_contract(template: dict[str, Any]) -> dict[str, Any]:
    return {
        "contract_state": "fixture_provenance_answer_validation_contract_ready_no_user_answers",
        "accepted_report_kind": USER_ANSWER_REPORT_KIND,
        "required_answer_state": "fixture_provenance_user_answers_recorded_no_approval",
        "expected_question_ids": expected_question_ids(template),
        "allowed_answer_statuses": list(ALLOWED_ANSWER_STATUSES),
        "forbidden_keys": list(FORBIDDEN_KEYS),
        "closed_gate_invariants": {
            "approval_can_be_issued_now": False,
            "approval_manifest_created": False,
            "fixture_approval_satisfied": False,
            "approval_gate_stays_closed": True,
            "native_load_gate": "closed",
            "accepted_aex_path": None,
        },
        "answers_do_not_approve_fixture": True,
        "answers_do_not_approve_publication": True,
        "answers_do_not_approve_native_load": True,
    }


def build_fixture_provenance_answer_validator_selftest(
    *,
    answer_template: dict[str, Any],
    answer_template_path: Path,
) -> dict[str, Any]:
    template_errors = validate_answer_template(answer_template)
    if template_errors:
        raise ValueError("; ".join(template_errors))
    results = run_synthetic_cases(answer_template)
    failed = [result for result in results if result["passed"] is not True]
    valid_cases = [result for result in results if result["actual_valid"] is True]
    rejected_cases = [result for result in results if result["actual_valid"] is False]
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_fixture_provenance_answer_validator_selftest",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_fixture_provenance_answer_template": str(answer_template_path),
        "validator_selftest_state": (
            "fixture_provenance_answer_validator_selftest_passed_no_user_answers"
            if not failed
            else "fixture_provenance_answer_validator_selftest_failed"
        ),
        "validator_ready": not failed,
        "source_answer_template_ready": True,
        "answer_template_only": True,
        "real_user_answer_artifact_consumed": False,
        "synthetic_user_answers_used": True,
        "synthetic_payloads_serialized": False,
        "answer_schema_validated": True,
        "answer_validation_contract": build_answer_validation_contract(answer_template),
        "synthetic_case_results": results,
        "synthetic_case_count": len(results),
        "synthetic_case_passed_count": sum(1 for result in results if result["passed"]),
        "synthetic_case_failed_count": len(failed),
        "synthetic_valid_case_count": len(valid_cases),
        "synthetic_rejected_case_count": len(rejected_cases),
        "candidate_relative_path": answer_template.get("candidate_relative_path"),
        "user_answer_artifact_required": True,
        "answers_present": False,
        "answered_question_count": 0,
        "pending_answer_count": answer_template.get("pending_answer_count"),
        "answers_validated_for_manual_review": False,
        "approval_can_be_issued_now": False,
        "approval_manifest_created": False,
        "current_fixture_approval_valid": False,
        "fixture_approval_satisfied": False,
        "approval_gate_stays_closed": True,
        "native_load_gate": "closed",
        "native_load_gate_stays_closed": True,
        "accepted_aex_path": None,
        "raw_input_paths_serialized": False,
        "blocked_actions": list(REQUIRED_BLOCKED_ACTIONS),
        "blockers": [
            "real_user_answers_not_supplied",
            "fixture_not_approved",
            "native_load_gate_closed",
            "publication_review_not_complete",
        ],
        "next_required_actions": [
            "Create a separate local-only user answer artifact and validate it with these rules.",
            "Keep fixture approval and native load gates closed until a later explicit approval artifact exists.",
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
            "Selftest reads the answer template JSON only.",
            "Synthetic answer payloads are held in memory and are not serialized.",
            "No real user answer artifact, approval manifest, AEX path, AEX file, native loader, AE, render, or OFX route is used.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Selftest fixture provenance answer validation")
    parser.add_argument(
        "--answer-template",
        required=True,
        help="Fixture provenance answer template JSON under target/fixture-provenance-answer-template",
    )
    parser.add_argument(
        "--out",
        required=True,
        help="Create-new selftest report under target/fixture-provenance-answer-validator-selftest",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    answer_template, answer_template_path = load_answer_template(Path(args.answer_template))
    report = build_fixture_provenance_answer_validator_selftest(
        answer_template=answer_template,
        answer_template_path=answer_template_path,
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

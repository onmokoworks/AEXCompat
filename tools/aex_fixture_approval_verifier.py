#!/usr/bin/env python3
"""Verify fixture approval evidence without opening the AEX load gate.

The verifier reads JSON evidence only. It checks the current fixture decision or
approval manifest, manual-review readiness, candidate dependency scope, path
policy closure, and candidate load-gate dry-run state. It never opens, hashes,
copies, loads, or executes AEX files.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
FIXTURE_APPROVAL_ROOT = TARGET_ROOT / "fixture-approval"
FIXTURE_MANUAL_REVIEW_ROOT = TARGET_ROOT / "fixture-manual-review"
CANDIDATE_DEPENDENCY_SCOPE_ROOT = TARGET_ROOT / "candidate-dependency-scope"
PATH_POLICY_SELFTEST_ROOT = TARGET_ROOT / "native-loader-path-policy-selftest"
CANDIDATE_LOAD_GATE_ROOT = TARGET_ROOT / "candidate-load-gate"
APPROVAL_VERIFIER_ROOT = TARGET_ROOT / "fixture-approval-verifier"

APPROVAL_TOKEN_NAME = "APPROVE_AEX_LOAD_GATE"
FORBIDDEN_APPROVED_ACTIONS = {
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
}
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
        raise ValueError("fixture approval verifier report must have .json extension")
    APPROVAL_VERIFIER_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, APPROVAL_VERIFIER_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(APPROVAL_VERIFIER_ROOT.resolve(strict=True)):
        raise ValueError(f"fixture approval verifier parent must stay under {APPROVAL_VERIFIER_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_fixture_decision(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, FIXTURE_APPROVAL_ROOT, "fixture decision or approval")
    return read_json_object(resolved), resolved


def load_fixture_manual_review(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, FIXTURE_MANUAL_REVIEW_ROOT, "fixture manual-review packet")
    return read_json_object(resolved), resolved


def load_candidate_dependency_scope(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, CANDIDATE_DEPENDENCY_SCOPE_ROOT, "candidate dependency scope")
    return read_json_object(resolved), resolved


def load_path_policy_selftest(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, PATH_POLICY_SELFTEST_ROOT, "path policy selftest")
    return read_json_object(resolved), resolved


def load_candidate_load_gate(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, CANDIDATE_LOAD_GATE_ROOT, "candidate load gate dry-run")
    return read_json_object(resolved), resolved


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def candidate_path_from_decision(decision: dict[str, Any]) -> str | None:
    value = decision.get("candidate_relative_path")
    return value if isinstance(value, str) else None


def validate_fixture_decision(decision: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if decision.get("publication_status") != "local-only":
        errors.append("fixture decision publication_status must be local-only")
    if decision.get("manifest_kind") not in {"aex_fixture_decision_manifest", "aex_fixture_approval_manifest"}:
        errors.append("fixture decision manifest_kind must be decision or approval manifest")
    if not isinstance(decision.get("candidate_relative_path"), str):
        errors.append("fixture decision candidate_relative_path must be a string")
    errors.extend(safety_errors(decision, "fixture decision"))
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
    if packet.get("candidate_relative_path") != candidate_relative_path:
        errors.append("fixture manual-review candidate_relative_path must match fixture decision")
    errors.extend(safety_errors(packet, "fixture manual-review"))
    return errors


def validate_candidate_dependency_scope(scope: dict[str, Any], candidate_relative_path: str | None) -> list[str]:
    errors: list[str] = []
    if scope.get("publication_status") != "local-only":
        errors.append("candidate dependency scope publication_status must be local-only")
    if scope.get("report_kind") != "aex_candidate_dependency_scope_packet":
        errors.append("candidate dependency scope report_kind must be aex_candidate_dependency_scope_packet")
    if scope.get("candidate_dependency_scope_state") != "candidate_dependency_scope_ready_no_load":
        errors.append("candidate dependency scope must be ready no-load")
    if scope.get("candidate_relative_path") != candidate_relative_path:
        errors.append("candidate dependency scope candidate_relative_path must match fixture decision")
    if scope.get("candidate_dependency_found_paths_exported") is not False:
        errors.append("candidate dependency scope must not export found paths")
    errors.extend(safety_errors(scope, "candidate dependency scope"))
    return errors


def validate_path_policy_selftest(selftest: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if selftest.get("publication_status") != "local-only":
        errors.append("path policy selftest publication_status must be local-only")
    if selftest.get("report_kind") != "aex_native_loader_path_policy_selftest":
        errors.append("path policy selftest report_kind must be aex_native_loader_path_policy_selftest")
    if selftest.get("path_policy_selftest_state") != "closed_path_policy_selftest_passed_no_aex_path":
        errors.append("path policy selftest state must be closed_path_policy_selftest_passed_no_aex_path")
    if selftest.get("path_policy_selftest_passed") is not True:
        errors.append("path_policy_selftest_passed must be true")
    if selftest.get("path_acceptance_ready") is not False:
        errors.append("path policy selftest path_acceptance_ready must be false")
    if selftest.get("aex_path_acceptance_enabled") is not False:
        errors.append("path policy selftest aex_path_acceptance_enabled must be false")
    if selftest.get("accepted_aex_path") is not None:
        errors.append("path policy selftest accepted_aex_path must be null")
    if selftest.get("raw_input_paths_serialized") is not False:
        errors.append("path policy selftest raw_input_paths_serialized must be false")
    errors.extend(safety_errors(selftest, "path policy selftest"))
    return errors


def validate_candidate_load_gate(gate: dict[str, Any], candidate_relative_path: str | None) -> list[str]:
    errors: list[str] = []
    if gate.get("publication_status") != "local-only":
        errors.append("candidate load gate publication_status must be local-only")
    if gate.get("report_kind") != "aex_candidate_load_gate_dryrun":
        errors.append("candidate load gate report_kind must be aex_candidate_load_gate_dryrun")
    if gate.get("candidate_load_gate_dryrun_state") != "candidate_load_gate_dryrun_ready_no_load":
        errors.append("candidate load gate dry-run must be ready no-load")
    if gate.get("candidate_relative_path") != candidate_relative_path:
        errors.append("candidate load gate candidate_relative_path must match fixture decision")
    if gate.get("native_load_gate") != "closed":
        errors.append("candidate load gate native_load_gate must be closed")
    errors.extend(safety_errors(gate, "candidate load gate"))
    return errors


def candidate_dependencies_clear(scope: dict[str, Any]) -> bool:
    return (
        scope.get("candidate_scope_ready") is True
        and scope.get("candidate_dependency_blockers_present") is False
        and int(scope.get("candidate_dependency_blocker_count") or 0) == 0
        and int(scope.get("candidate_dependency_missing_or_api_set_review_count") or 0) == 0
        and scope.get("candidate_dependency_found_paths_exported") is False
        and scope.get("global_dependency_blockers_apply_to_candidate") is False
    )


def evaluate_approval_manifest(
    manifest: dict[str, Any],
    *,
    candidate_relative_path: str | None,
    manual_review_approval_ready: bool,
    dependencies_clear: bool,
    path_policy_closed: bool,
) -> dict[str, Any]:
    reasons: list[str] = []
    if manifest.get("manifest_kind") != "aex_fixture_approval_manifest":
        reasons.append("manifest_kind_not_approval")
    if manifest.get("publication_status") != "local-only":
        reasons.append("publication_status_not_local_only")
    if manifest.get("approval_state") != "user_approved_for_load_gate":
        reasons.append("approval_state_not_user_approved_for_load_gate")
    if manifest.get("explicit_user_approval") is not True:
        reasons.append("explicit_user_approval_missing")
    if manifest.get("candidate_relative_path") != candidate_relative_path:
        reasons.append("candidate_relative_path_mismatch")
    approved_actions = manifest.get("approved_actions", [])
    if "prepare_native_load_gate" not in approved_actions:
        reasons.append("prepare_native_load_gate_not_approved")
    forbidden = sorted(action for action in approved_actions if action in FORBIDDEN_APPROVED_ACTIONS)
    if forbidden:
        reasons.append("forbidden_runtime_action_approved")
    if not manual_review_approval_ready:
        reasons.append("manual_review_not_approval_ready")
    if not dependencies_clear:
        reasons.append("candidate_dependencies_not_clear")
    if not path_policy_closed:
        reasons.append("path_policy_not_closed")
    for error in safety_errors(manifest, "fixture approval"):
        reasons.append(error.replace(" ", "_"))
    return {
        "valid": not reasons,
        "reasons": reasons,
        "approved_actions": approved_actions if isinstance(approved_actions, list) else [],
        "forbidden_approved_actions": forbidden,
        "approval_only_prepares_next_gate": not forbidden and "prepare_native_load_gate" in approved_actions,
    }


def synthetic_approval_checks(
    *,
    fixture_decision: dict[str, Any],
    manual_review: dict[str, Any],
    dependencies_clear: bool,
    path_policy_closed: bool,
) -> list[dict[str, Any]]:
    candidate_relative_path = fixture_decision.get("candidate_relative_path")
    base = {
        "schema_version": 1,
        "publication_status": "local-only",
        "manifest_kind": "aex_fixture_approval_manifest",
        "approval_state": "user_approved_for_load_gate",
        "explicit_user_approval": True,
        "candidate_relative_path": candidate_relative_path,
        "approved_actions": ["prepare_native_load_gate"],
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }
    cases = [
        {
            "case": "current_hold_manifest_rejected",
            "manifest": fixture_decision,
            "manual_ready": manual_review.get("approval_ready") is True,
            "expect_valid": False,
            "expect_reason": "manifest_kind_not_approval",
        },
        {
            "case": "missing_explicit_user_approval_rejected",
            "manifest": {**base, "explicit_user_approval": False},
            "manual_ready": True,
            "expect_valid": False,
            "expect_reason": "explicit_user_approval_missing",
        },
        {
            "case": "forbidden_runtime_action_rejected",
            "manifest": {**base, "approved_actions": ["prepare_native_load_gate", "load_aex_dll"]},
            "manual_ready": True,
            "expect_valid": False,
            "expect_reason": "forbidden_runtime_action_approved",
        },
        {
            "case": "manual_review_not_ready_rejected",
            "manifest": base,
            "manual_ready": False,
            "expect_valid": False,
            "expect_reason": "manual_review_not_approval_ready",
        },
        {
            "case": "future_valid_shape_only_prepares_next_gate",
            "manifest": base,
            "manual_ready": True,
            "expect_valid": True,
            "expect_reason": None,
        },
    ]
    results: list[dict[str, Any]] = []
    for case in cases:
        evaluation = evaluate_approval_manifest(
            case["manifest"],
            candidate_relative_path=candidate_relative_path,
            manual_review_approval_ready=case["manual_ready"],
            dependencies_clear=dependencies_clear,
            path_policy_closed=path_policy_closed,
        )
        if evaluation["valid"] != case["expect_valid"]:
            raise RuntimeError(f"synthetic approval case failed: {case['case']}")
        if case["expect_reason"] and case["expect_reason"] not in evaluation["reasons"]:
            raise RuntimeError(f"synthetic approval case reason mismatch: {case['case']}")
        results.append(
            {
                "case": case["case"],
                "valid": evaluation["valid"],
                "reasons": evaluation["reasons"],
                "approval_only_prepares_next_gate": evaluation["approval_only_prepares_next_gate"],
            }
        )
    return results


def build_fixture_approval_verifier(
    *,
    fixture_decision: dict[str, Any],
    fixture_decision_path: Path,
    fixture_manual_review: dict[str, Any],
    fixture_manual_review_path: Path,
    candidate_dependency_scope: dict[str, Any],
    candidate_dependency_scope_path: Path,
    path_policy_selftest: dict[str, Any],
    path_policy_selftest_path: Path,
    candidate_load_gate: dict[str, Any],
    candidate_load_gate_path: Path,
) -> dict[str, Any]:
    candidate_relative_path = candidate_path_from_decision(fixture_decision)
    errors = (
        validate_fixture_decision(fixture_decision)
        + validate_manual_review(fixture_manual_review, candidate_relative_path)
        + validate_candidate_dependency_scope(candidate_dependency_scope, candidate_relative_path)
        + validate_path_policy_selftest(path_policy_selftest)
        + validate_candidate_load_gate(candidate_load_gate, candidate_relative_path)
    )
    if errors:
        raise ValueError("; ".join(errors))

    manual_ready = fixture_manual_review.get("approval_ready") is True
    deps_clear = candidate_dependencies_clear(candidate_dependency_scope)
    path_closed = (
        path_policy_selftest.get("path_policy_selftest_passed") is True
        and path_policy_selftest.get("path_acceptance_ready") is False
        and path_policy_selftest.get("aex_path_acceptance_enabled") is False
        and path_policy_selftest.get("accepted_aex_path") is None
    )
    current_evaluation = evaluate_approval_manifest(
        fixture_decision,
        candidate_relative_path=candidate_relative_path,
        manual_review_approval_ready=manual_ready,
        dependencies_clear=deps_clear,
        path_policy_closed=path_closed,
    )
    synthetic_checks = synthetic_approval_checks(
        fixture_decision=fixture_decision,
        manual_review=fixture_manual_review,
        dependencies_clear=deps_clear,
        path_policy_closed=path_closed,
    )
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_fixture_approval_verifier",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_fixture_decision": str(fixture_decision_path),
        "source_fixture_manual_review": str(fixture_manual_review_path),
        "source_candidate_dependency_scope": str(candidate_dependency_scope_path),
        "source_path_policy_selftest": str(path_policy_selftest_path),
        "source_candidate_load_gate": str(candidate_load_gate_path),
        "approval_verifier_state": "fixture_approval_verifier_ready_no_approval",
        "approval_verifier_ready": True,
        "candidate_relative_path": candidate_relative_path,
        "approval_manifest_kind": fixture_decision.get("manifest_kind"),
        "decision_state": fixture_decision.get("decision_state"),
        "approval_state": fixture_decision.get("approval_state"),
        "current_fixture_approval_valid": current_evaluation["valid"],
        "fixture_approval_satisfied": False,
        "approval_gate_stays_closed": True,
        "manual_review_ready": fixture_manual_review.get("manual_review_ready"),
        "manual_review_approval_ready": manual_ready,
        "approval_blocker_count": fixture_manual_review.get("approval_blocker_count"),
        "candidate_dependencies_clear": deps_clear,
        "candidate_dependency_blockers_present": candidate_dependency_scope.get(
            "candidate_dependency_blockers_present"
        ),
        "path_policy_closed": path_closed,
        "raw_input_paths_serialized": path_policy_selftest.get("raw_input_paths_serialized"),
        "candidate_load_gate_closed": candidate_load_gate.get("native_load_gate") == "closed",
        "source_candidate_load_gate_state": candidate_load_gate.get("candidate_load_gate_dryrun_state"),
        "source_candidate_scoped_load_gate_state": candidate_load_gate.get(
            "candidate_scoped_load_gate_dry_run_state"
        ),
        "current_approval_evaluation": current_evaluation,
        "synthetic_approval_checks_passed": True,
        "synthetic_approval_checks": synthetic_checks,
        "required_approval_token_name": APPROVAL_TOKEN_NAME,
        "approval_token_not_stored_in_manifest": True,
        "approval_only_prepares_next_gate": True,
        "blocked_actions": sorted(FORBIDDEN_APPROVED_ACTIONS),
        "allowed_next_actions": [
            "complete manual fixture review before approval",
            "only accept a local-only approval manifest after explicit user approval",
            "rerun candidate load gate dry-run after approval evidence changes",
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
            "This verifier reads JSON evidence only.",
            "The current fixture decision is not converted into approval.",
            "A valid approval manifest would only prepare the next no-load gate, not permit native load.",
            "No AEX file or DLL is opened, copied, hashed, loaded, or executed.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Verify fixture approval evidence without native load")
    parser.add_argument("--fixture-decision", required=True, help="Fixture decision/approval JSON under target/fixture-approval")
    parser.add_argument(
        "--fixture-manual-review",
        required=True,
        help="Fixture manual-review JSON under target/fixture-manual-review",
    )
    parser.add_argument(
        "--candidate-dependency-scope",
        required=True,
        help="Candidate dependency scope JSON under target/candidate-dependency-scope",
    )
    parser.add_argument(
        "--path-policy-selftest",
        required=True,
        help="Path policy selftest JSON under target/native-loader-path-policy-selftest",
    )
    parser.add_argument(
        "--candidate-load-gate",
        required=True,
        help="Candidate load gate dry-run JSON under target/candidate-load-gate",
    )
    parser.add_argument("--out", required=True, help="Create-new report under target/fixture-approval-verifier")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    decision, decision_path = load_fixture_decision(Path(args.fixture_decision))
    manual_review, manual_review_path = load_fixture_manual_review(Path(args.fixture_manual_review))
    dependency_scope, dependency_scope_path = load_candidate_dependency_scope(
        Path(args.candidate_dependency_scope)
    )
    path_policy, path_policy_path = load_path_policy_selftest(Path(args.path_policy_selftest))
    load_gate, load_gate_path = load_candidate_load_gate(Path(args.candidate_load_gate))
    report = build_fixture_approval_verifier(
        fixture_decision=decision,
        fixture_decision_path=decision_path,
        fixture_manual_review=manual_review,
        fixture_manual_review_path=manual_review_path,
        candidate_dependency_scope=dependency_scope,
        candidate_dependency_scope_path=dependency_scope_path,
        path_policy_selftest=path_policy,
        path_policy_selftest_path=path_policy_path,
        candidate_load_gate=load_gate,
        candidate_load_gate_path=load_gate_path,
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

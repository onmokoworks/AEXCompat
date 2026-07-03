#!/usr/bin/env python3
"""Build a candidate-scoped no-load AEX gate dry-run report.

The report reads JSON evidence only. It exists to say, explicitly, whether the
selected fixture candidate is merely closer to a future loader gate, while still
keeping every native AEX/AE/OFX action closed.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
WORKER_DESIGN_ROOT = TARGET_ROOT / "worker-design"
WORKER_SELFTEST_ROOT = TARGET_ROOT / "worker-selftest"
FIXTURE_APPROVAL_ROOT = TARGET_ROOT / "fixture-approval"
FIXTURE_MANUAL_REVIEW_ROOT = TARGET_ROOT / "fixture-manual-review"
CANDIDATE_DEPENDENCY_SCOPE_ROOT = TARGET_ROOT / "candidate-dependency-scope"
LOAD_GATE_ROOT = TARGET_ROOT / "load-gate"
CANDIDATE_LOAD_GATE_ROOT = TARGET_ROOT / "candidate-load-gate"

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

BLOCKED_ACTIONS = [
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
]


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
        raise ValueError("candidate load gate dry-run report must have .json extension")
    CANDIDATE_LOAD_GATE_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, CANDIDATE_LOAD_GATE_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(
        CANDIDATE_LOAD_GATE_ROOT.resolve(strict=True)
    ):
        raise ValueError(f"candidate load gate dry-run parent must stay under {CANDIDATE_LOAD_GATE_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_worker_design(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, WORKER_DESIGN_ROOT, "worker design packet")
    return read_json_object(resolved), resolved


def load_worker_selftest(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, WORKER_SELFTEST_ROOT, "worker selftest report")
    return read_json_object(resolved), resolved


def load_fixture_decision(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, FIXTURE_APPROVAL_ROOT, "fixture decision or approval")
    return read_json_object(resolved), resolved


def load_fixture_manual_review(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, FIXTURE_MANUAL_REVIEW_ROOT, "fixture manual-review packet")
    return read_json_object(resolved), resolved


def load_candidate_dependency_scope(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, CANDIDATE_DEPENDENCY_SCOPE_ROOT, "candidate dependency scope")
    return read_json_object(resolved), resolved


def load_source_load_gate(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, LOAD_GATE_ROOT, "source load gate")
    return read_json_object(resolved), resolved


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def candidate_relative_path_from_design(design: dict[str, Any]) -> str | None:
    candidate = design.get("primary_review_candidate")
    if isinstance(candidate, dict) and isinstance(candidate.get("relative_path"), str):
        return candidate["relative_path"]
    return None


def primary_candidate(design: dict[str, Any]) -> dict[str, Any]:
    candidate = design.get("primary_review_candidate")
    return candidate if isinstance(candidate, dict) else {}


def validate_design(design: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if design.get("publication_status") != "local-only":
        errors.append("worker design publication_status must be local-only")
    if design.get("packet_kind") != "aex_worker_sandbox_design_packet":
        errors.append("worker design packet_kind must be aex_worker_sandbox_design_packet")
    if design.get("design_state") != "no_load_worker_boundary_only":
        errors.append("worker design_state must be no_load_worker_boundary_only")
    errors.extend(safety_errors(design, "worker design"))
    candidate = design.get("primary_review_candidate")
    if not isinstance(candidate, dict):
        errors.append("worker design primary_review_candidate must be an object")
    else:
        if candidate.get("approval_state") != "not_approved_for_load":
            errors.append("worker design primary candidate must remain not_approved_for_load")
        if candidate.get("compatibility_class") != "classic_pf_effect_candidate":
            errors.append("worker design primary candidate must be classic_pf_effect_candidate")
        if candidate.get("effect_main_export_present") is not True:
            errors.append("worker design primary candidate must export EffectMain")
        if int(candidate.get("aegp_marker_count") or 0) != 0:
            errors.append("worker design primary candidate must not include AEGP markers")
    blocked = design.get("blocked_actions", [])
    for action in ("load_aex_dll", "call_EffectMain", "render_with_aex", "route_through_ofx"):
        if action not in blocked:
            errors.append(f"worker design must block {action}")
    return errors


def validate_worker_selftest(selftest: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if selftest.get("publication_status") != "local-only":
        errors.append("worker selftest publication_status must be local-only")
    if selftest.get("report_kind") != "aex_no_load_worker_selftest":
        errors.append("worker selftest report_kind must be aex_no_load_worker_selftest")
    if selftest.get("worker_selftest_passed") is not True:
        errors.append("worker_selftest_passed must be true")
    errors.extend(safety_errors(selftest, "worker selftest"))
    steps = selftest.get("steps")
    if not isinstance(steps, list):
        errors.append("worker selftest steps must be a list")
    else:
        step_names = [step.get("step") for step in steps if isinstance(step, dict)]
        for required in ("hello", "inspect_environment", "inspect_ppm", "transform_ppm_identity", "blocked_load_aex", "quit"):
            if required not in step_names:
                errors.append(f"worker selftest missing step {required}")
        blocked_steps = [step for step in steps if isinstance(step, dict) and step.get("step") == "blocked_load_aex"]
        if not blocked_steps or blocked_steps[0].get("code") != "blocked_action":
            errors.append("worker selftest must prove load_aex fails with blocked_action")
    return errors


def validate_fixture_decision(decision: dict[str, Any], candidate_relative_path: str | None) -> list[str]:
    errors: list[str] = []
    manifest_kind = decision.get("manifest_kind")
    if decision.get("publication_status") != "local-only":
        errors.append("fixture decision publication_status must be local-only")
    if manifest_kind not in {"aex_fixture_decision_manifest", "aex_fixture_approval_manifest"}:
        errors.append("fixture decision manifest_kind must be decision or approval manifest")
    errors.extend(safety_errors(decision, "fixture decision"))
    if decision.get("candidate_relative_path") != candidate_relative_path:
        errors.append("fixture decision candidate_relative_path must match worker design primary candidate")
    if manifest_kind == "aex_fixture_approval_manifest":
        if decision.get("approval_state") != "user_approved_for_load_gate":
            errors.append("fixture approval_state must be user_approved_for_load_gate")
        if decision.get("explicit_user_approval") is not True:
            errors.append("fixture approval explicit_user_approval must be true")
        approved_actions = decision.get("approved_actions", [])
        if "prepare_native_load_gate" not in approved_actions:
            errors.append("fixture approval must include prepare_native_load_gate")
    return errors


def fixture_approval_satisfied(decision: dict[str, Any]) -> bool:
    return (
        decision.get("manifest_kind") == "aex_fixture_approval_manifest"
        and decision.get("approval_state") == "user_approved_for_load_gate"
        and decision.get("explicit_user_approval") is True
        and "prepare_native_load_gate" in decision.get("approved_actions", [])
    )


def validate_manual_review(manual_review: dict[str, Any], candidate_relative_path: str | None) -> list[str]:
    errors: list[str] = []
    if manual_review.get("publication_status") != "local-only":
        errors.append("fixture manual-review publication_status must be local-only")
    if manual_review.get("report_kind") != "aex_fixture_manual_review_packet":
        errors.append("fixture manual-review report_kind must be aex_fixture_manual_review_packet")
    if manual_review.get("review_packet_state") != "fixture_manual_review_packet_ready_no_load":
        errors.append("fixture manual-review packet must be ready no-load")
    if manual_review.get("manual_review_ready") is not True:
        errors.append("fixture manual-review manual_review_ready must be true")
    if manual_review.get("candidate_relative_path") != candidate_relative_path:
        errors.append("fixture manual-review candidate_relative_path must match worker design primary candidate")
    errors.extend(safety_errors(manual_review, "fixture manual-review"))
    return errors


def validate_candidate_dependency_scope(scope: dict[str, Any], candidate_relative_path: str | None) -> list[str]:
    errors: list[str] = []
    if scope.get("publication_status") != "local-only":
        errors.append("candidate dependency scope publication_status must be local-only")
    if scope.get("report_kind") != "aex_candidate_dependency_scope_packet":
        errors.append("candidate dependency scope report_kind must be aex_candidate_dependency_scope_packet")
    if scope.get("candidate_dependency_scope_state") != "candidate_dependency_scope_ready_no_load":
        errors.append("candidate dependency scope state must be ready no-load")
    if scope.get("candidate_scope_ready") is not True:
        errors.append("candidate_scope_ready must be true")
    if scope.get("candidate_relative_path") != candidate_relative_path:
        errors.append("candidate dependency scope candidate_relative_path must match worker design primary candidate")
    if scope.get("candidate_dependency_found_paths_exported") is not False:
        errors.append("candidate dependency scope must not export dependency found paths")
    errors.extend(safety_errors(scope, "candidate dependency scope"))
    return errors


def validate_source_load_gate(load_gate: dict[str, Any], candidate_relative_path: str | None) -> list[str]:
    errors: list[str] = []
    if load_gate.get("publication_status") != "local-only":
        errors.append("source load gate publication_status must be local-only")
    if load_gate.get("report_kind") != "aex_load_gate_check":
        errors.append("source load gate report_kind must be aex_load_gate_check")
    if not isinstance(load_gate.get("gate_state"), str):
        errors.append("source load gate gate_state must be a string")
    gate_candidate = load_gate.get("primary_review_candidate")
    if isinstance(gate_candidate, dict) and gate_candidate.get("relative_path") != candidate_relative_path:
        errors.append("source load gate primary candidate must match worker design primary candidate")
    errors.extend(safety_errors(load_gate, "source load gate"))
    return errors


def candidate_dependencies_clear(scope: dict[str, Any]) -> bool:
    return (
        scope.get("candidate_dependency_scope_state") == "candidate_dependency_scope_ready_no_load"
        and scope.get("candidate_scope_ready") is True
        and scope.get("candidate_dependency_blockers_present") is False
        and int(scope.get("candidate_dependency_blocker_count") or 0) == 0
        and int(scope.get("candidate_dependency_missing_or_api_set_review_count") or 0) == 0
        and scope.get("candidate_dependency_found_paths_exported") is False
        and scope.get("global_dependency_blockers_apply_to_candidate") is False
    )


def gate_item(gate: str, status: str, evidence: str, notes: list[str] | None = None) -> dict[str, Any]:
    return {
        "gate": gate,
        "status": status,
        "evidence": evidence,
        "notes": notes or [],
    }


def candidate_gate_state(
    *,
    hard_errors: list[str],
    fixture_satisfied: bool,
    dependencies_clear: bool,
    candidate_dependency_blockers_present: bool,
    global_dependency_blockers_present: bool,
    global_dependency_blockers_apply_to_candidate: bool,
) -> str:
    if hard_errors:
        return "invalid_evidence_closed"
    if candidate_dependency_blockers_present or global_dependency_blockers_apply_to_candidate or not dependencies_clear:
        return "closed_dry_run_candidate_dependency_blockers_or_reviews_present"
    if not fixture_satisfied and global_dependency_blockers_present:
        return "closed_dry_run_candidate_deps_clear_fixture_approval_missing_global_deps_blocked"
    if not fixture_satisfied:
        return "closed_dry_run_candidate_deps_clear_fixture_approval_missing"
    return "candidate_scoped_preconditions_satisfied_no_load_performed"


def build_candidate_load_gate_dryrun(
    *,
    worker_design: dict[str, Any],
    worker_design_path: Path,
    worker_selftest: dict[str, Any],
    worker_selftest_path: Path,
    fixture_decision: dict[str, Any],
    fixture_decision_path: Path,
    fixture_manual_review: dict[str, Any],
    fixture_manual_review_path: Path,
    candidate_dependency_scope: dict[str, Any],
    candidate_dependency_scope_path: Path,
    source_load_gate: dict[str, Any] | None = None,
    source_load_gate_path: Path | None = None,
) -> dict[str, Any]:
    candidate_relative_path = candidate_relative_path_from_design(worker_design)
    design_errors = validate_design(worker_design)
    selftest_errors = validate_worker_selftest(worker_selftest)
    fixture_errors = validate_fixture_decision(fixture_decision, candidate_relative_path)
    manual_review_errors = validate_manual_review(fixture_manual_review, candidate_relative_path)
    scope_errors = validate_candidate_dependency_scope(candidate_dependency_scope, candidate_relative_path)
    source_gate_errors = (
        validate_source_load_gate(source_load_gate, candidate_relative_path)
        if source_load_gate is not None
        else []
    )
    hard_errors = design_errors + selftest_errors + fixture_errors + manual_review_errors + scope_errors + source_gate_errors

    fixture_satisfied = not fixture_errors and fixture_approval_satisfied(fixture_decision)
    dependencies_clear = not scope_errors and candidate_dependencies_clear(candidate_dependency_scope)
    candidate_blockers = bool(candidate_dependency_scope.get("candidate_dependency_blockers_present"))
    global_blockers_present = bool(candidate_dependency_scope.get("global_dependency_blockers_present"))
    global_blockers_apply = bool(candidate_dependency_scope.get("global_dependency_blockers_apply_to_candidate"))
    dry_run_state = candidate_gate_state(
        hard_errors=hard_errors,
        fixture_satisfied=fixture_satisfied,
        dependencies_clear=dependencies_clear,
        candidate_dependency_blockers_present=candidate_blockers,
        global_dependency_blockers_present=global_blockers_present,
        global_dependency_blockers_apply_to_candidate=global_blockers_apply,
    )
    candidate_ready_for_separate_loader_design = (
        dry_run_state == "candidate_scoped_preconditions_satisfied_no_load_performed"
    )
    native_load_gate = "closed"
    if candidate_ready_for_separate_loader_design:
        native_load_gate = "closed_dry_run_ready_for_separate_loader_design"

    gates = [
        gate_item(
            "G2_worker_design_packet",
            "satisfied" if not design_errors else "failed",
            str(worker_design_path),
            design_errors,
        ),
        gate_item(
            "G4_no_load_worker_selftest",
            "satisfied" if not selftest_errors else "failed",
            str(worker_selftest_path),
            selftest_errors,
        ),
        gate_item(
            "G3_manual_fixture_approval",
            "satisfied" if fixture_satisfied else "not_satisfied",
            str(fixture_decision_path),
            [] if fixture_satisfied else ["fixture decision is not explicit approval for native load gate"],
        ),
        gate_item(
            "G3b_fixture_manual_review_packet",
            "satisfied" if not manual_review_errors else "failed",
            str(fixture_manual_review_path),
            manual_review_errors,
        ),
        gate_item(
            "G2c_candidate_dependency_scope",
            "satisfied" if dependencies_clear else "not_satisfied",
            str(candidate_dependency_scope_path),
            scope_errors
            + (
                []
                if dependencies_clear
                else ["candidate dependency blockers, reviews, or missing/API-set rows are still present"]
            ),
        ),
        gate_item(
            "G5_candidate_scoped_native_load_gate_dry_run",
            "ready_for_separate_loader_design" if candidate_ready_for_separate_loader_design else "closed",
            "this report",
            [
                "This dry-run never opens, hashes, copies, loads, or executes an AEX.",
                "A separate loader still requires explicit user action before accepting an AEX path.",
            ],
        ),
    ]
    if source_load_gate_path is not None:
        gates.insert(
            5,
            gate_item(
                "G2d_source_global_load_gate_comparison",
                "satisfied" if not source_gate_errors else "failed",
                str(source_load_gate_path),
                source_gate_errors,
            ),
        )

    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_load_gate_dryrun",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_worker_design": str(worker_design_path),
        "source_worker_selftest": str(worker_selftest_path),
        "source_fixture_decision": str(fixture_decision_path),
        "source_fixture_manual_review": str(fixture_manual_review_path),
        "source_candidate_dependency_scope": str(candidate_dependency_scope_path),
        "source_load_gate": str(source_load_gate_path) if source_load_gate_path else None,
        "candidate_relative_path": candidate_relative_path,
        "primary_review_candidate": primary_candidate(worker_design),
        "candidate_load_gate_dryrun_state": (
            "candidate_load_gate_dryrun_invalid_evidence_closed"
            if hard_errors
            else "candidate_load_gate_dryrun_ready_no_load"
        ),
        "candidate_scoped_load_gate_dry_run_state": dry_run_state,
        "gate_state": dry_run_state,
        "candidate_gate_ready_for_separate_loader_design": candidate_ready_for_separate_loader_design,
        "native_load_gate": native_load_gate,
        "fixture_approval_satisfied": fixture_satisfied,
        "fixture_decision_manifest_kind": fixture_decision.get("manifest_kind"),
        "fixture_decision_state": fixture_decision.get("decision_state"),
        "fixture_approval_state": fixture_decision.get("approval_state"),
        "approval_state": fixture_decision.get("approval_state"),
        "candidate_scope_ready": candidate_dependency_scope.get("candidate_scope_ready"),
        "fixture_manual_review_ready": fixture_manual_review.get("manual_review_ready"),
        "fixture_manual_review_approval_ready": fixture_manual_review.get("approval_ready"),
        "candidate_dependencies_clear": dependencies_clear,
        "candidate_dependency_blockers_present": candidate_blockers,
        "candidate_dependency_blocker_count": candidate_dependency_scope.get("candidate_dependency_blocker_count"),
        "candidate_dependency_review_count": candidate_dependency_scope.get("candidate_dependency_review_count"),
        "candidate_dependency_missing_or_api_set_review_count": candidate_dependency_scope.get(
            "candidate_dependency_missing_or_api_set_review_count"
        ),
        "candidate_dependency_found_paths_exported": candidate_dependency_scope.get(
            "candidate_dependency_found_paths_exported"
        ),
        "global_dependency_blockers_present": global_blockers_present,
        "global_dependency_blockers_apply_to_candidate": global_blockers_apply,
        "source_load_gate_state": source_load_gate.get("gate_state") if isinstance(source_load_gate, dict) else None,
        "source_load_gate_dependency_recommendation": source_load_gate.get("dependency_native_load_recommendation")
        if isinstance(source_load_gate, dict)
        else None,
        "scoped_gate_recommendation": candidate_dependency_scope.get("scoped_gate_recommendation"),
        "gate_errors": hard_errors,
        "gates": gates,
        "blocked_actions": BLOCKED_ACTIONS,
        "allowed_next_actions": [
            "manual fixture provenance/license review",
            "create explicit local-only fixture approval only after user review",
            "design a separate loader stub that still defaults closed",
        ]
        if not candidate_ready_for_separate_loader_design
        else [
            "design a separate native loader stub that still defaults closed",
            "request explicit user action before opening or copying any AEX fixture",
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
            "This report is produced from JSON evidence only.",
            "Candidate-scoped dependency clarity does not approve native loading.",
            "No AEX file or DLL is opened, copied, hashed, loaded, or executed.",
            "The current gate remains closed unless explicit fixture approval and a separate loader design exist.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build candidate-scoped no-load AEX gate dry-run report")
    parser.add_argument("--worker-design", required=True, help="Worker design JSON under target/worker-design")
    parser.add_argument("--worker-selftest", required=True, help="Worker selftest JSON under target/worker-selftest")
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
    parser.add_argument("--source-load-gate", help="Optional global load gate JSON under target/load-gate")
    parser.add_argument("--out", required=True, help="Create-new report under target/candidate-load-gate")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    design, design_path = load_worker_design(Path(args.worker_design))
    selftest, selftest_path = load_worker_selftest(Path(args.worker_selftest))
    decision, decision_path = load_fixture_decision(Path(args.fixture_decision))
    manual_review, manual_review_path = load_fixture_manual_review(Path(args.fixture_manual_review))
    scope, scope_path = load_candidate_dependency_scope(Path(args.candidate_dependency_scope))
    source_load_gate = None
    source_load_gate_path = None
    if args.source_load_gate:
        source_load_gate, source_load_gate_path = load_source_load_gate(Path(args.source_load_gate))
    report = build_candidate_load_gate_dryrun(
        worker_design=design,
        worker_design_path=design_path,
        worker_selftest=selftest,
        worker_selftest_path=selftest_path,
        fixture_decision=decision,
        fixture_decision_path=decision_path,
        fixture_manual_review=manual_review,
        fixture_manual_review_path=manual_review_path,
        candidate_dependency_scope=scope,
        candidate_dependency_scope_path=scope_path,
        source_load_gate=source_load_gate,
        source_load_gate_path=source_load_gate_path,
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

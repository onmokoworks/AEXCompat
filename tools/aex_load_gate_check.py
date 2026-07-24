#!/usr/bin/env python3
"""No-load AEX loader gate verifier.

The checker reads JSON evidence only. It never opens, copies, hashes, loads, or
executes AEX files. Its job is to keep native load closed until all gate evidence
exists and explicit approval artifacts are present.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
WORKER_DESIGN_ROOT = LAB_ROOT / "target" / "worker-design"
WORKER_SELFTEST_ROOT = LAB_ROOT / "target" / "worker-selftest"
FIXTURE_APPROVAL_ROOT = LAB_ROOT / "target" / "fixture-approval"
DEPENDENCY_REVIEW_ROOT = LAB_ROOT / "target" / "dependency-review"
LOAD_GATE_ROOT = LAB_ROOT / "target" / "load-gate"

SAFETY_FLAGS = (
    "native_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
)

BLOCKED_ACTIONS = [
    "copy_selected_aex_fixture",
    "load_aex_dll",
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
    root.mkdir(parents=True, exist_ok=True)
    return resolve_under_root(path, root, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("load gate report must have .json extension")
    LOAD_GATE_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, LOAD_GATE_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(LOAD_GATE_ROOT.resolve(strict=True)):
        raise ValueError(f"load gate report parent must stay under {LOAD_GATE_ROOT}")
    return resolved


def load_json(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return payload


def safety_errors(payload: dict[str, Any], label: str, *, include_aex_opened: bool = False) -> list[str]:
    flags = list(SAFETY_FLAGS)
    if include_aex_opened:
        flags.append("aex_file_opened")
    errors: list[str] = []
    for flag in flags:
        if payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def validate_design_packet(packet: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if packet.get("packet_kind") != "aex_worker_sandbox_design_packet":
        errors.append("design packet_kind must be aex_worker_sandbox_design_packet")
    if packet.get("publication_status") != "local-only":
        errors.append("design publication_status must be local-only")
    errors.extend(safety_errors(packet, "design"))
    if packet.get("design_state") != "no_load_worker_boundary_only":
        errors.append("design_state must be no_load_worker_boundary_only")
    candidate = packet.get("primary_review_candidate")
    if not isinstance(candidate, dict):
        errors.append("design primary_review_candidate must be an object")
    else:
        if candidate.get("approval_state") != "not_approved_for_load":
            errors.append("design primary candidate must remain not_approved_for_load")
        if candidate.get("compatibility_class") != "classic_pf_effect_candidate":
            errors.append("design primary candidate must be classic_pf_effect_candidate")
        if candidate.get("effect_main_export_present") is not True:
            errors.append("design primary candidate must export EffectMain")
        if int(candidate.get("aegp_marker_count") or 0) != 0:
            errors.append("design primary candidate must not include AEGP markers")
    blocked = packet.get("blocked_actions", [])
    for action in ("load_aex_dll", "call_EffectMain", "render_with_aex", "route_through_ofx"):
        if action not in blocked:
            errors.append(f"design packet must block {action}")
    return errors


def validate_worker_selftest(report: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if report.get("report_kind") != "aex_no_load_worker_selftest":
        errors.append("selftest report_kind must be aex_no_load_worker_selftest")
    if report.get("publication_status") != "local-only":
        errors.append("selftest publication_status must be local-only")
    errors.extend(safety_errors(report, "selftest", include_aex_opened=True))
    if report.get("worker_selftest_passed") is not True:
        errors.append("worker_selftest_passed must be true")
    steps = report.get("steps")
    if not isinstance(steps, list):
        errors.append("selftest steps must be a list")
    else:
        step_names = [step.get("step") for step in steps if isinstance(step, dict)]
        for required in ("hello", "inspect_environment", "inspect_ppm", "transform_ppm_identity", "blocked_load_aex", "quit"):
            if required not in step_names:
                errors.append(f"selftest missing step {required}")
        blocked_steps = [step for step in steps if isinstance(step, dict) and step.get("step") == "blocked_load_aex"]
        if not blocked_steps or blocked_steps[0].get("code") != "blocked_action":
            errors.append("selftest must prove load_aex fails with blocked_action")
    return errors


def validate_fixture_approval(approval: dict[str, Any], candidate: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if approval.get("manifest_kind") == "aex_fixture_decision_manifest":
        errors.extend(safety_errors(approval, "approval"))
        if approval.get("candidate_relative_path") != candidate.get("relative_path"):
            errors.append("decision candidate_relative_path must match design primary candidate")
        decision_state = approval.get("decision_state", "unknown")
        errors.append(f"fixture decision is not an approval: {decision_state}")
        return errors
    if approval.get("manifest_kind") != "aex_fixture_approval_manifest":
        errors.append("approval manifest_kind must be aex_fixture_approval_manifest")
    if approval.get("publication_status") != "local-only":
        errors.append("approval publication_status must be local-only")
    errors.extend(safety_errors(approval, "approval"))
    if approval.get("approval_state") != "user_approved_for_load_gate":
        errors.append("approval_state must be user_approved_for_load_gate")
    if approval.get("explicit_user_approval") is not True:
        errors.append("explicit_user_approval must be true")
    if approval.get("candidate_relative_path") != candidate.get("relative_path"):
        errors.append("approval candidate_relative_path must match design primary candidate")
    approved_actions = approval.get("approved_actions", [])
    if "prepare_native_load_gate" not in approved_actions:
        errors.append("approval must include prepare_native_load_gate")
    return errors


def validate_dependency_review(review: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if review.get("packet_kind") != "aex_dependency_review_packet":
        errors.append("dependency review packet_kind must be aex_dependency_review_packet")
    if review.get("publication_status") != "local-only":
        errors.append("dependency review publication_status must be local-only")
    for flag in (
        "native_load_enabled",
        "native_load_performed",
        "dll_load_performed",
        "render_performed",
        "ae_invoked",
        "ofx_route_invoked",
        "private_payload_copied",
        "aex_file_opened",
    ):
        if review.get(flag) is not False:
            errors.append(f"dependency review {flag} must be false")
    recommendation = review.get("native_load_recommendation")
    if recommendation == "do_not_open_native_load_gate":
        errors.append("dependency review recommendation blocks native load")
    elif recommendation == "hold_native_load_until_dependency_review_complete":
        errors.append("dependency review is still pending")
    elif recommendation != "manual_loader_design_review_only_no_auto_approval":
        errors.append("dependency review native_load_recommendation is not recognized")
    return errors


def load_evidence(
    *,
    design_packet_path: Path,
    worker_selftest_path: Path,
    dependency_review_path: Path,
    approval_path: Path | None,
) -> tuple[
    dict[str, Any],
    Path,
    dict[str, Any],
    Path,
    dict[str, Any],
    Path,
    dict[str, Any] | None,
    Path | None,
]:
    resolved_design = validate_json_input(design_packet_path, WORKER_DESIGN_ROOT, "design packet")
    resolved_selftest = validate_json_input(worker_selftest_path, WORKER_SELFTEST_ROOT, "worker selftest report")
    resolved_dependency_review = validate_json_input(
        dependency_review_path,
        DEPENDENCY_REVIEW_ROOT,
        "dependency review packet",
    )
    design = load_json(resolved_design)
    selftest = load_json(resolved_selftest)
    dependency_review = load_json(resolved_dependency_review)
    approval = None
    resolved_approval = None
    if approval_path is not None:
        resolved_approval = validate_json_input(approval_path, FIXTURE_APPROVAL_ROOT, "fixture approval manifest")
        approval = load_json(resolved_approval)
    return (
        design,
        resolved_design,
        selftest,
        resolved_selftest,
        dependency_review,
        resolved_dependency_review,
        approval,
        resolved_approval,
    )


def gate_item(gate: str, status: str, evidence: str, notes: list[str] | None = None) -> dict[str, Any]:
    return {
        "gate": gate,
        "status": status,
        "evidence": evidence,
        "notes": notes or [],
    }


def build_gate_report(
    *,
    design_packet: dict[str, Any],
    design_packet_path: Path,
    worker_selftest: dict[str, Any],
    worker_selftest_path: Path,
    dependency_review: dict[str, Any] | None = None,
    dependency_review_path: Path | None = None,
    fixture_approval: dict[str, Any] | None = None,
    fixture_approval_path: Path | None = None,
) -> dict[str, Any]:
    design_errors = validate_design_packet(design_packet)
    selftest_errors = validate_worker_selftest(worker_selftest)
    if dependency_review is None:
        dependency_review_state = "missing"
        dependency_errors = ["dependency review packet is missing"]
    else:
        dependency_review_state = "present"
        dependency_errors = validate_dependency_review(dependency_review)
    candidate = design_packet.get("primary_review_candidate") if isinstance(design_packet.get("primary_review_candidate"), dict) else {}
    approval_errors: list[str] = []
    if fixture_approval is None:
        approval_state = "missing"
        approval_errors = ["fixture approval manifest is missing"]
    else:
        approval_state = "present"
        approval_errors = validate_fixture_approval(fixture_approval, candidate)

    hard_errors = design_errors + selftest_errors
    missing_or_invalid = dependency_errors + approval_errors
    if hard_errors:
        gate_state = "invalid_evidence_closed"
    elif dependency_errors:
        gate_state = "closed_dependency_review_or_invalid_approval"
    elif missing_or_invalid:
        gate_state = "closed_missing_or_invalid_approval"
    else:
        gate_state = "preconditions_satisfied_no_load_performed"

    gates = [
        gate_item(
            "G2_worker_design_packet",
            "satisfied" if not design_errors else "failed",
            str(design_packet_path),
            design_errors,
        ),
        gate_item(
            "G4_no_load_worker_selftest",
            "satisfied" if not selftest_errors else "failed",
            str(worker_selftest_path),
            selftest_errors,
        ),
        gate_item(
            "G2b_dependency_review",
            "satisfied" if not dependency_errors else "not_satisfied",
            str(dependency_review_path) if dependency_review_path else "missing",
            dependency_errors,
        ),
        gate_item(
            "G3_manual_fixture_approval",
            "satisfied" if not approval_errors else "not_satisfied",
            str(fixture_approval_path) if fixture_approval_path else "missing",
            approval_errors,
        ),
        gate_item(
            "G5_native_load_gate",
            "closed" if gate_state != "preconditions_satisfied_no_load_performed" else "ready_for_separate_loader_design",
            "this report",
            [
                "This checker never performs native load.",
                "A later loader implementation still requires explicit user action before any AEX is opened or loaded.",
            ],
        ),
    ]
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_load_gate_check",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_design_packet": str(design_packet_path),
        "source_worker_selftest": str(worker_selftest_path),
        "source_dependency_review": str(dependency_review_path) if dependency_review_path else None,
        "source_fixture_approval": str(fixture_approval_path) if fixture_approval_path else None,
        "primary_review_candidate": candidate,
        "approval_state": approval_state,
        "dependency_review_state": dependency_review_state,
        "dependency_native_load_recommendation": dependency_review.get("native_load_recommendation")
        if isinstance(dependency_review, dict)
        else None,
        "gate_state": gate_state,
        "gate_errors": hard_errors + missing_or_invalid,
        "gates": gates,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "blocked_actions": BLOCKED_ACTIONS,
        "allowed_next_actions": [
            "manual fixture provenance/license review",
            "create local-only fixture approval or rejection manifest",
        ]
        if gate_state != "preconditions_satisfied_no_load_performed"
        else [
            "design a separate native loader stub that still defaults closed",
            "request explicit user approval before opening or copying any AEX fixture",
        ],
        "notes": [
            "This report is produced from JSON evidence only.",
            "No AEX file is opened, copied, hashed, loaded, or executed.",
            "A closed gate is the expected state until manual fixture approval exists.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Check no-load AEX native load gate evidence")
    parser.add_argument("--design-packet", required=True, help="Design packet JSON under target/worker-design")
    parser.add_argument("--worker-selftest", required=True, help="Worker selftest JSON under target/worker-selftest")
    parser.add_argument("--dependency-review", required=True, help="Dependency review JSON under target/dependency-review")
    parser.add_argument("--fixture-approval", help="Optional fixture approval JSON under target/fixture-approval")
    parser.add_argument("--out", required=True, help="Create-new load gate report under target/load-gate")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    (
        design,
        design_path,
        selftest,
        selftest_path,
        dependency_review,
        dependency_review_path,
        approval,
        approval_path,
    ) = load_evidence(
        design_packet_path=Path(args.design_packet),
        worker_selftest_path=Path(args.worker_selftest),
        dependency_review_path=Path(args.dependency_review),
        approval_path=Path(args.fixture_approval) if args.fixture_approval else None,
    )
    report = build_gate_report(
        design_packet=design,
        design_packet_path=design_path,
        worker_selftest=selftest,
        worker_selftest_path=selftest_path,
        dependency_review=dependency_review,
        dependency_review_path=dependency_review_path,
        fixture_approval=approval,
        fixture_approval_path=approval_path,
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0 if not report["gate_errors"] else 1


if __name__ == "__main__":
    raise SystemExit(main())

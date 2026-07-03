#!/usr/bin/env python3
"""Build a no-load native-loader design contract.

The contract reads JSON evidence only. It defines the future native-loader
boundary while keeping AEX path acceptance, DLL load, EffectMain calls, AE,
render, and OFX routing closed.
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
SANDBOX_POLICY_ROOT = TARGET_ROOT / "sandbox-policy"
CANDIDATE_LOAD_GATE_ROOT = TARGET_ROOT / "candidate-load-gate"
NATIVE_LOADER_STUB_ROOT = TARGET_ROOT / "native-loader-stub"
RENDER_VALIDATION_CONTRACT_ROOT = TARGET_ROOT / "render-validation-contract"
OFX_ROUTE_CONTRACT_ROOT = TARGET_ROOT / "ofx-route-contract"
NATIVE_LOADER_DESIGN_ROOT = TARGET_ROOT / "native-loader-design"

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
        raise ValueError("native loader design contract must have .json extension")
    NATIVE_LOADER_DESIGN_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, NATIVE_LOADER_DESIGN_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(
        NATIVE_LOADER_DESIGN_ROOT.resolve(strict=True)
    ):
        raise ValueError(f"native loader design contract parent must stay under {NATIVE_LOADER_DESIGN_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_worker_design(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, WORKER_DESIGN_ROOT, "worker design")
    return read_json_object(resolved), resolved


def load_sandbox_policy(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, SANDBOX_POLICY_ROOT, "sandbox policy")
    return read_json_object(resolved), resolved


def load_candidate_load_gate(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, CANDIDATE_LOAD_GATE_ROOT, "candidate load gate dry-run")
    return read_json_object(resolved), resolved


def load_native_loader_stub(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, NATIVE_LOADER_STUB_ROOT, "native loader stub")
    return read_json_object(resolved), resolved


def load_render_validation_contract(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, RENDER_VALIDATION_CONTRACT_ROOT, "render validation contract")
    return read_json_object(resolved), resolved


def load_ofx_route_contract(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, OFX_ROUTE_CONTRACT_ROOT, "OFX route contract")
    return read_json_object(resolved), resolved


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def primary_candidate_path(worker_design: dict[str, Any]) -> str | None:
    candidate = worker_design.get("primary_review_candidate")
    if isinstance(candidate, dict) and isinstance(candidate.get("relative_path"), str):
        return candidate["relative_path"]
    return None


def validate_worker_design(worker_design: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if worker_design.get("publication_status") != "local-only":
        errors.append("worker design publication_status must be local-only")
    if worker_design.get("packet_kind") != "aex_worker_sandbox_design_packet":
        errors.append("worker design packet_kind must be aex_worker_sandbox_design_packet")
    if worker_design.get("design_state") != "no_load_worker_boundary_only":
        errors.append("worker design_state must be no_load_worker_boundary_only")
    errors.extend(safety_errors(worker_design, "worker design"))
    candidate = worker_design.get("primary_review_candidate")
    if not isinstance(candidate, dict):
        errors.append("worker design primary_review_candidate must be an object")
    else:
        if candidate.get("approval_state") != "not_approved_for_load":
            errors.append("worker design candidate approval_state must remain not_approved_for_load")
        if candidate.get("compatibility_class") != "classic_pf_effect_candidate":
            errors.append("worker design candidate must be classic_pf_effect_candidate")
        if candidate.get("effect_main_export_present") is not True:
            errors.append("worker design candidate must export EffectMain")
        if int(candidate.get("aegp_marker_count") or 0) != 0:
            errors.append("worker design candidate must not include AEGP markers")
    blocked = worker_design.get("blocked_actions", [])
    for action in ("load_aex_dll", "call_EffectMain", "render_with_aex", "route_through_ofx"):
        if action not in blocked:
            errors.append(f"worker design must block {action}")
    return errors


def validate_sandbox_policy(sandbox_policy: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if sandbox_policy.get("publication_status") != "local-only":
        errors.append("sandbox policy publication_status must be local-only")
    if sandbox_policy.get("packet_kind") != "aex_sandbox_policy_packet":
        errors.append("sandbox policy packet_kind must be aex_sandbox_policy_packet")
    if sandbox_policy.get("sandbox_policy_state") != "policy_ready_no_native_load":
        errors.append("sandbox policy state must be policy_ready_no_native_load")
    errors.extend(safety_errors(sandbox_policy, "sandbox policy"))
    required = sandbox_policy.get("required_before_native_load", [])
    for item in (
        "explicit user fixture approval artifact",
        "passing load gate using the approved fixture",
        "worker process isolation design review",
    ):
        if item not in required:
            errors.append(f"sandbox policy required_before_native_load must include {item}")
    return errors


def validate_candidate_load_gate(candidate_gate: dict[str, Any], candidate_path: str | None) -> list[str]:
    errors: list[str] = []
    if candidate_gate.get("publication_status") != "local-only":
        errors.append("candidate load gate publication_status must be local-only")
    if candidate_gate.get("report_kind") != "aex_candidate_load_gate_dryrun":
        errors.append("candidate load gate report_kind must be aex_candidate_load_gate_dryrun")
    if candidate_gate.get("candidate_load_gate_dryrun_state") != "candidate_load_gate_dryrun_ready_no_load":
        errors.append("candidate load gate dryrun state must be ready no-load")
    if candidate_gate.get("native_load_gate") != "closed":
        errors.append("candidate load gate native_load_gate must be closed")
    if candidate_gate.get("fixture_approval_satisfied") is not False:
        errors.append("candidate load gate fixture approval must not be satisfied in this design contract")
    if candidate_gate.get("candidate_dependencies_clear") is not True:
        errors.append("candidate load gate candidate_dependencies_clear must be true")
    if candidate_gate.get("candidate_dependency_blockers_present") is not False:
        errors.append("candidate load gate candidate dependency blockers must be false")
    if candidate_gate.get("candidate_relative_path") != candidate_path:
        errors.append("candidate load gate candidate_relative_path must match worker design primary candidate")
    errors.extend(safety_errors(candidate_gate, "candidate load gate"))
    return errors


def validate_native_loader_stub(stub: dict[str, Any], candidate_path: str | None) -> list[str]:
    errors: list[str] = []
    if stub.get("publication_status") != "local-only":
        errors.append("native loader stub publication_status must be local-only")
    if stub.get("report_kind") != "aex_native_loader_stub_report":
        errors.append("native loader stub report_kind must be aex_native_loader_stub_report")
    if stub.get("stub_state") not in {"refused_gate_closed", "stub_ready_no_load_performed"}:
        errors.append("native loader stub state must be closed or ready no-load")
    if stub.get("loader_action") != "no_op":
        errors.append("native loader stub loader_action must be no_op")
    if stub.get("accepted_aex_path") is not None:
        errors.append("native loader stub accepted_aex_path must be null")
    candidate = stub.get("primary_review_candidate")
    if isinstance(candidate, dict) and candidate.get("relative_path") != candidate_path:
        errors.append("native loader stub primary candidate must match worker design primary candidate")
    blocked = stub.get("blocked_actions", [])
    for action in ("accept_aex_path", "open_aex_file", "load_aex_dll", "call_EffectMain"):
        if action not in blocked:
            errors.append(f"native loader stub must block {action}")
    errors.extend(safety_errors(stub, "native loader stub"))
    return errors


def validate_render_contract(render_contract: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if render_contract.get("publication_status") != "local-only":
        errors.append("render validation contract publication_status must be local-only")
    if render_contract.get("report_kind") != "aex_render_validation_contract":
        errors.append("render validation contract report_kind must be aex_render_validation_contract")
    if render_contract.get("contract_state") != "render_validation_contract_ready_render_closed":
        errors.append("render validation contract state must be ready render closed")
    if render_contract.get("real_render_open") is not False:
        errors.append("render validation contract real_render_open must be false")
    if render_contract.get("no_load_validation_ready") is not True:
        errors.append("render validation contract no_load_validation_ready must be true")
    errors.extend(safety_errors(render_contract, "render validation contract"))
    return errors


def validate_ofx_route_contract(ofx_contract: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if ofx_contract.get("publication_status") != "local-only":
        errors.append("OFX route contract publication_status must be local-only")
    if ofx_contract.get("report_kind") != "aex_ofx_route_contract_probe":
        errors.append("OFX route contract report_kind must be aex_ofx_route_contract_probe")
    if ofx_contract.get("contract_state") != "ofx_route_contract_ready_route_closed":
        errors.append("OFX route contract state must be ready route closed")
    if ofx_contract.get("real_route_open") is not False:
        errors.append("OFX route contract real_route_open must be false")
    if ofx_contract.get("mock_route_ready") is not True:
        errors.append("OFX route contract mock_route_ready must be true")
    errors.extend(safety_errors(ofx_contract, "OFX route contract"))
    return errors


def build_loader_contract(candidate_path: str | None) -> dict[str, Any]:
    return {
        "state": "loader_contract_defined_acceptance_closed",
        "candidate_relative_path": candidate_path,
        "controller_rule": "controller_must_never_load_aex_or_dependency_dlls",
        "loader_worker_rule": "future_worker_must_start_pathless_and_report_safety_state_before_accepting_any_path",
        "separate_process_required": True,
        "accepts_aex_path": False,
        "accepted_aex_path": None,
        "controller_loads_aex": False,
        "broker_protocol_allowed_now": [
            {"type": "hello", "purpose": "version and closed-safety handshake"},
            {"type": "inspect_environment", "purpose": "bitness, sandbox, and native_load_enabled=false report"},
            {"type": "quit", "purpose": "stop without accepting paths"},
        ],
        "messages_reserved_until_later_gate": [
            {"type": "accept_aex_path", "requires": ["explicit fixture approval", "passing scoped load gate"]},
            {"type": "open_aex_file", "requires": ["path allowlist", "create-new log root", "runtime approval"]},
            {"type": "load_aex_dll", "requires": ["sandbox review", "dependency policy", "runtime approval"]},
            {"type": "call_effect_main", "requires": ["selector allowlist", "loaded module handle", "runtime approval"]},
            {"type": "render_frame", "requires": ["render validation gate", "runtime approval"]},
            {"type": "ofx_route", "requires": ["OFX route gate", "runtime approval"]},
        ],
    }


def build_phase_plan() -> list[dict[str, Any]]:
    return [
        {
            "phase": "P0_design_contract_only",
            "status": "current",
            "allowed_actions": ["read_json_evidence", "write_create_new_design_contract"],
            "blocked_actions": BLOCKED_ACTIONS,
        },
        {
            "phase": "P1_pathless_loader_broker_selftest",
            "status": "future_no_load",
            "entry_condition": "this design contract plus explicit implementation review",
            "blocked_actions": ["accept_aex_path", "open_aex_file", "load_aex_dll", "call_EffectMain"],
        },
        {
            "phase": "P2_approved_fixture_path_acceptance",
            "status": "future_requires_explicit_user_approval",
            "entry_condition": "fixture approval manifest and passing scoped load gate",
            "blocked_actions": ["load_aex_dll", "call_EffectMain", "render_with_aex", "route_through_ofx"],
        },
        {
            "phase": "P3_first_native_load_experiment",
            "status": "future_requires_separate_runtime_approval",
            "entry_condition": "sandboxed worker, dependency policy, path acceptance, and crash containment reviewed",
            "blocked_actions": ["call_EffectMain", "render_with_aex", "route_through_ofx"],
        },
    ]


def build_required_evidence() -> list[dict[str, Any]]:
    return [
        {
            "gate": "explicit_fixture_approval",
            "current_state": "missing",
            "required_artifact": "aex_fixture_approval_manifest",
        },
        {
            "gate": "candidate_scoped_load_gate",
            "current_state": "closed_pending_fixture_approval",
            "required_state": "candidate_scoped_preconditions_satisfied_no_load_performed",
        },
        {
            "gate": "pathless_loader_broker_selftest",
            "current_state": "missing",
            "required_state": "broker_accepts_no_aex_path_and_reports_native_load_enabled_false",
        },
        {
            "gate": "runtime_containment_review",
            "current_state": "missing",
            "required_items": ["timeout", "crash isolation", "child cleanup", "log capture", "path allowlist"],
        },
        {
            "gate": "render_and_ofx_route_review",
            "current_state": "closed",
            "required_artifact": "separate render/OFX route approval after first native-load evidence",
        },
    ]


def build_approval_contract(candidate_gate: dict[str, Any]) -> dict[str, Any]:
    return {
        "required_manifest_kind": "aex_fixture_approval_manifest",
        "required_approval_state": "user_approved_for_load_gate",
        "requires_publication_status": "local-only",
        "requires_explicit_user_approval": True,
        "required_approved_actions": ["prepare_native_load_gate"],
        "approval_token_name": "APPROVE_AEX_LOAD_GATE",
        "approval_only_prepares_next_gate": True,
        "approval_does_not_permit_native_load": True,
        "current_manifest_kind": candidate_gate.get("fixture_decision_manifest_kind"),
        "current_decision_state": candidate_gate.get("fixture_decision_state"),
        "current_approval_state": candidate_gate.get("fixture_approval_state"),
        "current_fixture_approval_satisfied": candidate_gate.get("fixture_approval_satisfied"),
    }


def build_dependency_gate_contract(candidate_gate: dict[str, Any]) -> dict[str, Any]:
    return {
        "required_dependency_review_endpoint": "manual_loader_design_review_only_no_auto_approval",
        "current_source_load_gate_state": candidate_gate.get("source_load_gate_state"),
        "current_source_load_gate_dependency_recommendation": candidate_gate.get(
            "source_load_gate_dependency_recommendation"
        ),
        "candidate_dependencies_clear": candidate_gate.get("candidate_dependencies_clear"),
        "candidate_dependency_blockers_present": candidate_gate.get("candidate_dependency_blockers_present"),
        "global_dependency_blockers_present": candidate_gate.get("global_dependency_blockers_present"),
        "global_dependency_blockers_apply_to_candidate": candidate_gate.get(
            "global_dependency_blockers_apply_to_candidate"
        ),
        "scoped_gate_recommendation": candidate_gate.get("scoped_gate_recommendation"),
    }


def build_native_loader_design_contract(
    *,
    worker_design: dict[str, Any],
    worker_design_path: Path,
    sandbox_policy: dict[str, Any],
    sandbox_policy_path: Path,
    candidate_load_gate: dict[str, Any],
    candidate_load_gate_path: Path,
    native_loader_stub: dict[str, Any],
    native_loader_stub_path: Path,
    render_validation_contract: dict[str, Any],
    render_validation_contract_path: Path,
    ofx_route_contract: dict[str, Any],
    ofx_route_contract_path: Path,
) -> dict[str, Any]:
    candidate_path = primary_candidate_path(worker_design)
    errors = (
        validate_worker_design(worker_design)
        + validate_sandbox_policy(sandbox_policy)
        + validate_candidate_load_gate(candidate_load_gate, candidate_path)
        + validate_native_loader_stub(native_loader_stub, candidate_path)
        + validate_render_contract(render_validation_contract)
        + validate_ofx_route_contract(ofx_route_contract)
    )
    if errors:
        raise ValueError("; ".join(errors))

    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_native_loader_design_contract",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_worker_design": str(worker_design_path),
        "source_sandbox_policy": str(sandbox_policy_path),
        "source_candidate_load_gate": str(candidate_load_gate_path),
        "source_native_loader_stub": str(native_loader_stub_path),
        "source_render_validation_contract": str(render_validation_contract_path),
        "source_ofx_route_contract": str(ofx_route_contract_path),
        "native_loader_design_state": "native_loader_design_ready_loader_closed",
        "contract_state": "native_loader_design_contract_ready_loader_closed_pending_fixture_approval",
        "loader_design_ready": True,
        "candidate_relative_path": candidate_path,
        "candidate_load_gate_state": candidate_load_gate.get("candidate_scoped_load_gate_dry_run_state"),
        "source_candidate_load_gate_state": candidate_load_gate.get("candidate_scoped_load_gate_dry_run_state"),
        "candidate_dependencies_clear": candidate_load_gate.get("candidate_dependencies_clear"),
        "fixture_approval_satisfied": candidate_load_gate.get("fixture_approval_satisfied"),
        "source_stub_state": native_loader_stub.get("stub_state"),
        "source_sandbox_policy_state": sandbox_policy.get("sandbox_policy_state"),
        "source_render_contract_state": render_validation_contract.get("contract_state"),
        "source_ofx_route_contract_state": ofx_route_contract.get("contract_state"),
        "native_load_gate": "closed",
        "approval_required_before_aex_path": True,
        "runtime_approval_required_before_load": True,
        "separate_process_required": True,
        "accepts_aex_path": False,
        "accepted_aex_path": None,
        "controller_loads_aex": False,
        "approval_contract": build_approval_contract(candidate_load_gate),
        "dependency_gate_contract": build_dependency_gate_contract(candidate_load_gate),
        "loader_contract": build_loader_contract(candidate_path),
        "phase_plan": build_phase_plan(),
        "required_evidence_before_aex_path_acceptance": build_required_evidence(),
        "blocked_actions": BLOCKED_ACTIONS,
        "allowed_next_actions": [
            "implement a pathless loader broker selftest",
            "extend the broker to report native_load_enabled=false before any path acceptance",
            "complete manual fixture approval review",
            "repeat candidate-scoped load gate dry-run after approval artifact exists",
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
            "This contract reads JSON evidence only.",
            "It is not a native loader implementation and accepts no AEX path.",
            "The controller must not load AEX files; any future load experiment must be out-of-process.",
            "Candidate dependency clarity does not override missing fixture approval.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build no-load native-loader design contract")
    parser.add_argument("--worker-design", required=True, help="Worker design JSON under target/worker-design")
    parser.add_argument("--sandbox-policy", required=True, help="Sandbox policy JSON under target/sandbox-policy")
    parser.add_argument(
        "--candidate-load-gate",
        required=True,
        help="Candidate load gate dry-run JSON under target/candidate-load-gate",
    )
    parser.add_argument(
        "--native-loader-stub",
        required=True,
        help="Native loader stub JSON under target/native-loader-stub",
    )
    parser.add_argument(
        "--render-validation-contract",
        required=True,
        help="Render validation contract JSON under target/render-validation-contract",
    )
    parser.add_argument(
        "--ofx-route-contract",
        required=True,
        help="OFX route contract JSON under target/ofx-route-contract",
    )
    parser.add_argument("--out", required=True, help="Create-new contract under target/native-loader-design")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    worker_design, worker_design_path = load_worker_design(Path(args.worker_design))
    sandbox_policy, sandbox_policy_path = load_sandbox_policy(Path(args.sandbox_policy))
    candidate_load_gate, candidate_load_gate_path = load_candidate_load_gate(Path(args.candidate_load_gate))
    native_loader_stub, native_loader_stub_path = load_native_loader_stub(Path(args.native_loader_stub))
    render_validation_contract, render_validation_contract_path = load_render_validation_contract(
        Path(args.render_validation_contract)
    )
    ofx_route_contract, ofx_route_contract_path = load_ofx_route_contract(Path(args.ofx_route_contract))
    report = build_native_loader_design_contract(
        worker_design=worker_design,
        worker_design_path=worker_design_path,
        sandbox_policy=sandbox_policy,
        sandbox_policy_path=sandbox_policy_path,
        candidate_load_gate=candidate_load_gate,
        candidate_load_gate_path=candidate_load_gate_path,
        native_loader_stub=native_loader_stub,
        native_loader_stub_path=native_loader_stub_path,
        render_validation_contract=render_validation_contract,
        render_validation_contract_path=render_validation_contract_path,
        ofx_route_contract=ofx_route_contract,
        ofx_route_contract_path=ofx_route_contract_path,
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

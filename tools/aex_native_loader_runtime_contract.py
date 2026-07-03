#!/usr/bin/env python3
"""Build a no-load runtime containment contract for future native-loader work.

The contract reads JSON evidence only. It keeps AEX path acceptance, file
open/hash/copy, DLL load, EffectMain calls, render, AE, and OFX routing closed
while recording the containment rules required before any later path gate.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
NATIVE_LOADER_DESIGN_ROOT = TARGET_ROOT / "native-loader-design"
BROKER_SELFTEST_ROOT = TARGET_ROOT / "native-loader-broker-selftest"
CANDIDATE_LOAD_GATE_ROOT = TARGET_ROOT / "candidate-load-gate"
SANDBOX_POLICY_ROOT = TARGET_ROOT / "sandbox-policy"
RUNTIME_CONTRACT_ROOT = TARGET_ROOT / "native-loader-runtime-contract"

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
    "call_effect_main",
    "dispatch_PF_Cmd",
    "render_frame",
    "render_with_aex",
    "ofx_describe",
    "route_through_ofx",
    "start_after_effects",
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
        raise ValueError("native-loader runtime contract must have .json extension")
    RUNTIME_CONTRACT_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, RUNTIME_CONTRACT_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(RUNTIME_CONTRACT_ROOT.resolve(strict=True)):
        raise ValueError(f"native-loader runtime contract parent must stay under {RUNTIME_CONTRACT_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_native_loader_design(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, NATIVE_LOADER_DESIGN_ROOT, "native-loader design contract")
    return read_json_object(resolved), resolved


def load_broker_selftest(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, BROKER_SELFTEST_ROOT, "native-loader broker selftest")
    return read_json_object(resolved), resolved


def load_candidate_load_gate(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, CANDIDATE_LOAD_GATE_ROOT, "candidate load gate dry-run")
    return read_json_object(resolved), resolved


def load_sandbox_policy(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, SANDBOX_POLICY_ROOT, "sandbox policy")
    return read_json_object(resolved), resolved


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def validate_design_contract(contract: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if contract.get("publication_status") != "local-only":
        errors.append("native-loader design contract publication_status must be local-only")
    if contract.get("report_kind") != "aex_native_loader_design_contract":
        errors.append("native-loader design contract report_kind must be aex_native_loader_design_contract")
    if contract.get("native_loader_design_state") != "native_loader_design_ready_loader_closed":
        errors.append("native-loader design state must be native_loader_design_ready_loader_closed")
    if contract.get("contract_state") != "native_loader_design_contract_ready_loader_closed_pending_fixture_approval":
        errors.append("native-loader design contract_state must be ready loader closed pending fixture approval")
    if contract.get("loader_design_ready") is not True:
        errors.append("loader_design_ready must be true")
    if contract.get("native_load_gate") != "closed":
        errors.append("native-loader design native_load_gate must be closed")
    if contract.get("approval_required_before_aex_path") is not True:
        errors.append("approval_required_before_aex_path must be true")
    if contract.get("runtime_approval_required_before_load") is not True:
        errors.append("runtime_approval_required_before_load must be true")
    if contract.get("separate_process_required") is not True:
        errors.append("separate_process_required must be true")
    if contract.get("accepts_aex_path") is not False:
        errors.append("design contract accepts_aex_path must be false")
    if contract.get("accepted_aex_path") is not None:
        errors.append("design contract accepted_aex_path must be null")
    if contract.get("controller_loads_aex") is not False:
        errors.append("design contract controller_loads_aex must be false")
    if contract.get("candidate_dependencies_clear") is not True:
        errors.append("design contract candidate_dependencies_clear must be true")
    if contract.get("fixture_approval_satisfied") is not False:
        errors.append("design contract fixture approval must not be satisfied")
    blocked = contract.get("blocked_actions", [])
    for action in ("accept_aex_path", "open_aex_file", "load_aex_dll", "call_EffectMain", "render_with_aex"):
        if action not in blocked:
            errors.append(f"design contract must block {action}")
    errors.extend(safety_errors(contract, "native-loader design contract"))
    return errors


def validate_broker_selftest(selftest: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if selftest.get("publication_status") != "local-only":
        errors.append("native-loader broker selftest publication_status must be local-only")
    if selftest.get("report_kind") != "aex_native_loader_broker_selftest":
        errors.append("native-loader broker selftest report_kind must be aex_native_loader_broker_selftest")
    if selftest.get("broker_selftest_state") != "pathless_native_loader_broker_selftest_passed":
        errors.append("broker selftest state must be pathless_native_loader_broker_selftest_passed")
    if selftest.get("pathless_broker_ready") is not True:
        errors.append("pathless_broker_ready must be true")
    if selftest.get("native_loader_design_ready") is not True:
        errors.append("native_loader_design_ready must be true")
    if selftest.get("accepts_aex_path") is not False:
        errors.append("broker selftest accepts_aex_path must be false")
    if selftest.get("accepted_aex_path") is not None:
        errors.append("broker selftest accepted_aex_path must be null")
    if selftest.get("path_payload_supplied") is not False:
        errors.append("broker selftest path_payload_supplied must be false")
    if selftest.get("blocked_action_count") != 6:
        errors.append("broker selftest blocked_action_count must be 6")
    if selftest.get("candidate_dependencies_clear") is not True:
        errors.append("broker selftest candidate_dependencies_clear must be true")
    if selftest.get("fixture_approval_satisfied") is not False:
        errors.append("broker selftest fixture approval must not be satisfied")
    checks = selftest.get("blocked_action_checks", [])
    if not isinstance(checks, list) or len(checks) != 6:
        errors.append("broker selftest must include six blocked_action_checks")
    else:
        for check in checks:
            if not isinstance(check, dict):
                errors.append("broker selftest blocked_action_checks must contain objects")
                continue
            if check.get("code") != "blocked_action":
                errors.append("broker selftest blocked action check must have code=blocked_action")
            if check.get("path_payload_supplied") is not False:
                errors.append("broker selftest blocked action checks must not supply path payloads")
    errors.extend(safety_errors(selftest, "native-loader broker selftest"))
    return errors


def validate_candidate_load_gate(gate: dict[str, Any], candidate_path: str | None) -> list[str]:
    errors: list[str] = []
    if gate.get("publication_status") != "local-only":
        errors.append("candidate load gate publication_status must be local-only")
    if gate.get("report_kind") != "aex_candidate_load_gate_dryrun":
        errors.append("candidate load gate report_kind must be aex_candidate_load_gate_dryrun")
    if gate.get("candidate_load_gate_dryrun_state") != "candidate_load_gate_dryrun_ready_no_load":
        errors.append("candidate load gate dryrun state must be ready no-load")
    if gate.get("native_load_gate") != "closed":
        errors.append("candidate load gate native_load_gate must be closed")
    if gate.get("fixture_approval_satisfied") is not False:
        errors.append("candidate load gate fixture approval must not be satisfied")
    if gate.get("candidate_dependencies_clear") is not True:
        errors.append("candidate load gate candidate_dependencies_clear must be true")
    if gate.get("candidate_dependency_blockers_present") is not False:
        errors.append("candidate load gate candidate dependency blockers must be false")
    if gate.get("candidate_relative_path") != candidate_path:
        errors.append("candidate load gate candidate_relative_path must match native-loader design")
    errors.extend(safety_errors(gate, "candidate load gate"))
    return errors


def validate_sandbox_policy(policy: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if policy.get("publication_status") != "local-only":
        errors.append("sandbox policy publication_status must be local-only")
    if policy.get("packet_kind") != "aex_sandbox_policy_packet":
        errors.append("sandbox policy packet_kind must be aex_sandbox_policy_packet")
    if policy.get("sandbox_policy_state") != "policy_ready_no_native_load":
        errors.append("sandbox policy state must be policy_ready_no_native_load")
    required = policy.get("required_before_native_load", [])
    for item in (
        "explicit user fixture approval artifact",
        "passing load gate using the approved fixture",
        "worker process isolation design review",
    ):
        if item not in required:
            errors.append(f"sandbox policy required_before_native_load must include {item}")
    errors.extend(safety_errors(policy, "sandbox policy"))
    return errors


def build_path_policy() -> dict[str, Any]:
    return {
        "state": "closed_no_aex_paths_accepted",
        "aex_path_acceptance_enabled": False,
        "accepted_aex_path": None,
        "allowed_path_roots_now": [],
        "future_path_allowlist_required": True,
        "future_path_rules": [
            "resolve under an explicit approved fixture root",
            "reject traversal components before resolve",
            "reject symlink or junction escape from the approved root",
            "require create-new logs under target/native-loader-runtime-logs",
            "do not echo absolute private paths into public artifacts",
        ],
    }


def build_timeout_policy() -> dict[str, Any]:
    return {
        "state": "timeout_contract_defined_no_native_timeout_exercised",
        "broker_start_timeout_ms": 5000,
        "broker_message_timeout_ms": 5000,
        "broker_shutdown_timeout_ms": 5000,
        "first_native_load_timeout_ms": None,
        "requires_review_before_first_load": True,
    }


def build_crash_containment_policy() -> dict[str, Any]:
    return {
        "state": "out_of_process_crash_containment_required",
        "controller_must_not_load_aex_or_dependency_dlls": True,
        "future_worker_must_report_exit_code": True,
        "future_worker_must_capture_stderr": True,
        "future_worker_must_survive_failed_messages": True,
        "minidump_or_crash_log_policy": "local_only_create_new_after_explicit_runtime_approval",
    }


def build_child_cleanup_policy() -> dict[str, Any]:
    return {
        "state": "child_cleanup_required_before_path_acceptance",
        "normal_shutdown": "send quit and wait for clean exit",
        "timeout_shutdown": "terminate child, then escalate only after review",
        "orphan_process_policy": "controller must verify child exit before reporting success",
        "cleanup_exercised_now": "pathless_broker_quit_only",
    }


def build_log_policy() -> dict[str, Any]:
    return {
        "state": "stdout_stderr_capture_required_create_new",
        "stdout_capture": "jsonl responses only",
        "stderr_capture": "captured and summarized in local-only reports",
        "absolute_path_redaction_required": True,
        "raw_payload_logging_allowed": False,
        "log_root": "target/native-loader-runtime-logs",
        "log_root_enabled_now": False,
    }


def build_runtime_contract(
    *,
    native_loader_design: dict[str, Any],
    native_loader_design_path: Path,
    broker_selftest: dict[str, Any],
    broker_selftest_path: Path,
    candidate_load_gate: dict[str, Any] | None = None,
    candidate_load_gate_path: Path | None = None,
    sandbox_policy: dict[str, Any] | None = None,
    sandbox_policy_path: Path | None = None,
) -> dict[str, Any]:
    candidate_path = native_loader_design.get("candidate_relative_path")
    errors = validate_design_contract(native_loader_design) + validate_broker_selftest(broker_selftest)
    if candidate_load_gate is not None:
        errors.extend(validate_candidate_load_gate(candidate_load_gate, candidate_path))
    if sandbox_policy is not None:
        errors.extend(validate_sandbox_policy(sandbox_policy))
    if broker_selftest.get("source_native_loader_design_state") != native_loader_design.get(
        "native_loader_design_state"
    ):
        errors.append("broker selftest source native-loader design state must match design contract")
    if broker_selftest.get("candidate_relative_path") != candidate_path:
        errors.append("broker selftest candidate_relative_path must match native-loader design")
    if errors:
        raise ValueError("; ".join(errors))

    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_native_loader_runtime_contract",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_native_loader_design": str(native_loader_design_path),
        "source_native_loader_broker_selftest": str(broker_selftest_path),
        "source_candidate_load_gate": str(candidate_load_gate_path) if candidate_load_gate_path else None,
        "source_sandbox_policy": str(sandbox_policy_path) if sandbox_policy_path else None,
        "native_loader_runtime_contract_state": "runtime_containment_contract_ready_no_load",
        "contract_state": "runtime_containment_contract_ready_path_acceptance_closed",
        "runtime_containment_ready": True,
        "path_allowlist_state": "closed_no_aex_paths_accepted",
        "path_acceptance_ready": False,
        "aex_path_acceptance_enabled": False,
        "approval_required_before_aex_path": True,
        "runtime_approval_required_before_load": True,
        "native_load_gate": "closed",
        "accepted_aex_path": None,
        "path_payload_supplied": False,
        "broker_selftest_passed": True,
        "pathless_broker_ready": True,
        "native_loader_design_ready": True,
        "process_isolation_required": True,
        "controller_loads_aex": False,
        "candidate_relative_path": candidate_path,
        "candidate_dependencies_clear": native_loader_design.get("candidate_dependencies_clear"),
        "fixture_approval_satisfied": native_loader_design.get("fixture_approval_satisfied"),
        "source_native_loader_design_state": native_loader_design.get("native_loader_design_state"),
        "source_native_loader_design_contract_state": native_loader_design.get("contract_state"),
        "source_broker_selftest_state": broker_selftest.get("broker_selftest_state"),
        "source_sandbox_policy_state": sandbox_policy.get("sandbox_policy_state") if sandbox_policy else None,
        "source_candidate_load_gate_state": candidate_load_gate.get("candidate_load_gate_dryrun_state")
        if candidate_load_gate
        else None,
        "source_candidate_scoped_load_gate_state": candidate_load_gate.get(
            "candidate_scoped_load_gate_dry_run_state"
        )
        if candidate_load_gate
        else None,
        "path_policy": build_path_policy(),
        "timeout_policy": build_timeout_policy(),
        "crash_containment_policy": build_crash_containment_policy(),
        "child_cleanup_policy": build_child_cleanup_policy(),
        "stdout_stderr_capture_policy": build_log_policy(),
        "log_policy": build_log_policy(),
        "runtime_containment_checks": [
            {
                "check": "pathless_broker_start_and_quit",
                "status": "satisfied_no_aex_path",
                "source": "native_loader_broker_selftest",
            },
            {
                "check": "path_allowlist_review",
                "status": "closed_pending_manual_review",
                "source": "runtime_contract",
            },
            {
                "check": "native_load_timeout_review",
                "status": "closed_pending_runtime_approval",
                "source": "runtime_contract",
            },
            {
                "check": "crash_cleanup_review",
                "status": "defined_pending_runtime_approval",
                "source": "runtime_contract",
            },
        ],
        "required_before_path_acceptance": [
            "explicit fixture approval artifact",
            "passing candidate-scoped load gate after approval",
            "reviewed path allowlist rooted in approved fixture storage",
            "runtime containment review covering timeout, crash isolation, child cleanup, and log capture",
            "dependency endpoint review for the selected fixture",
            "explicit user approval before any broker message includes an AEX path",
        ],
        "blocked_actions": BLOCKED_ACTIONS,
        "blocked_action_count": len(BLOCKED_ACTIONS),
        "allowed_next_actions": [
            "keep the broker pathless",
            "review path allowlist rules without accepting paths",
            "draft timeout and child cleanup tests using synthetic subprocesses only",
            "repeat readiness indexing after this no-load contract is created",
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
            "It accepts no AEX path and opens no plugin file.",
            "It does not load DLLs, call EffectMain, render, launch AE, or route through OFX.",
            "Runtime containment readiness here means the no-load contract is defined, not that native load is approved.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build no-load native-loader runtime containment contract")
    parser.add_argument(
        "--native-loader-design",
        required=True,
        help="Native-loader design contract JSON under target/native-loader-design",
    )
    parser.add_argument(
        "--broker-selftest",
        required=True,
        help="Native-loader broker selftest JSON under target/native-loader-broker-selftest",
    )
    parser.add_argument(
        "--candidate-load-gate",
        help="Optional candidate load gate dry-run JSON under target/candidate-load-gate",
    )
    parser.add_argument("--sandbox-policy", help="Optional sandbox policy JSON under target/sandbox-policy")
    parser.add_argument("--out", required=True, help="Create-new contract under target/native-loader-runtime-contract")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    design, design_path = load_native_loader_design(Path(args.native_loader_design))
    broker_selftest, broker_selftest_path = load_broker_selftest(Path(args.broker_selftest))
    candidate_load_gate = None
    candidate_load_gate_path = None
    if args.candidate_load_gate:
        candidate_load_gate, candidate_load_gate_path = load_candidate_load_gate(Path(args.candidate_load_gate))
    sandbox_policy = None
    sandbox_policy_path = None
    if args.sandbox_policy:
        sandbox_policy, sandbox_policy_path = load_sandbox_policy(Path(args.sandbox_policy))
    report = build_runtime_contract(
        native_loader_design=design,
        native_loader_design_path=design_path,
        broker_selftest=broker_selftest,
        broker_selftest_path=broker_selftest_path,
        candidate_load_gate=candidate_load_gate,
        candidate_load_gate_path=candidate_load_gate_path,
        sandbox_policy=sandbox_policy,
        sandbox_policy_path=sandbox_policy_path,
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

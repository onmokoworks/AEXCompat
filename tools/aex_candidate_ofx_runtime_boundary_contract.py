#!/usr/bin/env python3
"""Build a no-load candidate OFX runtime boundary contract.

The boundary contract consumes JSON evidence only. It ties the candidate host
harness synthetic selftest to the closed OFX route contract and native-loader
runtime containment contract. It does not build or instantiate an OFX runtime,
open AEX files, read PPM pixels, describe real effects, render, or route pixels.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
HOST_HARNESS_SELFTEST_ROOT = TARGET_ROOT / "candidate-ofx-host-harness-selftest"
HOST_HARNESS_DRYRUN_ROOT = TARGET_ROOT / "candidate-ofx-host-harness-dryrun"
CANDIDATE_OFX_BRIDGE_ROOT = TARGET_ROOT / "candidate-ofx-bridge"
OFX_ROUTE_CONTRACT_ROOT = TARGET_ROOT / "ofx-route-contract"
NATIVE_LOADER_RUNTIME_CONTRACT_ROOT = TARGET_ROOT / "native-loader-runtime-contract"
CANDIDATE_OFX_RUNTIME_BOUNDARY_CONTRACT_ROOT = TARGET_ROOT / "candidate-ofx-runtime-boundary-contract"

SAFETY_FLAGS = (
    "native_load_enabled",
    "native_load_performed",
    "dll_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "ofx_runtime_invoked",
    "host_process_launch_enabled",
    "private_payload_copied",
    "aex_file_opened",
    "aex_file_hashed",
    "aex_file_copied",
    "aepx_file_modified",
    "aep_binary_modified",
    "ae_project_write_performed",
    "ofx_plugin_built",
    "ofx_describe_performed",
    "ofx_render_performed",
    "aex_render_performed",
    "render_validation_performed",
    "ppm_pixel_read_performed",
    "pipl_payload_parsed",
    "parameter_schema_emitted",
    "redacted_schema_emitted",
    "real_pipl_payload_parser_enabled",
    "real_pipl_payload_parsed",
    "resource_payload_opened",
    "resource_payload_extracted",
    "raw_payload_serialized",
)

BLOCKED_ACTIONS = (
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
    "build_ofx_binary",
    "accept_ofx_host_path",
    "accept_ofx_plugin_binary_path",
    "launch_ofx_host_process",
    "route_through_real_ofx",
    "route_through_ofx",
    "instantiate_ofx_runtime",
    "load_ofx_plugin",
    "ofx_describe_from_aex",
    "ofx_render_with_aex",
    "read_ppm_pixels",
    "open_candidate_mock_ppm",
    "compare_aex_render_pixels",
    "claim_render_equivalence",
    "parse_real_pipl_payload",
    "emit_parameter_schema",
    "emit_redacted_schema",
)


def path_has_traversal(path: Path) -> bool:
    return any(part in ("..", ".") for part in path.parts)


def resolve_under_root(path: Path, root: Path, *, must_exist: bool) -> Path:
    if path_has_traversal(path):
        raise ValueError("path must not contain traversal components")
    absolute = path if path.is_absolute() else LAB_ROOT / path
    resolved_root = root.resolve(strict=True)
    try:
        resolved = absolute.resolve(strict=must_exist)
    except FileNotFoundError as exc:
        raise ValueError(f"path does not exist: {absolute}") from exc
    if not resolved.is_relative_to(resolved_root):
        raise ValueError(f"path must stay under {root}")
    return resolved


def validate_json_input(path: Path, root: Path, label: str) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError(f"{label} must have .json extension")
    return resolve_under_root(path, root, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("candidate OFX runtime boundary contract must have .json extension")
    CANDIDATE_OFX_RUNTIME_BOUNDARY_CONTRACT_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, CANDIDATE_OFX_RUNTIME_BOUNDARY_CONTRACT_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(
        CANDIDATE_OFX_RUNTIME_BOUNDARY_CONTRACT_ROOT.resolve(strict=True)
    ):
        raise ValueError(
            f"candidate OFX runtime boundary parent must stay under {CANDIDATE_OFX_RUNTIME_BOUNDARY_CONTRACT_ROOT}"
        )
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_harness_selftest(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, HOST_HARNESS_SELFTEST_ROOT, "candidate OFX host harness selftest")
    return read_json_object(resolved), resolved


def load_harness_dryrun(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, HOST_HARNESS_DRYRUN_ROOT, "candidate OFX host harness dry-run")
    return read_json_object(resolved), resolved


def load_candidate_ofx_bridge(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, CANDIDATE_OFX_BRIDGE_ROOT, "candidate OFX bridge")
    return read_json_object(resolved), resolved


def load_ofx_route_contract(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, OFX_ROUTE_CONTRACT_ROOT, "OFX route contract")
    return read_json_object(resolved), resolved


def load_native_runtime_contract(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(
        path,
        NATIVE_LOADER_RUNTIME_CONTRACT_ROOT,
        "native-loader runtime contract",
    )
    return read_json_object(resolved), resolved


def relative_to_lab(path: Path) -> str:
    return path.resolve(strict=False).relative_to(LAB_ROOT.resolve(strict=True)).as_posix()


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def validate_harness_selftest(selftest: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if selftest.get("publication_status") != "local-only":
        errors.append("host harness selftest publication_status must be local-only")
    if selftest.get("report_kind") != "aex_candidate_ofx_host_harness_selftest":
        errors.append("host harness selftest report_kind must be aex_candidate_ofx_host_harness_selftest")
    if selftest.get("host_harness_selftest_state") != "candidate_ofx_host_harness_selftest_passed_synthetic_route_closed":
        errors.append("host_harness_selftest_state must be candidate_ofx_host_harness_selftest_passed_synthetic_route_closed")
    if selftest.get("host_harness_selftest_ready") is not True:
        errors.append("host_harness_selftest_ready must be true")
    if selftest.get("host_harness_kind") != "ofx_noop_host_harness_synthetic_selftest":
        errors.append("host_harness_kind must be ofx_noop_host_harness_synthetic_selftest")
    if selftest.get("synthetic_only") is not True:
        errors.append("synthetic_only must be true")
    if selftest.get("synthetic_contract_checks_performed") is not True:
        errors.append("synthetic_contract_checks_performed must be true")
    if selftest.get("real_harness_execution_performed") is not False:
        errors.append("real_harness_execution_performed must be false")
    if selftest.get("source_harness_dryrun_state") != "candidate_ofx_host_harness_dryrun_ready_route_closed":
        errors.append("source_harness_dryrun_state must be candidate_ofx_host_harness_dryrun_ready_route_closed")
    if selftest.get("source_bridge_state") != "candidate_ofx_bridge_ready_no_load_route_closed":
        errors.append("source_bridge_state must be candidate_ofx_bridge_ready_no_load_route_closed")
    if selftest.get("source_bridge_allowed_route") != "no_op_identity_only":
        errors.append("source_bridge_allowed_route must be no_op_identity_only")
    if selftest.get("checked_case_count") != 2:
        errors.append("checked_case_count must be 2")
    if selftest.get("checked_noop_describe_case_count") != 1:
        errors.append("checked_noop_describe_case_count must be 1")
    if selftest.get("checked_noop_render_case_count") != 1:
        errors.append("checked_noop_render_case_count must be 1")
    if selftest.get("checked_real_describe_case_count") != 0:
        errors.append("checked_real_describe_case_count must be 0")
    if selftest.get("checked_real_render_case_count") != 0:
        errors.append("checked_real_render_case_count must be 0")
    if selftest.get("case_passed_count") != 2:
        errors.append("case_passed_count must be 2")
    if selftest.get("descriptor_contract_checked") is not True:
        errors.append("descriptor_contract_checked must be true")
    if selftest.get("render_identity_contract_checked") is not True:
        errors.append("render_identity_contract_checked must be true")
    if selftest.get("ppm_pixel_read_performed") is not False:
        errors.append("ppm_pixel_read_performed must be false")
    for key in (
        "real_route_open",
        "real_ofx_route_ready",
        "ofx_runtime_invoked",
        "aex_runtime_invoked",
        "ofx_describe_ready",
        "ofx_render_ready",
        "render_equivalence_claim_ready",
        "absolute_ppm_paths_exported",
        "absolute_aex_paths_exported",
        "host_harness_path_payload_exported",
    ):
        if selftest.get(key) is not False:
            errors.append(f"host harness selftest {key} must be false")
    if selftest.get("requires_future_runtime_approval") is not True:
        errors.append("host harness selftest requires_future_runtime_approval must be true")
    errors.extend(safety_errors(selftest, "host harness selftest"))
    return errors


def validate_harness_dryrun(dryrun: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if dryrun.get("publication_status") != "local-only":
        errors.append("host harness dry-run publication_status must be local-only")
    if dryrun.get("report_kind") != "aex_candidate_ofx_host_harness_dryrun":
        errors.append("host harness dry-run report_kind must be aex_candidate_ofx_host_harness_dryrun")
    if dryrun.get("harness_dryrun_state") != "candidate_ofx_host_harness_dryrun_ready_route_closed":
        errors.append("harness_dryrun_state must be candidate_ofx_host_harness_dryrun_ready_route_closed")
    if dryrun.get("harness_dryrun_ready") is not True:
        errors.append("harness_dryrun_ready must be true")
    if dryrun.get("dry_run_only") is not True:
        errors.append("dry_run_only must be true")
    if dryrun.get("would_execute") is not False:
        errors.append("host harness dry-run would_execute must be false")
    if dryrun.get("execution_performed") is not False:
        errors.append("host harness dry-run execution_performed must be false")
    if dryrun.get("source_bridge_state") != "candidate_ofx_bridge_ready_no_load_route_closed":
        errors.append("host harness dry-run source_bridge_state must be candidate_ofx_bridge_ready_no_load_route_closed")
    if dryrun.get("source_bridge_allowed_route") != "no_op_identity_only":
        errors.append("host harness dry-run source_bridge_allowed_route must be no_op_identity_only")
    if dryrun.get("planned_real_describe_case_count") != 0:
        errors.append("host harness dry-run planned_real_describe_case_count must be 0")
    if dryrun.get("planned_real_render_case_count") != 0:
        errors.append("host harness dry-run planned_real_render_case_count must be 0")
    for key in (
        "real_route_open",
        "real_ofx_route_ready",
        "ofx_runtime_invoked",
        "aex_runtime_invoked",
        "ofx_describe_ready",
        "ofx_render_ready",
        "render_equivalence_claim_ready",
        "absolute_ppm_paths_exported",
        "absolute_aex_paths_exported",
        "host_harness_path_payload_exported",
    ):
        if dryrun.get(key) is not False:
            errors.append(f"host harness dry-run {key} must be false")
    if dryrun.get("requires_future_runtime_approval") is not True:
        errors.append("host harness dry-run requires_future_runtime_approval must be true")
    errors.extend(safety_errors(dryrun, "host harness dry-run"))
    return errors


def validate_candidate_ofx_bridge(bridge: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if bridge.get("publication_status") != "local-only":
        errors.append("candidate OFX bridge publication_status must be local-only")
    if bridge.get("report_kind") != "aex_candidate_ofx_bridge_packet":
        errors.append("candidate OFX bridge report_kind must be aex_candidate_ofx_bridge_packet")
    if bridge.get("bridge_state") != "candidate_ofx_bridge_ready_no_load_route_closed":
        errors.append("bridge_state must be candidate_ofx_bridge_ready_no_load_route_closed")
    if bridge.get("bridge_ready") is not True:
        errors.append("bridge_ready must be true")
    if bridge.get("bridge_allowed_route") != "no_op_identity_only":
        errors.append("bridge_allowed_route must be no_op_identity_only")
    if bridge.get("mock_route_ready") is not True:
        errors.append("bridge mock_route_ready must be true")
    for key in (
        "real_route_open",
        "real_ofx_route_ready",
        "ofx_runtime_invoked",
        "aex_runtime_invoked",
        "ofx_describe_ready",
        "ofx_render_ready",
        "render_equivalence_claim_ready",
        "absolute_ppm_paths_exported",
        "absolute_aex_paths_exported",
        "ofx_bridge_path_payload_exported",
    ):
        if bridge.get(key) is not False:
            errors.append(f"candidate OFX bridge {key} must be false")
    errors.extend(safety_errors(bridge, "candidate OFX bridge"))
    return errors


def validate_ofx_route_contract(contract: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if contract.get("publication_status") != "local-only":
        errors.append("OFX route contract publication_status must be local-only")
    if contract.get("report_kind") != "aex_ofx_route_contract_probe":
        errors.append("OFX route contract report_kind must be aex_ofx_route_contract_probe")
    if contract.get("contract_state") != "ofx_route_contract_ready_route_closed":
        errors.append("OFX route contract state must be ofx_route_contract_ready_route_closed")
    if contract.get("real_route_open") is not False:
        errors.append("OFX route contract real_route_open must be false")
    if contract.get("mock_route_ready") is not True:
        errors.append("OFX route contract mock_route_ready must be true")
    route_contract = contract.get("route_contract")
    if not isinstance(route_contract, dict):
        errors.append("OFX route contract route_contract must be an object")
    else:
        if route_contract.get("allowed_route") != "no_op_identity_only":
            errors.append("OFX route contract allowed_route must be no_op_identity_only")
        for key in ("real_route_open", "ofx_runtime_invoked", "aex_runtime_invoked"):
            if route_contract.get(key) is not False:
                errors.append(f"OFX route contract route_contract {key} must be false")
    for block_name in ("describe_contract", "render_contract"):
        block = contract.get(block_name)
        if not isinstance(block, dict):
            errors.append(f"OFX route contract {block_name} must be an object")
        elif not str(block.get("state", "")).startswith("blocked_"):
            errors.append(f"OFX route contract {block_name} must remain blocked")
    blocked = contract.get("blocked_actions")
    if not isinstance(blocked, list):
        errors.append("OFX route contract blocked_actions must be a list")
    else:
        for action in ("build_ofx_binary", "ofx_describe_from_aex", "ofx_render_with_aex", "route_through_ofx"):
            if action not in blocked:
                errors.append(f"OFX route contract must block {action}")
    errors.extend(safety_errors(contract, "OFX route contract"))
    return errors


def validate_native_runtime_contract(contract: dict[str, Any], candidate_path: str | None) -> list[str]:
    errors: list[str] = []
    if contract.get("publication_status") != "local-only":
        errors.append("native runtime contract publication_status must be local-only")
    if contract.get("report_kind") != "aex_native_loader_runtime_contract":
        errors.append("native runtime contract report_kind must be aex_native_loader_runtime_contract")
    if contract.get("native_loader_runtime_contract_state") != "runtime_containment_contract_ready_no_load":
        errors.append("native runtime contract state must be runtime_containment_contract_ready_no_load")
    if contract.get("contract_state") != "runtime_containment_contract_ready_path_acceptance_closed":
        errors.append("native runtime contract_state must be runtime_containment_contract_ready_path_acceptance_closed")
    if contract.get("runtime_containment_ready") is not True:
        errors.append("runtime_containment_ready must be true")
    if contract.get("path_acceptance_ready") is not False:
        errors.append("path_acceptance_ready must be false")
    if contract.get("aex_path_acceptance_enabled") is not False:
        errors.append("aex_path_acceptance_enabled must be false")
    if contract.get("runtime_approval_required_before_load") is not True:
        errors.append("runtime_approval_required_before_load must be true")
    if contract.get("native_load_gate") != "closed":
        errors.append("native_load_gate must be closed")
    if contract.get("accepted_aex_path") is not None:
        errors.append("accepted_aex_path must be null")
    if contract.get("path_payload_supplied") is not False:
        errors.append("path_payload_supplied must be false")
    if contract.get("process_isolation_required") is not True:
        errors.append("process_isolation_required must be true")
    if contract.get("controller_loads_aex") is not False:
        errors.append("controller_loads_aex must be false")
    if contract.get("candidate_dependencies_clear") is not True:
        errors.append("candidate_dependencies_clear must be true")
    if contract.get("fixture_approval_satisfied") is not False:
        errors.append("fixture_approval_satisfied must be false")
    if contract.get("candidate_relative_path") != candidate_path:
        errors.append("native runtime candidate_relative_path must match host harness selftest")
    blocked = contract.get("blocked_actions")
    if not isinstance(blocked, list):
        errors.append("native runtime blocked_actions must be a list")
    else:
        for action in ("accept_aex_path", "open_aex_file", "load_aex_dll", "call_EffectMain", "render_with_aex"):
            if action not in blocked:
                errors.append(f"native runtime contract must block {action}")
    errors.extend(safety_errors(contract, "native runtime contract"))
    return errors


def build_runtime_boundary_plan() -> dict[str, Any]:
    return {
        "boundary_kind": "future_ofx_runtime_boundary",
        "runtime_process_model": "separate_reviewed_process_required",
        "controller_loads_aex_or_ofx": False,
        "allowed_now": [
            "review JSON contract evidence",
            "define future runtime process boundary",
            "define local-only log and timeout policy",
            "keep no-op identity route as the only current route",
        ],
        "approval_gates_required": [
            "explicit user approval for OFX runtime instantiation",
            "explicit user fixture approval before any AEX path is accepted",
            "reviewed OFX host binary or host shim provenance",
            "reviewed process timeout and crash cleanup selftest",
            "reviewed local-only runtime log redaction policy",
            "reviewed parameter schema and render validation evidence",
        ],
        "required_runtime_evidence": [
            "runtime executable provenance",
            "sandbox or worker isolation evidence",
            "timeout/kill behavior evidence",
            "stdout/stderr capture evidence",
            "no absolute private path export evidence",
            "first real describe/render approval packet",
        ],
    }


def build_boundary_contract(
    *,
    candidate_ofx_bridge: dict[str, Any],
    candidate_ofx_bridge_path: Path,
    host_harness_dryrun: dict[str, Any],
    host_harness_dryrun_path: Path,
    host_harness_selftest: dict[str, Any],
    host_harness_selftest_path: Path,
    ofx_route_contract: dict[str, Any],
    ofx_route_contract_path: Path,
    native_runtime_contract: dict[str, Any],
    native_runtime_contract_path: Path,
) -> dict[str, Any]:
    candidate_path = host_harness_selftest.get("candidate_relative_path")
    errors = validate_candidate_ofx_bridge(candidate_ofx_bridge)
    errors.extend(validate_harness_dryrun(host_harness_dryrun))
    errors.extend(validate_harness_selftest(host_harness_selftest))
    errors.extend(validate_ofx_route_contract(ofx_route_contract))
    errors.extend(validate_native_runtime_contract(native_runtime_contract, candidate_path))
    if candidate_ofx_bridge.get("candidate_relative_path") != candidate_path:
        errors.append("candidate OFX bridge candidate_relative_path must match host harness selftest")
    if host_harness_dryrun.get("candidate_relative_path") != candidate_path:
        errors.append("host harness dry-run candidate_relative_path must match host harness selftest")
    if host_harness_dryrun.get("source_bridge_state") != candidate_ofx_bridge.get("bridge_state"):
        errors.append("host harness dry-run source bridge state must match candidate OFX bridge")
    if host_harness_selftest.get("source_harness_dryrun_state") != host_harness_dryrun.get(
        "harness_dryrun_state"
    ):
        errors.append("host harness selftest source dry-run state must match host harness dry-run")
    if errors:
        raise ValueError("; ".join(errors))

    boundary_plan = build_runtime_boundary_plan()
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_ofx_runtime_boundary_contract",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_candidate_ofx_bridge": relative_to_lab(candidate_ofx_bridge_path),
        "source_candidate_ofx_host_harness_dryrun": relative_to_lab(host_harness_dryrun_path),
        "source_candidate_ofx_host_harness_selftest": relative_to_lab(host_harness_selftest_path),
        "source_ofx_route_contract": relative_to_lab(ofx_route_contract_path),
        "source_native_loader_runtime_contract": relative_to_lab(native_runtime_contract_path),
        "candidate_relative_path": candidate_path,
        "candidate_ofx_runtime_boundary_contract_state": (
            "candidate_ofx_runtime_boundary_contract_ready_no_runtime_route_closed"
        ),
        "candidate_ofx_runtime_boundary_state": "candidate_ofx_runtime_boundary_ready_no_load_route_closed",
        "contract_state": "candidate_ofx_runtime_boundary_contract_ready_runtime_closed",
        "runtime_boundary_ready": True,
        "runtime_boundary_kind": "candidate_ofx_runtime_boundary_contract",
        "boundary_contract_ready": True,
        "source_bridge_state": candidate_ofx_bridge.get("bridge_state"),
        "source_bridge_ready": candidate_ofx_bridge.get("bridge_ready"),
        "source_bridge_allowed_route": candidate_ofx_bridge.get("bridge_allowed_route"),
        "source_harness_dryrun_state": host_harness_dryrun.get("harness_dryrun_state"),
        "source_harness_dryrun_ready": host_harness_dryrun.get("harness_dryrun_ready"),
        "source_harness_dryrun_only": host_harness_dryrun.get("dry_run_only"),
        "source_host_harness_selftest_state": host_harness_selftest.get("host_harness_selftest_state"),
        "source_host_harness_selftest_ready": host_harness_selftest.get("host_harness_selftest_ready"),
        "source_harness_kind": host_harness_selftest.get("host_harness_kind"),
        "source_synthetic_only": host_harness_selftest.get("synthetic_only"),
        "source_real_harness_execution_performed": host_harness_selftest.get(
            "real_harness_execution_performed"
        ),
        "source_ppm_pixel_read_performed": host_harness_selftest.get("ppm_pixel_read_performed"),
        "source_bridge_state": host_harness_selftest.get("source_bridge_state"),
        "source_bridge_allowed_route": host_harness_selftest.get("source_bridge_allowed_route"),
        "source_ofx_route_contract_state": ofx_route_contract.get("contract_state"),
        "source_ofx_route_allowed_route": ofx_route_contract.get("route_contract", {}).get("allowed_route")
        if isinstance(ofx_route_contract.get("route_contract"), dict)
        else None,
        "source_ofx_route_real_route_open": ofx_route_contract.get("real_route_open"),
        "source_ofx_route_mock_route_ready": ofx_route_contract.get("mock_route_ready"),
        "source_native_runtime_contract_state": native_runtime_contract.get(
            "native_loader_runtime_contract_state"
        ),
        "source_native_runtime_contract_ready": native_runtime_contract.get("runtime_containment_ready"),
        "source_native_runtime_path_acceptance_ready": native_runtime_contract.get("path_acceptance_ready"),
        "source_native_runtime_process_isolation_required": native_runtime_contract.get(
            "process_isolation_required"
        ),
        "source_native_runtime_candidate_dependencies_clear": native_runtime_contract.get(
            "candidate_dependencies_clear"
        ),
        "source_fixture_approval_satisfied": native_runtime_contract.get("fixture_approval_satisfied"),
        "no_load_boundary_contract_created": True,
        "runtime_boundary_plan": boundary_plan,
        "runtime_boundary_step_count": 6,
        "approval_gate_count": 6,
        "required_runtime_evidence_count": 6,
        "ofx_runtime_allowed_now": False,
        "ofx_runtime_invocation_ready": False,
        "ofx_runtime_instantiation_ready": False,
        "ofx_runtime_instantiation_performed": False,
        "ofx_binary_build_allowed_now": False,
        "ofx_binary_built": False,
        "real_ofx_describe_allowed_now": False,
        "real_ofx_render_allowed_now": False,
        "real_route_open": False,
        "real_ofx_route_ready": False,
        "mock_route_ready": True,
        "no_op_identity_route_preserved": True,
        "ofx_runtime_invoked": False,
        "aex_runtime_invoked": False,
        "ofx_describe_ready": False,
        "ofx_render_ready": False,
        "render_equivalence_claim_ready": False,
        "ppm_pixel_read_performed": False,
        "absolute_ppm_paths_exported": False,
        "absolute_aex_paths_exported": False,
        "runtime_boundary_path_payload_exported": False,
        "ofx_host_path_payload_supplied": False,
        "ofx_plugin_binary_path_payload_supplied": False,
        "path_acceptance_ready": False,
        "aex_path_acceptance_enabled": False,
        "accepted_aex_path": None,
        "host_process_launch_enabled": False,
        "requires_future_runtime_approval": True,
        "runtime_approval_required_before_invocation": True,
        "requires_future_fixture_approval": True,
        "requires_future_render_validation_approval": True,
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "ofx_runtime_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "aex_file_hashed": False,
        "aex_file_copied": False,
        "aepx_file_modified": False,
        "aep_binary_modified": False,
        "ae_project_write_performed": False,
        "ofx_plugin_built": False,
        "ofx_describe_performed": False,
        "ofx_render_performed": False,
        "aex_render_performed": False,
        "render_validation_performed": False,
        "ppm_pixel_read_performed": False,
        "pipl_payload_parsed": False,
        "parameter_schema_emitted": False,
        "redacted_schema_emitted": False,
        "real_pipl_payload_parser_enabled": False,
        "real_pipl_payload_parsed": False,
        "resource_payload_opened": False,
        "resource_payload_extracted": False,
        "raw_payload_serialized": False,
        "allowed_boundary_actions": [
            "define_future_ofx_runtime_boundary",
            "bind_candidate_harness_selftest_to_closed_route_contract",
            "bind_native_runtime_containment_requirements",
            "record_runtime_approval_gates",
            "record_local_only_log_and_timeout_policy",
        ],
        "host_process_policy": {
            "state": "closed_no_host_process_launch",
            "host_process_launch_enabled": False,
            "ofx_host_path_payload_supplied": False,
            "runtime_approval_required_before_invocation": True,
        },
        "ofx_plugin_binary_policy": {
            "state": "closed_no_plugin_binary_path",
            "ofx_plugin_binary_path_payload_supplied": False,
            "ofx_plugin_built": False,
            "plugin_binary_provenance_required": True,
        },
        "describe_boundary_policy": {
            "state": "blocked_pending_schema_and_runtime_approval",
            "real_ofx_describe_allowed_now": False,
            "ofx_describe_ready": False,
        },
        "render_boundary_policy": {
            "state": "blocked_pending_render_validation_and_runtime_approval",
            "real_ofx_render_allowed_now": False,
            "ofx_render_ready": False,
            "ppm_pixel_read_performed": False,
        },
        "crash_timeout_log_policy": {
            "state": "defined_for_future_runtime_review",
            "process_isolation_required": True,
            "timeout_review_required": True,
            "local_only_logs_required": True,
            "absolute_path_redaction_required": True,
        },
        "required_before_runtime_invocation": boundary_plan["approval_gates_required"],
        "blocked_actions": list(BLOCKED_ACTIONS),
        "next_required_actions": [
            "Add a separate reviewed OFX runtime approval request before any runtime instantiation.",
            "Keep OFX describe/render and AEX-backed routes closed until explicit approvals and containment evidence exist.",
            "Keep candidate image surfaces as relative strings until a reviewed pixel-read validation phase is approved.",
        ],
        "notes": [
            "This boundary contract reads JSON evidence only.",
            "It does not build or instantiate an OFX runtime and does not open AEX or PPM files.",
            "It prepares future review gates while preserving the no-op identity-only route.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build candidate OFX runtime boundary contract")
    parser.add_argument(
        "--candidate-ofx-bridge",
        required=True,
        help="Candidate OFX bridge under target/candidate-ofx-bridge",
    )
    parser.add_argument(
        "--host-harness-dryrun",
        required=True,
        help="Candidate OFX host harness dry-run under target/candidate-ofx-host-harness-dryrun",
    )
    parser.add_argument(
        "--host-harness-selftest",
        required=True,
        help="Candidate OFX host harness selftest under target/candidate-ofx-host-harness-selftest",
    )
    parser.add_argument(
        "--ofx-route-contract",
        required=True,
        help="Closed OFX route contract under target/ofx-route-contract",
    )
    parser.add_argument(
        "--native-runtime-contract",
        required=True,
        help="Native-loader runtime contract under target/native-loader-runtime-contract",
    )
    parser.add_argument(
        "--out",
        required=True,
        help="Create-new boundary JSON under target/candidate-ofx-runtime-boundary-contract",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    bridge, bridge_path = load_candidate_ofx_bridge(Path(args.candidate_ofx_bridge))
    dryrun, dryrun_path = load_harness_dryrun(Path(args.host_harness_dryrun))
    selftest, selftest_path = load_harness_selftest(Path(args.host_harness_selftest))
    route_contract, route_contract_path = load_ofx_route_contract(Path(args.ofx_route_contract))
    native_contract, native_contract_path = load_native_runtime_contract(Path(args.native_runtime_contract))
    report = build_boundary_contract(
        candidate_ofx_bridge=bridge,
        candidate_ofx_bridge_path=bridge_path,
        host_harness_dryrun=dryrun,
        host_harness_dryrun_path=dryrun_path,
        host_harness_selftest=selftest,
        host_harness_selftest_path=selftest_path,
        ofx_route_contract=route_contract,
        ofx_route_contract_path=route_contract_path,
        native_runtime_contract=native_contract,
        native_runtime_contract_path=native_contract_path,
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

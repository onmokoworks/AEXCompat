#!/usr/bin/env python3
"""Build a no-load OFX host harness dry-run packet for a selected AEX candidate.

The dry-run consumes the candidate OFX bridge packet only. It does not build or
invoke an OFX runtime, open AEX files, read PPM pixels, describe effects, render,
or route pixels.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
CANDIDATE_OFX_BRIDGE_ROOT = TARGET_ROOT / "candidate-ofx-bridge"
HOST_HARNESS_DRYRUN_ROOT = TARGET_ROOT / "candidate-ofx-host-harness-dryrun"

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
    "aepx_file_modified",
    "aep_binary_modified",
    "ae_project_write_performed",
    "ofx_plugin_built",
    "ofx_describe_performed",
    "ofx_render_performed",
    "aex_render_performed",
    "render_validation_performed",
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
    "route_through_real_ofx",
    "build_ofx_binary",
    "instantiate_ofx_runtime",
    "ofx_describe_from_aex",
    "ofx_render_with_aex",
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
        raise ValueError("candidate OFX host harness dry-run must have .json extension")
    HOST_HARNESS_DRYRUN_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, HOST_HARNESS_DRYRUN_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(HOST_HARNESS_DRYRUN_ROOT.resolve(strict=True)):
        raise ValueError(f"candidate OFX host harness dry-run parent must stay under {HOST_HARNESS_DRYRUN_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_bridge(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, CANDIDATE_OFX_BRIDGE_ROOT, "candidate OFX bridge packet")
    return read_json_object(resolved), resolved


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def validate_bridge(bridge: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if bridge.get("publication_status") != "local-only":
        errors.append("bridge publication_status must be local-only")
    if bridge.get("report_kind") != "aex_candidate_ofx_bridge_packet":
        errors.append("bridge report_kind must be aex_candidate_ofx_bridge_packet")
    if bridge.get("bridge_state") != "candidate_ofx_bridge_ready_no_load_route_closed":
        errors.append("bridge_state must be candidate_ofx_bridge_ready_no_load_route_closed")
    if bridge.get("bridge_ready") is not True:
        errors.append("bridge_ready must be true")
    if bridge.get("bridge_allowed_route") != "no_op_identity_only":
        errors.append("bridge_allowed_route must be no_op_identity_only")
    if bridge.get("mock_route_ready") is not True:
        errors.append("mock_route_ready must be true")
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
            errors.append(f"bridge {key} must be false")
    if not bridge.get("candidate_relative_path"):
        errors.append("bridge candidate_relative_path must be present")
    image_surface = bridge.get("image_surface")
    if not isinstance(image_surface, dict):
        errors.append("bridge image_surface must be an object")
    else:
        if image_surface.get("state") != "candidate_mock_output_available_relative_path_only":
            errors.append("bridge image_surface state must be candidate_mock_output_available_relative_path_only")
        for key in ("input_ppm_absolute_path_exported", "output_ppm_absolute_path_exported"):
            if image_surface.get(key) is not False:
                errors.append(f"bridge image_surface {key} must be false")
    blocked = bridge.get("blocked_actions")
    if not isinstance(blocked, list):
        errors.append("bridge blocked_actions must be a list")
    else:
        for action in ("build_ofx_binary", "ofx_describe_from_aex", "ofx_render_with_aex", "route_through_real_ofx"):
            if action not in blocked:
                errors.append(f"bridge must block {action}")
    errors.extend(safety_errors(bridge, "bridge"))
    return errors


def relative_to_lab(path: Path) -> str:
    return path.resolve(strict=False).relative_to(LAB_ROOT.resolve(strict=True)).as_posix()


def planned_cases(bridge: dict[str, Any]) -> list[dict[str, Any]]:
    image_surface = bridge.get("image_surface") if isinstance(bridge.get("image_surface"), dict) else {}
    output_ppm = image_surface.get("output_ppm_relative")
    return [
        {
            "case_id": "noop_describe_contract",
            "phase": "describe",
            "would_execute": False,
            "planned_action": "validate_static_noop_describe_shape_only",
            "uses_aex_metadata": False,
            "uses_real_ofx_runtime": False,
            "blocked_real_action": "ofx_describe_from_aex",
        },
        {
            "case_id": "noop_render_identity_contract",
            "phase": "render",
            "would_execute": False,
            "planned_action": "bind_candidate_mock_output_as_future_noop_input",
            "candidate_mock_output_ppm_relative": output_ppm,
            "uses_aex_pixels": False,
            "uses_real_ofx_runtime": False,
            "blocked_real_action": "ofx_render_with_aex",
        },
    ]


def build_harness_dryrun(*, bridge: dict[str, Any], bridge_path: Path) -> dict[str, Any]:
    errors = validate_bridge(bridge)
    if errors:
        raise ValueError("; ".join(errors))
    cases = planned_cases(bridge)
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_ofx_host_harness_dryrun",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_candidate_ofx_bridge": relative_to_lab(bridge_path),
        "source_bridge_state": bridge.get("bridge_state"),
        "candidate_relative_path": bridge.get("candidate_relative_path"),
        "harness_dryrun_state": "candidate_ofx_host_harness_dryrun_ready_route_closed",
        "harness_dryrun_ready": True,
        "dry_run_only": True,
        "would_execute": False,
        "execution_performed": False,
        "host_harness_kind": "ofx_noop_host_harness_planning",
        "allowed_harness_actions": [
            "plan_noop_describe_contract",
            "plan_noop_render_identity_contract",
            "reuse_candidate_mock_image_surface_relative_paths",
        ],
        "planned_cases": cases,
        "planned_case_count": len(cases),
        "planned_noop_describe_case_count": 1,
        "planned_noop_render_case_count": 1,
        "planned_real_describe_case_count": 0,
        "planned_real_render_case_count": 0,
        "blocked_case_count": len(BLOCKED_ACTIONS),
        "source_bridge_allowed_route": bridge.get("bridge_allowed_route"),
        "source_mock_route_ready": bridge.get("mock_route_ready"),
        "source_real_route_open": bridge.get("real_route_open"),
        "source_real_ofx_route_ready": bridge.get("real_ofx_route_ready"),
        "source_ofx_runtime_invoked": bridge.get("ofx_runtime_invoked"),
        "source_aex_runtime_invoked": bridge.get("aex_runtime_invoked"),
        "source_ofx_describe_ready": bridge.get("ofx_describe_ready"),
        "source_ofx_render_ready": bridge.get("ofx_render_ready"),
        "source_render_equivalence_claim_ready": bridge.get("render_equivalence_claim_ready"),
        "real_route_open": False,
        "real_ofx_route_ready": False,
        "ofx_runtime_invoked": False,
        "aex_runtime_invoked": False,
        "ofx_describe_ready": False,
        "ofx_render_ready": False,
        "render_equivalence_claim_ready": False,
        "absolute_ppm_paths_exported": False,
        "absolute_aex_paths_exported": False,
        "host_harness_path_payload_exported": False,
        "requires_future_runtime_approval": True,
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
        "aepx_file_modified": False,
        "aep_binary_modified": False,
        "ae_project_write_performed": False,
        "ofx_plugin_built": False,
        "ofx_describe_performed": False,
        "ofx_render_performed": False,
        "aex_render_performed": False,
        "render_validation_performed": False,
        "pipl_payload_parsed": False,
        "parameter_schema_emitted": False,
        "redacted_schema_emitted": False,
        "real_pipl_payload_parser_enabled": False,
        "real_pipl_payload_parsed": False,
        "resource_payload_opened": False,
        "resource_payload_extracted": False,
        "raw_payload_serialized": False,
        "blocked_actions": list(BLOCKED_ACTIONS),
        "next_required_actions": [
            "Implement a no-op harness only as a separate reviewed dry-run consumer first.",
            "Keep real OFX runtime creation disabled until explicit runtime approval exists.",
            "Keep AEX-backed describe/render closed until fixture approval, native loader, and schema evidence are complete.",
        ],
        "notes": [
            "This dry-run plans host harness cases only and executes nothing.",
            "It consumes bridge JSON evidence only and does not read PPM pixels.",
            "No AEX, AE, DLL, OFX runtime, describe, render, project write, or payload action is performed.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build candidate OFX host harness dry-run packet")
    parser.add_argument("--candidate-ofx-bridge", required=True, help="Candidate OFX bridge packet under target/candidate-ofx-bridge")
    parser.add_argument("--out", required=True, help="Create-new dry-run JSON under target/candidate-ofx-host-harness-dryrun")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    bridge, bridge_path = load_bridge(Path(args.candidate_ofx_bridge))
    packet = build_harness_dryrun(bridge=bridge, bridge_path=bridge_path)
    written = write_json_create_new(Path(args.out), packet)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Run synthetic no-load checks for the candidate OFX host harness plan.

The selftest consumes the candidate OFX host-harness dry-run JSON only. It does
not build or invoke an OFX runtime, open AEX files, read PPM pixels, describe
real effects, render pixels, or route through OFX.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
HOST_HARNESS_DRYRUN_ROOT = TARGET_ROOT / "candidate-ofx-host-harness-dryrun"
HOST_HARNESS_SELFTEST_ROOT = TARGET_ROOT / "candidate-ofx-host-harness-selftest"

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
    "read_ppm_pixels",
    "open_candidate_mock_ppm",
    "read_ppm_pixels_for_render",
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
        raise ValueError("candidate OFX host harness selftest must have .json extension")
    HOST_HARNESS_SELFTEST_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, HOST_HARNESS_SELFTEST_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(HOST_HARNESS_SELFTEST_ROOT.resolve(strict=True)):
        raise ValueError(f"candidate OFX host harness selftest parent must stay under {HOST_HARNESS_SELFTEST_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_harness_dryrun(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, HOST_HARNESS_DRYRUN_ROOT, "candidate OFX host harness dry-run")
    return read_json_object(resolved), resolved


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def validate_relative_ppm_surface(value: Any) -> list[str]:
    errors: list[str] = []
    if not isinstance(value, str) or not value:
        return ["render case candidate_mock_output_ppm_relative must be present"]
    path = Path(value)
    if path.is_absolute():
        errors.append("render case candidate_mock_output_ppm_relative must be relative")
    if path_has_traversal(path):
        errors.append("render case candidate_mock_output_ppm_relative must not contain traversal components")
    if path.suffix.lower() != ".ppm":
        errors.append("render case candidate_mock_output_ppm_relative must point to a .ppm surface")
    if not value.replace("\\", "/").startswith("target/candidate-image-compat-mock/"):
        errors.append("render case candidate_mock_output_ppm_relative must stay in candidate image mock output root")
    return errors


def planned_case_map(dryrun: dict[str, Any]) -> dict[str, dict[str, Any]]:
    cases = dryrun.get("planned_cases")
    if not isinstance(cases, list):
        return {}
    mapped: dict[str, dict[str, Any]] = {}
    for case in cases:
        if isinstance(case, dict) and isinstance(case.get("case_id"), str):
            mapped[case["case_id"]] = case
    return mapped


def validate_planned_case(case: dict[str, Any], *, expected_phase: str, blocked_real_action: str) -> list[str]:
    errors: list[str] = []
    if case.get("phase") != expected_phase:
        errors.append(f"{case.get('case_id')} phase must be {expected_phase}")
    if case.get("would_execute") is not False:
        errors.append(f"{case.get('case_id')} would_execute must be false")
    if case.get("uses_real_ofx_runtime") is not False:
        errors.append(f"{case.get('case_id')} uses_real_ofx_runtime must be false")
    if case.get("blocked_real_action") != blocked_real_action:
        errors.append(f"{case.get('case_id')} must block {blocked_real_action}")
    return errors


def validate_harness_dryrun(dryrun: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if dryrun.get("publication_status") != "local-only":
        errors.append("dry-run publication_status must be local-only")
    if dryrun.get("report_kind") != "aex_candidate_ofx_host_harness_dryrun":
        errors.append("dry-run report_kind must be aex_candidate_ofx_host_harness_dryrun")
    if dryrun.get("harness_dryrun_state") != "candidate_ofx_host_harness_dryrun_ready_route_closed":
        errors.append("harness_dryrun_state must be candidate_ofx_host_harness_dryrun_ready_route_closed")
    if dryrun.get("harness_dryrun_ready") is not True:
        errors.append("harness_dryrun_ready must be true")
    if dryrun.get("dry_run_only") is not True:
        errors.append("dry_run_only must be true")
    if dryrun.get("would_execute") is not False:
        errors.append("would_execute must be false")
    if dryrun.get("execution_performed") is not False:
        errors.append("execution_performed must be false")
    if dryrun.get("host_harness_kind") != "ofx_noop_host_harness_planning":
        errors.append("host_harness_kind must be ofx_noop_host_harness_planning")
    if dryrun.get("planned_case_count") != 2:
        errors.append("planned_case_count must be 2")
    if dryrun.get("planned_noop_describe_case_count") != 1:
        errors.append("planned_noop_describe_case_count must be 1")
    if dryrun.get("planned_noop_render_case_count") != 1:
        errors.append("planned_noop_render_case_count must be 1")
    if dryrun.get("planned_real_describe_case_count") != 0:
        errors.append("planned_real_describe_case_count must be 0")
    if dryrun.get("planned_real_render_case_count") != 0:
        errors.append("planned_real_render_case_count must be 0")
    if dryrun.get("source_bridge_state") != "candidate_ofx_bridge_ready_no_load_route_closed":
        errors.append("source_bridge_state must be candidate_ofx_bridge_ready_no_load_route_closed")
    if dryrun.get("source_bridge_allowed_route") != "no_op_identity_only":
        errors.append("source_bridge_allowed_route must be no_op_identity_only")
    if dryrun.get("source_mock_route_ready") is not True:
        errors.append("source_mock_route_ready must be true")
    for key in (
        "source_real_route_open",
        "source_real_ofx_route_ready",
        "source_ofx_runtime_invoked",
        "source_aex_runtime_invoked",
        "source_ofx_describe_ready",
        "source_ofx_render_ready",
        "source_render_equivalence_claim_ready",
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
            errors.append(f"dry-run {key} must be false")
    if dryrun.get("requires_future_runtime_approval") is not True:
        errors.append("requires_future_runtime_approval must be true")

    cases = planned_case_map(dryrun)
    describe = cases.get("noop_describe_contract")
    render = cases.get("noop_render_identity_contract")
    if describe is None:
        errors.append("planned_cases must include noop_describe_contract")
    else:
        errors.extend(validate_planned_case(describe, expected_phase="describe", blocked_real_action="ofx_describe_from_aex"))
        if describe.get("uses_aex_metadata") is not False:
            errors.append("noop_describe_contract uses_aex_metadata must be false")
    if render is None:
        errors.append("planned_cases must include noop_render_identity_contract")
    else:
        errors.extend(validate_planned_case(render, expected_phase="render", blocked_real_action="ofx_render_with_aex"))
        if render.get("uses_aex_pixels") is not False:
            errors.append("noop_render_identity_contract uses_aex_pixels must be false")
        errors.extend(validate_relative_ppm_surface(render.get("candidate_mock_output_ppm_relative")))

    blocked = dryrun.get("blocked_actions")
    if not isinstance(blocked, list):
        errors.append("dry-run blocked_actions must be a list")
    else:
        for action in ("instantiate_ofx_runtime", "ofx_describe_from_aex", "ofx_render_with_aex", "route_through_real_ofx"):
            if action not in blocked:
                errors.append(f"dry-run must block {action}")
    errors.extend(safety_errors(dryrun, "dry-run"))
    return errors


def relative_to_lab(path: Path) -> str:
    return path.resolve(strict=False).relative_to(LAB_ROOT.resolve(strict=True)).as_posix()


def build_synthetic_descriptor(dryrun: dict[str, Any]) -> dict[str, Any]:
    return {
        "descriptor_kind": "synthetic_ofx_noop_descriptor",
        "plugin_identifier": "local.aexcompatlab.candidate.noop.identity",
        "candidate_relative_path": dryrun.get("candidate_relative_path"),
        "context": "filter",
        "parameters": [],
        "clips": [
            {"name": "Source", "component": "RGBA", "path_payload": None},
            {"name": "Output", "component": "RGBA", "path_payload": None},
        ],
        "descriptor_metadata_only": True,
        "aex_parameter_schema_used": False,
        "real_ofx_describe_performed": False,
    }


def build_case_results(dryrun: dict[str, Any]) -> list[dict[str, Any]]:
    cases = planned_case_map(dryrun)
    render_surface = cases["noop_render_identity_contract"]["candidate_mock_output_ppm_relative"]
    return [
        {
            "case_id": "noop_describe_contract",
            "case_status": "passed",
            "synthetic_descriptor_created": True,
            "descriptor_metadata_only": True,
            "descriptor_has_no_aex_parameters": True,
            "real_ofx_runtime_used": False,
            "real_ofx_describe_performed": False,
            "aex_metadata_used": False,
        },
        {
            "case_id": "noop_render_identity_contract",
            "case_status": "passed",
            "synthetic_render_contract_created": True,
            "identity_contract_checked": True,
            "candidate_mock_output_ppm_relative": render_surface,
            "candidate_mock_surface_reused_as_string_only": True,
            "ppm_pixel_read_performed": False,
            "real_ofx_runtime_used": False,
            "real_ofx_render_performed": False,
            "aex_pixels_used": False,
        },
    ]


def build_harness_selftest(*, dryrun: dict[str, Any], dryrun_path: Path) -> dict[str, Any]:
    errors = validate_harness_dryrun(dryrun)
    if errors:
        raise ValueError("; ".join(errors))
    case_results = build_case_results(dryrun)
    passed_count = sum(1 for case in case_results if case["case_status"] == "passed")
    render_surface = planned_case_map(dryrun)["noop_render_identity_contract"]["candidate_mock_output_ppm_relative"]
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_ofx_host_harness_selftest",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_candidate_ofx_host_harness_dryrun": relative_to_lab(dryrun_path),
        "source_harness_dryrun_state": dryrun.get("harness_dryrun_state"),
        "source_harness_dryrun_ready": dryrun.get("harness_dryrun_ready"),
        "source_dry_run_only": dryrun.get("dry_run_only"),
        "source_would_execute": dryrun.get("would_execute"),
        "source_execution_performed": dryrun.get("execution_performed"),
        "source_host_harness_kind": dryrun.get("host_harness_kind"),
        "source_bridge_state": dryrun.get("source_bridge_state"),
        "source_bridge_allowed_route": dryrun.get("source_bridge_allowed_route"),
        "candidate_relative_path": dryrun.get("candidate_relative_path"),
        "host_harness_selftest_state": "candidate_ofx_host_harness_selftest_passed_synthetic_route_closed",
        "host_harness_selftest_ready": True,
        "host_harness_kind": "ofx_noop_host_harness_synthetic_selftest",
        "selftest_state": "candidate_ofx_host_harness_selftest_passed_no_load",
        "selftest_ready": True,
        "selftest_kind": "synthetic_ofx_noop_host_harness_contract_selftest",
        "synthetic_only": True,
        "synthetic_contract_checks_performed": True,
        "synthetic_contract_execution_performed": True,
        "real_execution_performed": False,
        "real_harness_execution_performed": False,
        "dry_run_consumed": True,
        "planned_cases_verified": True,
        "planned_case_count": dryrun.get("planned_case_count"),
        "checked_case_count": len(case_results),
        "checked_noop_describe_case_count": 1,
        "checked_noop_render_case_count": 1,
        "checked_real_describe_case_count": 0,
        "checked_real_render_case_count": 0,
        "case_result_count": len(case_results),
        "case_passed_count": passed_count,
        "case_failed_count": len(case_results) - passed_count,
        "descriptor_contract_checked": True,
        "render_identity_contract_checked": True,
        "synthetic_descriptor_created": True,
        "synthetic_render_contract_created": True,
        "synthetic_descriptor": build_synthetic_descriptor(dryrun),
        "case_results": case_results,
        "candidate_mock_output_ppm_relative": render_surface,
        "candidate_mock_surface_reused": True,
        "candidate_mock_surface_reused_as_string_only": True,
        "ppm_pixel_read_performed": False,
        "absolute_ppm_paths_exported": False,
        "absolute_aex_paths_exported": False,
        "host_harness_path_payload_exported": False,
        "requires_future_runtime_approval": True,
        "real_route_open": False,
        "real_ofx_route_ready": False,
        "ofx_runtime_invoked": False,
        "aex_runtime_invoked": False,
        "ofx_describe_ready": False,
        "ofx_render_ready": False,
        "render_equivalence_claim_ready": False,
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
        "allowed_selftest_actions": [
            "validate_planned_cases_shape",
            "synthesize_noop_descriptor_contract",
            "synthesize_identity_render_contract",
            "validate_relative_surface_path_string",
        ],
        "blocked_actions": list(BLOCKED_ACTIONS),
        "next_required_actions": [
            "Keep this as a synthetic no-load selftest until explicit OFX runtime approval exists.",
            "Add a reviewed runtime harness boundary before any real OFX describe/render action.",
            "Keep AEX-backed describe/render closed until fixture approval and native-loader gates are complete.",
        ],
        "notes": [
            "This selftest executes synthetic contract checks only.",
            "It consumes host harness dry-run JSON and reuses the candidate mock PPM path as a relative string.",
            "No AEX, AE, DLL, OFX runtime, real describe, real render, pixel read, project write, or payload action is performed.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run candidate OFX host harness synthetic selftest")
    parser.add_argument(
        "--host-harness-dryrun",
        required=True,
        help="Candidate OFX host harness dry-run under target/candidate-ofx-host-harness-dryrun",
    )
    parser.add_argument("--out", required=True, help="Create-new selftest JSON under target/candidate-ofx-host-harness-selftest")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    dryrun, dryrun_path = load_harness_dryrun(Path(args.host_harness_dryrun))
    report = build_harness_selftest(dryrun=dryrun, dryrun_path=dryrun_path)
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

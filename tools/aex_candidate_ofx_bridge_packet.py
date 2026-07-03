#!/usr/bin/env python3
"""Build a no-load candidate-to-OFX bridge packet.

The packet connects a selected candidate compatibility card and image mock
report to the existing closed OFX facade/route contract evidence. It reads JSON
artifacts only and never opens AEX files, reads PPM pixels, invokes OFX, starts
After Effects, or renders.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
COMPAT_CARD_ROOT = TARGET_ROOT / "candidate-compat-card"
IMAGE_COMPAT_MOCK_ROOT = TARGET_ROOT / "candidate-image-compat-mock"
OFX_FACADE_ROOT = TARGET_ROOT / "ofx-facade"
OFX_ROUTE_CONTRACT_ROOT = TARGET_ROOT / "ofx-route-contract"
CANDIDATE_OFX_BRIDGE_ROOT = TARGET_ROOT / "candidate-ofx-bridge"

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
        raise ValueError("candidate OFX bridge packet must have .json extension")
    CANDIDATE_OFX_BRIDGE_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, CANDIDATE_OFX_BRIDGE_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(CANDIDATE_OFX_BRIDGE_ROOT.resolve(strict=True)):
        raise ValueError(f"candidate OFX bridge parent must stay under {CANDIDATE_OFX_BRIDGE_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_candidate_compatibility_card(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, COMPAT_CARD_ROOT, "candidate compatibility card")
    return read_json_object(resolved), resolved


def load_candidate_image_mock(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, IMAGE_COMPAT_MOCK_ROOT, "candidate image compatibility mock")
    return read_json_object(resolved), resolved


def load_ofx_facade(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, OFX_FACADE_ROOT, "OFX facade packet")
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


def require_blocked_actions(payload: dict[str, Any], label: str, actions: tuple[str, ...]) -> list[str]:
    blocked = payload.get("blocked_actions")
    if not isinstance(blocked, list):
        return [f"{label} blocked_actions must be a list"]
    return [f"{label} must block {action}" for action in actions if action not in blocked]


def path_field_errors(value: Any, label: str) -> list[str]:
    if not isinstance(value, str) or not value:
        return [f"{label} must be a non-empty relative path string"]
    if Path(value).is_absolute():
        return [f"{label} must not be an absolute path"]
    if path_has_traversal(Path(value)):
        return [f"{label} must not contain traversal components"]
    return []


def validate_compatibility_card(card: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if card.get("publication_status") != "local-only":
        errors.append("compatibility card publication_status must be local-only")
    if card.get("report_kind") != "aex_candidate_compatibility_card":
        errors.append("compatibility card report_kind must be aex_candidate_compatibility_card")
    if card.get("compatibility_card_state") != "candidate_compatibility_card_ready_no_load":
        errors.append("compatibility card state must be candidate_compatibility_card_ready_no_load")
    if card.get("compatibility_card_ready") is not True:
        errors.append("compatibility card must be ready")
    for key in (
        "unsafe_exports_present",
        "absolute_ppm_paths_exported",
        "absolute_aex_paths_exported",
        "fixture_approval_satisfied",
        "path_acceptance_ready",
        "aex_path_acceptance_enabled",
        "real_render_open",
        "real_route_open",
        "aex_file_hashed",
        "aex_file_copied",
    ):
        if card.get(key) is not False:
            errors.append(f"compatibility card {key} must be false")
    if card.get("native_load_gate") != "closed":
        errors.append("compatibility card native_load_gate must be closed")
    if card.get("native_load_gate_stays_closed") is not True:
        errors.append("compatibility card native_load_gate_stays_closed must be true")
    if not card.get("candidate_relative_path"):
        errors.append("compatibility card candidate_relative_path must be present")
    errors.extend(require_blocked_actions(card, "compatibility card", ("load_aex_dll", "route_through_real_ofx", "ofx_render_with_aex")))
    errors.extend(safety_errors(card, "compatibility card"))
    return errors


def validate_image_mock(mock: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if mock.get("publication_status") != "local-only":
        errors.append("image mock publication_status must be local-only")
    if mock.get("report_kind") != "aex_candidate_image_compat_mock":
        errors.append("image mock report_kind must be aex_candidate_image_compat_mock")
    if mock.get("mock_state") != "candidate_image_compat_mock_passed_no_load":
        errors.append("image mock state must be candidate_image_compat_mock_passed_no_load")
    if mock.get("mock_ready") is not True:
        errors.append("image mock must be ready")
    if mock.get("source_compatibility_card_state") != "candidate_compatibility_card_ready_no_load":
        errors.append("image mock source compatibility card state must be ready")
    if mock.get("source_native_load_gate") != "closed":
        errors.append("image mock source native load gate must be closed")
    for key in (
        "source_real_render_open",
        "source_real_route_open",
        "source_path_acceptance_ready",
        "source_aex_path_acceptance_enabled",
        "source_fixture_approval_satisfied",
        "source_absolute_ppm_paths_exported",
        "source_absolute_aex_paths_exported",
        "input_ppm_absolute_path_exported",
        "output_ppm_absolute_path_exported",
    ):
        if mock.get(key) is not False:
            errors.append(f"image mock {key} must be false")
    check = mock.get("transform_check")
    if not isinstance(check, dict):
        errors.append("image mock transform_check must be an object")
    else:
        for key in ("pixel_match_expected", "dimension_match_expected", "input_dimension_match"):
            if check.get(key) is not True:
                errors.append(f"image mock transform_check {key} must be true")
    errors.extend(path_field_errors(mock.get("input_ppm_relative"), "image mock input_ppm_relative"))
    errors.extend(path_field_errors(mock.get("output_ppm_relative"), "image mock output_ppm_relative"))
    errors.extend(require_blocked_actions(mock, "image mock", ("load_aex_dll", "route_through_real_ofx", "ofx_render_with_aex")))
    errors.extend(safety_errors(mock, "image mock"))
    return errors


def validate_ofx_facade(facade: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if facade.get("publication_status") != "local-only":
        errors.append("OFX facade publication_status must be local-only")
    if facade.get("packet_kind") != "aex_ofx_facade_deferred_packet":
        errors.append("OFX facade packet_kind must be aex_ofx_facade_deferred_packet")
    if facade.get("facade_state") != "deferred_loader_not_ready":
        errors.append("OFX facade facade_state must be deferred_loader_not_ready")
    if facade.get("ofx_route_action") != "no_op":
        errors.append("OFX facade ofx_route_action must be no_op")
    mapping_plan = facade.get("mapping_plan")
    if not isinstance(mapping_plan, dict) or mapping_plan.get("state") != "planning_only":
        errors.append("OFX facade mapping_plan state must be planning_only")
    errors.extend(require_blocked_actions(facade, "OFX facade", ("ofx_describe_from_aex", "ofx_render_with_aex", "route_through_ofx")))
    errors.extend(safety_errors(facade, "OFX facade"))
    return errors


def validate_route_contract(contract: dict[str, Any]) -> list[str]:
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
    route = contract.get("route_contract")
    if not isinstance(route, dict):
        errors.append("OFX route contract route_contract must be an object")
    else:
        if route.get("allowed_route") != "no_op_identity_only":
            errors.append("OFX route contract allowed_route must be no_op_identity_only")
        if route.get("ofx_runtime_invoked") is not False:
            errors.append("OFX route contract ofx_runtime_invoked must be false")
        if route.get("aex_runtime_invoked") is not False:
            errors.append("OFX route contract aex_runtime_invoked must be false")
    blockers = contract.get("blockers")
    if not isinstance(blockers, list) or not blockers:
        errors.append("OFX route contract blockers must be a non-empty list")
    errors.extend(require_blocked_actions(contract, "OFX route contract", ("ofx_describe_from_aex", "ofx_render_with_aex", "route_through_ofx")))
    errors.extend(safety_errors(contract, "OFX route contract"))
    return errors


def bridge_errors(
    *,
    card: dict[str, Any],
    image_mock: dict[str, Any],
    facade: dict[str, Any],
    route_contract: dict[str, Any],
) -> list[str]:
    errors: list[str] = []
    errors.extend(validate_compatibility_card(card))
    errors.extend(validate_image_mock(image_mock))
    errors.extend(validate_ofx_facade(facade))
    errors.extend(validate_route_contract(route_contract))
    candidate = card.get("candidate_relative_path")
    if image_mock.get("candidate_relative_path") != candidate:
        errors.append("image mock candidate_relative_path must match compatibility card")
    primary_candidate = facade.get("primary_review_candidate")
    if isinstance(primary_candidate, dict) and primary_candidate.get("relative_path") not in (None, candidate):
        errors.append("OFX facade primary_review_candidate must match compatibility card")
    return errors


def relative_to_lab(path: Path) -> str:
    return path.resolve(strict=False).relative_to(LAB_ROOT.resolve(strict=True)).as_posix()


def build_bridge_packet(
    *,
    card: dict[str, Any],
    card_path: Path,
    image_mock: dict[str, Any],
    image_mock_path: Path,
    facade: dict[str, Any],
    facade_path: Path,
    route_contract: dict[str, Any],
    route_contract_path: Path,
) -> dict[str, Any]:
    errors = bridge_errors(card=card, image_mock=image_mock, facade=facade, route_contract=route_contract)
    if errors:
        raise ValueError("; ".join(errors))
    route = route_contract.get("route_contract") if isinstance(route_contract.get("route_contract"), dict) else {}
    transform_check = image_mock.get("transform_check") if isinstance(image_mock.get("transform_check"), dict) else {}
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_ofx_bridge_packet",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_candidate_compatibility_card": relative_to_lab(card_path),
        "source_candidate_image_compat_mock": relative_to_lab(image_mock_path),
        "source_ofx_facade": relative_to_lab(facade_path),
        "source_ofx_route_contract": relative_to_lab(route_contract_path),
        "candidate_relative_path": card.get("candidate_relative_path"),
        "bridge_state": "candidate_ofx_bridge_ready_no_load_route_closed",
        "bridge_ready": True,
        "candidate_image_mock_available": True,
        "ofx_closed_route_contract_available": True,
        "ofx_bridge_packet_created": True,
        "image_surface": {
            "state": "candidate_mock_output_available_relative_path_only",
            "operation": image_mock.get("operation"),
            "input_ppm_relative": image_mock.get("input_ppm_relative"),
            "output_ppm_relative": image_mock.get("output_ppm_relative"),
            "input_ppm_absolute_path_exported": image_mock.get("input_ppm_absolute_path_exported"),
            "output_ppm_absolute_path_exported": image_mock.get("output_ppm_absolute_path_exported"),
            "transform_check": transform_check,
        },
        "ofx_bridge_plan": {
            "state": "mock_output_bound_to_closed_ofx_contract",
            "allowed_bridge": "candidate_mock_image_surface_to_noop_ofx_contract_only",
            "allowed_route": route.get("allowed_route"),
            "mock_route_ready": route_contract.get("mock_route_ready"),
            "real_route_open": False,
            "real_ofx_route_ready": False,
            "ofx_runtime_invoked": False,
            "aex_runtime_invoked": False,
            "ofx_describe_ready": False,
            "ofx_render_ready": False,
            "render_equivalence_claim_ready": False,
        },
        "gate_card": {
            "native_load_gate": card.get("native_load_gate"),
            "native_load_gate_stays_closed": card.get("native_load_gate_stays_closed"),
            "fixture_approval_satisfied": card.get("fixture_approval_satisfied"),
            "path_acceptance_ready": card.get("path_acceptance_ready"),
            "aex_path_acceptance_enabled": card.get("aex_path_acceptance_enabled"),
            "real_render_open": False,
            "real_route_open": False,
            "route_contract_state": route_contract.get("contract_state"),
            "facade_state": facade.get("facade_state"),
        },
        "source_states": {
            "compatibility_card_state": card.get("compatibility_card_state"),
            "image_mock_state": image_mock.get("mock_state"),
            "ofx_facade_state": facade.get("facade_state"),
            "ofx_route_contract_state": route_contract.get("contract_state"),
        },
        "source_compatibility_card_state": card.get("compatibility_card_state"),
        "source_image_mock_state": image_mock.get("mock_state"),
        "source_ofx_facade_state": facade.get("facade_state"),
        "source_ofx_route_contract_state": route_contract.get("contract_state"),
        "bridge_allowed_route": route.get("allowed_route"),
        "mock_route_ready": route_contract.get("mock_route_ready"),
        "real_route_open": False,
        "real_ofx_route_ready": False,
        "ofx_runtime_invoked": False,
        "aex_runtime_invoked": False,
        "ofx_describe_ready": False,
        "ofx_render_ready": False,
        "render_equivalence_claim_ready": False,
        "absolute_ppm_paths_exported": False,
        "absolute_aex_paths_exported": False,
        "ofx_bridge_path_payload_exported": False,
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
            "Keep the bridge as JSON-only evidence until fixture approval exists.",
            "Use this packet to design a no-op OFX host harness without invoking an OFX runtime.",
            "Keep real OFX describe/render and AEX-backed routes closed until native loader and schema evidence are approved.",
        ],
        "notes": [
            "This bridge packet records a safe handoff surface only.",
            "It does not read PPM pixels; it trusts the candidate image mock report already generated.",
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
    parser = argparse.ArgumentParser(description="Build no-load candidate OFX bridge packet")
    parser.add_argument("--compat-card", required=True, help="Candidate compatibility card under target/candidate-compat-card")
    parser.add_argument("--image-mock", required=True, help="Candidate image mock report under target/candidate-image-compat-mock")
    parser.add_argument("--ofx-facade", required=True, help="OFX facade packet under target/ofx-facade")
    parser.add_argument("--ofx-route-contract", required=True, help="OFX route contract under target/ofx-route-contract")
    parser.add_argument("--out", required=True, help="Create-new candidate OFX bridge packet under target/candidate-ofx-bridge")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    card, card_path = load_candidate_compatibility_card(Path(args.compat_card))
    image_mock, image_mock_path = load_candidate_image_mock(Path(args.image_mock))
    facade, facade_path = load_ofx_facade(Path(args.ofx_facade))
    route_contract, route_contract_path = load_ofx_route_contract(Path(args.ofx_route_contract))
    packet = build_bridge_packet(
        card=card,
        card_path=card_path,
        image_mock=image_mock,
        image_mock_path=image_mock_path,
        facade=facade,
        facade_path=facade_path,
        route_contract=route_contract,
        route_contract_path=route_contract_path,
    )
    written = write_json_create_new(Path(args.out), packet)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

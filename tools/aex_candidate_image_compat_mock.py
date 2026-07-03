#!/usr/bin/env python3
"""Run a no-load candidate image compatibility mock transform.

This tool is a deliberately small image-facing bridge: it reads a selected
candidate compatibility card and a generated PPM fixture only, then applies a
deterministic mock PPM transform. It never opens, hashes, copies, loads, renders,
or routes the candidate AEX.
"""

from __future__ import annotations

import argparse
import json
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TOOLS_ROOT = LAB_ROOT / "tools"
if str(TOOLS_ROOT) not in sys.path:
    sys.path.insert(0, str(TOOLS_ROOT))

import ppm_fixture_tool

TARGET_ROOT = LAB_ROOT / "target"
COMPAT_CARD_ROOT = TARGET_ROOT / "candidate-compat-card"
PPM_FIXTURE_ROOT = TARGET_ROOT / "ppm-fixtures"
IMAGE_COMPAT_MOCK_ROOT = TARGET_ROOT / "candidate-image-compat-mock"
ALLOWED_OPERATIONS = ("identity", "invert")

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


def validate_input_ppm(path: Path) -> Path:
    if path.suffix.lower() != ".ppm":
        raise ValueError("input PPM must have .ppm extension")
    PPM_FIXTURE_ROOT.mkdir(parents=True, exist_ok=True)
    return resolve_under_root(path, PPM_FIXTURE_ROOT, must_exist=True)


def validate_output_path(path: Path, suffix: str, label: str) -> Path:
    if path.suffix.lower() != suffix:
        raise ValueError(f"{label} must have {suffix} extension")
    IMAGE_COMPAT_MOCK_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, IMAGE_COMPAT_MOCK_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(IMAGE_COMPAT_MOCK_ROOT.resolve(strict=True)):
        raise ValueError(f"{label} parent must stay under {IMAGE_COMPAT_MOCK_ROOT}")
    return resolved


def validate_output_ppm(path: Path) -> Path:
    return validate_output_path(path, ".ppm", "candidate image mock output PPM")


def validate_output_json(path: Path) -> Path:
    return validate_output_path(path, ".json", "candidate image mock report")


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_compat_card(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, COMPAT_CARD_ROOT, "candidate compatibility card")
    return read_json_object(resolved), resolved


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def validate_compat_card(card: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if card.get("publication_status") != "local-only":
        errors.append("compatibility card publication_status must be local-only")
    if card.get("report_kind") != "aex_candidate_compatibility_card":
        errors.append("compatibility card report_kind must be aex_candidate_compatibility_card")
    if card.get("compatibility_card_state") != "candidate_compatibility_card_ready_no_load":
        errors.append("compatibility card state must be candidate_compatibility_card_ready_no_load")
    if card.get("compatibility_card_ready") is not True:
        errors.append("compatibility_card_ready must be true")
    if not card.get("candidate_relative_path"):
        errors.append("candidate_relative_path must be present")

    expected_false = (
        "unsafe_exports_present",
        "absolute_ppm_paths_exported",
        "absolute_aex_paths_exported",
        "approval_can_be_issued_now",
        "approval_manifest_created",
        "current_fixture_approval_valid",
        "fixture_approval_satisfied",
        "path_acceptance_ready",
        "aex_path_acceptance_enabled",
        "path_payload_supplied",
        "real_render_open",
        "real_route_open",
        "aex_file_hashed",
        "aex_file_copied",
    )
    for key in expected_false:
        if card.get(key) is not False:
            errors.append(f"compatibility card {key} must be false")
    if card.get("native_load_gate") != "closed":
        errors.append("compatibility card native_load_gate must be closed")
    if card.get("native_load_gate_stays_closed") is not True:
        errors.append("compatibility card native_load_gate_stays_closed must be true")
    if card.get("approval_gate_stays_closed") is not True:
        errors.append("compatibility card approval_gate_stays_closed must be true")
    if card.get("accepted_aex_path") is not None:
        errors.append("compatibility card accepted_aex_path must be null")

    no_load_card = card.get("no_load_test_card")
    if not isinstance(no_load_card, dict):
        errors.append("compatibility card no_load_test_card must be an object")
    else:
        if no_load_card.get("worker_identity_passed") is not True:
            errors.append("no_load_test_card worker_identity_passed must be true")
        if no_load_card.get("ofx_noop_identity_passed") is not True:
            errors.append("no_load_test_card ofx_noop_identity_passed must be true")
        if no_load_card.get("blocked_load_aex_verified") is not True:
            errors.append("no_load_test_card blocked_load_aex_verified must be true")
        if no_load_card.get("ppm_paths_exported") is not False:
            errors.append("no_load_test_card ppm_paths_exported must be false")
        if no_load_card.get("real_render_open") is not False:
            errors.append("no_load_test_card real_render_open must be false")
        if no_load_card.get("real_route_open") is not False:
            errors.append("no_load_test_card real_route_open must be false")

    errors.extend(safety_errors(card, "compatibility card"))
    return errors


def relative_to_lab(path: Path) -> str:
    return path.resolve(strict=False).relative_to(LAB_ROOT.resolve(strict=True)).as_posix()


def write_ppm_create_new(path: Path, image: ppm_fixture_tool.PpmImage) -> Path:
    if image.width <= 0 or image.height <= 0:
        raise ValueError("image dimensions must be positive")
    if image.width * image.height > ppm_fixture_tool.MAX_PIXELS:
        raise ValueError("image exceeds max fixture pixel count")
    expected = image.width * image.height * 3
    if len(image.pixels) != expected:
        raise ValueError(f"pixel byte count mismatch: expected {expected}, got {len(image.pixels)}")
    resolved = validate_output_ppm(path)
    header = f"P6\n{image.width} {image.height}\n255\n".encode("ascii")
    with resolved.open("xb") as handle:
        handle.write(header)
        handle.write(image.pixels)
    return resolved


def transform_check(
    *,
    input_image: ppm_fixture_tool.PpmImage,
    output_image: ppm_fixture_tool.PpmImage,
    expected_image: ppm_fixture_tool.PpmImage,
) -> dict[str, Any]:
    pixel_match_expected = output_image.pixels == expected_image.pixels
    dimension_match_expected = output_image.width == expected_image.width and output_image.height == expected_image.height
    input_dimension_match = output_image.width == input_image.width and output_image.height == input_image.height
    if not pixel_match_expected or not dimension_match_expected:
        raise AssertionError("candidate image mock output did not match expected transform")
    return {
        "width": output_image.width,
        "height": output_image.height,
        "bytes": len(output_image.pixels),
        "pixel_match_expected": pixel_match_expected,
        "dimension_match_expected": dimension_match_expected,
        "input_dimension_match": input_dimension_match,
    }


def build_mock_report(
    *,
    card: dict[str, Any],
    card_path: Path,
    input_ppm: Path,
    output_ppm: Path,
    operation: str,
) -> dict[str, Any]:
    if operation not in ALLOWED_OPERATIONS:
        raise ValueError(f"unsupported operation: {operation}")
    card_errors = validate_compat_card(card)
    resolved_input = validate_input_ppm(input_ppm)
    resolved_output = validate_output_ppm(output_ppm)
    if card_errors:
        raise ValueError("; ".join(card_errors))

    input_image = ppm_fixture_tool.read_ppm(resolved_input)
    expected_image = ppm_fixture_tool.transform_image(input_image, operation)
    output_path = write_ppm_create_new(resolved_output, expected_image)
    output_image = ppm_fixture_tool.read_ppm(output_path)
    check = transform_check(input_image=input_image, output_image=output_image, expected_image=expected_image)

    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_image_compat_mock",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_candidate_compatibility_card": relative_to_lab(card_path),
        "source_compatibility_card_state": card.get("compatibility_card_state"),
        "source_compatibility_card_ready": card.get("compatibility_card_ready"),
        "candidate_relative_path": card.get("candidate_relative_path"),
        "mock_state": "candidate_image_compat_mock_passed_no_load",
        "mock_ready": True,
        "operation": operation,
        "allowed_operations": list(ALLOWED_OPERATIONS),
        "input_ppm_relative": relative_to_lab(resolved_input),
        "output_ppm_relative": relative_to_lab(output_path),
        "input_ppm_absolute_path_exported": False,
        "output_ppm_absolute_path_exported": False,
        "transform_check": check,
        "candidate_image_mock_performed": True,
        "mock_transform_performed": True,
        "source_native_load_gate": card.get("native_load_gate"),
        "source_native_load_gate_stays_closed": card.get("native_load_gate_stays_closed"),
        "source_real_render_open": card.get("real_render_open"),
        "source_real_route_open": card.get("real_route_open"),
        "source_path_acceptance_ready": card.get("path_acceptance_ready"),
        "source_aex_path_acceptance_enabled": card.get("aex_path_acceptance_enabled"),
        "source_fixture_approval_satisfied": card.get("fixture_approval_satisfied"),
        "source_absolute_ppm_paths_exported": card.get("absolute_ppm_paths_exported"),
        "source_absolute_aex_paths_exported": card.get("absolute_aex_paths_exported"),
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
        "notes": [
            "This is a deterministic no-load image mock, not an AEX render.",
            "The candidate compatibility card is used as gate evidence only.",
            "The report exports relative PPM paths only and no AEX paths.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_json(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run no-load candidate image compatibility mock")
    parser.add_argument("--compat-card", required=True, help="Candidate compatibility card under target/candidate-compat-card")
    parser.add_argument("--input-ppm", required=True, help="Input PPM under target/ppm-fixtures")
    parser.add_argument("--operation", choices=ALLOWED_OPERATIONS, default="identity")
    parser.add_argument("--output-ppm", required=True, help="Create-new output PPM under target/candidate-image-compat-mock")
    parser.add_argument("--out", required=True, help="Create-new JSON report under target/candidate-image-compat-mock")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    card, card_path = load_compat_card(Path(args.compat_card))
    report = build_mock_report(
        card=card,
        card_path=card_path,
        input_ppm=Path(args.input_ppm),
        output_ppm=Path(args.output_ppm),
        operation=args.operation,
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

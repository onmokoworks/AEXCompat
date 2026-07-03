#!/usr/bin/env python3
"""Build a no-load render validation contract from local JSON evidence.

The contract reads image validation, image smoke, load gate, and OFX route
contract JSON only. It never opens AEX files, loads DLLs, invokes AE/OFX,
renders, or routes pixels through AEX/OFX.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
IMAGE_VALIDATION_ROOT = TARGET_ROOT / "image-fixture-validation"
IMAGE_SMOKE_ROOT = TARGET_ROOT / "image-input-smoke"
LOAD_GATE_ROOT = TARGET_ROOT / "load-gate"
OFX_ROUTE_CONTRACT_ROOT = TARGET_ROOT / "ofx-route-contract"
RENDER_CONTRACT_ROOT = TARGET_ROOT / "render-validation-contract"

SAFETY_FLAGS = (
    "native_load_enabled",
    "native_load_performed",
    "dll_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
    "aex_file_opened",
    "aepx_file_modified",
    "aep_binary_modified",
    "ae_project_write_performed",
    "ofx_plugin_built",
    "ofx_describe_performed",
    "ofx_render_performed",
)

LOAD_GATE_CLOSED_STATES = {
    "closed_missing_or_invalid_approval",
    "closed_dependency_review_or_invalid_approval",
}


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
        raise ValueError("render validation contract must have .json extension")
    RENDER_CONTRACT_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, RENDER_CONTRACT_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(RENDER_CONTRACT_ROOT.resolve(strict=True)):
        raise ValueError(f"render validation contract parent must stay under {RENDER_CONTRACT_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_image_validation(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, IMAGE_VALIDATION_ROOT, "image validation report")
    return read_json_object(resolved), resolved


def load_image_smoke(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, IMAGE_SMOKE_ROOT, "image input smoke report")
    return read_json_object(resolved), resolved


def load_load_gate(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, LOAD_GATE_ROOT, "load gate report")
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


def require_local_only(payload: dict[str, Any], label: str) -> list[str]:
    if payload.get("publication_status") != "local-only":
        return [f"{label} publication_status must be local-only"]
    return []


def validate_image_validation(report: dict[str, Any]) -> list[str]:
    errors = require_local_only(report, "image validation")
    if report.get("report_kind") != "aex_image_fixture_validation":
        errors.append("image validation report_kind must be aex_image_fixture_validation")
    if report.get("validation_state") != "image_fixture_validation_passed_no_load":
        errors.append("image validation state must be image_fixture_validation_passed_no_load")
    if report.get("validation_passed") is not True:
        errors.append("image validation validation_passed must be true")
    summary = report.get("summary")
    if not isinstance(summary, dict):
        errors.append("image validation summary must be an object")
    else:
        if not isinstance(summary.get("fixture_count"), int) or summary.get("fixture_count") < 1:
            errors.append("image validation fixture_count must be positive")
        if summary.get("failed_count") not in (0, None):
            errors.append("image validation failed_count must be 0")
    fixture_results = report.get("fixture_results")
    if not isinstance(fixture_results, list) or not fixture_results:
        errors.append("image validation fixture_results must be a non-empty list")
    errors.extend(safety_errors(report, "image validation"))
    return errors


def validate_image_smoke(report: dict[str, Any]) -> list[str]:
    errors = require_local_only(report, "image smoke")
    if report.get("report_kind") != "aex_image_input_smoke_tool":
        errors.append("image smoke report_kind must be aex_image_input_smoke_tool")
    if report.get("smoke_state") != "image_input_smoke_passed_route_closed":
        errors.append("image smoke state must be image_input_smoke_passed_route_closed")
    if report.get("worker_identity_passed") is not True:
        errors.append("image smoke worker_identity_passed must be true")
    if report.get("ofx_identity_passed") is not True:
        errors.append("image smoke ofx_identity_passed must be true")
    for label in ("worker_identity_check", "ofx_identity_check", "route_contract_summary", "input_summary"):
        if not isinstance(report.get(label), dict):
            errors.append(f"image smoke {label} must be an object")
    for check_key in ("worker_identity_check", "ofx_identity_check"):
        check = report.get(check_key)
        if isinstance(check, dict):
            if check.get("pixel_match") is not True:
                errors.append(f"image smoke {check_key}.pixel_match must be true")
            if check.get("dimension_match") is not True:
                errors.append(f"image smoke {check_key}.dimension_match must be true")
    if report.get("ofx_runtime_invoked") is not False:
        errors.append("image smoke ofx_runtime_invoked must be false")
    errors.extend(safety_errors(report, "image smoke"))
    return errors


def validate_load_gate(report: dict[str, Any]) -> list[str]:
    errors = require_local_only(report, "load gate")
    if report.get("report_kind") != "aex_load_gate_check":
        errors.append("load gate report_kind must be aex_load_gate_check")
    if report.get("gate_state") not in LOAD_GATE_CLOSED_STATES:
        errors.append("load gate gate_state must remain closed")
    blocked_actions = report.get("blocked_actions")
    if not isinstance(blocked_actions, list):
        errors.append("load gate blocked_actions must be a list")
    else:
        for action in ("load_aex_dll", "call_EffectMain", "render_with_aex", "route_through_ofx"):
            if action not in blocked_actions:
                errors.append(f"load gate must block {action}")
    errors.extend(safety_errors(report, "load gate"))
    return errors


def validate_ofx_route_contract(report: dict[str, Any]) -> list[str]:
    errors = require_local_only(report, "OFX route contract")
    if report.get("report_kind") != "aex_ofx_route_contract_probe":
        errors.append("OFX route contract report_kind must be aex_ofx_route_contract_probe")
    if report.get("contract_state") != "ofx_route_contract_ready_route_closed":
        errors.append("OFX route contract state must be ofx_route_contract_ready_route_closed")
    if report.get("real_route_open") is not False:
        errors.append("OFX route contract real_route_open must be false")
    if report.get("mock_route_ready") is not True:
        errors.append("OFX route contract mock_route_ready must be true")
    route = report.get("route_contract")
    if not isinstance(route, dict):
        errors.append("OFX route contract route_contract must be an object")
    else:
        if route.get("ofx_runtime_invoked") is not False:
            errors.append("OFX route contract route_contract.ofx_runtime_invoked must be false")
        if route.get("aex_runtime_invoked") is not False:
            errors.append("OFX route contract route_contract.aex_runtime_invoked must be false")
    errors.extend(safety_errors(report, "OFX route contract"))
    return errors


def source_errors(
    *,
    image_validation: dict[str, Any],
    image_smoke: dict[str, Any],
    load_gate: dict[str, Any],
    ofx_route_contract: dict[str, Any],
) -> list[str]:
    errors: list[str] = []
    errors.extend(validate_image_validation(image_validation))
    errors.extend(validate_image_smoke(image_smoke))
    errors.extend(validate_load_gate(load_gate))
    errors.extend(validate_ofx_route_contract(ofx_route_contract))
    smoke_route = image_smoke.get("route_contract_summary")
    if isinstance(smoke_route, dict):
        if smoke_route.get("contract_state") != ofx_route_contract.get("contract_state"):
            errors.append("image smoke route contract state must match OFX route contract")
        if smoke_route.get("real_route_open") is not False:
            errors.append("image smoke route summary real_route_open must be false")
    validation_count = image_validation.get("summary", {}).get("fixture_count") if isinstance(image_validation.get("summary"), dict) else None
    if isinstance(validation_count, int) and validation_count < 1:
        errors.append("image validation must include at least one fixture")
    return errors


def build_render_validation_contract(
    *,
    image_validation: dict[str, Any],
    image_validation_path: Path,
    image_smoke: dict[str, Any],
    image_smoke_path: Path,
    load_gate: dict[str, Any],
    load_gate_path: Path,
    ofx_route_contract: dict[str, Any],
    ofx_route_contract_path: Path,
) -> dict[str, Any]:
    errors = source_errors(
        image_validation=image_validation,
        image_smoke=image_smoke,
        load_gate=load_gate,
        ofx_route_contract=ofx_route_contract,
    )
    if errors:
        raise ValueError("; ".join(errors))

    validation_summary = image_validation.get("summary", {}) if isinstance(image_validation.get("summary"), dict) else {}
    smoke_input = image_smoke.get("input_summary", {}) if isinstance(image_smoke.get("input_summary"), dict) else {}
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_render_validation_contract",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_image_validation": str(image_validation_path),
        "source_image_smoke": str(image_smoke_path),
        "source_load_gate": str(load_gate_path),
        "source_ofx_route_contract": str(ofx_route_contract_path),
        "contract_state": "render_validation_contract_ready_render_closed",
        "real_render_open": False,
        "no_load_validation_ready": True,
        "render_contract": {
            "state": "blocked_pending_fixture_approval_native_loader_and_render_harness",
            "real_render_open": False,
            "allowed_current_validation": "generated_ppm_identity_only",
            "forbidden_current_validation": "aex_backed_render_or_ofx_render",
            "required_evidence_before_real_render": [
                "explicit fixture approval artifact",
                "dependency review with no default-deny blockers",
                "sandboxed native loader worker",
                "crash and timeout containment",
                "AEX parameter/schema mapping plan",
                "output image validator with tolerance policy",
            ],
        },
        "image_validation_contract": {
            "state": "no_load_image_fixtures_ready",
            "fixture_count": validation_summary.get("fixture_count"),
            "validated_fixture_count": validation_summary.get("passed_count"),
            "failed_fixture_count": validation_summary.get("failed_count"),
            "total_pixel_bytes": validation_summary.get("total_pixel_bytes"),
            "pattern_counts": validation_summary.get("pattern_counts", {}),
            "smoke_input_summary": smoke_input,
            "smoke_worker_identity_passed": image_smoke.get("worker_identity_passed"),
            "smoke_ofx_identity_passed": image_smoke.get("ofx_identity_passed"),
            "current_tolerance_policy": "exact_identity_only_no_aex_render",
        },
        "gate_contract": {
            "load_gate_state": load_gate.get("gate_state"),
            "dependency_native_load_recommendation": load_gate.get("dependency_native_load_recommendation"),
            "ofx_contract_state": ofx_route_contract.get("contract_state"),
            "ofx_real_route_open": ofx_route_contract.get("real_route_open"),
        },
        "blockers": [
            "load_gate_closed",
            "fixture_not_approved",
            "dependency_review_blocks_native_load",
            "no_sandboxed_native_loader",
            "no_aex_parameter_schema_mapping",
            "no_real_render_harness",
            "no_aex_render_baseline",
            "ofx_route_closed",
        ],
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "aepx_file_modified": False,
        "aep_binary_modified": False,
        "ae_project_write_performed": False,
        "ofx_plugin_built": False,
        "ofx_describe_performed": False,
        "ofx_render_performed": False,
        "aex_render_performed": False,
        "render_validation_performed": False,
        "blocked_actions": [
            "accept_aex_path",
            "load_aex_dll",
            "call_EffectMain",
            "dispatch_PF_Cmd",
            "start_after_effects",
            "render_with_aex",
            "build_ofx_binary",
            "ofx_describe_from_aex",
            "ofx_render_with_aex",
            "route_through_ofx",
        ],
        "next_required_actions": [
            "Keep current validation limited to generated PPM identity checks.",
            "Add a redacted AEX parameter/schema mapping plan before render planning.",
            "Require explicit user fixture approval before any native loader accepts an AEX path.",
            "Design a crash-contained render worker only after the load gate opens.",
        ],
        "notes": [
            "Contract reads JSON artifacts only.",
            "No real AEX render or OFX render is performed.",
            "No AEX, DLL, AE, OFX runtime, project write, or binary-payload operation is performed.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build no-load render validation contract")
    parser.add_argument("--image-validation", required=True, help="Image validation JSON under target/image-fixture-validation")
    parser.add_argument("--image-smoke", required=True, help="Image input smoke JSON under target/image-input-smoke")
    parser.add_argument("--load-gate", required=True, help="Load gate JSON under target/load-gate")
    parser.add_argument("--ofx-route-contract", required=True, help="OFX route contract JSON under target/ofx-route-contract")
    parser.add_argument("--out", required=True, help="Create-new contract JSON under target/render-validation-contract")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    image_validation, image_validation_path = load_image_validation(Path(args.image_validation))
    image_smoke, image_smoke_path = load_image_smoke(Path(args.image_smoke))
    load_gate, load_gate_path = load_load_gate(Path(args.load_gate))
    ofx_route_contract, ofx_route_contract_path = load_ofx_route_contract(Path(args.ofx_route_contract))
    report = build_render_validation_contract(
        image_validation=image_validation,
        image_validation_path=image_validation_path,
        image_smoke=image_smoke,
        image_smoke_path=image_smoke_path,
        load_gate=load_gate,
        load_gate_path=load_gate_path,
        ofx_route_contract=ofx_route_contract,
        ofx_route_contract_path=ofx_route_contract_path,
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

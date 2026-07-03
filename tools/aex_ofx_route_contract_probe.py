#!/usr/bin/env python3
"""Build a no-load OFX route contract packet from local JSON evidence.

The contract reads OFX facade, OFX no-op suite, image validation, load gate,
and dependency review JSON only. It never opens AEX files, loads DLLs, invokes
AE/OFX, builds OFX binaries, describes effects, renders, or routes pixels.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
OFX_FACADE_ROOT = TARGET_ROOT / "ofx-facade"
OFX_SUITE_ROOT = TARGET_ROOT / "ofx-suite-selftest"
IMAGE_VALIDATION_ROOT = TARGET_ROOT / "image-fixture-validation"
LOAD_GATE_ROOT = TARGET_ROOT / "load-gate"
DEPENDENCY_REVIEW_ROOT = TARGET_ROOT / "dependency-review"
OFX_ROUTE_CONTRACT_ROOT = TARGET_ROOT / "ofx-route-contract"

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

DEPENDENCY_RECOMMENDATIONS_KEEP_CLOSED = {
    "do_not_open_native_load_gate",
    "hold_native_load_until_dependency_review_complete",
    "manual_loader_design_review_only_no_auto_approval",
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
        raise ValueError("OFX route contract report must have .json extension")
    OFX_ROUTE_CONTRACT_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, OFX_ROUTE_CONTRACT_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(OFX_ROUTE_CONTRACT_ROOT.resolve(strict=True)):
        raise ValueError(f"OFX route contract parent must stay under {OFX_ROUTE_CONTRACT_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_json_source(path: Path, root: Path, label: str) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, root, label)
    return read_json_object(resolved), resolved


def load_facade(path: Path) -> tuple[dict[str, Any], Path]:
    return load_json_source(path, OFX_FACADE_ROOT, "OFX facade packet")


def load_ofx_suite(path: Path) -> tuple[dict[str, Any], Path]:
    return load_json_source(path, OFX_SUITE_ROOT, "OFX suite selftest")


def load_image_validation(path: Path) -> tuple[dict[str, Any], Path]:
    return load_json_source(path, IMAGE_VALIDATION_ROOT, "image fixture validation")


def load_load_gate(path: Path) -> tuple[dict[str, Any], Path]:
    return load_json_source(path, LOAD_GATE_ROOT, "load gate report")


def load_dependency_review(path: Path) -> tuple[dict[str, Any], Path]:
    return load_json_source(path, DEPENDENCY_REVIEW_ROOT, "dependency review packet")


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def require_publication_local_only(payload: dict[str, Any], label: str) -> list[str]:
    if payload.get("publication_status") != "local-only":
        return [f"{label} publication_status must be local-only"]
    return []


def validate_facade(facade: dict[str, Any]) -> list[str]:
    errors = require_publication_local_only(facade, "OFX facade")
    if facade.get("packet_kind") != "aex_ofx_facade_deferred_packet":
        errors.append("OFX facade packet_kind must be aex_ofx_facade_deferred_packet")
    if facade.get("facade_state") != "deferred_loader_not_ready":
        errors.append("OFX facade facade_state must be deferred_loader_not_ready")
    if facade.get("ofx_route_action") != "no_op":
        errors.append("OFX facade ofx_route_action must be no_op")
    if not isinstance(facade.get("mapping_plan"), dict):
        errors.append("OFX facade mapping_plan must be an object")
    blocked_actions = facade.get("blocked_actions")
    if not isinstance(blocked_actions, list):
        errors.append("OFX facade blocked_actions must be a list")
    else:
        for action in ("load_aex_dll", "call_EffectMain", "ofx_describe_from_aex", "ofx_render_with_aex", "route_through_ofx"):
            if action not in blocked_actions:
                errors.append(f"OFX facade must block {action}")
    errors.extend(safety_errors(facade, "OFX facade"))
    return errors


def validate_ofx_suite(suite: dict[str, Any]) -> list[str]:
    errors = require_publication_local_only(suite, "OFX suite")
    if suite.get("report_kind") != "aex_ofx_suite_noop_selftest":
        errors.append("OFX suite report_kind must be aex_ofx_suite_noop_selftest")
    if suite.get("ofx_suite_selftest_state") != "ofx_suite_noop_identity_passed_route_closed":
        errors.append("OFX suite state must be ofx_suite_noop_identity_passed_route_closed")
    fixture_results = suite.get("fixture_results")
    fixture_count = suite.get("fixture_count")
    if not isinstance(fixture_results, list) or not fixture_results:
        errors.append("OFX suite fixture_results must be a non-empty list")
    if not isinstance(fixture_count, int) or fixture_count < 1:
        errors.append("OFX suite fixture_count must be a positive integer")
    elif isinstance(fixture_results, list) and len(fixture_results) != fixture_count:
        errors.append("OFX suite fixture_count must match fixture_results length")
    if isinstance(fixture_results, list):
        for index, result in enumerate(fixture_results):
            if not isinstance(result, dict):
                errors.append(f"OFX suite fixture result {index} must be an object")
                continue
            if result.get("mock_state") != "mock_identity_completed_route_closed":
                errors.append(f"OFX suite fixture result {index} mock_state must be mock_identity_completed_route_closed")
            identity = result.get("identity_check")
            if not isinstance(identity, dict):
                errors.append(f"OFX suite fixture result {index} identity_check must be an object")
            else:
                if identity.get("pixel_match") is not True:
                    errors.append(f"OFX suite fixture result {index} pixel_match must be true")
                if identity.get("dimension_match") is not True:
                    errors.append(f"OFX suite fixture result {index} dimension_match must be true")
    errors.extend(safety_errors(suite, "OFX suite"))
    return errors


def validate_image_validation(validation: dict[str, Any]) -> list[str]:
    errors = require_publication_local_only(validation, "image validation")
    if validation.get("report_kind") != "aex_image_fixture_validation":
        errors.append("image validation report_kind must be aex_image_fixture_validation")
    if validation.get("validation_state") != "image_fixture_validation_passed_no_load":
        errors.append("image validation validation_state must be image_fixture_validation_passed_no_load")
    if validation.get("validation_passed") is not True:
        errors.append("image validation validation_passed must be true")
    summary = validation.get("summary")
    if not isinstance(summary, dict):
        errors.append("image validation summary must be an object")
    else:
        if not isinstance(summary.get("fixture_count"), int) or summary.get("fixture_count") < 1:
            errors.append("image validation summary fixture_count must be positive")
        if summary.get("failed_count") not in (0, None):
            errors.append("image validation summary failed_count must be 0")
    fixture_results = validation.get("fixture_results")
    if not isinstance(fixture_results, list) or not fixture_results:
        errors.append("image validation fixture_results must be a non-empty list")
    errors.extend(safety_errors(validation, "image validation"))
    return errors


def validate_load_gate(gate: dict[str, Any]) -> list[str]:
    errors = require_publication_local_only(gate, "load gate")
    if gate.get("report_kind") != "aex_load_gate_check":
        errors.append("load gate report_kind must be aex_load_gate_check")
    if gate.get("gate_state") not in LOAD_GATE_CLOSED_STATES:
        errors.append("load gate gate_state must remain closed")
    gates = gate.get("gates")
    if not isinstance(gates, list) or not gates:
        errors.append("load gate gates must be a non-empty list")
    blocked_actions = gate.get("blocked_actions")
    if not isinstance(blocked_actions, list):
        errors.append("load gate blocked_actions must be a list")
    else:
        for action in ("load_aex_dll", "call_EffectMain", "start_after_effects", "render_with_aex", "route_through_ofx"):
            if action not in blocked_actions:
                errors.append(f"load gate must block {action}")
    errors.extend(safety_errors(gate, "load gate"))
    return errors


def validate_dependency_review(review: dict[str, Any]) -> list[str]:
    errors = require_publication_local_only(review, "dependency review")
    if review.get("packet_kind") != "aex_dependency_review_packet":
        errors.append("dependency review packet_kind must be aex_dependency_review_packet")
    if review.get("review_state") != "dependency_review_pending_native_load_blocked":
        errors.append("dependency review state must block native load in this route contract")
    if review.get("native_load_recommendation") not in DEPENDENCY_RECOMMENDATIONS_KEEP_CLOSED:
        errors.append("dependency review native_load_recommendation must keep native load closed")
    review_items = review.get("review_items")
    if not isinstance(review_items, list) or not review_items:
        errors.append("dependency review review_items must be a non-empty list")
    summary = review.get("summary")
    if not isinstance(summary, dict):
        errors.append("dependency review summary must be an object")
    elif int(summary.get("native_load_blocker_count") or 0) < 1:
        errors.append("dependency review summary must include at least one native load blocker")
    errors.extend(safety_errors(review, "dependency review"))
    return errors


def source_errors(
    *,
    facade: dict[str, Any],
    ofx_suite: dict[str, Any],
    image_validation: dict[str, Any],
    load_gate: dict[str, Any],
    dependency_review: dict[str, Any],
) -> list[str]:
    errors: list[str] = []
    errors.extend(validate_facade(facade))
    errors.extend(validate_ofx_suite(ofx_suite))
    errors.extend(validate_image_validation(image_validation))
    errors.extend(validate_load_gate(load_gate))
    errors.extend(validate_dependency_review(dependency_review))
    suite_count = ofx_suite.get("fixture_count")
    validation_count = image_validation.get("summary", {}).get("fixture_count") if isinstance(image_validation.get("summary"), dict) else None
    if isinstance(suite_count, int) and isinstance(validation_count, int) and suite_count != validation_count:
        errors.append("OFX suite fixture_count must match image validation fixture_count")
    if ofx_suite.get("source_facade_state") not in (None, facade.get("facade_state")):
        errors.append("OFX suite source_facade_state must match OFX facade facade_state")
    if load_gate.get("dependency_native_load_recommendation") not in (None, dependency_review.get("native_load_recommendation")):
        errors.append("load gate dependency recommendation must match dependency review")
    return errors


def image_contract(ofx_suite: dict[str, Any], image_validation: dict[str, Any]) -> dict[str, Any]:
    summary = image_validation.get("summary") if isinstance(image_validation.get("summary"), dict) else {}
    return {
        "state": "validated_noop_identity_inputs",
        "ofx_suite_fixture_count": ofx_suite.get("fixture_count"),
        "validation_fixture_count": summary.get("fixture_count"),
        "validation_passed_count": summary.get("passed_count"),
        "total_pixel_bytes": summary.get("total_pixel_bytes"),
        "pattern_counts": summary.get("pattern_counts", {}),
        "claim": "Generated image fixtures and no-op identity flow are validated; no AEX/OFX render equivalence is claimed.",
    }


def build_contract_report(
    *,
    facade: dict[str, Any],
    facade_path: Path,
    ofx_suite: dict[str, Any],
    ofx_suite_path: Path,
    image_validation: dict[str, Any],
    image_validation_path: Path,
    load_gate: dict[str, Any],
    load_gate_path: Path,
    dependency_review: dict[str, Any],
    dependency_review_path: Path,
) -> dict[str, Any]:
    errors = source_errors(
        facade=facade,
        ofx_suite=ofx_suite,
        image_validation=image_validation,
        load_gate=load_gate,
        dependency_review=dependency_review,
    )
    if errors:
        raise ValueError("; ".join(errors))

    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_ofx_route_contract_probe",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_ofx_facade": str(facade_path),
        "source_ofx_suite_selftest": str(ofx_suite_path),
        "source_image_fixture_validation": str(image_validation_path),
        "source_load_gate": str(load_gate_path),
        "source_dependency_review": str(dependency_review_path),
        "source_states": {
            "facade_state": facade.get("facade_state"),
            "ofx_route_action": facade.get("ofx_route_action"),
            "ofx_suite_selftest_state": ofx_suite.get("ofx_suite_selftest_state"),
            "image_validation_state": image_validation.get("validation_state"),
            "load_gate_state": load_gate.get("gate_state"),
            "dependency_review_state": dependency_review.get("review_state"),
            "dependency_native_load_recommendation": dependency_review.get("native_load_recommendation"),
        },
        "contract_state": "ofx_route_contract_ready_route_closed",
        "real_route_open": False,
        "mock_route_ready": True,
        "route_contract": {
            "real_route_open": False,
            "mock_route_ready": True,
            "ofx_runtime_invoked": False,
            "aex_runtime_invoked": False,
            "ofx_facade_state": facade.get("facade_state"),
            "ofx_noop_suite_state": ofx_suite.get("ofx_suite_selftest_state"),
            "allowed_route": "no_op_identity_only",
            "forbidden_route": "real_aex_backed_ofx_describe_or_render",
        },
        "describe_contract": {
            "state": "blocked_pending_native_loader_and_schema",
            "required_evidence": [
                "explicit user approval artifact",
                "native loader gate opened by reviewed evidence",
                "PiPL/resource parameter schema extraction review",
                "AEX-derived metadata redaction policy",
                "OFX host describe harness",
            ],
            "current_evidence": {
                "facade_state": facade.get("facade_state"),
                "mapping_plan_state": facade.get("mapping_plan", {}).get("state") if isinstance(facade.get("mapping_plan"), dict) else None,
            },
        },
        "render_contract": {
            "state": "blocked_pending_render_harness",
            "required_evidence": [
                "approved fixture candidate",
                "sandboxed native loader worker",
                "timeout and crash containment",
                "image output validator",
                "OFX host render harness",
            ],
            "current_evidence": {
                "load_gate_state": load_gate.get("gate_state"),
                "image_contract_state": "validated_noop_identity_inputs",
            },
        },
        "image_contract": image_contract(ofx_suite, image_validation),
        "blockers": [
            "load_gate_closed",
            "dependency_review_blocks_native_load",
            "fixture_not_approved",
            "no_real_native_loader",
            "no_ofx_host_runtime",
            "no_aex_parameter_schema_mapping",
            "publication_not_ready",
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
        "blocked_actions": [
            "accept_aex_path",
            "load_aex_dll",
            "call_EffectMain",
            "dispatch_PF_Cmd",
            "start_after_effects",
            "build_ofx_binary",
            "ofx_describe_from_aex",
            "ofx_render_with_aex",
            "render_with_aex",
            "route_through_ofx",
        ],
        "next_required_actions": [
            "Keep this route contract in the artifact index and readiness matrix.",
            "Add an OFX host harness only as a no-op test until native loader evidence changes.",
            "Map PiPL/resource metadata to a redacted parameter schema before any describe claim.",
            "Require explicit user approval before any AEX file is opened or loaded.",
        ],
        "notes": [
            "This contract reads local JSON evidence only.",
            "The route is ready only as a closed, no-op identity contract.",
            "No AEX, AE, DLL, OFX runtime, describe, render, project write, or binary-payload action is performed.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build a no-load OFX route contract packet")
    parser.add_argument("--ofx-facade", required=True, help="Deferred OFX facade JSON under target/ofx-facade")
    parser.add_argument("--ofx-suite-selftest", required=True, help="OFX suite selftest JSON under target/ofx-suite-selftest")
    parser.add_argument("--image-validation", required=True, help="Image validation JSON under target/image-fixture-validation")
    parser.add_argument("--load-gate", required=True, help="Load gate JSON under target/load-gate")
    parser.add_argument("--dependency-review", required=True, help="Dependency review JSON under target/dependency-review")
    parser.add_argument("--out", required=True, help="Create-new route contract JSON under target/ofx-route-contract")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    facade, facade_path = load_facade(Path(args.ofx_facade))
    ofx_suite, ofx_suite_path = load_ofx_suite(Path(args.ofx_suite_selftest))
    image_validation, image_validation_path = load_image_validation(Path(args.image_validation))
    load_gate, load_gate_path = load_load_gate(Path(args.load_gate))
    dependency_review, dependency_review_path = load_dependency_review(Path(args.dependency_review))
    report = build_contract_report(
        facade=facade,
        facade_path=facade_path,
        ofx_suite=ofx_suite,
        ofx_suite_path=ofx_suite_path,
        image_validation=image_validation,
        image_validation_path=image_validation_path,
        load_gate=load_gate,
        load_gate_path=load_gate_path,
        dependency_review=dependency_review,
        dependency_review_path=dependency_review_path,
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

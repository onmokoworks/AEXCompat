#!/usr/bin/env python3
"""Run a single-image no-load smoke test through worker and OFX no-op paths.

This mini tool reads a generated PPM fixture, a closed OFX route contract, and
a deferred OFX facade packet only. It never opens AEX files, loads DLLs, starts
After Effects, invokes an OFX runtime, describes effects, renders, or routes
pixels through AEX/OFX.
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

import aex_ofx_noop_mock
import aex_worker_selftest
import ppm_fixture_tool

TARGET_ROOT = LAB_ROOT / "target"
PPM_FIXTURE_ROOT = TARGET_ROOT / "ppm-fixtures"
WORKER_SELFTEST_ROOT = TARGET_ROOT / "worker-selftest"
OFX_NOOP_ROOT = TARGET_ROOT / "ofx-noop-mock"
OFX_FACADE_ROOT = TARGET_ROOT / "ofx-facade"
OFX_ROUTE_CONTRACT_ROOT = TARGET_ROOT / "ofx-route-contract"
IMAGE_INPUT_SMOKE_ROOT = TARGET_ROOT / "image-input-smoke"

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


def validate_input_ppm(path: Path) -> Path:
    if path.suffix.lower() != ".ppm":
        raise ValueError("input PPM must have .ppm extension")
    PPM_FIXTURE_ROOT.mkdir(parents=True, exist_ok=True)
    return resolve_under_root(path, PPM_FIXTURE_ROOT, must_exist=True)


def validate_worker_path(path: Path) -> Path:
    if path.suffix.lower() != ".py":
        raise ValueError("worker path must have .py extension")
    return resolve_under_root(path, TOOLS_ROOT, must_exist=True)


def validate_worker_output_ppm(path: Path) -> Path:
    if path.suffix.lower() != ".ppm":
        raise ValueError("worker output PPM must have .ppm extension")
    WORKER_SELFTEST_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, WORKER_SELFTEST_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(WORKER_SELFTEST_ROOT.resolve(strict=True)):
        raise ValueError(f"worker output parent must stay under {WORKER_SELFTEST_ROOT}")
    return resolved


def validate_ofx_output_ppm(path: Path) -> Path:
    if path.suffix.lower() != ".ppm":
        raise ValueError("OFX output PPM must have .ppm extension")
    OFX_NOOP_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, OFX_NOOP_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(OFX_NOOP_ROOT.resolve(strict=True)):
        raise ValueError(f"OFX output parent must stay under {OFX_NOOP_ROOT}")
    return resolved


def validate_output_json(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("image input smoke report must have .json extension")
    IMAGE_INPUT_SMOKE_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, IMAGE_INPUT_SMOKE_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(IMAGE_INPUT_SMOKE_ROOT.resolve(strict=True)):
        raise ValueError(f"image input smoke parent must stay under {IMAGE_INPUT_SMOKE_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_route_contract(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, OFX_ROUTE_CONTRACT_ROOT, "OFX route contract")
    return read_json_object(resolved), resolved


def load_ofx_facade(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, OFX_FACADE_ROOT, "OFX facade packet")
    return read_json_object(resolved), resolved


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def validate_route_contract(contract: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if contract.get("publication_status") != "local-only":
        errors.append("route contract publication_status must be local-only")
    if contract.get("report_kind") != "aex_ofx_route_contract_probe":
        errors.append("route contract report_kind must be aex_ofx_route_contract_probe")
    if contract.get("contract_state") != "ofx_route_contract_ready_route_closed":
        errors.append("route contract contract_state must be ofx_route_contract_ready_route_closed")
    if contract.get("real_route_open") is not False:
        errors.append("route contract real_route_open must be false")
    if contract.get("mock_route_ready") is not True:
        errors.append("route contract mock_route_ready must be true")
    route = contract.get("route_contract")
    if not isinstance(route, dict):
        errors.append("route contract route_contract must be an object")
    else:
        if route.get("real_route_open") is not False:
            errors.append("route_contract.real_route_open must be false")
        if route.get("mock_route_ready") is not True:
            errors.append("route_contract.mock_route_ready must be true")
        if route.get("ofx_runtime_invoked") is not False:
            errors.append("route_contract.ofx_runtime_invoked must be false")
        if route.get("aex_runtime_invoked") is not False:
            errors.append("route_contract.aex_runtime_invoked must be false")
        if route.get("allowed_route") != "no_op_identity_only":
            errors.append("route_contract.allowed_route must be no_op_identity_only")
    blockers = contract.get("blockers")
    if not isinstance(blockers, list) or not blockers:
        errors.append("route contract blockers must be a non-empty list")
    errors.extend(safety_errors(contract, "route contract"))
    return errors


def validate_ofx_facade(facade: dict[str, Any]) -> list[str]:
    errors = aex_ofx_noop_mock.validate_packet(facade)
    if facade.get("facade_state") != "deferred_loader_not_ready":
        errors.append("OFX facade facade_state must be deferred_loader_not_ready")
    return errors


def safe_label(value: Any) -> str:
    text = str(value)
    return "".join(ch if ch.isalnum() or ch in ("-", "_") else "_" for ch in text)[:80] or "image"


def output_paths(output_prefix: str) -> tuple[Path, Path]:
    prefix = safe_label(output_prefix)
    worker_ppm = validate_worker_output_ppm(WORKER_SELFTEST_ROOT / f"{prefix}-worker-identity.ppm")
    ofx_ppm = validate_ofx_output_ppm(OFX_NOOP_ROOT / f"{prefix}-ofx-identity.ppm")
    return worker_ppm, ofx_ppm


def run_worker_identity(*, worker_path: Path, input_ppm: Path, output_ppm: Path) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    resolved_worker = validate_worker_path(worker_path)
    resolved_input = validate_input_ppm(input_ppm)
    resolved_output = validate_worker_output_ppm(output_ppm)
    steps: list[dict[str, Any]] = []
    with aex_worker_selftest.WorkerClient(resolved_worker) as client:
        hello = client.send({"id": "hello", "type": "hello"})
        aex_worker_selftest.ensure_safety_state(hello)
        if hello.get("type") != "hello_ack":
            raise AssertionError(f"unexpected hello response: {hello}")
        steps.append({"step": "hello", "response_type": hello.get("type")})

        environment = client.send({"id": "env", "type": "inspect_environment"})
        aex_worker_selftest.ensure_safety_state(environment)
        if environment.get("native_load_enabled") is not False:
            raise AssertionError("worker reported native_load_enabled != false")
        steps.append(
            {
                "step": "inspect_environment",
                "response_type": environment.get("type"),
                "process_bitness": environment.get("process_bitness"),
                "native_load_enabled": environment.get("native_load_enabled"),
            }
        )

        inspect = client.send({"id": "inspect", "type": "inspect_ppm", "input": str(resolved_input)})
        aex_worker_selftest.ensure_safety_state(inspect)
        if inspect.get("type") != "ppm_summary":
            raise AssertionError(f"unexpected inspect response: {inspect}")
        steps.append(
            {
                "step": "inspect_ppm",
                "response_type": inspect.get("type"),
                "width": inspect.get("width"),
                "height": inspect.get("height"),
                "bytes": inspect.get("bytes"),
            }
        )

        transform = client.send(
            {
                "id": "identity",
                "type": "transform_ppm_identity",
                "input": str(resolved_input),
                "out": str(resolved_output),
            }
        )
        aex_worker_selftest.ensure_safety_state(transform)
        if transform.get("type") != "created_output":
            raise AssertionError(f"unexpected transform response: {transform}")
        identity_check = aex_worker_selftest.compare_ppm_identity(resolved_input, resolved_output)
        steps.append(
            {
                "step": "transform_ppm_identity",
                "response_type": transform.get("type"),
                "output": str(resolved_output),
                "identity_check": identity_check,
            }
        )

        blocked = client.send({"id": "blocked", "type": "load_aex", "path": "not-used.aex"})
        aex_worker_selftest.ensure_safety_state(blocked)
        if blocked.get("type") != "error" or blocked.get("code") != "blocked_action":
            raise AssertionError(f"blocked action was not rejected: {blocked}")
        steps.append({"step": "blocked_load_aex", "response_type": blocked.get("type"), "code": blocked.get("code")})

        quit_response = client.send({"id": "quit", "type": "quit"})
        aex_worker_selftest.ensure_safety_state(quit_response)
        if quit_response.get("type") != "quit_ack":
            raise AssertionError(f"unexpected quit response: {quit_response}")
        return_code, stderr = client.wait()
        if return_code != 0:
            raise AssertionError(f"worker exited with {return_code}: {stderr}")
        steps.append({"step": "quit", "response_type": quit_response.get("type"), "return_code": return_code})
    return identity_check, steps


def build_smoke_report(
    *,
    input_ppm: Path,
    route_contract: dict[str, Any],
    route_contract_path: Path,
    ofx_facade: dict[str, Any],
    ofx_facade_path: Path,
    worker_path: Path,
    output_prefix: str,
) -> dict[str, Any]:
    contract_errors = validate_route_contract(route_contract)
    facade_errors = validate_ofx_facade(ofx_facade)
    if contract_errors or facade_errors:
        raise ValueError("; ".join(contract_errors + facade_errors))

    resolved_input = validate_input_ppm(input_ppm)
    worker_output_ppm, ofx_output_ppm = output_paths(output_prefix)
    worker_identity_check, worker_steps = run_worker_identity(
        worker_path=worker_path,
        input_ppm=resolved_input,
        output_ppm=worker_output_ppm,
    )
    ofx_mock_report = aex_ofx_noop_mock.build_mock_report(
        packet=ofx_facade,
        packet_path=ofx_facade_path,
        input_ppm=resolved_input,
        output_ppm=ofx_output_ppm,
    )
    if ofx_mock_report.get("mock_state") != "mock_identity_completed_route_closed":
        raise AssertionError(f"OFX no-op mock did not complete identity: {ofx_mock_report}")
    ofx_identity_check = ofx_mock_report.get("identity_check")
    if not isinstance(ofx_identity_check, dict):
        raise AssertionError("OFX no-op mock identity_check must be present")
    if ofx_identity_check.get("pixel_match") is not True or ofx_identity_check.get("dimension_match") is not True:
        raise AssertionError("OFX no-op mock identity did not match")

    image = ppm_fixture_tool.read_ppm(resolved_input)
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_image_input_smoke_tool",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_input_ppm": str(resolved_input),
        "source_ofx_route_contract": str(route_contract_path),
        "source_ofx_facade": str(ofx_facade_path),
        "worker": str(validate_worker_path(worker_path)),
        "smoke_state": "image_input_smoke_passed_route_closed",
        "input_summary": {
            "width": image.width,
            "height": image.height,
            "pixel_bytes": len(image.pixels),
        },
        "route_contract_summary": {
            "contract_state": route_contract.get("contract_state"),
            "real_route_open": route_contract.get("real_route_open"),
            "mock_route_ready": route_contract.get("mock_route_ready"),
            "allowed_route": route_contract.get("route_contract", {}).get("allowed_route")
            if isinstance(route_contract.get("route_contract"), dict)
            else None,
            "blockers": route_contract.get("blockers", []),
        },
        "worker_output_ppm": str(worker_output_ppm),
        "worker_identity_check": worker_identity_check,
        "worker_identity_passed": True,
        "worker_steps": worker_steps,
        "ofx_output_ppm": str(ofx_output_ppm),
        "ofx_identity_check": ofx_identity_check,
        "ofx_identity_passed": True,
        "ofx_mock_state": ofx_mock_report.get("mock_state"),
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
        "no_load_worker_invoked": True,
        "ofx_runtime_invoked": False,
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
        "notes": [
            "This is a single-image no-load smoke test for the compatibility harness.",
            "The no-load worker is invoked only for PPM inspect/identity and blocked load_aex proof.",
            "The OFX path is the no-op mock identity path guarded by a closed route contract.",
            "No AEX, AE, DLL, OFX runtime, describe, render, project write, or binary payload action is performed.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_json(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run single-image no-load compatibility smoke test")
    parser.add_argument("--input-ppm", required=True, help="Input PPM under target/ppm-fixtures")
    parser.add_argument("--route-contract", required=True, help="OFX route contract JSON under target/ofx-route-contract")
    parser.add_argument("--ofx-facade", required=True, help="Deferred OFX facade JSON under target/ofx-facade")
    parser.add_argument("--worker", default=str(TOOLS_ROOT / "aex_no_load_worker.py"))
    parser.add_argument("--output-prefix", required=True, help="Prefix for create-new worker/OFX PPM outputs")
    parser.add_argument("--out", required=True, help="Create-new smoke report JSON under target/image-input-smoke")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    route_contract, route_contract_path = load_route_contract(Path(args.route_contract))
    ofx_facade, ofx_facade_path = load_ofx_facade(Path(args.ofx_facade))
    report = build_smoke_report(
        input_ppm=Path(args.input_ppm),
        route_contract=route_contract,
        route_contract_path=route_contract_path,
        ofx_facade=ofx_facade,
        ofx_facade_path=ofx_facade_path,
        worker_path=Path(args.worker),
        output_prefix=args.output_prefix,
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

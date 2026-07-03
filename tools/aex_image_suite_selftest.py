#!/usr/bin/env python3
"""Run a no-load worker selftest over every PPM in an image fixture suite.

This tool reads image-suite JSON and generated PPM fixtures only. It does not
open AEX files, load libraries, invoke AE, render, or route OFX.
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

import aex_worker_selftest
import ppm_fixture_tool

IMAGE_SUITE_ROOT = LAB_ROOT / "target" / "image-fixture-suite"
IMAGE_SUITE_SELFTEST_ROOT = LAB_ROOT / "target" / "image-suite-selftest"
PPM_FIXTURE_ROOT = LAB_ROOT / "target" / "ppm-fixtures"
WORKER_SELFTEST_ROOT = LAB_ROOT / "target" / "worker-selftest"

SAFETY_FLAGS = (
    "native_load_enabled",
    "native_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
    "aex_file_opened",
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


def validate_suite_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("image fixture suite must have .json extension")
    return resolve_under_root(path, IMAGE_SUITE_ROOT, must_exist=True)


def validate_output_report(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("image suite selftest report must have .json extension")
    IMAGE_SUITE_SELFTEST_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, IMAGE_SUITE_SELFTEST_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(IMAGE_SUITE_SELFTEST_ROOT.resolve(strict=True)):
        raise ValueError(f"image suite selftest parent must stay under {IMAGE_SUITE_SELFTEST_ROOT}")
    return resolved


def validate_input_ppm(path: Path) -> Path:
    if path.suffix.lower() != ".ppm":
        raise ValueError("fixture PPM must have .ppm extension")
    PPM_FIXTURE_ROOT.mkdir(parents=True, exist_ok=True)
    return resolve_under_root(path, PPM_FIXTURE_ROOT, must_exist=True)


def validate_output_ppm(path: Path) -> Path:
    if path.suffix.lower() != ".ppm":
        raise ValueError("output PPM must have .ppm extension")
    WORKER_SELFTEST_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, WORKER_SELFTEST_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    return resolved


def validate_worker_path(path: Path) -> Path:
    if path.suffix.lower() != ".py":
        raise ValueError("worker path must have .py extension")
    return resolve_under_root(path, TOOLS_ROOT, must_exist=True)


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_image_suite(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_suite_path(path)
    return read_json_object(resolved), resolved


def validate_suite(suite: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if suite.get("report_kind") != "aex_image_fixture_suite":
        errors.append("source report_kind must be aex_image_fixture_suite")
    if suite.get("suite_state") != "image_fixture_suite_ready":
        errors.append("source suite_state must be image_fixture_suite_ready")
    if suite.get("publication_status") != "local-only":
        errors.append("source publication_status must be local-only")
    fixtures = suite.get("fixtures")
    if not isinstance(fixtures, list) or not fixtures:
        errors.append("source fixtures must be a non-empty list")
    for flag in SAFETY_FLAGS:
        if suite.get(flag) is not False:
            errors.append(f"source {flag} must be false")
    if isinstance(fixtures, list):
        for index, fixture in enumerate(fixtures):
            if not isinstance(fixture, dict):
                errors.append(f"fixture {index} must be an object")
                continue
            if not isinstance(fixture.get("ppm_path"), str):
                errors.append(f"fixture {index} ppm_path must be a string")
    return errors


def safe_label(value: Any) -> str:
    text = str(value)
    return "".join(ch if ch.isalnum() or ch in ("-", "_") else "_" for ch in text)[:80] or "case"


def output_ppm_path(output_prefix: str, fixture: dict[str, Any], index: int) -> Path:
    case_id = safe_label(fixture.get("case_id", f"case_{index}"))
    prefix = safe_label(output_prefix)
    return validate_output_ppm(WORKER_SELFTEST_ROOT / f"{prefix}-{index:02d}-{case_id}-identity.ppm")


def compare_identity(input_path: Path, output_path: Path) -> dict[str, Any]:
    input_image = ppm_fixture_tool.read_ppm(input_path)
    output_image = ppm_fixture_tool.read_ppm(output_path)
    pixel_match = input_image.pixels == output_image.pixels
    dimension_match = input_image.width == output_image.width and input_image.height == output_image.height
    if not pixel_match or not dimension_match:
        raise AssertionError("suite identity output did not match input pixels/dimensions")
    return {
        "width": input_image.width,
        "height": input_image.height,
        "bytes": len(input_image.pixels),
        "pixel_match": pixel_match,
        "dimension_match": dimension_match,
    }


def run_suite_selftest(
    *,
    suite: dict[str, Any],
    suite_path: Path,
    worker_path: Path,
    output_prefix: str,
) -> dict[str, Any]:
    errors = validate_suite(suite)
    if errors:
        raise ValueError("; ".join(errors))
    resolved_worker = validate_worker_path(worker_path)

    fixture_results: list[dict[str, Any]] = []
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

        for index, fixture in enumerate(suite.get("fixtures", [])):
            input_path = validate_input_ppm(Path(str(fixture["ppm_path"])))
            output_path = output_ppm_path(output_prefix, fixture, index)
            inspect = client.send(
                {
                    "id": f"inspect-{index}",
                    "type": "inspect_ppm",
                    "input": str(input_path),
                }
            )
            aex_worker_selftest.ensure_safety_state(inspect)
            if inspect.get("type") != "ppm_summary":
                raise AssertionError(f"unexpected inspect response: {inspect}")

            transform = client.send(
                {
                    "id": f"identity-{index}",
                    "type": "transform_ppm_identity",
                    "input": str(input_path),
                    "out": str(output_path),
                }
            )
            aex_worker_selftest.ensure_safety_state(transform)
            if transform.get("type") != "created_output":
                raise AssertionError(f"unexpected transform response: {transform}")
            identity_check = compare_identity(input_path, output_path)
            fixture_results.append(
                {
                    "case_id": fixture.get("case_id"),
                    "pattern": fixture.get("pattern"),
                    "input_ppm": str(input_path),
                    "output_ppm": str(output_path),
                    "inspect": {
                        "width": inspect.get("width"),
                        "height": inspect.get("height"),
                        "bytes": inspect.get("bytes"),
                    },
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

    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_image_suite_worker_selftest",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_image_fixture_suite": str(suite_path),
        "source_suite_state": suite.get("suite_state"),
        "target_candidate": suite.get("target_candidate"),
        "worker": str(resolved_worker),
        "suite_selftest_state": "image_suite_worker_selftest_passed",
        "fixture_count": len(fixture_results),
        "fixture_results": fixture_results,
        "steps": steps,
        "native_load_enabled": False,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "notes": [
            "Suite selftest drives the no-load worker across all image fixtures.",
            "Every fixture is inspected and identity-transformed with pixel/dimension comparison.",
            "The blocked load_aex message is verified to fail closed.",
            "No AEX file is opened, copied, hashed, loaded, or executed.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_report(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run no-load worker selftest over an image fixture suite")
    parser.add_argument("--image-suite", required=True, help="Image fixture suite JSON under target/image-fixture-suite")
    parser.add_argument("--worker", default=str(TOOLS_ROOT / "aex_no_load_worker.py"))
    parser.add_argument("--output-prefix", required=True, help="Prefix for create-new worker output PPMs")
    parser.add_argument("--out", required=True, help="Create-new selftest JSON under target/image-suite-selftest")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    suite, suite_path = load_image_suite(Path(args.image_suite))
    report = run_suite_selftest(
        suite=suite,
        suite_path=suite_path,
        worker_path=Path(args.worker),
        output_prefix=args.output_prefix,
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Run the no-op OFX mock across every fixture in an image suite.

This is not an OFX runtime invocation. It reads a deferred OFX facade packet,
image-suite JSON, and generated PPM fixtures only. It never opens AEX files,
builds OFX binaries, invokes OFX callbacks, renders, or routes pixels through
AEX/OFX.
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

OFX_FACADE_ROOT = LAB_ROOT / "target" / "ofx-facade"
IMAGE_SUITE_ROOT = LAB_ROOT / "target" / "image-fixture-suite"
OFX_NOOP_ROOT = LAB_ROOT / "target" / "ofx-noop-mock"
OFX_SUITE_ROOT = LAB_ROOT / "target" / "ofx-suite-selftest"
PPM_FIXTURE_ROOT = LAB_ROOT / "target" / "ppm-fixtures"

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


def validate_packet_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("OFX facade packet must have .json extension")
    return resolve_under_root(path, OFX_FACADE_ROOT, must_exist=True)


def validate_suite_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("image fixture suite must have .json extension")
    return resolve_under_root(path, IMAGE_SUITE_ROOT, must_exist=True)


def validate_input_ppm(path: Path) -> Path:
    if path.suffix.lower() != ".ppm":
        raise ValueError("input PPM must have .ppm extension")
    PPM_FIXTURE_ROOT.mkdir(parents=True, exist_ok=True)
    return resolve_under_root(path, PPM_FIXTURE_ROOT, must_exist=True)


def validate_output_ppm(path: Path) -> Path:
    if path.suffix.lower() != ".ppm":
        raise ValueError("output PPM must have .ppm extension")
    OFX_NOOP_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, OFX_NOOP_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(OFX_NOOP_ROOT.resolve(strict=True)):
        raise ValueError(f"output PPM parent must stay under {OFX_NOOP_ROOT}")
    return resolved


def validate_output_json(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("OFX suite selftest report must have .json extension")
    OFX_SUITE_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, OFX_SUITE_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(OFX_SUITE_ROOT.resolve(strict=True)):
        raise ValueError(f"OFX suite selftest parent must stay under {OFX_SUITE_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_packet(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_packet_path(path)
    return read_json_object(resolved), resolved


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
            errors.append(f"source suite {flag} must be false")
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
    prefix = safe_label(output_prefix)
    case_id = safe_label(fixture.get("case_id", f"case_{index}"))
    return validate_output_ppm(OFX_NOOP_ROOT / f"{prefix}-{index:02d}-{case_id}-identity.ppm")


def build_suite_report(
    *,
    packet: dict[str, Any],
    packet_path: Path,
    suite: dict[str, Any],
    suite_path: Path,
    output_prefix: str,
) -> dict[str, Any]:
    suite_errors = validate_suite(suite)
    packet_errors = aex_ofx_noop_mock.validate_packet(packet)
    if suite_errors or packet_errors:
        raise ValueError("; ".join(suite_errors + packet_errors))

    results: list[dict[str, Any]] = []
    for index, fixture in enumerate(suite.get("fixtures", [])):
        input_path = validate_input_ppm(Path(str(fixture["ppm_path"])))
        output_path = output_ppm_path(output_prefix, fixture, index)
        mock_report = aex_ofx_noop_mock.build_mock_report(
            packet=packet,
            packet_path=packet_path,
            input_ppm=input_path,
            output_ppm=output_path,
        )
        if mock_report.get("mock_state") != "mock_identity_completed_route_closed":
            raise AssertionError(f"OFX no-op mock did not complete identity: {mock_report}")
        identity_check = mock_report.get("identity_check")
        if not isinstance(identity_check, dict):
            raise AssertionError("identity_check must be present")
        if identity_check.get("pixel_match") is not True or identity_check.get("dimension_match") is not True:
            raise AssertionError("OFX no-op identity check did not match")
        results.append(
            {
                "case_id": fixture.get("case_id"),
                "pattern": fixture.get("pattern"),
                "input_ppm": str(input_path),
                "output_ppm": mock_report.get("output_ppm"),
                "identity_check": identity_check,
                "mock_state": mock_report.get("mock_state"),
            }
        )

    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_ofx_suite_noop_selftest",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_ofx_facade_packet": str(packet_path),
        "source_facade_state": packet.get("facade_state"),
        "source_image_fixture_suite": str(suite_path),
        "source_suite_state": suite.get("suite_state"),
        "target_candidate": suite.get("target_candidate"),
        "ofx_suite_selftest_state": "ofx_suite_noop_identity_passed_route_closed",
        "fixture_count": len(results),
        "fixture_results": results,
        "native_load_enabled": False,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "ofx_plugin_built": False,
        "ofx_describe_performed": False,
        "ofx_render_performed": False,
        "mock_identity_transform_performed": True,
        "blocked_actions": [
            "accept_aex_path",
            "load_aex_dll",
            "call_EffectMain",
            "build_ofx_binary",
            "ofx_describe_from_aex",
            "ofx_render_with_aex",
            "route_through_ofx",
        ],
        "notes": [
            "Suite selftest reuses the no-op OFX mock over every generated PPM fixture.",
            "This is not an OFX runtime invocation and makes no render compatibility claim.",
            "No AEX file is opened, copied, hashed, loaded, or executed.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_json(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run no-op OFX suite selftest with image fixtures")
    parser.add_argument("--ofx-packet", required=True, help="Deferred OFX packet under target/ofx-facade")
    parser.add_argument("--image-suite", required=True, help="Image fixture suite JSON under target/image-fixture-suite")
    parser.add_argument("--output-prefix", required=True, help="Prefix for create-new OFX no-op output PPMs")
    parser.add_argument("--out", required=True, help="Create-new selftest JSON under target/ofx-suite-selftest")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    packet, packet_path = load_packet(Path(args.ofx_packet))
    suite, suite_path = load_image_suite(Path(args.image_suite))
    report = build_suite_report(
        packet=packet,
        packet_path=packet_path,
        suite=suite,
        suite_path=suite_path,
        output_prefix=args.output_prefix,
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""No-op OFX host/mock selftest for AEX compatibility groundwork.

This tool reads a deferred OFX facade packet and a generated PPM fixture only.
It never opens AEX files, builds OFX binaries, invokes OFX runtime callbacks, or
routes pixels through AEX/OFX.
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

OFX_FACADE_ROOT = LAB_ROOT / "target" / "ofx-facade"
PPM_FIXTURE_ROOT = LAB_ROOT / "target" / "ppm-fixtures"
OFX_NOOP_ROOT = LAB_ROOT / "target" / "ofx-noop-mock"
MAX_PIXELS = 4096 * 4096

SAFETY_FLAGS = (
    "native_load_enabled",
    "native_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
    "aex_file_opened",
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
    try:
        resolved = absolute.resolve(strict=must_exist)
    except FileNotFoundError as exc:
        raise ValueError(f"path does not exist: {absolute}") from exc
    if not resolved.is_relative_to(resolved_root):
        raise ValueError(f"path must stay under {root}")
    return resolved


def validate_packet_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("OFX facade packet must have .json extension")
    return resolve_under_root(path, OFX_FACADE_ROOT, must_exist=True)


def validate_input_ppm(path: Path) -> Path:
    if path.suffix.lower() != ".ppm":
        raise ValueError("input PPM must have .ppm extension")
    PPM_FIXTURE_ROOT.mkdir(parents=True, exist_ok=True)
    return resolve_under_root(path, PPM_FIXTURE_ROOT, must_exist=True)


def validate_output_path(path: Path, suffix: str, label: str) -> Path:
    if path.suffix.lower() != suffix:
        raise ValueError(f"{label} must have {suffix} extension")
    OFX_NOOP_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, OFX_NOOP_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(OFX_NOOP_ROOT.resolve(strict=True)):
        raise ValueError(f"{label} parent must stay under {OFX_NOOP_ROOT}")
    return resolved


def validate_output_ppm(path: Path) -> Path:
    return validate_output_path(path, ".ppm", "output PPM")


def validate_output_json(path: Path) -> Path:
    return validate_output_path(path, ".json", "OFX no-op mock report")


def load_packet(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_packet_path(path)
    with resolved.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("OFX facade packet must be a JSON object")
    return payload, resolved


def validate_packet(packet: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if packet.get("packet_kind") != "aex_ofx_facade_deferred_packet":
        errors.append("packet_kind must be aex_ofx_facade_deferred_packet")
    if packet.get("publication_status") != "local-only":
        errors.append("publication_status must be local-only")
    if packet.get("ofx_route_action") != "no_op":
        errors.append("ofx_route_action must be no_op")
    for flag in SAFETY_FLAGS:
        if packet.get(flag) is not False:
            errors.append(f"OFX facade packet {flag} must be false")
    blocked = packet.get("blocked_actions", [])
    if not isinstance(blocked, list):
        errors.append("blocked_actions must be a list")
    else:
        for action in ("ofx_describe_from_aex", "ofx_render_with_aex", "route_through_ofx"):
            if action not in blocked:
                errors.append(f"OFX facade packet must block {action}")
    return errors


def write_ppm_create_new(path: Path, image: ppm_fixture_tool.PpmImage) -> Path:
    if image.width <= 0 or image.height <= 0:
        raise ValueError("image dimensions must be positive")
    if image.width * image.height > MAX_PIXELS:
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


def compare_identity(input_path: Path, output_path: Path) -> dict[str, Any]:
    input_image = ppm_fixture_tool.read_ppm(input_path)
    output_image = ppm_fixture_tool.read_ppm(output_path)
    pixel_match = input_image.pixels == output_image.pixels
    dimension_match = input_image.width == output_image.width and input_image.height == output_image.height
    if not pixel_match or not dimension_match:
        raise AssertionError("OFX no-op mock identity output did not match input")
    return {
        "width": input_image.width,
        "height": input_image.height,
        "bytes": len(input_image.pixels),
        "pixel_match": pixel_match,
        "dimension_match": dimension_match,
    }


def build_mock_report(
    *,
    packet: dict[str, Any],
    packet_path: Path,
    input_ppm: Path,
    output_ppm: Path,
) -> dict[str, Any]:
    packet_errors = validate_packet(packet)
    resolved_input = validate_input_ppm(input_ppm)
    resolved_output = validate_output_ppm(output_ppm)
    if packet_errors:
        mock_state = "invalid_ofx_packet_refused"
        identity_check = None
        output_path = None
    else:
        image = ppm_fixture_tool.read_ppm(resolved_input)
        output_path = write_ppm_create_new(resolved_output, image)
        identity_check = compare_identity(resolved_input, output_path)
        mock_state = "mock_identity_completed_route_closed"

    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_ofx_noop_mock_selftest",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_ofx_facade_packet": str(packet_path),
        "source_facade_state": packet.get("facade_state"),
        "source_stub_state": packet.get("source_stub_state"),
        "primary_review_candidate": packet.get("primary_review_candidate"),
        "mock_state": mock_state,
        "input_ppm": str(resolved_input),
        "output_ppm": str(output_path) if output_path else None,
        "identity_check": identity_check,
        "packet_errors": packet_errors,
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
        "mock_describe_performed": not bool(packet_errors),
        "mock_identity_transform_performed": not bool(packet_errors),
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
            "This is a no-op OFX mock selftest, not an OFX runtime invocation.",
            "It reads a deferred OFX packet and a generated PPM fixture only.",
            "It performs no AEX open/copy/hash/load/render and no OFX build/describe/render/route.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_json(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run no-op OFX mock selftest with PPM fixtures")
    parser.add_argument("--ofx-packet", required=True, help="Deferred OFX packet under target/ofx-facade")
    parser.add_argument("--input-ppm", required=True, help="Input PPM under target/ppm-fixtures")
    parser.add_argument("--output-ppm", required=True, help="Create-new output PPM under target/ofx-noop-mock")
    parser.add_argument("--out", required=True, help="Create-new selftest JSON under target/ofx-noop-mock")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    packet, packet_path = load_packet(Path(args.ofx_packet))
    report = build_mock_report(
        packet=packet,
        packet_path=packet_path,
        input_ppm=Path(args.input_ppm),
        output_ppm=Path(args.output_ppm),
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

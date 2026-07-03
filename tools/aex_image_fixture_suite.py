#!/usr/bin/env python3
"""Create a no-load PPM image fixture suite for future AEX render checks.

The suite reads sandbox-policy JSON only and creates deterministic PPM fixtures.
It does not open AEX files, load libraries, invoke AE, render, or route OFX.
"""

from __future__ import annotations

import argparse
import json
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
SANDBOX_POLICY_ROOT = TARGET_ROOT / "sandbox-policy"
PPM_FIXTURE_ROOT = TARGET_ROOT / "ppm-fixtures"
SUITE_ROOT = TARGET_ROOT / "image-fixture-suite"
MAX_PIXELS = 4096 * 4096

SAFETY_FLAGS = (
    "native_load_enabled",
    "native_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
    "aex_file_opened",
)

DEFAULT_CASES = (
    ("gradient_small", 16, 12, "gradient", "channel/ramp sampling sanity"),
    ("checker_edges", 17, 13, "checker", "edge and non-even dimension sanity"),
    ("solid_color", 8, 8, "solid", "constant-color baseline"),
    ("gradient_wide", 32, 9, "gradient", "non-square aspect baseline"),
)


@dataclass(frozen=True)
class PpmImage:
    width: int
    height: int
    pixels: bytes


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


def validate_policy_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("sandbox policy must have .json extension")
    return resolve_under_root(path, SANDBOX_POLICY_ROOT, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("image fixture suite output must have .json extension")
    SUITE_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, SUITE_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(SUITE_ROOT.resolve(strict=True)):
        raise ValueError(f"image fixture suite parent must stay under {SUITE_ROOT}")
    return resolved


def validate_ppm_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".ppm":
        raise ValueError("PPM output must have .ppm extension")
    PPM_FIXTURE_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, PPM_FIXTURE_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(PPM_FIXTURE_ROOT.resolve(strict=True)):
        raise ValueError(f"PPM parent must stay under {PPM_FIXTURE_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_sandbox_policy(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_policy_path(path)
    return read_json_object(resolved), resolved


def validate_policy(policy: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if policy.get("packet_kind") != "aex_sandbox_policy_packet":
        errors.append("source packet_kind must be aex_sandbox_policy_packet")
    if policy.get("sandbox_policy_state") != "policy_ready_no_native_load":
        errors.append("source sandbox_policy_state must be policy_ready_no_native_load")
    if policy.get("publication_status") != "local-only":
        errors.append("source publication_status must be local-only")
    primary = policy.get("primary_policy_candidate")
    if not isinstance(primary, dict):
        errors.append("source primary_policy_candidate must be an object")
    for flag in SAFETY_FLAGS:
        if policy.get(flag) is not False:
            errors.append(f"source {flag} must be false")
    return errors


def generate_image(width: int, height: int, pattern: str) -> PpmImage:
    if width <= 0 or height <= 0 or width * height > MAX_PIXELS:
        raise ValueError("invalid or too-large dimensions")
    pixels = bytearray()
    for y in range(height):
        for x in range(width):
            if pattern == "checker":
                value = 255 if (x // 4 + y // 4) % 2 == 0 else 32
                pixels.extend((value, value, value))
            elif pattern == "solid":
                pixels.extend((96, 144, 224))
            elif pattern == "gradient":
                r = int(255 * x / max(1, width - 1))
                g = int(255 * y / max(1, height - 1))
                b = (r ^ g) & 0xFF
                pixels.extend((r, g, b))
            else:
                raise ValueError(f"unsupported pattern: {pattern}")
    return PpmImage(width, height, bytes(pixels))


def write_ppm_create_new(path: Path, image: PpmImage) -> Path:
    expected = image.width * image.height * 3
    if len(image.pixels) != expected:
        raise ValueError("pixel byte count mismatch")
    resolved = validate_ppm_output_path(path)
    header = f"P6\n{image.width} {image.height}\n255\n".encode("ascii")
    with resolved.open("xb") as handle:
        handle.write(header)
        handle.write(image.pixels)
    return resolved


def case_filename(suite_id: str, case_id: str, width: int, height: int, pattern: str) -> str:
    safe_suite = "".join(ch if ch.isalnum() or ch in ("-", "_") else "_" for ch in suite_id)
    safe_case = "".join(ch if ch.isalnum() or ch in ("-", "_") else "_" for ch in case_id)
    return f"{safe_suite}-{safe_case}-{pattern}-{width}x{height}.ppm"


def build_suite(policy: dict[str, Any], policy_path: Path, *, suite_id: str) -> dict[str, Any]:
    errors = validate_policy(policy)
    if errors:
        raise ValueError("; ".join(errors))
    primary = policy["primary_policy_candidate"]
    fixtures: list[dict[str, Any]] = []
    for case_id, width, height, pattern, purpose in DEFAULT_CASES:
        image = generate_image(width, height, pattern)
        output = PPM_FIXTURE_ROOT / case_filename(suite_id, case_id, width, height, pattern)
        written = write_ppm_create_new(output, image)
        fixtures.append(
            {
                "case_id": case_id,
                "pattern": pattern,
                "width": width,
                "height": height,
                "pixel_bytes": len(image.pixels),
                "ppm_path": str(written),
                "purpose": purpose,
                "future_expected_baseline": "fixture_input_available_no_aex_render_claim",
            }
        )
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_image_fixture_suite",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "suite_id": suite_id,
        "source_sandbox_policy": str(policy_path),
        "source_sandbox_policy_state": policy.get("sandbox_policy_state"),
        "target_candidate": {
            "relative_path": primary.get("relative_path"),
            "candidate_policy_state": primary.get("candidate_policy_state"),
            "native_load_approval": primary.get("native_load_approval"),
        },
        "suite_state": "image_fixture_suite_ready",
        "fixture_count": len(fixtures),
        "fixtures": fixtures,
        "allowed_current_use": [
            "no-load worker PPM inspection",
            "future render harness input planning",
            "OFX no-op mock input planning",
        ],
        "blocked_actions": [
            "load_aex_dll",
            "call_EffectMain",
            "start_after_effects",
            "render_with_aex",
            "route_through_ofx",
        ],
        "native_load_enabled": False,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "notes": [
            "Suite reads sandbox policy JSON and writes generated PPM fixtures only.",
            "No AEX file is opened and no render claim is made.",
            "Existing fixture outputs are never overwritten.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build local AEX image fixture suite")
    parser.add_argument("--sandbox-policy", required=True, help="Sandbox policy JSON under target/sandbox-policy")
    parser.add_argument("--suite-id", required=True, help="Unique suite ID used for create-new PPM filenames")
    parser.add_argument("--out", required=True, help="Create-new suite JSON under target/image-fixture-suite")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    policy, policy_path = load_sandbox_policy(Path(args.sandbox_policy))
    suite = build_suite(policy, policy_path, suite_id=args.suite_id)
    written = write_json_create_new(Path(args.out), suite)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

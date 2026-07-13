#!/usr/bin/env python3
"""Verify AE ScatterMap PNGs against the fixed ARGB8 oracle."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

from PIL import Image

try:
    from tools.scattermap_reference_oracle import generated_luma_map, gradient, render_case
except ModuleNotFoundError:  # Direct execution places tools/ on sys.path.
    from scattermap_reference_oracle import generated_luma_map, gradient, render_case


CASES = {
    "default": {},
    "identity": {"amount": 0},
    "horizontal": {"direction": 1},
    "vertical": {"direction": 2},
    "amount_max": {"amount": 500},
    "seed_max": {"seed": 10000},
    "mix_zero": {"mix": 0.0},
    "connected_map": {"width": 11, "height": 7, "luma_map": generated_luma_map(11, 7, 5, 3)},
    "inverted_map": {"width": 11, "height": 7, "luma_map": generated_luma_map(11, 7, 11, 7, True)},
}


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest().upper()


def rgba_to_argb(rgba: bytes) -> bytes:
    if len(rgba) % 4:
        raise ValueError("RGBA byte count must be divisible by four")
    argb = bytearray(len(rgba))
    for offset in range(0, len(rgba), 4):
        argb[offset:offset + 4] = (
            rgba[offset + 3], rgba[offset], rgba[offset + 1], rgba[offset + 2]
        )
    return bytes(argb)


def verify(path: Path, case_id: str = "default") -> dict[str, object]:
    if case_id not in CASES:
        raise ValueError(f"unknown case: {case_id}")
    with Image.open(path) as image:
        size = image.size
        actual = rgba_to_argb(image.convert("RGBA").tobytes())
    parameters = CASES[case_id]
    width = int(parameters.get("width", 16))
    height = int(parameters.get("height", 12))
    expected = render_case(**parameters)
    source = gradient(width, height)
    if size != (width, height):
        raise ValueError(f"expected {width}x{height} PNG, got {size[0]}x{size[1]}")
    differences = [
        (index, actual_byte, expected_byte)
        for index, (actual_byte, expected_byte) in enumerate(zip(actual, expected))
        if actual_byte != expected_byte
    ]
    return {
        "schema_version": 1,
        "case_id": f"scattermap_{case_id}_argb8_{width}x{height}",
        "channel_normalization": "PNG RGBA to PF_Pixel8 ARGB",
        "width": width,
        "height": height,
        "actual_argb_sha256": sha256(actual),
        "expected_argb_sha256": sha256(expected),
        "source_argb_sha256": sha256(source),
        "different_bytes": len(differences),
        "different_pixels": len({index // 4 for index, _, _ in differences}),
        "max_channel_delta": max(
            (abs(actual_byte - expected_byte) for _, actual_byte, expected_byte in differences),
            default=0,
        ),
        "pixel_match": actual == expected,
        "non_identity_output": actual != source,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--png", required=True, type=Path)
    parser.add_argument("--case", choices=sorted(CASES), default="default")
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    report = verify(args.png, args.case)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(report, handle, indent=2, sort_keys=True)
        handle.write("\n")
    expected_non_identity = args.case not in {"identity", "mix_zero"}
    return 0 if report["pixel_match"] and report["non_identity_output"] == expected_non_identity else 1


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Verify AE output against an exact identity-rendered arbitrary input."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

from PIL import Image

try:
    from tools.ae_scattermap_render_verify import rgba_to_argb
    from tools.scattermap_reference_oracle import render_source
except ModuleNotFoundError:
    from ae_scattermap_render_verify import rgba_to_argb
    from scattermap_reference_oracle import render_source


def premultiply_argb(source: bytes) -> bytes:
    result = bytearray(source)
    for offset in range(0, len(result), 4):
        alpha = result[offset]
        for channel in (1, 2, 3):
            result[offset + channel] = round(result[offset + channel] * alpha / 255.0)
    return bytes(result)


def verify(identity_path: Path, output_path: Path,
           case_id: str = "scattermap_downsample_2x2_default", *,
           amount: int = 5, direction: int = 3, seed: int = 0,
           repeat_edge: bool = True, mix: float = 100.0,
           premultiply_output: bool = False) -> dict[str, object]:
    with Image.open(identity_path) as identity_image:
        size = identity_image.size
        source = rgba_to_argb(identity_image.convert("RGBA").tobytes())
    with Image.open(output_path) as output_image:
        if output_image.size != size:
            raise ValueError("identity and output dimensions differ")
        actual = rgba_to_argb(output_image.convert("RGBA").tobytes())
    width, height = size
    expected = render_source(source, width, height, amount=amount,
                             direction=direction, seed=seed,
                             repeat_edge=repeat_edge, mix=mix)
    if premultiply_output:
        expected = premultiply_argb(expected)
    differences = [abs(a - b) for a, b in zip(actual, expected) if a != b]
    digest = lambda data: hashlib.sha256(data).hexdigest().upper()
    return {
        "schema_version": 1,
        "case_id": case_id,
        "width": width,
        "height": height,
        "parameters": {"amount": amount, "direction": direction, "seed": seed,
                       "repeat_edge": repeat_edge, "mix": mix},
        "output_transform": "round_rgb_times_alpha" if premultiply_output else "none",
        "identity_argb_sha256": digest(source),
        "actual_argb_sha256": digest(actual),
        "expected_argb_sha256": digest(expected),
        "different_bytes": len(differences),
        "max_channel_delta": max(differences, default=0),
        "pixel_match": actual == expected,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--identity", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--case-id", default="scattermap_downsample_2x2_default")
    parser.add_argument("--amount", type=int, default=5)
    parser.add_argument("--direction", type=int, choices=(1, 2, 3), default=3)
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--no-repeat", action="store_true")
    parser.add_argument("--mix", type=float, default=100.0)
    parser.add_argument("--premultiply-output", action="store_true")
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    report = verify(args.identity, args.output, args.case_id, amount=args.amount,
                    direction=args.direction, seed=args.seed,
                    repeat_edge=not args.no_repeat, mix=args.mix,
                    premultiply_output=args.premultiply_output)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(report, handle, indent=2, sort_keys=True)
        handle.write("\n")
    return 0 if report["pixel_match"] else 1


if __name__ == "__main__":
    raise SystemExit(main())

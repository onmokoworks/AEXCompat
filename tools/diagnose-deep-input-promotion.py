#!/usr/bin/env python3
"""Verify the host's 8-bit PNG to AE ARGB16 input-world promotion."""

from __future__ import annotations

import argparse
import hashlib
import json
import struct
import sys
from pathlib import Path
from typing import Sequence

try:
    from tools.ae_png_depth_inspect import decode_png
except ModuleNotFoundError:
    from ae_png_depth_inspect import decode_png


CHANNELS = ("r", "g", "b", "a")
RAW_CHUNK_BYTES = 64 * 1024


class InputError(ValueError):
    pass


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(RAW_CHUNK_BYTES):
            digest.update(chunk)
    return digest.hexdigest()


def diagnose(source_png: Path, host_input: Path) -> dict[str, object]:
    metadata, rgba = decode_png(source_png)
    width = int(metadata["width"])
    height = int(metadata["height"])
    if metadata["bit_depth"] != 8:
        raise InputError("source PNG must be RGBA8")
    expected_bytes = width * height * 4 * 2
    actual_bytes = host_input.stat().st_size
    if actual_bytes != expected_bytes:
        raise InputError(
            f"host input byte count is {actual_bytes}, expected {expected_bytes}"
        )

    mismatches = 0
    maxima = [0, 0, 0, 0]
    first = None
    raw_hash = hashlib.sha256()
    index = 0
    with host_input.open("rb") as handle:
        while chunk := handle.read(RAW_CHUNK_BYTES):
            raw_hash.update(chunk)
            for (sample16,) in struct.iter_unpack("<H", chunk):
                sample8 = rgba[index]
                # AE_Macros.h CONVERT8TO16 with PF_MAX_CHAN16=32768.
                expected = (sample8 * 32768 + 127) // 255
                delta = abs(sample16 - expected)
                channel = index % 4
                maxima[channel] = max(maxima[channel], delta)
                if delta:
                    mismatches += 1
                    if first is None:
                        pixel = index // 4
                        first = {
                            "x": pixel % width,
                            "y": pixel // width,
                            "channel": CHANNELS[channel],
                            "source_rgba8": sample8,
                            "expected_argb16_value": expected,
                            "actual_argb16_value": sample16,
                        }
                index += 1

    return {
        "schema_version": 1,
        "diagnostic": "png8_to_host_argb16_input_world",
        "match": mismatches == 0,
        "dimensions": {"width": width, "height": height},
        "conversion": {
            "contract": "AE_Macros.h CONVERT8TO16",
            "formula": "(value8 * 32768 + 127) // 255",
            "white": 32768,
        },
        "hashes": {
            "source_png_sha256": _sha256(source_png),
            "host_input_raw_sha256": raw_hash.hexdigest(),
        },
        "mismatched_channels": mismatches,
        "max_abs_error": dict(zip(CHANNELS, maxima)),
        "first_mismatch": first,
        "conclusion": (
            "host input promotion matches the SDK conversion; investigate the "
            "plug-in call contract or AE-side input world next"
            if mismatches == 0
            else "host input promotion differs from the SDK conversion"
        ),
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-png", required=True, type=Path)
    parser.add_argument("--host-input", required=True, type=Path)
    parser.add_argument("--out", type=Path)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        report = diagnose(args.source_png, args.host_input)
    except (InputError, OSError, ValueError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    payload = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(payload, encoding="utf-8", newline="\n")
    else:
        sys.stdout.write(payload)
    return 0 if report["match"] else 1


if __name__ == "__main__":
    raise SystemExit(main())

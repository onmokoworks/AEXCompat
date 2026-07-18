#!/usr/bin/env python3
"""Generate a deterministic RGBA8 PNG oracle input.

The pixel content is a pure function of (x, y, size), so any machine can
regenerate the same image and verify it against the recorded hashes. With
scale(v, m) = floor((v * 255 * 2 + m) / (m * 2)) - integer round-half-up
of v * 255 / m, and 0 when m <= 0:

    r = scale(x, width - 1)
    g = scale(y, height - 1)
    b = scale(x + y, width + height - 2)
    a = 255                                   (--alpha-mode opaque)
    a = scale(y, height - 1)                  (--alpha-mode vertical-gradient)

The PNG is written non-interlaced, filter type 0, zlib level 9, so
`tools/ae_png_depth_inspect.py` can decode it without precision loss.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import struct
import sys
import zlib
from pathlib import Path


MAX_DIMENSION = 4096
ALPHA_MODES = ("opaque", "vertical-gradient")


def scaled(value: int, maximum: int) -> int:
    if maximum <= 0:
        return 0
    return (value * 255 * 2 + maximum) // (maximum * 2)


def rgba_rows(width: int, height: int, alpha_mode: str) -> bytes:
    rows = bytearray()
    for y in range(height):
        green = scaled(y, height - 1)
        alpha = 255 if alpha_mode == "opaque" else scaled(y, height - 1)
        rows.append(0)  # filter type 0 (None)
        for x in range(width):
            rows.append(scaled(x, width - 1))
            rows.append(green)
            rows.append(scaled(x + y, width + height - 2))
            rows.append(alpha)
    return bytes(rows)


def png_chunk(chunk_type: bytes, payload: bytes) -> bytes:
    return (
        struct.pack(">I", len(payload))
        + chunk_type
        + payload
        + struct.pack(">I", zlib.crc32(chunk_type + payload) & 0xFFFFFFFF)
    )


def write_png(path: Path, width: int, height: int, alpha_mode: str) -> bytes:
    decoded = rgba_rows(width, height, alpha_mode)
    payload = (
        b"\x89PNG\r\n\x1a\n"
        + png_chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + png_chunk(b"IDAT", zlib.compress(decoded, 9))
        + png_chunk(b"IEND", b"")
    )
    with path.open("xb") as handle:
        handle.write(payload)
    return payload


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--width", required=True, type=int)
    parser.add_argument("--height", required=True, type=int)
    parser.add_argument("--alpha-mode", choices=ALPHA_MODES, default="opaque")
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args(argv)
    if not (1 <= args.width <= MAX_DIMENSION and 1 <= args.height <= MAX_DIMENSION):
        print(f"error: dimensions must be 1..{MAX_DIMENSION}", file=sys.stderr)
        return 2
    try:
        payload = write_png(args.out, args.width, args.height, args.alpha_mode)
    except FileExistsError:
        print(f"error: refusing to overwrite {args.out}", file=sys.stderr)
        return 2
    report = {
        "schema_version": 1,
        "width": args.width,
        "height": args.height,
        "alpha_mode": args.alpha_mode,
        "png_sha256": hashlib.sha256(payload).hexdigest(),
    }
    json.dump(report, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Convert a non-interlaced RGBA PNG to the raw buffer format that
`tools/compare-pixel-oracles.py --raw` expects (rgba8 or rgba16le),
without precision loss (PIL silently truncates 16-bit PNGs, so this
goes through `tools.ae_png_depth_inspect.decode_png` instead)."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

try:
    from tools.ae_png_depth_inspect import decode_png
except ModuleNotFoundError:  # Direct execution places tools/ on sys.path.
    from ae_png_depth_inspect import decode_png


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--png", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        metadata, decoded = decode_png(args.png)
    except (OSError, ValueError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    if metadata["bit_depth"] == 8:
        raw = bytes(decoded)
        raw_format = "rgba8"
    else:
        # Swap each big-endian sample to little-endian with C-level slice
        # assignments; unpacking tens of millions of samples into Python
        # integers would balloon memory on 4K-sized deep renders.
        swapped = bytearray(len(decoded))
        swapped[0::2] = decoded[1::2]
        swapped[1::2] = decoded[0::2]
        raw = bytes(swapped)
        raw_format = "rgba16le"
    try:
        with args.out.open("xb") as handle:
            handle.write(raw)
    except FileExistsError:
        print(f"error: refusing to overwrite {args.out}", file=sys.stderr)
        return 2
    report = {
        "schema_version": 1,
        "width": metadata["width"],
        "height": metadata["height"],
        "raw_format": raw_format,
        "png_sha256": hashlib.sha256(args.png.read_bytes()).hexdigest(),
        "raw_sha256": hashlib.sha256(raw).hexdigest(),
    }
    json.dump(report, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

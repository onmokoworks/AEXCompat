#!/usr/bin/env python3
"""Verify the 8-to-16 promotion/export mechanism claims against artifacts.

Two machine-checkable claims from the ntsc-rs 16 bpc oracle investigation
(docs/AE_ORACLE_NTSC_RS_CAPTURE_2026-07-18.md, issue #53/#61):

1. The host worker promotes an 8-bit RGBA input into the smart-input ARGB16
   world (AE range, white = 32768) with exactly
   ``round(v * 32768 / 255) = (v * 32768 + 127) // 255`` per sample. The
   world snapshot is RGBA-ordered transport bytes, so it compares 1:1 with
   the decoded PNG stream.
2. After Effects' composed 8-bit import -> 16 bpc -> ``saveFrameToPng``
   chain, observed via a no-effect 16 bpc capture of the same input, maps
   each 8-bit value ``v`` deterministically to ``v * 257 + d`` with a
   deviation ``d`` in {-1, 0, +1}, and rounding the PNG16 sample back with
   ``round(v16 / 257)`` recovers ``v`` exactly.

The tool exits nonzero if either claim fails, so evidence refresh scripts
can fail closed. The emitted JSON manifest carries only hashes, counts, and
histograms - never raw image contents.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from ae_png_depth_inspect import decode_png  # noqa: E402


def sha256_of(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify_smart_input(input_png: Path, dump: Path) -> dict:
    meta, rgba = decode_png(input_png)
    if meta["bit_depth"] != 8:
        raise SystemExit(f"input PNG must be 8-bit, got {meta['bit_depth']}")
    raw = dump.read_bytes()
    samples = len(rgba)
    if len(raw) != samples * 2:
        raise SystemExit(
            f"smart-input dump has {len(raw)} bytes, expected {samples * 2}")
    values = struct.unpack(f"<{samples}H", raw)
    mismatches = sum(
        1 for v8, v16 in zip(rgba, values)
        if v16 != (v8 * 32768 + 127) // 255)
    return {
        "input_png_sha256": sha256_of(input_png),
        "input_decoded_bit_depth": 8,
        "smart_input_dump_sha256": sha256_of(dump),
        "formula": "round(v * 32768 / 255) = (v * 32768 + 127) // 255",
        "total_samples": samples,
        "mismatched_samples": mismatches,
        "holds": mismatches == 0,
    }


def verify_noeffect_map(input_png: Path, noeffect_png: Path) -> dict:
    meta8, rgba = decode_png(input_png)
    meta16, data16 = decode_png(noeffect_png)
    if meta16["bit_depth"] != 16:
        raise SystemExit(
            f"no-effect PNG must be 16-bit, got {meta16['bit_depth']}")
    samples = len(rgba)
    if len(data16) != samples * 2:
        raise SystemExit(
            f"no-effect PNG has {len(data16)} decoded bytes, "
            f"expected {samples * 2}")
    values16 = struct.unpack(f">{samples}H", data16)

    mapping: dict[int, int] = {}
    deterministic = True
    for v8, v16 in zip(rgba, values16):
        known = mapping.setdefault(v8, v16)
        if known != v16:
            deterministic = False
            break

    deviations: dict[str, int] = {}
    deviation_bounded = True
    roundtrip_exact = True
    if deterministic:
        for v8, v16 in sorted(mapping.items()):
            deviation = v16 - v8 * 257
            deviations[str(deviation)] = deviations.get(str(deviation), 0) + 1
            if abs(deviation) > 1:
                deviation_bounded = False
            if int(v16 / 257 + 0.5) != v8:
                roundtrip_exact = False
    return {
        "input_png_sha256": sha256_of(input_png),
        "noeffect_png_sha256": sha256_of(noeffect_png),
        "formula": "v * 257 + d, d in {-1, 0, +1}",
        "distinct_8bit_values_observed": len(mapping),
        "mapping_deterministic": deterministic,
        "deviation_histogram": deviations,
        "deviation_bounded_by_one": deviation_bounded,
        "roundtrip_round_v16_div_257_exact": roundtrip_exact,
        "holds": deterministic and deviation_bounded and roundtrip_exact,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input-png", required=True, type=Path,
                        help="8-bit RGBA oracle input PNG")
    parser.add_argument("--smart-input-dump", required=True, type=Path,
                        help="rgba16le smart-input world snapshot of that input")
    parser.add_argument("--noeffect-png", required=True, type=Path,
                        help="AE 16 bpc no-effect capture PNG of that input")
    parser.add_argument("--out", type=Path,
                        help="write the manifest JSON here as well as stdout")
    args = parser.parse_args()

    manifest = {
        "schema_version": 1,
        "generated_by": "tools/verify-deep16-mechanism.py",
        "host_promotion": verify_smart_input(
            args.input_png, args.smart_input_dump),
        "ae_composed_map": verify_noeffect_map(
            args.input_png, args.noeffect_png),
    }
    manifest["holds"] = (manifest["host_promotion"]["holds"]
                         and manifest["ae_composed_map"]["holds"])
    text = json.dumps(manifest, indent=2, sort_keys=True)
    if args.out:
        args.out.write_text(text + "\n", encoding="utf-8")
    print(text)
    return 0 if manifest["holds"] else 1


if __name__ == "__main__":
    raise SystemExit(main())

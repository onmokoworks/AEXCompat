#!/usr/bin/env python3
"""Inspect non-interlaced RGBA PNG depth output without precision loss."""

from __future__ import annotations

import argparse
import hashlib
import json
import struct
import zlib
from pathlib import Path


PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"


def paeth(left: int, up: int, upper_left: int) -> int:
    estimate = left + up - upper_left
    left_distance = abs(estimate - left)
    up_distance = abs(estimate - up)
    upper_left_distance = abs(estimate - upper_left)
    if left_distance <= up_distance and left_distance <= upper_left_distance:
        return left
    return up if up_distance <= upper_left_distance else upper_left


def decode_png(path: Path) -> tuple[dict[str, int], bytes]:
    data = path.read_bytes()
    if not data.startswith(PNG_SIGNATURE):
        raise ValueError("invalid PNG signature")
    offset = len(PNG_SIGNATURE)
    header = None
    compressed = bytearray()
    while offset < len(data):
        length = struct.unpack_from(">I", data, offset)[0]
        chunk_type = data[offset + 4:offset + 8]
        payload = data[offset + 8:offset + 8 + length]
        if chunk_type == b"IHDR":
            header = struct.unpack(">IIBBBBB", payload)
        elif chunk_type == b"IDAT":
            compressed.extend(payload)
        elif chunk_type == b"IEND":
            break
        offset += 12 + length
    if header is None:
        raise ValueError("IHDR missing")
    width, height, bit_depth, color_type, compression, filtering, interlace = header
    if color_type != 6 or bit_depth not in (8, 16) or compression or filtering or interlace:
        raise ValueError("only non-interlaced RGBA8/RGBA16 PNG is supported")
    bytes_per_pixel = 4 * (bit_depth // 8)
    row_bytes = width * bytes_per_pixel
    filtered = zlib.decompress(bytes(compressed))
    if len(filtered) != height * (row_bytes + 1):
        raise ValueError("unexpected decompressed byte count")
    decoded = bytearray(height * row_bytes)
    source_offset = 0
    for y in range(height):
        filter_type = filtered[source_offset]
        source_offset += 1
        for x in range(row_bytes):
            raw = filtered[source_offset + x]
            left = decoded[y * row_bytes + x - bytes_per_pixel] if x >= bytes_per_pixel else 0
            up = decoded[(y - 1) * row_bytes + x] if y else 0
            upper_left = decoded[(y - 1) * row_bytes + x - bytes_per_pixel] if y and x >= bytes_per_pixel else 0
            if filter_type == 0:
                value = raw
            elif filter_type == 1:
                value = raw + left
            elif filter_type == 2:
                value = raw + up
            elif filter_type == 3:
                value = raw + ((left + up) // 2)
            elif filter_type == 4:
                value = raw + paeth(left, up, upper_left)
            else:
                raise ValueError(f"unsupported filter type {filter_type}")
            decoded[y * row_bytes + x] = value & 0xFF
        source_offset += row_bytes
    return {
        "width": width,
        "height": height,
        "bit_depth": bit_depth,
    }, bytes(decoded)


def inspect_png(path: Path) -> dict[str, object]:
    metadata, decoded = decode_png(path)
    width = metadata["width"]
    height = metadata["height"]
    bit_depth = metadata["bit_depth"]
    data = path.read_bytes()
    samples = list(decoded) if bit_depth == 8 else list(struct.unpack(f">{width * height * 4}H", decoded))
    return {
        "schema_version": 1,
        "width": width,
        "height": height,
        "bit_depth": bit_depth,
        "color_type": "rgba",
        "decoded_rgba_sha256": hashlib.sha256(decoded).hexdigest().upper(),
        "png_sha256": hashlib.sha256(data).hexdigest().upper(),
        "sample_min": min(samples),
        "sample_max": max(samples),
        "sample_count": len(samples),
    }


def compare_pngs(first: Path, second: Path) -> dict[str, object]:
    first_metadata, first_decoded = decode_png(first)
    second_metadata, second_decoded = decode_png(second)
    if first_metadata != second_metadata:
        raise ValueError("PNG dimensions or bit depth differ")
    bit_depth = first_metadata["bit_depth"]
    if bit_depth == 8:
        first_samples = list(first_decoded)
        second_samples = list(second_decoded)
    else:
        count = first_metadata["width"] * first_metadata["height"] * 4
        first_samples = list(struct.unpack(f">{count}H", first_decoded))
        second_samples = list(struct.unpack(f">{count}H", second_decoded))
    differences = [abs(a - b) for a, b in zip(first_samples, second_samples) if a != b]
    return {
        "schema_version": 1,
        **first_metadata,
        "sample_count": len(first_samples),
        "different_samples": len(differences),
        "stable_samples": len(first_samples) - len(differences),
        "max_sample_delta": max(differences, default=0),
        "decoded_match": not differences,
        "first_decoded_rgba_sha256": hashlib.sha256(first_decoded).hexdigest().upper(),
        "second_decoded_rgba_sha256": hashlib.sha256(second_decoded).hexdigest().upper(),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--png", required=True, type=Path)
    parser.add_argument("--compare", type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    report = compare_pngs(args.png, args.compare) if args.compare else inspect_png(args.png)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(report, handle, indent=2, sort_keys=True)
        handle.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

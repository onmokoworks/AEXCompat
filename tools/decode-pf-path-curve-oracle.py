#!/usr/bin/env python3
"""Decode the reversible PF Path Curve oracle byte stream from an image."""

from __future__ import annotations

import argparse
import json
import struct
import zlib
from pathlib import Path
from typing import Iterable

MAGIC = b"PFPCURV\0"
VERSION = 1
HEADER = struct.Struct("<8sHHIII")
RECORD_SIZE = 112
SENTINEL_BITS = 0x7FF4A5A5DEADBEEF
FREQUENCIES = (-1, 0, 1, 1023, 1024, 1025)
LENGTH_CASES = ("-inf", "-1", "-0", "0", "nextafter(0,+inf)",
                "nextafter(length,-inf)", "length", "nextafter(length,+inf)",
                "+inf", "nan")


def _i32(data: bytes, offset: int) -> int:
    return struct.unpack_from("<i", data, offset)[0]


def _u64(data: bytes, offset: int) -> int:
    return struct.unpack_from("<Q", data, offset)[0]


def _field(bits: int) -> dict[str, object]:
    return {"bits": f"0x{bits:016x}", "value": struct.unpack("<d", struct.pack("<Q", bits))[0]}


def decode_bytes(data: bytes) -> dict[str, object]:
    if len(data) < HEADER.size:
        raise ValueError("image does not contain a complete oracle header")
    magic, version, header_size, record_size, count, expected_crc = HEADER.unpack_from(data)
    if magic != MAGIC:
        raise ValueError(f"bad magic: {magic!r}")
    if version != VERSION or header_size != HEADER.size or record_size != RECORD_SIZE:
        raise ValueError("unsupported oracle layout")
    end = header_size + record_size * count
    if end > len(data):
        raise ValueError("truncated oracle payload")
    payload = data[header_size:end]
    actual_crc = zlib.crc32(payload) & 0xFFFFFFFF
    if actual_crc != expected_crc:
        raise ValueError(f"CRC32 mismatch: expected {expected_crc:08x}, got {actual_crc:08x}")
    records = []
    for index in range(count):
        raw = payload[index * record_size:(index + 1) * record_size]
        case_index = struct.unpack_from("<I", raw, 4)[0]
        records.append({
            "frequency": _i32(raw, 0), "length_case": case_index,
            "length_case_name": LENGTH_CASES[case_index] if case_index < len(LENGTH_CASES) else "unknown",
            "requested_length": _field(_u64(raw, 8)),
            "prepare_error": _i32(raw, 16), "get_length_error": _i32(raw, 20),
            "segment_length": _field(_u64(raw, 24)),
            "prep_state": list(raw[32:37]), "sentinel_bits": f"0x{_u64(raw, 40):016x}",
            "eval_error": _i32(raw, 48), "eval_x": _field(_u64(raw, 52)),
            "eval_y": _field(_u64(raw, 60)), "deriv_error": _i32(raw, 68),
            "deriv_x": _field(_u64(raw, 72)), "deriv_y": _field(_u64(raw, 80)),
            "deriv_dx": _field(_u64(raw, 88)), "deriv_dy": _field(_u64(raw, 96)),
            "cleanup_error": _i32(raw, 104),
        })
    return {"magic": magic.rstrip(b"\0").decode("ascii"), "version": version,
            "record_size": record_size, "count": count, "crc32": f"{actual_crc:08x}",
            "records": records}


def argb_bytes(pixels: Iterable[tuple[int, ...]], depth: int) -> bytes:
    divisor = 1 if depth == 8 else 257
    result = bytearray()
    for pixel in pixels:
        if len(pixel) != 4:
            raise ValueError("oracle image must have four channels")
        for value in pixel:
            if value % divisor:
                raise ValueError("16-bit channel is not byte * 257")
            byte = value // divisor
            if not 0 <= byte <= 255:
                raise ValueError("channel is outside oracle byte range")
            result.append(byte)
    return bytes(result)


def decode_image(path: Path) -> dict[str, object]:
    from PIL import Image
    with Image.open(path) as image:
        if image.mode == "RGBA":
            pixels = list(image.get_flattened_data())
            if any(a != 255 for _, _, _, a in pixels):
                raise ValueError("oracle image alpha must remain fully opaque")
            return decode_bytes(bytes(channel for r, g, b, _ in pixels for channel in (r, g, b)))
        raise ValueError(f"unsupported image mode {image.mode!r}; use lossless RGBA")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("image", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    result = decode_image(args.image)
    rendered = json.dumps(result, indent=2, allow_nan=True) + "\n"
    if args.output:
        args.output.write_text(rendered, encoding="utf-8")
    else:
        print(rendered, end="")


if __name__ == "__main__":
    main()

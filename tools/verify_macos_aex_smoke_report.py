#!/usr/bin/env python3
"""Validate a packaged arm64 worker render-trace report and PNG output."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import struct
import sys
import zlib


# A bounded 50,000-event execution trace is commonly about 5 MiB. Keep the
# package gate above that producer bound while still rejecting unbounded input.
MAX_REPORT_BYTES = 16 * 1024 * 1024
MAX_OUTPUT_BYTES = 512 * 1024 * 1024
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"


class DuplicateKeyError(ValueError):
    pass


def fail(message: str) -> "NoReturn":
    raise SystemExit(f"macos_aex_smoke_error: {message}")


def reject_duplicate_keys(pairs: list[tuple[str, object]]) -> dict[str, object]:
    value: dict[str, object] = {}
    for key, item in pairs:
        if key in value:
            raise DuplicateKeyError(key)
        value[key] = item
    return value


def validate_png(payload: bytes) -> tuple[int, int]:
    if payload[:8] != PNG_SIGNATURE:
        fail("render output does not have a PNG signature")
    offset = len(PNG_SIGNATURE)
    ihdr: bytes | None = None
    idat = bytearray()
    saw_iend = False
    saw_non_idat_after_idat = False
    palette = False
    while offset < len(payload):
        if len(payload) - offset < 12:
            fail("render output has a truncated PNG chunk")
        length = struct.unpack(">I", payload[offset : offset + 4])[0]
        chunk_type = payload[offset + 4 : offset + 8]
        chunk_end = offset + 12 + length
        if length > MAX_OUTPUT_BYTES or chunk_end > len(payload):
            fail("render output has an invalid PNG chunk length")
        data = payload[offset + 8 : offset + 8 + length]
        expected_crc = struct.unpack(">I", payload[offset + 8 + length : chunk_end])[0]
        if zlib.crc32(chunk_type + data) != expected_crc:
            fail("render output has an invalid PNG chunk CRC")
        if ihdr is None and chunk_type != b"IHDR":
            fail("render output PNG does not begin with IHDR")
        if chunk_type == b"IHDR":
            if ihdr is not None or length != 13:
                fail("render output has an invalid or duplicate IHDR")
            ihdr = data
        elif chunk_type == b"PLTE":
            if idat or length == 0 or length % 3 != 0 or length > 768:
                fail("render output has an invalid PLTE")
            palette = True
        elif chunk_type == b"IDAT":
            if saw_non_idat_after_idat:
                fail("render output has non-consecutive IDAT chunks")
            idat.extend(data)
            if len(idat) > MAX_OUTPUT_BYTES:
                fail("render output IDAT payload is too large")
        elif chunk_type == b"IEND":
            if length != 0 or not idat or chunk_end != len(payload):
                fail("render output has an invalid IEND or trailing bytes")
            saw_iend = True
        elif idat:
            saw_non_idat_after_idat = True
        offset = chunk_end
        if saw_iend:
            break
    if ihdr is None or not idat or not saw_iend:
        fail("render output is missing required PNG chunks")

    width, height, bit_depth, color_type, compression, filtering, interlace = struct.unpack(
        ">IIBBBBB", ihdr
    )
    if width == 0 or height == 0 or width > 32768 or height > 32768:
        fail("render output dimensions are invalid")
    allowed_depths = {
        0: {1, 2, 4, 8, 16},
        2: {8, 16},
        3: {1, 2, 4, 8},
        4: {8, 16},
        6: {8, 16},
    }
    channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}
    if color_type not in allowed_depths or bit_depth not in allowed_depths[color_type]:
        fail("render output has an unsupported PNG color/depth combination")
    if color_type == 3 and not palette:
        fail("indexed render output is missing PLTE")
    if compression != 0 or filtering != 0 or interlace != 0:
        fail("render output uses unsupported PNG compression/filter/interlace")
    row_bytes = (width * channels[color_type] * bit_depth + 7) // 8
    expected_size = height * (row_bytes + 1)
    if expected_size > MAX_OUTPUT_BYTES:
        fail("render output decoded pixels exceed the bounded contract")
    decoder = zlib.decompressobj()
    try:
        decoded = decoder.decompress(bytes(idat), expected_size + 1)
        if len(decoded) > expected_size:
            fail("render output decoded scanline size is invalid")
        decoded += decoder.flush(expected_size + 1 - len(decoded))
    except zlib.error as error:
        fail(f"render output IDAT is not valid zlib data: {error}")
    if not decoder.eof or decoder.unused_data or len(decoded) != expected_size:
        fail("render output decoded scanline size is invalid")
    stride = row_bytes + 1
    if any(decoded[offset] > 4 for offset in range(0, len(decoded), stride)):
        fail("render output contains an invalid PNG filter byte")
    return width, height


def validate(report_path: Path, output_path: Path) -> dict[str, object]:
    if not report_path.is_file():
        fail("diagnostic report is missing")
    report_size = report_path.stat().st_size
    if report_size <= 0 or report_size > MAX_REPORT_BYTES:
        fail("diagnostic report size is outside the bounded contract")
    try:
        report = json.loads(
            report_path.read_text(encoding="utf-8"),
            object_pairs_hook=reject_duplicate_keys,
        )
    except DuplicateKeyError as error:
        fail(f"diagnostic report contains duplicate JSON key: {error}")
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        fail(f"diagnostic report is not bounded UTF-8 JSON: {error}")
    if not isinstance(report, dict) or report.get("schema_version") != 1:
        fail("diagnostic report schema_version must be 1")
    if report.get("error") is not None or report.get("render_error") not in (None, 0):
        fail("diagnostic report contains a render error")
    gpu = report.get("gpu")
    if not isinstance(gpu, dict) or gpu.get("requested_backend") != "cpu":
        fail("diagnostic does not identify the Unicorn CPU correctness path")
    pre_render = gpu.get("pre_render")
    if not isinstance(pre_render, dict):
        fail("diagnostic is missing pre_render state")
    if pre_render.get("attempted") is True:
        if pre_render.get("completed") is not True or pre_render.get("error") != 0:
            fail("diagnostic pre_render did not complete cleanly")
    elif pre_render.get("attempted") is False:
        if pre_render.get("completed") is not False or pre_render.get("error") is not None:
            fail("diagnostic unattempted pre_render state is inconsistent")
    else:
        fail("diagnostic pre_render attempted flag is malformed")
    render = gpu.get("render")
    if not isinstance(render, dict):
        fail("diagnostic is missing render state")
    if render.get("attempted") is not True or render.get("completed") is not True:
        fail("diagnostic render did not complete")
    if render.get("error") != 0:
        fail("diagnostic render returned an error")
    if gpu.get("cleanup_complete") is not True:
        fail("diagnostic cleanup is incomplete")
    if report.get("unsupported_suite_calls") != []:
        fail("diagnostic contains unsupported suite calls")
    if report.get("dropped_unsupported_suite_calls") != 0:
        fail("diagnostic dropped unsupported suite evidence")
    requests = report.get("suite_requests")
    if not isinstance(requests, list) or len(requests) > 256 or not all(
        isinstance(request, str) and len(request) <= 256 for request in requests
    ):
        fail("suite request evidence is malformed or unbounded")

    if not output_path.is_file():
        fail("render output is missing")
    size = output_path.stat().st_size
    if size < 24 or size > MAX_OUTPUT_BYTES:
        fail("render output size is outside the bounded PNG contract")
    payload = output_path.read_bytes()
    width, height = validate_png(payload)
    return {
        "schema_version": 1,
        "status": "verified",
        "width": width,
        "height": height,
        "output_size": size,
        "output_sha256": hashlib.sha256(payload).hexdigest(),
        "suite_request_count": len(requests),
    }


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        fail("usage: verify_macos_aex_smoke_report.py <diagnostic.json> <output.png>")
    print(json.dumps(validate(Path(argv[1]), Path(argv[2])), sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

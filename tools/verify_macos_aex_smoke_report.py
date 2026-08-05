#!/usr/bin/env python3
"""Validate a packaged arm64 worker render-trace report and PNG output."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import struct
import sys


# A bounded 50,000-event execution trace is commonly about 5 MiB. Keep the
# package gate above that producer bound while still rejecting unbounded input.
MAX_REPORT_BYTES = 16 * 1024 * 1024
MAX_OUTPUT_BYTES = 512 * 1024 * 1024
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"


def fail(message: str) -> "NoReturn":
    raise SystemExit(f"macos_aex_smoke_error: {message}")


def validate(report_path: Path, output_path: Path) -> dict[str, object]:
    if not report_path.is_file():
        fail("diagnostic report is missing")
    report_size = report_path.stat().st_size
    if report_size <= 0 or report_size > MAX_REPORT_BYTES:
        fail("diagnostic report size is outside the bounded contract")
    try:
        report = json.loads(report_path.read_text(encoding="utf-8"))
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
    if pre_render.get("attempted") is True and (
        pre_render.get("completed") is not True or pre_render.get("error") != 0
    ):
        fail("diagnostic pre_render did not complete cleanly")
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
    if payload[:8] != PNG_SIGNATURE or payload[12:16] != b"IHDR":
        fail("render output is not a PNG with an IHDR")
    width, height = struct.unpack(">II", payload[16:24])
    if width == 0 or height == 0 or width > 32768 or height > 32768:
        fail("render output dimensions are invalid")
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

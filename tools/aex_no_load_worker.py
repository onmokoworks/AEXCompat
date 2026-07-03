#!/usr/bin/env python3
"""JSONL no-load worker for AEX compatibility harness selftests.

This worker intentionally supports only PPM fixture inspection/transforms.
It does not read, copy, hash, load, or execute AEX files.
"""

from __future__ import annotations

import json
import platform
import struct
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, TextIO


LAB_ROOT = Path(__file__).resolve().parents[1]
PPM_FIXTURE_ROOT = LAB_ROOT / "target" / "ppm-fixtures"
WORKER_OUTPUT_ROOT = LAB_ROOT / "target" / "worker-selftest"
MAX_PIXELS = 4096 * 4096

SCHEMA_VERSION = 1
WORKER_KIND = "aex_no_load_worker"

ALLOWED_MESSAGES = [
    "hello",
    "inspect_environment",
    "inspect_ppm",
    "transform_ppm_identity",
    "quit",
]

BLOCKED_MESSAGES = {
    "load_aex",
    "load_aex_dll",
    "call_effect_main",
    "dispatch_PF_Cmd",
    "render_frame",
    "render_with_aex",
    "ofx_describe",
    "route_through_ofx",
}


@dataclass(frozen=True)
class PpmImage:
    width: int
    height: int
    pixels: bytes


def safety_state() -> dict[str, Any]:
    return {
        "native_load_enabled": False,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


def path_has_traversal(path: Path) -> bool:
    return any(part in ("..", ".") for part in path.parts)


def resolve_under_root(path: Path, root: Path, *, must_exist: bool) -> Path:
    if path_has_traversal(path):
        raise ValueError("path must not contain traversal components")
    absolute = path if path.is_absolute() else LAB_ROOT / path
    resolved_root = root.resolve(strict=True)
    resolved = absolute.resolve(strict=must_exist)
    if not resolved.is_relative_to(resolved_root):
        raise ValueError(f"path must stay under {root}")
    return resolved


def validate_input_ppm(path: Path) -> Path:
    if path.suffix.lower() != ".ppm":
        raise ValueError("input path must have .ppm extension")
    PPM_FIXTURE_ROOT.mkdir(parents=True, exist_ok=True)
    return resolve_under_root(path, PPM_FIXTURE_ROOT, must_exist=True)


def validate_output_ppm(path: Path) -> Path:
    if path.suffix.lower() != ".ppm":
        raise ValueError("output path must have .ppm extension")
    WORKER_OUTPUT_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, WORKER_OUTPUT_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(WORKER_OUTPUT_ROOT.resolve(strict=True)):
        raise ValueError(f"output parent must stay under {WORKER_OUTPUT_ROOT}")
    return resolved


def read_token(data: bytes, offset: int) -> tuple[bytes, int]:
    while offset < len(data) and data[offset] in b" \t\r\n":
        offset += 1
    if offset < len(data) and data[offset] == ord("#"):
        while offset < len(data) and data[offset] not in b"\r\n":
            offset += 1
        return read_token(data, offset)
    start = offset
    while offset < len(data) and data[offset] not in b" \t\r\n":
        offset += 1
    return data[start:offset], offset


def read_ppm(path: Path) -> PpmImage:
    resolved = validate_input_ppm(path)
    data = resolved.read_bytes()
    magic, offset = read_token(data, 0)
    if magic != b"P6":
        raise ValueError("only binary P6 PPM is supported")
    width_token, offset = read_token(data, offset)
    height_token, offset = read_token(data, offset)
    max_token, offset = read_token(data, offset)
    width = int(width_token)
    height = int(height_token)
    max_value = int(max_token)
    if width <= 0 or height <= 0 or width * height > MAX_PIXELS:
        raise ValueError("invalid or too-large PPM dimensions")
    if max_value != 255:
        raise ValueError("only max value 255 is supported")
    while offset < len(data) and data[offset] in b" \t\r\n":
        offset += 1
        break
    pixels = data[offset:]
    expected = width * height * 3
    if len(pixels) != expected:
        raise ValueError(f"pixel byte count mismatch: expected {expected}, got {len(pixels)}")
    return PpmImage(width, height, pixels)


def write_ppm_create_new(path: Path, image: PpmImage) -> Path:
    if image.width <= 0 or image.height <= 0:
        raise ValueError("image dimensions must be positive")
    if image.width * image.height > MAX_PIXELS:
        raise ValueError("image exceeds max fixture pixel count")
    expected = image.width * image.height * 3
    if len(image.pixels) != expected:
        raise ValueError(f"pixel byte count mismatch: expected {expected}, got {len(image.pixels)}")
    resolved = validate_output_ppm(path)
    header = f"P6\n{image.width} {image.height}\n255\n".encode("ascii")
    with resolved.open("xb") as handle:
        handle.write(header)
        handle.write(image.pixels)
    return resolved


def error_response(code: str, message: str, request_id: Any = None) -> dict[str, Any]:
    return {
        "type": "error",
        "request_id": request_id,
        "code": code,
        "message": message,
        "safety_state": safety_state(),
    }


def with_request_id(response: dict[str, Any], request_id: Any) -> dict[str, Any]:
    if request_id is not None:
        response["request_id"] = request_id
    return response


def handle_message(message: dict[str, Any]) -> dict[str, Any]:
    if not isinstance(message, dict):
        return error_response("invalid_message", "message must be an object")
    request_id = message.get("id")
    message_type = message.get("type")
    if not isinstance(message_type, str):
        return error_response("invalid_message", "message type must be a string", request_id)
    if message_type in BLOCKED_MESSAGES:
        return error_response("blocked_action", f"{message_type} is blocked by the no-load worker", request_id)
    if message_type == "hello":
        return with_request_id(
            {
                "type": "hello_ack",
                "schema_version": SCHEMA_VERSION,
                "worker_kind": WORKER_KIND,
                "allowed_messages": ALLOWED_MESSAGES,
                "blocked_messages": sorted(BLOCKED_MESSAGES),
                "safety_state": safety_state(),
            },
            request_id,
        )
    if message_type == "inspect_environment":
        return with_request_id(
            {
                "type": "environment_report",
                "schema_version": SCHEMA_VERSION,
                "process_bitness": struct.calcsize("P") * 8,
                "platform": platform.system(),
                "native_load_enabled": False,
                "safety_state": safety_state(),
            },
            request_id,
        )
    if message_type == "inspect_ppm":
        try:
            image = read_ppm(Path(str(message.get("input", ""))))
        except Exception as exc:
            return error_response("ppm_inspect_failed", str(exc), request_id)
        return with_request_id(
            {
                "type": "ppm_summary",
                "width": image.width,
                "height": image.height,
                "bytes": len(image.pixels),
                "safety_state": safety_state(),
            },
            request_id,
        )
    if message_type == "transform_ppm_identity":
        try:
            image = read_ppm(Path(str(message.get("input", ""))))
            output = write_ppm_create_new(Path(str(message.get("out", ""))), image)
        except Exception as exc:
            return error_response("ppm_transform_failed", str(exc), request_id)
        return with_request_id(
            {
                "type": "created_output",
                "operation": "identity",
                "path": str(output),
                "width": image.width,
                "height": image.height,
                "bytes": len(image.pixels),
                "safety_state": safety_state(),
            },
            request_id,
        )
    if message_type == "quit":
        return with_request_id({"type": "quit_ack", "safety_state": safety_state()}, request_id)
    return error_response("unknown_message", f"unknown message type: {message_type}", request_id)


def run_jsonl_loop(stdin: TextIO = sys.stdin, stdout: TextIO = sys.stdout) -> int:
    for line in stdin:
        if not line.strip():
            continue
        try:
            message = json.loads(line)
            response = handle_message(message)
        except Exception as exc:
            response = error_response("invalid_json", str(exc))
        stdout.write(json.dumps(response, ensure_ascii=False, separators=(",", ":")) + "\n")
        stdout.flush()
        if response.get("type") == "quit_ack":
            return 0
    return 0


def main() -> int:
    return run_jsonl_loop()


if __name__ == "__main__":
    raise SystemExit(main())

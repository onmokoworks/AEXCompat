"""Bounded JSONL session wrapper used by the Blender addon.

This wrapper deliberately has no AEX loader.  Its only executable transport
mode is ``identity_no_aex``; the response carries enough evidence to prevent
that transport check from being mistaken for an AEX render.
"""

from __future__ import annotations

import base64
import hashlib
import json
import sys
from pathlib import Path, PureWindowsPath
from typing import Any


SCHEMA_VERSION = 1
MAX_DIMENSION = 8192
MAX_BYTES = 8192 * 8192 * 4


class SessionRequestError(ValueError):
    """A request is malformed or asks for an unsupported operation."""

    def __init__(self, message: str, failure_class: str = "request_validation_error"):
        super().__init__(message)
        self.failure_class = failure_class


def _sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _worker_identity() -> dict[str, Any]:
    source = Path(__file__).read_bytes()
    return {
        "name": "blender_aexcompat_session",
        "version": "1",
        "source_sha256": _sha256(source),
    }


def _require_int(value: Any, name: str, *, minimum: int = 1) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < minimum:
        raise SessionRequestError(f"{name} must be an integer >= {minimum}")
    return value


def _validate_relative_plugin_path(value: Any) -> str | None:
    """Return a canonical relative path or reject ambiguous root traversal."""

    if value is None:
        return None
    if not isinstance(value, str) or not value or "\x00" in value:
        raise SessionRequestError("plugin.source_relative_path must be relative text")
    normalized = value.replace("\\", "/")
    windows = PureWindowsPath(value)
    if normalized.startswith("/") or windows.is_absolute() or windows.drive or ":" in normalized:
        raise SessionRequestError("plugin.source_relative_path must be relative text")
    parts = normalized.split("/")
    if any(part in {"", ".", ".."} for part in parts):
        raise SessionRequestError("plugin.source_relative_path must not contain traversal")
    return "/".join(parts)


def _decode_rgba8(request: dict[str, Any]) -> tuple[bytes, dict[str, Any]]:
    frame = request.get("frame")
    image = request.get("input")
    if not isinstance(frame, dict) or not isinstance(image, dict):
        raise SessionRequestError("frame and input objects are required")

    width = _require_int(frame.get("width"), "frame.width")
    height = _require_int(frame.get("height"), "frame.height")
    if width > MAX_DIMENSION or height > MAX_DIMENSION:
        raise SessionRequestError("frame dimensions exceed the bounded limit")
    stride = _require_int(frame.get("stride"), "frame.stride")
    minimum_stride = width * 4
    if stride < minimum_stride or stride % 4:
        raise SessionRequestError("frame.stride must be a 4-byte aligned RGBA row stride")
    if frame.get("channels") != "RGBA8":
        raise SessionRequestError("only RGBA8 channel order is supported")
    if frame.get("alpha") not in {"straight", "premultiplied"}:
        raise SessionRequestError("frame.alpha must be straight or premultiplied")
    if not isinstance(frame.get("color_space"), str) or not frame["color_space"]:
        raise SessionRequestError("frame.color_space is required")
    if not isinstance(image.get("encoding"), str) or image["encoding"] != "base64-rgba8":
        raise SessionRequestError("input.encoding must be base64-rgba8")
    encoded = image.get("data")
    if not isinstance(encoded, str):
        raise SessionRequestError("input.data must be base64 text")
    try:
        pixels = base64.b64decode(encoded, validate=True)
    except Exception as exc:  # pragma: no cover - exact binascii type varies
        raise SessionRequestError("input.data is not valid base64") from exc
    if not pixels:
        raise SessionRequestError("input image is empty", "empty_input")
    expected = stride * height
    if expected > MAX_BYTES or len(pixels) != expected:
        raise SessionRequestError(f"input byte count {len(pixels)} does not match {expected}")
    supplied_sha = image.get("sha256")
    actual_sha = _sha256(pixels)
    if supplied_sha != actual_sha:
        raise SessionRequestError("input.sha256 does not match input.data")
    return pixels, {
        "width": width,
        "height": height,
        "stride": stride,
        "channels": "RGBA8",
        "alpha": frame["alpha"],
        "color_space": frame["color_space"],
        "frame_time": frame.get("frame_time", {"numerator": 0, "denominator": 1}),
    }


def build_response(request: dict[str, Any]) -> dict[str, Any]:
    if not isinstance(request, dict) or request.get("schema_version") != SCHEMA_VERSION:
        raise SessionRequestError("schema_version 1 is required")
    if request.get("request_kind") != "aexcompat_blender_session":
        raise SessionRequestError("request_kind is not supported")
    plugin = request.get("plugin", {})
    if not isinstance(plugin, dict):
        raise SessionRequestError("plugin must be an object")
    source_relative_path = _validate_relative_plugin_path(plugin.get("source_relative_path"))
    pixels, frame = _decode_rgba8(request)
    mode = request.get("mode", "identity_no_aex")
    if mode not in {"identity_no_aex", "fixture_invert_no_aex"}:
        return {
            "schema_version": SCHEMA_VERSION,
            "response_kind": "aexcompat_blender_session_result",
            "status": "unsupported",
            "failure_class": "aex_not_loaded",
            "aex_render_performed": False,
            "host_success": False,
            "worker_identity": _worker_identity(),
            "plugin_identity": {"state": "not_loaded", "source_relative_path": source_relative_path, "sha256": None},
            "error": "AEX loading/rendering is not implemented in this slice",
        }

    if mode == "fixture_invert_no_aex":
        transformed = bytearray(pixels)
        for row_start in range(0, len(transformed), frame["stride"]):
            row_end = row_start + frame["width"] * 4
            for offset in range(row_start, row_end, 4):
                transformed[offset] = 255 - transformed[offset]
                transformed[offset + 1] = 255 - transformed[offset + 1]
                transformed[offset + 2] = 255 - transformed[offset + 2]
        output_pixels = bytes(transformed)
        status = "fixture_transform"
        diagnostics = ["fixture invert applied; no AEX was loaded or rendered"]
    else:
        output_pixels = pixels
        status = "identity_only"
        diagnostics = ["transport validated; no AEX was loaded or rendered"]
    input_sha = _sha256(pixels)
    output_sha = _sha256(output_pixels)
    return {
        "schema_version": SCHEMA_VERSION,
        "response_kind": "aexcompat_blender_session_result",
        "status": status,
        "failure_class": "aex_not_loaded",
        "aex_render_performed": False,
        "host_success": False,
        "worker_identity": _worker_identity(),
        "plugin_identity": {"state": "not_loaded", "source_relative_path": source_relative_path, "sha256": None},
        "frame": frame,
        "input": {"sha256": input_sha, "bytes": len(pixels)},
        "output": {
            "encoding": "base64-rgba8",
            "data": base64.b64encode(output_pixels).decode("ascii"),
            "sha256": output_sha,
            "bytes": len(output_pixels),
        },
        "diff": {
            "byte_count": len(pixels),
            "changed_bytes": sum(left != right for left, right in zip(pixels, output_pixels)),
            "max_abs_delta": max((abs(left - right) for left, right in zip(pixels, output_pixels)), default=0),
        },
        "diagnostics": diagnostics,
    }


def _error_response(error: Exception) -> dict[str, Any]:
    return {
        "schema_version": SCHEMA_VERSION,
        "response_kind": "aexcompat_blender_session_result",
        "status": "error",
        "failure_class": getattr(error, "failure_class", "request_validation_error"),
        "aex_render_performed": False,
        "host_success": False,
        "worker_identity": _worker_identity(),
        "plugin_identity": {"state": "not_loaded", "source_relative_path": None, "sha256": None},
        "error": str(error),
    }


def main() -> int:
    for line in sys.stdin:
        if not line.strip():
            continue
        try:
            request = json.loads(line)
            response = build_response(request)
        except Exception as exc:  # protocol errors are still explicit JSON
            response = _error_response(exc)
        sys.stdout.write(json.dumps(response, sort_keys=True, separators=(",", ":")) + "\n")
        sys.stdout.flush()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

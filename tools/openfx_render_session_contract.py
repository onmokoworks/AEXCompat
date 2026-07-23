#!/usr/bin/env python3
"""Validate the host-neutral OpenFX -> RenderSession frame contract.

This module deliberately stops at the adapter boundary.  It does not open an
AEX, load a DLL, start a worker, or claim that a Blender/Resolve host rendered
anything.  Host adapters can use the packet to exchange bounded RGBA8 frames
with the existing broker ``RenderSession`` API and classify failures without
turning a no-op or unsupported route into success.
"""

from __future__ import annotations

import argparse
import base64
import binascii
import hashlib
import json
import re
from pathlib import Path, PureWindowsPath
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
SCHEMA_PATH = ROOT / "contracts" / "openfx" / "render_session_bridge.schema.json"
MAX_DIMENSION = 4096
MAX_PIXELS = MAX_DIMENSION * MAX_DIMENSION
MAX_ROW_BYTES = MAX_DIMENSION * 16
MAX_TRANSPORT_BYTES = 64 * 1024 * 1024
SHA256_RE = re.compile(r"^[A-Fa-f0-9]{64}$")
FAILED_STATUSES = {"unsupported", "timeout", "worker_crash", "identity_mismatch", "protocol_error"}


def _integer(value: Any) -> bool:
    return isinstance(value, int) and not isinstance(value, bool)


def _relative_path_errors(value: Any, label: str) -> list[str]:
    if not isinstance(value, str) or not value or "\x00" in value:
        return [f"{label} must be a non-empty relative path"]
    windows = PureWindowsPath(value)
    if windows.is_absolute() or windows.drive or value.startswith(("/", "\\")):
        return [f"{label} must be relative"]
    if any(part in ("", ".", "..") for part in windows.parts):
        return [f"{label} must not contain traversal or empty components"]
    return []


def _sha_errors(value: Any, label: str) -> list[str]:
    if not isinstance(value, str) or not SHA256_RE.fullmatch(value):
        return [f"{label} must be a 64-character SHA-256 hex string"]
    return []


def _decode_payload(value: Any, label: str) -> tuple[bytes | None, list[str]]:
    if not isinstance(value, str) or not value:
        return None, [f"{label} must be non-empty base64"]
    try:
        decoded = base64.b64decode(value, validate=True)
    except (ValueError, binascii.Error):
        return None, [f"{label} must be valid base64"]
    if not decoded:
        return None, [f"{label} must not decode to an empty payload"]
    return decoded, []


def _frame_errors(frame: Any, label: str, transport: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if not isinstance(frame, dict):
        return [f"{label} must be an object"]
    width, height, rowbytes = (frame.get(key) for key in ("width", "height", "rowbytes"))
    if not _integer(width) or not 1 <= width <= MAX_DIMENSION:
        errors.append(f"{label}.width must be an integer in 1..{MAX_DIMENSION}")
    if not _integer(height) or not 1 <= height <= MAX_DIMENSION:
        errors.append(f"{label}.height must be an integer in 1..{MAX_DIMENSION}")
    if _integer(width) and _integer(height) and width * height > MAX_PIXELS:
        errors.append(f"{label} exceeds the pixel bound")
    minimum_rowbytes = width * 4 if _integer(width) else 4
    if not _integer(rowbytes) or not minimum_rowbytes <= rowbytes <= MAX_ROW_BYTES:
        errors.append(f"{label}.rowbytes must cover RGBA8 pixels and stay within the bound")
    if _integer(rowbytes) and rowbytes % 4:
        errors.append(f"{label}.rowbytes must be aligned to one RGBA8 pixel")
    for key in ("pixel_format", "channel_order"):
        if frame.get(key) != transport.get(key):
            errors.append(f"{label}.{key} must match transport")
    if frame.get("alpha_mode") != transport.get("alpha_mode"):
        errors.append(f"{label}.alpha_mode must match transport")
    errors.extend(_sha_errors(frame.get("sha256"), f"{label}.sha256"))
    decoded, decode_errors = _decode_payload(frame.get("data_base64"), f"{label}.data_base64")
    errors.extend(decode_errors)
    if decoded is not None and _integer(rowbytes) and _integer(height):
        expected = rowbytes * height
        if expected > MAX_TRANSPORT_BYTES:
            errors.append(f"{label} exceeds the {MAX_TRANSPORT_BYTES}-byte transport bound")
        if len(decoded) != expected:
            errors.append(f"{label}.data_base64 length must equal rowbytes*height")
        if isinstance(frame.get("sha256"), str) and hashlib.sha256(decoded).hexdigest() != frame["sha256"].lower():
            errors.append(f"{label}.sha256 does not match data_base64")
    return errors


def _schema_errors(packet: dict[str, Any]) -> list[str]:
    try:
        import jsonschema
    except ImportError:
        return ["jsonschema is required to validate the OpenFX bridge schema"]
    schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
    validator = jsonschema.Draft202012Validator(schema)
    return [error.message for error in sorted(validator.iter_errors(packet), key=str)]


def validate_bridge_packet(packet: Any) -> list[str]:
    """Return all schema and semantic errors for one bridge packet."""

    if not isinstance(packet, dict):
        return ["bridge packet must be an object"]
    errors = _schema_errors(packet)
    transport = packet.get("transport") if isinstance(packet.get("transport"), dict) else {}
    session = packet.get("session_open") if isinstance(packet.get("session_open"), dict) else {}
    plugin = session.get("plugin") if isinstance(session.get("plugin"), dict) else {}
    worker = session.get("worker") if isinstance(session.get("worker"), dict) else {}
    geometry = session.get("geometry") if isinstance(session.get("geometry"), dict) else {}
    session_time = session.get("time") if isinstance(session.get("time"), dict) else {}

    errors.extend(_relative_path_errors(plugin.get("relative_path"), "session_open.plugin.relative_path"))
    errors.extend(_sha_errors(plugin.get("sha256"), "session_open.plugin.sha256"))
    errors.extend(_sha_errors(worker.get("sha256"), "session_open.worker.sha256"))
    if geometry.get("rowbytes") != geometry.get("width", 0) * 4:
        errors.append("session_open.geometry.rowbytes must be tight RGBA8 rowbytes")
    if _integer(session_time.get("time_step")) and session_time["time_step"] <= 0:
        errors.append("session_open.time.time_step must be positive")
    if _integer(session_time.get("total_time")) and session_time["total_time"] <= 0:
        errors.append("session_open.time.total_time must be positive")
    if _integer(session_time.get("time_scale")) and session_time["time_scale"] <= 0:
        errors.append("session_open.time.time_scale must be positive")

    exchange = packet.get("frame_exchange") if isinstance(packet.get("frame_exchange"), dict) else {}
    request = exchange.get("request") if isinstance(exchange.get("request"), dict) else {}
    errors.extend(_frame_errors(request.get("input"), "frame_exchange.request.input", transport))
    if request.get("current_time") != session_time.get("current_time"):
        errors.append("frame request current_time must match session_open time")
    response = exchange.get("response") if isinstance(exchange.get("response"), dict) else {}
    status = response.get("status")
    if status == "rendered":
        if packet.get("contract_state") != "ready":
            errors.append("rendered response requires contract_state=ready")
        errors.extend(_frame_errors(response.get("output"), "frame_exchange.response.output", transport))
        identity = response.get("identity") if isinstance(response.get("identity"), dict) else {}
        if identity.get("plugin_sha256", "").lower() != str(plugin.get("sha256", "")).lower():
            errors.append("render response plugin identity does not match session_open")
        if identity.get("worker_sha256", "").lower() != str(worker.get("sha256", "")).lower():
            errors.append("render response worker identity does not match session_open")
    elif status in FAILED_STATUSES:
        if packet.get("contract_state") != "rejected":
            errors.append("failed response requires contract_state=rejected")
        if "output" in response:
            errors.append("failed render response must not contain output")
        if not isinstance(response.get("classification"), str) or not response["classification"]:
            errors.append("failed render response needs a classification")
    else:
        errors.append("response status must be rendered or an explicit fail-closed status")
    close = packet.get("close") if isinstance(packet.get("close"), dict) else {}
    if close.get("status") == "closed" and status != "rendered":
        errors.append("a non-rendered response cannot close as healthy")
    if close.get("status") == "invalidated" and status == "rendered":
        errors.append("a rendered response cannot close as invalidated")
    return errors


def frame_from_bytes(
    *, width: int, height: int, rowbytes: int, pixels: bytes, alpha_mode: str = "straight"
) -> dict[str, Any]:
    """Encode one bounded RGBA8 row-major frame for a bridge packet."""

    return {
        "width": width,
        "height": height,
        "rowbytes": rowbytes,
        "pixel_format": "rgba8",
        "channel_order": "rgba",
        "alpha_mode": alpha_mode,
        "sha256": hashlib.sha256(pixels).hexdigest(),
        "data_base64": base64.b64encode(pixels).decode("ascii"),
    }


def build_bridge_packet(
    *,
    plugin_relative_path: str,
    plugin_sha256: str,
    worker_sha256: str,
    width: int,
    height: int,
    rowbytes: int,
    pixels: bytes,
    frame_index: int = 0,
    current_time: int = 0,
    time_step: int = 1,
    total_time: int = 1,
    time_scale: int = 1,
    alpha_mode: str = "straight",
    response_status: str = "rendered",
    output_pixels: bytes | None = None,
) -> dict[str, Any]:
    """Build a testable common packet without invoking a host or worker."""

    input_frame = frame_from_bytes(
        width=width, height=height, rowbytes=rowbytes, pixels=pixels, alpha_mode=alpha_mode
    )
    output = None
    if response_status == "rendered":
        output = frame_from_bytes(
            width=width,
            height=height,
            rowbytes=rowbytes,
            pixels=pixels if output_pixels is None else output_pixels,
            alpha_mode=alpha_mode,
        )
    response: dict[str, Any] = {"status": response_status}
    if output is not None:
        response.update(
            {
                "output": output,
                "identity": {"plugin_sha256": plugin_sha256, "worker_sha256": worker_sha256},
            }
        )
    else:
        response["classification"] = response_status
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "contract_kind": "aexcompat_openfx_render_session_bridge",
        "contract_state": "ready" if response_status == "rendered" else "rejected",
        "transport": {
            "pixel_format": "rgba8",
            "channel_order": "rgba",
            "alpha_mode": alpha_mode,
            "rowbytes_unit": "bytes",
            "payload_encoding": "base64_row_major",
        },
        "session_open": {
            "plugin": {"relative_path": plugin_relative_path, "sha256": plugin_sha256},
            "worker": {"kind": "render", "sha256": worker_sha256},
            "geometry": {"width": width, "height": height, "rowbytes": width * 4},
            "time": {
                "current_time": current_time,
                "time_step": time_step,
                "total_time": total_time,
                "time_scale": time_scale,
            },
        },
        "frame_exchange": {
            "request": {"frame_index": frame_index, "current_time": current_time, "input": input_frame},
            "response": response,
        },
        "close": {
            "status": "closed" if response_status == "rendered" else "invalidated",
            "frames_ok": 1 if response_status == "rendered" else 0,
            "frames_errored": 0 if response_status == "rendered" else 1,
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--packet", type=Path, required=True)
    args = parser.parse_args()
    packet = json.loads(args.packet.read_text(encoding="utf-8"))
    errors = validate_bridge_packet(packet)
    if errors:
        print(json.dumps({"valid": False, "errors": errors}, ensure_ascii=False, indent=2))
        return 1
    print(json.dumps({"valid": True, "contract_kind": packet["contract_kind"]}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

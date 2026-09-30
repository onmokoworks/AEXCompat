"""Run one bounded OpenFX RGBA8 frame through the macOS AEX fixture worker.

This is the host-neutral execution side of the OpenFX bridge packet. The
existing Blender transport owns the macOS fixture/artifact checks; this module
reuses that route and publishes a bridge packet only after those checks pass.
"""

from __future__ import annotations

import base64
import binascii
import hashlib
import json
import os
import sys
from pathlib import Path
from typing import Any

import blender_aexcompat_session as macos_backend
from openfx_render_session_contract import (
    MAX_DIMENSION,
    MAX_ROW_BYTES,
    MAX_TRANSPORT_BYTES,
    build_bridge_packet,
    frame_from_bytes,
    validate_bridge_packet,
)


MAX_REQUEST_JSON_BYTES = (MAX_TRANSPORT_BYTES * 4 // 3) + 65_536


def _file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _runtime_files(relative_path: str) -> tuple[Path, Path, Path]:
    root_value = os.environ.get("AEXCOMPAT_PLUGIN_ROOT")
    harness_value = os.environ.get("AEXCOMPAT_HARNESS")
    worker_value = os.environ.get("AEXCOMPAT_GUEST_WORKER")
    if not root_value or not harness_value or not worker_value:
        raise ValueError("AEXCompat macOS plug-in root, harness, and worker are required")
    root = Path(root_value).expanduser().resolve()
    plugin = (root / relative_path).resolve()
    harness = Path(harness_value).expanduser().resolve()
    worker = Path(worker_value).expanduser().resolve()
    if not root.is_dir() or not plugin.is_relative_to(root) or not plugin.is_file():
        raise ValueError("AEX path is missing or escapes the configured root")
    if not harness.is_file() or not worker.is_file():
        raise ValueError("macOS harness or guest worker is missing")
    return plugin, harness, worker


def _checked_packet(packet: dict[str, Any]) -> dict[str, Any]:
    errors = validate_bridge_packet(packet)
    if errors:
        raise ValueError("invalid OpenFX bridge packet: " + "; ".join(errors))
    return packet


def _failure_packet(
    *, status: str, detail: str, settings: dict[str, Any]
) -> dict[str, Any]:
    packet = build_bridge_packet(**settings, response_status=status)
    packet["frame_exchange"]["response"]["classification"] = detail[:120] or status
    return _checked_packet(packet)


def render_macos_frame(
    *,
    plugin_relative_path: str,
    width: int,
    height: int,
    rowbytes: int,
    pixels: bytes,
    frame_index: int = 0,
    current_time: int = 0,
    time_step: int = 1,
    total_time: int = 1,
    time_scale: int = 1000,
    alpha_mode: str = "straight",
    render_path: str = "classic",
) -> dict[str, Any]:
    """Return a verified bridge packet for one real AEX frame.

    The fixture transport currently represents time in milliseconds and one
    frame per process. Unsupported inputs fail before the worker is launched.
    Identity fields record the files actually seen; they are not launch gates.
    """

    if sys.platform != "darwin":
        raise ValueError("macOS AEX fixture worker is required")
    relative_path = macos_backend._validate_relative_plugin_path(plugin_relative_path)
    if relative_path is None or len(relative_path) > 260:
        raise ValueError("AEX relative path must fit the OpenFX contract")
    if any(isinstance(value, bool) or not isinstance(value, int) for value in (
        width, height, rowbytes, frame_index, current_time, time_step, total_time, time_scale,
    )):
        raise ValueError("OpenFX geometry and time must be integers")
    if not (1 <= width <= MAX_DIMENSION and 1 <= height <= MAX_DIMENSION):
        raise ValueError("OpenFX dimensions are outside the bounded range")
    if not (width * 4 <= rowbytes <= MAX_ROW_BYTES) or rowbytes % 4:
        raise ValueError("OpenFX rowbytes must cover aligned RGBA8 rows")
    if not isinstance(pixels, bytes):
        raise ValueError("OpenFX input must be bytes")
    if rowbytes * height > MAX_TRANSPORT_BYTES or len(pixels) != rowbytes * height:
        raise ValueError("OpenFX input length exceeds or differs from rowbytes * height")
    if frame_index != 0 or time_scale != 1000 or time_step != 1 or total_time != max(1, current_time):
        raise ValueError("this macOS fixture route supports one frame at millisecond time")
    if not 0 <= current_time <= 3_600_000:
        raise ValueError("OpenFX frame time is outside the fixture range")
    if alpha_mode != "straight" or render_path not in {"classic", "smart"}:
        raise ValueError("unsupported OpenFX alpha mode or AEX render path")

    plugin, harness, worker = _runtime_files(relative_path)
    plugin_sha = _file_sha256(plugin)
    harness_sha = _file_sha256(harness)
    worker_sha = _file_sha256(worker)
    settings = dict(
        plugin_relative_path=relative_path,
        plugin_sha256=plugin_sha,
        worker_sha256=worker_sha,
        width=width,
        height=height,
        rowbytes=rowbytes,
        pixels=pixels,
        frame_index=frame_index,
        current_time=current_time,
        time_step=time_step,
        total_time=total_time,
        time_scale=time_scale,
        alpha_mode=alpha_mode,
    )
    packed = b"".join(pixels[row * rowbytes : row * rowbytes + width * 4] for row in range(height))
    request = {
        "schema_version": 1,
        "request_kind": "aexcompat_blender_session",
        "mode": "render_aex",
        "render_path": render_path,
        "plugin": {"source_relative_path": relative_path},
        "frame": {
            "width": width, "height": height, "stride": width * 4,
            "channels": "RGBA8", "alpha": "straight", "color_space": "scene_linear",
            "frame_time": {"seconds": current_time / time_scale},
        },
        "input": {
            "encoding": "base64-rgba8",
            "data": base64.b64encode(packed).decode("ascii"),
            "sha256": hashlib.sha256(packed).hexdigest(),
        },
    }
    try:
        result = macos_backend.build_response(request)
    except macos_backend.SessionRequestError as error:
        status = {
            "session_timeout": "timeout",
            "worker_crash": "worker_crash",
            "worker_failure": "unsupported",
            "artifact_mismatch": "protocol_error",
        }.get(error.failure_class, "unsupported")
        return _failure_packet(status=status, detail=error.failure_class, settings=settings)
    except OSError:
        # The backend may lose a plug-in or executable while hashing after
        # render. Keep the failure bounded and do not publish unverified pixels.
        try:
            stable_files = (
                _file_sha256(plugin) == plugin_sha
                and _file_sha256(harness) == harness_sha
                and _file_sha256(worker) == worker_sha
            )
        except OSError:
            stable_files = False
        return _failure_packet(
            status="protocol_error" if stable_files else "identity_mismatch",
            detail="backend_io_error" if stable_files else "runtime_file_unavailable",
            settings=settings,
        )

    if not isinstance(result, dict):
        return _failure_packet(status="protocol_error", detail="invalid backend response", settings=settings)
    if result.get("status") != "rendered" or result.get("aex_render_performed") is not True or result.get("host_success") is not True:
        return _failure_packet(status="unsupported", detail="AEX render was not observed", settings=settings)
    if result.get("render_path") != render_path:
        return _failure_packet(status="protocol_error", detail="backend render path differs from request", settings=settings)
    identity = result.get("render_identity", {})
    output = result.get("output", {})
    plugin_identity = result.get("plugin_identity", {})
    observed_frame = result.get("frame", {})
    if not all(isinstance(value, dict) for value in (identity, output, plugin_identity, observed_frame)):
        return _failure_packet(status="protocol_error", detail="invalid backend evidence", settings=settings)
    if any(observed_frame.get(key) != request["frame"][key] for key in (
        "width", "height", "stride", "channels", "alpha", "frame_time",
    )):
        return _failure_packet(status="protocol_error", detail="backend frame differs from OpenFX request", settings=settings)
    post_run = identity.get("post_run", {})
    if not isinstance(post_run, dict):
        return _failure_packet(status="protocol_error", detail="invalid post-run identity", settings=settings)
    observed_plugin_sha = plugin_identity.get("sha256")
    observed_harness_sha = identity.get("harness_sha256")
    observed_worker_sha = identity.get("guest_worker_sha256")
    try:
        current_plugin_sha = _file_sha256(plugin)
        current_harness_sha = _file_sha256(harness)
        current_worker_sha = _file_sha256(worker)
    except OSError:
        return _failure_packet(status="identity_mismatch", detail="runtime_file_unavailable", settings=settings)
    if (
        plugin_identity.get("source_relative_path") != relative_path
        or identity.get("files_unchanged") is not True
        or post_run.get("plugin_sha256") != observed_plugin_sha
        or post_run.get("harness_sha256") != observed_harness_sha
        or post_run.get("guest_worker_sha256") != observed_worker_sha
        or current_plugin_sha != observed_plugin_sha
        or current_harness_sha != observed_harness_sha
        or current_worker_sha != observed_worker_sha
    ):
        return _failure_packet(status="identity_mismatch", detail="observed runtime file identity changed", settings=settings)
    try:
        rendered = base64.b64decode(output["data"], validate=True)
    except (binascii.Error, KeyError, TypeError, ValueError):
        return _failure_packet(status="protocol_error", detail="invalid rendered pixel encoding", settings=settings)
    backend_input = result.get("input", {})
    if not isinstance(backend_input, dict):
        return _failure_packet(status="protocol_error", detail="invalid backend input evidence", settings=settings)
    if (
        len(rendered) != width * height * 4
        or output.get("encoding") != "base64-rgba8"
        or output.get("bytes") != len(rendered)
        or hashlib.sha256(rendered).hexdigest() != output.get("sha256")
        or backend_input.get("bytes") != len(packed)
        or backend_input.get("sha256") != hashlib.sha256(packed).hexdigest()
    ):
        return _failure_packet(status="protocol_error", detail="rendered pixel or input identity mismatch", settings=settings)
    # A file may have been rebuilt after the adapter's first read. The backend
    # records what it actually ran; use that stable observed identity here.
    settings["plugin_sha256"] = observed_plugin_sha
    settings["worker_sha256"] = observed_worker_sha
    packet = build_bridge_packet(**settings, output_pixels=rendered)
    packet["frame_exchange"]["response"]["output"] = frame_from_bytes(
        width=width, height=height, rowbytes=width * 4, pixels=rendered, alpha_mode=alpha_mode
    )
    return _checked_packet(packet)


def process_request(request: Any) -> dict[str, Any]:
    """Decode one host request and return its fully checked bridge packet."""

    required = {"plugin_relative_path", "width", "height", "rowbytes", "pixels_base64"}
    optional = {"frame_index", "current_time", "time_step", "total_time", "time_scale", "alpha_mode", "render_path"}
    if not isinstance(request, dict) or not required <= request.keys() or request.keys() - required - optional:
        raise ValueError("invalid OpenFX host request fields")
    encoded = request["pixels_base64"]
    if not isinstance(encoded, str) or len(encoded) > MAX_REQUEST_JSON_BYTES:
        raise ValueError("OpenFX pixel payload is not bounded base64")
    try:
        pixels = base64.b64decode(encoded, validate=True)
    except binascii.Error as error:
        raise ValueError("invalid OpenFX pixel payload") from error
    fields = {key: value for key, value in request.items() if key != "pixels_base64"}
    return render_macos_frame(**fields, pixels=pixels)


def _unique_pairs(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    value: dict[str, Any] = {}
    for key, item in pairs:
        if key in value:
            raise ValueError("duplicate OpenFX request key")
        value[key] = item
    return value


def main() -> int:
    """Read one bounded JSON request from stdin and write one result to stdout."""

    try:
        raw = sys.stdin.buffer.read(MAX_REQUEST_JSON_BYTES + 1)
        if len(raw) > MAX_REQUEST_JSON_BYTES:
            raise ValueError("OpenFX request exceeds bounded size")
        request = json.loads(
            raw,
            object_pairs_hook=_unique_pairs,
            parse_constant=lambda _value: (_ for _ in ()).throw(ValueError("non-finite JSON number")),
        )
        packet = process_request(request)
    except (UnicodeError, ValueError, TypeError, OSError):
        print(json.dumps({"status": "rejected", "classification": "invalid_request"}))
        return 2
    print(json.dumps(packet, separators=(",", ":")))
    return 0 if packet["frame_exchange"]["response"]["status"] == "rendered" else 1


if __name__ == "__main__":
    raise SystemExit(main())

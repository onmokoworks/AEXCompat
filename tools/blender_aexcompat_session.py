"""Bounded JSONL session wrapper used by the Blender addon.

The real render mode delegates to the existing macOS fixture harness.  Fixture
transport modes remain explicitly separate from an AEX render.
"""

from __future__ import annotations

import base64
import hashlib
import json
import os
import signal
import struct
import subprocess
import sys
import tempfile
import zlib
from pathlib import Path, PureWindowsPath
from typing import Any


SCHEMA_VERSION = 1
MAX_DIMENSION = 8192
MAX_BYTES = 8192 * 8192 * 4
MAX_RENDER_TIMEOUT_MS = 180_000
MAX_RENDER_METADATA_BYTES = 1_000_000
_ACTIVE_HARNESS_PID: int | None = None


class SessionRequestError(ValueError):
    """A request is malformed or asks for an unsupported operation."""

    def __init__(self, message: str, failure_class: str = "request_validation_error"):
        super().__init__(message)
        self.failure_class = failure_class


def _sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


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


def _strict_json(data: str | bytes) -> Any:
    def unique(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
        result: dict[str, Any] = {}
        for key, value in pairs:
            if key in result:
                raise SessionRequestError("duplicate JSON key in render artifact", "artifact_mismatch")
            result[key] = value
        return result

    try:
        return json.loads(data, object_pairs_hook=unique)
    except (ValueError, UnicodeError) as exc:
        raise SessionRequestError("invalid JSON render artifact", "artifact_mismatch") from exc


def _png_chunk(kind: bytes, data: bytes) -> bytes:
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)


def _rgba_png(pixels: bytes, frame: dict[str, Any]) -> bytes:
    width, height, stride = frame["width"], frame["height"], frame["stride"]
    scanlines = b"".join(
        b"\x00" + pixels[row * stride : row * stride + width * 4]
        for row in range(height)
    )
    header = b"\x89PNG\r\n\x1a\n"
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return header + _png_chunk(b"IHDR", ihdr) + _png_chunk(b"IDAT", zlib.compress(scanlines)) + _png_chunk(b"IEND", b"")


def _require_file(variable: str, failure_class: str) -> Path:
    value = os.environ.get(variable)
    if not value:
        raise SessionRequestError(f"{variable} is required for render_aex", failure_class)
    path = Path(value).expanduser().resolve()
    if not path.is_file():
        raise SessionRequestError(f"{variable} does not identify a file", failure_class)
    return path


def _argb_to_rgba(argb: bytes) -> bytes:
    rgba = bytearray(len(argb))
    for offset in range(0, len(argb), 4):
        rgba[offset : offset + 4] = argb[offset + 1 : offset + 4] + argb[offset : offset + 1]
    return bytes(rgba)


def _invoke_harness(command: list[str], environment: dict[str, str], timeout_ms: int) -> bytes:
    global _ACTIVE_HARNESS_PID
    with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
        try:
            process = subprocess.Popen(
                command, stdout=stdout, stderr=stderr,
                env=environment, start_new_session=True,
            )
        except OSError as exc:
            raise SessionRequestError("harness could not start", "worker_crash") from exc
        _ACTIVE_HARNESS_PID = process.pid
        try:
            process.wait(timeout=timeout_ms / 1000)
        except subprocess.TimeoutExpired as exc:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()
            raise SessionRequestError("AEX render timed out", "session_timeout") from exc
        finally:
            _ACTIVE_HARNESS_PID = None
        if process.returncode != 0:
            raise SessionRequestError("harness or AEX render failed", "worker_failure")
        stdout.seek(0)
        packet = stdout.read(MAX_RENDER_METADATA_BYTES + 1)
        if len(packet) > MAX_RENDER_METADATA_BYTES:
            raise SessionRequestError("fixture report exceeds bounded size", "artifact_mismatch")
        return packet


def _checked_artifact(
    case_dir: Path, stage: str, width: int, height: int, plugin_sha: str, case_identity: dict[str, Any]
) -> tuple[bytes, dict[str, Any]]:
    artifact = (case_dir / stage).resolve()
    if not artifact.is_relative_to(case_dir):
        raise SessionRequestError("render artifact path escapes fixture case", "artifact_mismatch")
    for name in ("output.bin", "output.json"):
        if not (artifact / name).resolve().is_relative_to(artifact):
            raise SessionRequestError("render artifact file escapes fixture case", "artifact_mismatch")
    expected_size = width * height * 4
    try:
        with (artifact / "output.bin").open("rb") as stream:
            raw = stream.read(expected_size + 1)
        if len(raw) != expected_size:
            raise SessionRequestError("render artifact size mismatch", "artifact_mismatch")
        with (artifact / "output.json").open("rb") as stream:
            metadata_bytes = stream.read(MAX_RENDER_METADATA_BYTES + 1)
        if len(metadata_bytes) > MAX_RENDER_METADATA_BYTES:
            raise SessionRequestError("render artifact metadata exceeds bounded size", "artifact_mismatch")
        metadata = _strict_json(metadata_bytes)
    except OSError as exc:
        raise SessionRequestError("render artifact is missing", "artifact_mismatch") from exc
    if (
        not isinstance(metadata, dict)
        or metadata.get("schema") != "aexcompat.render_raw"
        or metadata.get("width") != width
        or metadata.get("height") != height
        or metadata.get("pixel_format") != "argb8"
        or metadata.get("channel_order") != "ARGB"
        or metadata.get("rowbytes") != width * 4
        or metadata.get("data_size_bytes") != expected_size
        or metadata.get("data_sha256") != _sha256(raw)
        or metadata.get("data_file") != "output.bin"
    ):
        raise SessionRequestError("render artifact geometry or checksum mismatch", "artifact_mismatch")
    identity = metadata.get("comparison_identity")
    if (
        not isinstance(identity, dict)
        or identity.get("plugin_sha256") != plugin_sha
        or identity.get("fixture_case") != case_identity
        or identity.get("pixel_format") != "argb8"
        or identity.get("render_path") != "classic"
    ):
        raise SessionRequestError("render artifact identity mismatch", "artifact_mismatch")
    return raw, metadata


def _render_aex(
    request: dict[str, Any], pixels: bytes, frame: dict[str, Any], source_relative_path: str | None
) -> dict[str, Any]:
    if sys.platform != "darwin":
        raise SessionRequestError("render_aex requires macOS", "unsupported_platform")
    if source_relative_path is None:
        raise SessionRequestError("plugin.source_relative_path is required", "aex_not_loaded")
    if frame["width"] > 4096 or frame["height"] > 4096:
        raise SessionRequestError("render dimensions exceed harness limit")
    if frame["stride"] != frame["width"] * 4:
        raise SessionRequestError("render_aex requires packed RGBA8 rows")
    if frame["alpha"] != "straight":
        raise SessionRequestError("render_aex currently requires straight alpha")
    timing = frame["frame_time"]
    seconds = timing.get("seconds") if isinstance(timing, dict) else None
    if isinstance(seconds, bool) or not isinstance(seconds, (int, float)) or not 0 <= seconds <= 3600:
        raise SessionRequestError("frame_time.seconds must be finite and in 0..3600")
    ticks = round(seconds * 1000)
    timeout_ms = request.get("timeout_ms", MAX_RENDER_TIMEOUT_MS)
    if isinstance(timeout_ms, bool) or not isinstance(timeout_ms, int) or not 1000 <= timeout_ms <= MAX_RENDER_TIMEOUT_MS:
        raise SessionRequestError("timeout_ms is outside the bounded render range")
    harness = _require_file("AEXCOMPAT_HARNESS", "worker_unavailable")
    worker = _require_file("AEXCOMPAT_GUEST_WORKER", "worker_unavailable")
    root_value = os.environ.get("AEXCOMPAT_PLUGIN_ROOT")
    if not root_value:
        raise SessionRequestError("AEXCOMPAT_PLUGIN_ROOT is required for render_aex", "aex_not_loaded")
    root = Path(root_value).expanduser().resolve()
    if not root.is_dir():
        raise SessionRequestError("AEXCOMPAT_PLUGIN_ROOT is not a directory", "aex_not_loaded")
    plugin = (root / source_relative_path).resolve()
    if not plugin.is_relative_to(root) or not plugin.is_file():
        raise SessionRequestError("AEX is missing or escapes the configured root", "aex_not_loaded")
    plugin_sha = _sha256_file(plugin)
    harness_sha_before = _sha256_file(harness)
    worker_sha_before = _sha256_file(worker)
    fixture = {
        "schema": "aexcompat.render_fixture", "schema_version": 2,
        "primary_layer": "input.png", "parameters": [], "matrix": [],
        "pixel_format": "argb8", "render_path": "classic",
        "premultiplication": "straight",
        "timing": {"current_time": ticks, "time_step": 1, "total_time": max(1, ticks), "time_scale": 1000},
        "final_artifact": "raw",
        "checkpoints": [{"id": "input", "stage": "classic-input"}],
    }
    with tempfile.TemporaryDirectory(prefix="aexcompat-blender-") as temporary:
        scratch = Path(temporary)
        (scratch / "input.png").write_bytes(_rgba_png(pixels, frame))
        fixture_bytes = json.dumps(fixture, sort_keys=True, separators=(",", ":")).encode("utf-8")
        (scratch / "fixture.json").write_bytes(fixture_bytes)
        environment = os.environ.copy()
        environment["AEXCOMPAT_GUEST_WORKER"] = str(worker)
        stdout = _invoke_harness(
            [str(harness), "--headless", "--render-fixture", str(plugin), str(scratch / "fixture.json"), str(scratch / "output")],
            environment, timeout_ms,
        )
        report = _strict_json(stdout)
        if (
            not isinstance(report, dict)
            or report.get("schema") != "aexcompat.render_fixture_report"
            or report.get("schema_version") != 2
            or report.get("complete") is not True
            or report.get("fixture_sha256") != _sha256(fixture_bytes)
            or not isinstance(report.get("cases"), list)
            or len(report["cases"]) != 1
        ):
            raise SessionRequestError("invalid fixture report", "artifact_mismatch")
        case = report["cases"][0]
        identity = case.get("case_identity") if isinstance(case, dict) else None
        if (
            not isinstance(identity, dict)
            or identity.get("plugin_sha256") != plugin_sha
            or identity.get("fixture_sha256") != _sha256(fixture_bytes)
            or identity.get("case_index") != 0
            or identity.get("pixel_format") != "argb8"
            or identity.get("render_path") != "classic"
            or not isinstance(identity.get("sha256"), str)
            or len(identity["sha256"]) != 64
            or any(character not in "0123456789abcdef" for character in identity["sha256"])
            or case.get("artifact_directory") != "cases/" + identity["sha256"]
        ):
            raise SessionRequestError("invalid fixture case identity", "artifact_mismatch")
        output_root = (scratch / "output").resolve()
        case_dir = (output_root / case["artifact_directory"]).resolve()
        if not case_dir.is_relative_to(output_root):
            raise SessionRequestError("fixture case path escapes output", "artifact_mismatch")
        raw_input, input_meta = _checked_artifact(case_dir, "checkpoints/input", frame["width"], frame["height"], plugin_sha, identity)
        raw_output, output_meta = _checked_artifact(case_dir, "final", frame["width"], frame["height"], plugin_sha, identity)
        case_report = case.get("report")
        if (
            not isinstance(case_report, dict)
            or case_report.get("schema") != "aexcompat.render_fixture_report"
            or case_report.get("schema_version") != 1
            or case_report.get("pixel_format") != "argb8"
            or case_report.get("render_path") != "classic"
            or case_report.get("final_artifact") != output_meta
            or case_report.get("checkpoints") != {"input": input_meta}
        ):
            raise SessionRequestError("fixture report differs from written artifacts", "artifact_mismatch")
        for raw, metadata in ((raw_input, input_meta), (raw_output, output_meta)):
            comparison = metadata["comparison_identity"]
            if (
                metadata.get("premultiplication") != "straight"
                or comparison.get("timing") != fixture["timing"]
                or comparison.get("world_sha256") != _sha256(raw)
            ):
                raise SessionRequestError("render artifact timing or world identity mismatch", "artifact_mismatch")
        packed_input = b"".join(pixels[row * frame["stride"] : row * frame["stride"] + frame["width"] * 4] for row in range(frame["height"]))
        if _argb_to_rgba(raw_input) != packed_input:
            raise SessionRequestError("harness input differs from Blender input", "artifact_mismatch")
        if input_meta["comparison_identity"].get("input_sha256") != _sha256(raw_input):
            raise SessionRequestError("harness input identity mismatch", "artifact_mismatch")
        if output_meta["comparison_identity"].get("input_sha256") != _sha256(raw_input):
            raise SessionRequestError("harness output input identity mismatch", "artifact_mismatch")
        output_pixels = _argb_to_rgba(raw_output)
    plugin_sha_after = _sha256_file(plugin)
    harness_sha_after = _sha256_file(harness)
    worker_sha_after = _sha256_file(worker)
    files_unchanged = (
        plugin_sha == plugin_sha_after
        and harness_sha_before == harness_sha_after
        and worker_sha_before == worker_sha_after
    )
    return {
        "schema_version": SCHEMA_VERSION, "response_kind": "aexcompat_blender_session_result",
        "status": "rendered", "failure_class": "none",
        "aex_render_performed": True, "host_success": True,
        "worker_identity": _worker_identity(),
        "render_identity": {
            "harness_sha256": harness_sha_before,
            "guest_worker_sha256": worker_sha_before,
            "post_run": {
                "plugin_sha256": plugin_sha_after,
                "harness_sha256": harness_sha_after,
                "guest_worker_sha256": worker_sha_after,
            },
            "files_unchanged": files_unchanged,
        },
        "plugin_identity": {"state": "loaded", "source_relative_path": source_relative_path, "sha256": plugin_sha},
        "frame": frame,
        "input": {"sha256": _sha256(pixels), "bytes": len(pixels)},
        "output": {"encoding": "base64-rgba8", "data": base64.b64encode(output_pixels).decode("ascii"), "sha256": _sha256(output_pixels), "bytes": len(output_pixels)},
        "diff": {"byte_count": len(output_pixels), "changed_bytes": sum(a != b for a, b in zip(packed_input, output_pixels)), "max_abs_delta": max((abs(a - b) for a, b in zip(packed_input, output_pixels)), default=0)},
        "diagnostics": [
            "real AEX rendered through macOS fixture harness; frame count=1",
            "file hashes changed during render; stable file identity is unverified"
            if not files_unchanged else "file hashes unchanged before and after render",
        ],
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
    if mode == "render_aex":
        return _render_aex(request, pixels, frame, source_relative_path)
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
    if sys.platform == "darwin":
        def terminate_active_harness(_signal: int, _frame: Any) -> None:
            if _ACTIVE_HARNESS_PID is not None:
                try:
                    os.killpg(_ACTIVE_HARNESS_PID, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            raise SystemExit(143)

        signal.signal(signal.SIGTERM, terminate_active_harness)
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

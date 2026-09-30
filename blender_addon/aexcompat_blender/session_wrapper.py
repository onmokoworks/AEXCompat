"""Bounded JSONL session wrapper used by the Blender addon.

The real render mode delegates to the existing macOS fixture harness.  Fixture
transport modes remain explicitly separate from an AEX render.
"""

from __future__ import annotations

import base64
import hashlib
import json
import math
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


def _finite_number(value: Any) -> bool:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        return False
    try:
        return math.isfinite(float(value))
    except OverflowError:
        return False


def _point_components(value: Any) -> list[float] | None:
    if not isinstance(value, list) or len(value) != 2 or any(
        not _finite_number(component) or not -32768 <= float(component) <= 32768
        for component in value
    ):
        return None
    return [float(component) for component in value]


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
        return json.loads(
            data, object_pairs_hook=unique,
            parse_constant=lambda _value: (_ for _ in ()).throw(ValueError("non-finite JSON number")),
        )
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


def _invoke_harness(
    command: list[str], environment: dict[str, str], timeout_ms: int | None,
    failure_class: str = "worker_failure",
) -> bytes:
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
            process.wait(timeout=timeout_ms / 1000 if timeout_ms is not None else None)
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
            raise SessionRequestError("harness command failed", failure_class)
        stdout.seek(0)
        packet = stdout.read(MAX_RENDER_METADATA_BYTES + 1)
        if len(packet) > MAX_RENDER_METADATA_BYTES:
            raise SessionRequestError("fixture report exceeds bounded size", "artifact_mismatch")
        return packet


def _checked_artifact(
    case_dir: Path, stage: str, width: int, height: int, plugin_sha: str,
    case_identity: dict[str, Any], render_path: str = "classic",
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
        or identity.get("render_path") != render_path
    ):
        raise SessionRequestError("render artifact identity mismatch", "artifact_mismatch")
    return raw, metadata


_DESCRIPTION_PARAMETER_KEYS = {
    "slot", "name", "kind", "minimum", "maximum", "value", "choices", "color",
    "components", "component_count", "layer_path", "enabled", "visible", "supervised",
    "debug_summary", "custom_ui_events", "control_size",
}
_DESCRIPTION_KINDS = {
    "layer", "integer", "float", "angle", "color", "point", "custom", "no_data",
    "arbitrary_data", "path", "group_start", "group_end", "button", "point3d",
}


def _valid_sha256(value: Any) -> bool:
    return isinstance(value, str) and len(value) == 64 and all(character in "0123456789abcdef" for character in value)


def _valid_description_parameter(parameter: Any) -> bool:
    if not isinstance(parameter, dict) or set(parameter) != _DESCRIPTION_PARAMETER_KEYS:
        return False
    color, components, control_size = parameter["color"], parameter["components"], parameter["control_size"]
    return (
        type(parameter["slot"]) is int and parameter["slot"] >= 1
        and isinstance(parameter["name"], str)
        and isinstance(parameter["kind"], str) and parameter["kind"] in _DESCRIPTION_KINDS
        and all(_finite_number(parameter[key]) for key in ("minimum", "maximum", "value"))
        and isinstance(parameter["choices"], list)
        and all(isinstance(choice, str) for choice in parameter["choices"])
        and isinstance(color, list) and len(color) == 4
        and all(type(channel) is int and 0 <= channel <= 255 for channel in color)
        and isinstance(components, list) and len(components) == 3
        and all(_finite_number(component) for component in components)
        and type(parameter["component_count"]) is int and 0 <= parameter["component_count"] <= 3
        and parameter["layer_path"] is None
        and all(type(parameter[key]) is bool for key in ("enabled", "visible", "supervised"))
        and parameter["debug_summary"] is None
        and type(parameter["custom_ui_events"]) is int and parameter["custom_ui_events"] >= 0
        and isinstance(control_size, list) and len(control_size) == 2
        and all(type(size) is int and 0 <= size <= 65535 for size in control_size)
    )


def _valid_description(description: Any) -> bool:
    if not isinstance(description, dict) or set(description) != {
        "schema", "schema_version", "plugin_identity", "parameters", "defaults",
    }:
        return False
    identity = description["plugin_identity"]
    if not isinstance(identity, dict) or set(identity) != {"sha256", "post_setup_sha256", "files_unchanged"}:
        return False
    post_setup = identity["post_setup_sha256"]
    if (
        description["schema"] != "aexcompat.macos_aex_description"
        or type(description["schema_version"]) is not int or description["schema_version"] != 1
        or not _valid_sha256(identity["sha256"])
        or (post_setup is not None and not _valid_sha256(post_setup))
        or type(identity["files_unchanged"]) not in (bool, type(None))
    ):
        return False
    for key in ("parameters", "defaults"):
        records = description[key]
        if not isinstance(records, list) or len(records) > 256 or not all(
            _valid_description_parameter(record) for record in records
        ):
            return False
    return True


def _resolve_parameter_override(
    override: Any, harness: Path, plugin: Path, worker: Path,
    plugin_sha: str, timeout_ms: int,
) -> tuple[list[dict[str, Any]], dict[str, Any] | None]:
    if override is None:
        return [], None
    if not isinstance(override, dict) or set(override) != {"slot", "value"}:
        raise SessionRequestError("legacy parameter_override requires slot and scalar value")
    parameters, records = _resolve_parameter_overrides(
        [override], harness, plugin, worker, plugin_sha, timeout_ms, require_visible=False,
    )
    return parameters, records[0]


def _resolve_parameter_overrides(
    overrides: Any, harness: Path, plugin: Path, worker: Path,
    plugin_sha: str, timeout_ms: int, *, require_visible: bool = True,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    if not isinstance(overrides, list) or not 1 <= len(overrides) <= 16:
        raise SessionRequestError("parameter_overrides requires 1..16 entries")
    seen: set[int] = set()
    checked: list[tuple[int, str, float | list[int] | list[float]]] = []
    for override in overrides:
        if not isinstance(override, dict) or set(override) not in (
            {"slot", "value"}, {"slot", "color"}, {"slot", "components"},
        ):
            raise SessionRequestError("parameter override requires slot and one typed value")
        slot = _require_int(override["slot"], "parameter_override.slot")
        if slot in seen:
            raise SessionRequestError("parameter_overrides has duplicate slots")
        seen.add(slot)
        if "color" in override:
            color = override["color"]
            if not isinstance(color, list) or len(color) != 4 or any(type(channel) is not int or not 0 <= channel <= 255 for channel in color):
                raise SessionRequestError("parameter_override.color must be four ARGB8 channels")
            checked.append((slot, "color", color))
            continue
        if "components" in override:
            components = _point_components(override["components"])
            if components is None:
                raise SessionRequestError("parameter_override.components requires two bounded finite coordinates")
            checked.append((slot, "components", components))
            continue
        value = override["value"]
        if isinstance(value, bool) or not isinstance(value, (int, float)):
            raise SessionRequestError("parameter_override.value must be finite")
        try:
            value = float(value)
        except OverflowError as exc:
            raise SessionRequestError("parameter_override.value must be finite") from exc
        if not math.isfinite(value):
            raise SessionRequestError("parameter_override.value must be finite")
        checked.append((slot, "value", value))
    raw, description = _read_description(harness, plugin, worker, timeout_ms)
    identity = description["plugin_identity"]
    catalog = description["parameters"]
    staged_sha = identity["sha256"]
    parameters: list[dict[str, Any]] = []
    records: list[dict[str, Any]] = []
    for slot, field, value in checked:
        matches = [parameter for parameter in catalog if parameter["slot"] == slot]
        if len(matches) != 1:
            raise SessionRequestError("parameter_override.slot is not uniquely editable")
        parameter = dict(matches[0])
        kind = parameter["kind"]
        if kind not in {"integer", "float", "angle", "color", "point"} or require_visible and (
            not parameter["enabled"] or not parameter["visible"]
        ):
            raise SessionRequestError("parameter_override.kind is not editable")
        if field == "color":
            if kind != "color":
                raise SessionRequestError("parameter_override.color requires color parameter")
            parameter["color"] = value
        elif field == "components":
            if kind != "point" or parameter["component_count"] != 2:
                raise SessionRequestError("parameter_override.components requires two-component point parameter")
            parameter["components"] = [*value, parameter["components"][2]]
        elif kind in {"color", "point"}:
            raise SessionRequestError("parameter_override.value requires scalar parameter")
        elif kind == "angle":
            if not -32768 <= value <= 32768 or parameter["component_count"] != 1:
                raise SessionRequestError("parameter_override.angle is outside the supported range")
            parameter["components"] = [float(value), *parameter["components"][1:]]
        else:
            minimum, maximum = float(parameter["minimum"]), float(parameter["maximum"])
            if minimum > maximum:
                raise SessionRequestError("invalid scalar descriptor", "parameter_description_error")
            if not minimum <= value <= maximum or (kind == "integer" and (
                not value.is_integer() or not -(2**31) <= value < 2**31
            )):
                raise SessionRequestError("parameter_override.value is outside the declared range")
            parameter["value"] = int(value) if kind == "integer" else float(value)
        parameters.append(parameter)
        records.append({
            "slot": slot, "kind": kind,
            **({field: value} if field != "value" else {"value": float(value)}),
            "description_sha256": _sha256(raw),
            "description_plugin_sha256": staged_sha,
            "description_files_unchanged": identity["files_unchanged"],
            "description_matches_render_plugin": staged_sha == plugin_sha,
        })
    return parameters, records


def _popup_choices(parameter: dict[str, Any]) -> list[dict[str, int | str]]:
    """Only label a popup when its declared integer range matches every choice."""
    if parameter["kind"] != "integer":
        return []
    labels = parameter["choices"]
    minimum, maximum = parameter["minimum"], parameter["maximum"]
    if (
        not 2 <= len(labels) <= 16
        or not float(minimum).is_integer() or not float(maximum).is_integer()
        or not -(2**31) <= minimum <= maximum < 2**31
        or maximum - minimum + 1 != len(labels)
        or any(not label or label != label.strip() or "\x00" in label for label in labels)
        or len(set(labels)) != len(labels)
    ):
        return []
    return [{"value": int(minimum) + index, "label": label} for index, label in enumerate(labels)]


def _read_description(
    harness: Path, plugin: Path, worker: Path, timeout_ms: int | None,
) -> tuple[bytes, dict[str, Any]]:
    environment = os.environ.copy()
    environment["AEXCOMPAT_GUEST_WORKER"] = str(worker)
    try:
        raw = _invoke_harness(
            [str(harness), "--headless", "--describe-aex", str(plugin)],
            environment, timeout_ms, "parameter_description_error",
        )
    except SessionRequestError as exc:
        if exc.failure_class == "artifact_mismatch":
            raise SessionRequestError("AEX parameter description exceeds bounded size", "parameter_description_error") from exc
        raise
    try:
        description = _strict_json(raw)
    except SessionRequestError as exc:
        raise SessionRequestError("invalid AEX parameter description", "parameter_description_error") from exc
    if not _valid_description(description):
        raise SessionRequestError("invalid AEX parameter description", "parameter_description_error")
    return raw, description


def _describe_aex(request: dict[str, Any], source_relative_path: str | None) -> dict[str, Any]:
    if sys.platform != "darwin":
        raise SessionRequestError("describe_aex requires macOS", "unsupported_platform")
    if source_relative_path is None:
        raise SessionRequestError("plugin.source_relative_path is required", "aex_not_loaded")
    harness = _require_file("AEXCOMPAT_HARNESS", "worker_unavailable")
    worker = _require_file("AEXCOMPAT_GUEST_WORKER", "worker_unavailable")
    root_value = os.environ.get("AEXCOMPAT_PLUGIN_ROOT")
    if not root_value:
        raise SessionRequestError("AEXCOMPAT_PLUGIN_ROOT is required", "aex_not_loaded")
    root = Path(root_value).expanduser().resolve()
    if not root.is_dir():
        raise SessionRequestError("AEXCOMPAT_PLUGIN_ROOT is not a directory", "aex_not_loaded")
    plugin = (root / source_relative_path).resolve()
    if not plugin.is_relative_to(root) or not plugin.is_file():
        raise SessionRequestError("AEX is missing or escapes the configured root", "aex_not_loaded")
    source_sha_before = _sha256_file(plugin)
    harness_sha_before = _sha256_file(harness)
    worker_sha_before = _sha256_file(worker)
    raw, description = _read_description(harness, plugin, worker, None)
    parameters = []
    slots: set[int] = set()
    for record in description["parameters"]:
        if record["slot"] in slots:
            raise SessionRequestError("ambiguous parameter slot", "parameter_description_error")
        slots.add(record["slot"])
        kind = record["kind"]
        if kind not in {"integer", "float", "angle", "color", "point"} or not record["enabled"] or not record["visible"]:
            continue
        if kind == "color":
            parameters.append({
                "slot": record["slot"], "name": record["name"], "kind": kind,
                "color": record["color"],
            })
            continue
        if kind == "point":
            components = _point_components(record["components"][:2]) if record["component_count"] == 2 else None
            if components is None:
                raise SessionRequestError("invalid point parameter default", "parameter_description_error")
            parameters.append({
                "slot": record["slot"], "name": record["name"], "kind": kind,
                "components": components,
            })
            continue
        minimum, maximum = (
            (-32768.0, 32768.0) if kind == "angle"
            else (float(record["minimum"]), float(record["maximum"]))
        )
        if minimum > maximum or (kind == "angle" and record["component_count"] != 1):
            raise SessionRequestError("invalid scalar parameter bounds", "parameter_description_error")
        value = float(record["components"][0] if kind == "angle" else record["value"])
        if not minimum <= value <= maximum or kind == "integer" and (
            not value.is_integer() or not -(2**31) <= value < 2**31
        ):
            raise SessionRequestError("invalid scalar parameter default", "parameter_description_error")
        entry = {
            "slot": record["slot"], "name": record["name"], "kind": kind,
            "minimum": minimum, "maximum": maximum, "value": value,
        }
        choices = _popup_choices(record)
        if choices:
            entry["choices"] = choices
        parameters.append(entry)
    source_sha_after = _sha256_file(plugin)
    harness_sha_after = _sha256_file(harness)
    worker_sha_after = _sha256_file(worker)
    return {
        "schema_version": SCHEMA_VERSION, "response_kind": "aexcompat_blender_session_result",
        "status": "described", "failure_class": "none", "aex_render_performed": False,
        "host_success": True, "worker_identity": _worker_identity(),
        "plugin_identity": {"state": "loaded", "source_relative_path": source_relative_path, "sha256": source_sha_before},
        "parameter_catalog": parameters,
        "description_identity": {
            "description_sha256": _sha256(raw),
            "description_plugin_sha256": description["plugin_identity"]["sha256"],
            "description_files_unchanged": description["plugin_identity"]["files_unchanged"],
            "description_matches_source": description["plugin_identity"]["sha256"] == source_sha_before,
            "source_sha256_after": source_sha_after,
            "harness_sha256": harness_sha_before,
            "harness_sha256_after": harness_sha_after,
            "guest_worker_sha256": worker_sha_before,
            "guest_worker_sha256_after": worker_sha_after,
            "files_unchanged": source_sha_before == source_sha_after and harness_sha_before == harness_sha_after and worker_sha_before == worker_sha_after,
        },
    }


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
    render_path = request.get("render_path", "classic")
    if render_path not in {"classic", "smart"}:
        raise SessionRequestError("render_path must be classic or smart")
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
    if "parameter_override" in request and "parameter_overrides" in request:
        raise SessionRequestError("choose one parameter override form")
    applied_overrides = None
    if "parameter_overrides" in request:
        parameters, applied_overrides = _resolve_parameter_overrides(
            request["parameter_overrides"], harness, plugin, worker, plugin_sha, timeout_ms,
        )
        applied_override = None
    else:
        parameters, applied_override = _resolve_parameter_override(
            request.get("parameter_override"), harness, plugin, worker, plugin_sha, timeout_ms,
        )
    artifact_render_path = "smartfx" if render_path == "smart" else "classic"
    fixture = {
        "schema": "aexcompat.render_fixture", "schema_version": 2,
        "primary_layer": "input.png", "parameters": parameters, "matrix": [],
        "pixel_format": "argb8", "render_path": render_path,
        "premultiplication": "straight",
        "timing": {"current_time": ticks, "time_step": 1, "total_time": max(1, ticks), "time_scale": 1000},
        "final_artifact": "raw",
        "checkpoints": [{"id": "input", "stage": f"{render_path}-input"}],
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
            or identity.get("render_path") != render_path
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
        raw_input, input_meta = _checked_artifact(case_dir, "checkpoints/input", frame["width"], frame["height"], plugin_sha, identity, artifact_render_path)
        raw_output, output_meta = _checked_artifact(case_dir, "final", frame["width"], frame["height"], plugin_sha, identity, artifact_render_path)
        case_report = case.get("report")
        if (
            not isinstance(case_report, dict)
            or case_report.get("schema") != "aexcompat.render_fixture_report"
            or case_report.get("schema_version") != 1
            or case_report.get("pixel_format") != "argb8"
            or case_report.get("render_path") != render_path
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
    response = {
        "schema_version": SCHEMA_VERSION, "response_kind": "aexcompat_blender_session_result",
        "status": "rendered", "failure_class": "none",
        "render_path": render_path,
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
    if applied_override is not None:
        response["parameter_override"] = applied_override
    if applied_overrides is not None:
        response["parameter_overrides"] = applied_overrides
    return response


def build_response(request: dict[str, Any]) -> dict[str, Any]:
    if not isinstance(request, dict) or request.get("schema_version") != SCHEMA_VERSION:
        raise SessionRequestError("schema_version 1 is required")
    if request.get("request_kind") != "aexcompat_blender_session":
        raise SessionRequestError("request_kind is not supported")
    plugin = request.get("plugin", {})
    if not isinstance(plugin, dict):
        raise SessionRequestError("plugin must be an object")
    source_relative_path = _validate_relative_plugin_path(plugin.get("source_relative_path"))
    mode = request.get("mode", "identity_no_aex")
    if mode == "describe_aex":
        if any(key in request for key in ("frame", "input", "output", "parameter_override", "parameter_overrides", "render_path", "timeout_ms")):
            raise SessionRequestError("describe_aex accepts only plugin")
        return _describe_aex(request, source_relative_path)
    pixels, frame = _decode_rgba8(request)
    if mode == "render_aex":
        return _render_aex(request, pixels, frame, source_relative_path)
    if "parameter_override" in request or "parameter_overrides" in request or "render_path" in request:
        raise SessionRequestError("AEX parameters and render_path require render_aex")
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

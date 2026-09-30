"""AEXCompat Blender compositor adapter.

Blender has no public ``CompositorNodeOFX`` type in the supported versions.
This addon therefore exposes an explicit, public-API Python node and uses an
out-of-process JSONL session wrapper for bounded RGBA8 transport checks.
"""

from __future__ import annotations

import base64
import hashlib
import json
import math
import os
import signal
import subprocess
import sys
from pathlib import Path
from typing import Any

import bpy
from bpy.props import BoolProperty, CollectionProperty, EnumProperty, FloatProperty, IntProperty, IntVectorProperty, StringProperty


ADDON_VERSION = (1, 0, 0)
SCHEMA_VERSION = 1


class AEXCompatSessionError(RuntimeError):
    """A bounded session did not produce an explicitly valid response."""


def _sha256_text(value: Any) -> bool:
    return isinstance(value, str) and len(value) == 64 and all(character in "0123456789abcdef" for character in value)


def _finite_scalar(value: Any) -> bool:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        return False
    try:
        return math.isfinite(float(value))
    except OverflowError:
        return False


def _argb8(value: Any) -> bool:
    return isinstance(value, list) and len(value) == 4 and all(
        type(channel) is int and 0 <= channel <= 255 for channel in value
    )


def _point2(value: Any) -> bool:
    return isinstance(value, list) and len(value) == 2 and all(
        _finite_scalar(component) and -32768 <= float(component) <= 32768
        for component in value
    )


def _valid_group_path(value: Any) -> bool:
    return isinstance(value, list) and 1 <= len(value) <= 8 and all(
        isinstance(label, str) and 1 <= len(label) <= 128
        and label == label.strip() and "\x00" not in label
        for label in value
    )


def _stored_group_path(value: str) -> list[str]:
    try:
        path = json.loads(value)
    except (TypeError, ValueError):
        return []
    return path if _valid_group_path(path) else []


def _parameter_label(item: Any) -> str:
    return " / ".join([*_stored_group_path(item.group_path_json), item.name])


def _point_text_values(x: str, y: str) -> list[float]:
    try:
        components = [float(x), float(y)]
    except (ValueError, OverflowError) as exc:
        raise AEXCompatSessionError("point must contain two bounded finite coordinates") from exc
    if not _point2(components):
        raise AEXCompatSessionError("point must contain two bounded finite coordinates")
    return components


def _valid_choice_mapping(choices: Any, minimum: Any, maximum: Any) -> bool:
    if (
        not isinstance(choices, list) or not 2 <= len(choices) <= 16
        or not _finite_scalar(minimum) or not _finite_scalar(maximum)
        or not float(minimum).is_integer() or not float(maximum).is_integer()
        or not -(2**31) <= minimum <= maximum < 2**31
        or maximum - minimum + 1 != len(choices)
    ):
        return False
    labels: list[str] = []
    for index, choice in enumerate(choices):
        if (
            not isinstance(choice, dict) or set(choice) != {"value", "label"}
            or type(choice["value"]) is not int or choice["value"] != int(minimum) + index
            or not isinstance(choice["label"], str) or not choice["label"]
            or choice["label"] != choice["label"].strip() or "\x00" in choice["label"]
        ):
            return False
        labels.append(choice["label"])
    return len(set(labels)) == len(labels)


def _stored_choices(raw: str) -> list[dict[str, Any]]:
    try:
        choices = json.loads(raw) if raw else []
    except (TypeError, ValueError):
        return []
    if not isinstance(choices, list) or not choices:
        return []
    minimum = choices[0].get("value") if isinstance(choices[0], dict) else None
    maximum = choices[-1].get("value") if isinstance(choices[-1], dict) else None
    return choices if _valid_choice_mapping(choices, minimum, maximum) else []


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def _wrapper_path() -> Path:
    # Prefer the bundled worker for addon-only or zip installations.
    packaged = Path(__file__).with_name("session_wrapper.py")
    if packaged.is_file():
        return packaged
    return _repo_root() / "tools" / "blender_aexcompat_session.py"


def _python_command() -> list[str]:
    override = os.environ.get("AEXCOMPAT_SESSION_PYTHON")
    if override:
        return [override]
    executable = Path(sys.executable)
    if "blender" not in executable.name.lower():
        return [str(executable)]
    for candidate in ("python", "python3"):
        from shutil import which

        resolved = which(candidate)
        if resolved:
            return [resolved]
    raise AEXCompatSessionError("session_python_unavailable")


def _run_session(request: dict[str, Any], timeout_ms: int | None = 5000) -> dict[str, Any]:
    wrapper = _wrapper_path()
    if not wrapper.is_file():
        raise AEXCompatSessionError("blender_session_wrapper_missing")
    harness_calls = 2 if request.get("mode") == "render_aex" and ("parameter_override" in request or "parameter_overrides" in request) else 1
    try:
        process = subprocess.Popen(
            _python_command() + [str(wrapper)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            cwd=str(wrapper.parent),
            start_new_session=os.name == "posix",
        )
        try:
            stdout, _stderr = process.communicate(
                json.dumps(request, sort_keys=True) + "\n",
                timeout=(max(1, timeout_ms) * harness_calls + 15_000) / 1000.0 if timeout_ms is not None else None,
            )
        except subprocess.TimeoutExpired as exc:
            try:
                process.terminate()
            except ProcessLookupError:
                pass
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                if os.name == "posix":
                    try:
                        os.killpg(process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                else:
                    process.kill()
            process.communicate()
            raise AEXCompatSessionError("session_timeout") from exc
    except OSError as exc:
        raise AEXCompatSessionError("worker_crash") from exc
    if process.returncode != 0 or not stdout.strip():
        raise AEXCompatSessionError("worker_crash")
    def unique(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
        result: dict[str, Any] = {}
        for key, value in pairs:
            if key in result:
                raise AEXCompatSessionError("worker_protocol_error")
            result[key] = value
        return result
    try:
        response = json.loads(
            stdout.splitlines()[-1], object_pairs_hook=unique,
            parse_constant=lambda _value: (_ for _ in ()).throw(ValueError("non-finite JSON number")),
        )
    except (ValueError, UnicodeError, IndexError) as exc:
        raise AEXCompatSessionError("worker_protocol_error") from exc
    if not isinstance(response, dict) or response.get("schema_version") != SCHEMA_VERSION:
        raise AEXCompatSessionError("worker_protocol_error")
    return response


def describe_aex(plugin_source: str) -> dict[str, Any]:
    """Wait for one editable parameter catalog without starting a frame render."""
    response = _run_session({
        "schema_version": SCHEMA_VERSION,
        "request_kind": "aexcompat_blender_session",
        "mode": "describe_aex",
        "plugin": {"source_relative_path": plugin_source},
    }, timeout_ms=None)
    if response.get("status") != "described":
        raise AEXCompatSessionError(str(response.get("failure_class", "parameter_description_error")))
    identity = response.get("plugin_identity")
    worker_identity = response.get("worker_identity")
    detail = response.get("description_identity")
    catalog = response.get("parameter_catalog")
    if (
        response.get("response_kind") != "aexcompat_blender_session_result"
        or response.get("failure_class") != "none"
        or response.get("host_success") is not True
        or response.get("aex_render_performed") is not False
        or not isinstance(worker_identity, dict)
        or set(worker_identity) != {"name", "version", "source_sha256"}
        or worker_identity["name"] != "blender_aexcompat_session"
        or worker_identity["version"] != "1"
        or not _sha256_text(worker_identity["source_sha256"])
        or not isinstance(identity, dict)
        or identity.get("state") != "loaded"
        or identity.get("source_relative_path") != plugin_source
        or not _sha256_text(identity.get("sha256"))
        or not isinstance(detail, dict)
        or not isinstance(catalog, list)
        or len(catalog) > 256
    ):
        raise AEXCompatSessionError("worker_protocol_error")
    if (
        any(not _sha256_text(detail.get(key)) for key in (
            "description_sha256", "description_plugin_sha256", "source_sha256_after",
            "harness_sha256", "harness_sha256_after", "guest_worker_sha256", "guest_worker_sha256_after",
        ))
        or type(detail.get("description_files_unchanged")) not in (bool, type(None))
        or type(detail.get("description_matches_source")) is not bool
        or detail["description_matches_source"] is not (detail["description_plugin_sha256"] == identity["sha256"])
        or type(detail.get("files_unchanged")) is not bool
        or detail["files_unchanged"] is not (
            identity["sha256"] == detail["source_sha256_after"]
            and detail["harness_sha256"] == detail["harness_sha256_after"]
            and detail["guest_worker_sha256"] == detail["guest_worker_sha256_after"]
        )
    ):
        raise AEXCompatSessionError("worker_protocol_error")
    seen: set[int] = set()
    for entry in catalog:
        group_path = entry.get("group_path") if isinstance(entry, dict) else None
        keys = set(entry) - {"group_path"} if isinstance(entry, dict) else set()
        if not isinstance(entry, dict) or (
            type(entry.get("slot")) is not int or entry["slot"] < 1 or entry["slot"] in seen
            or not isinstance(entry.get("name"), str)
            or not isinstance(entry.get("kind"), str)
            or ("group_path" in entry and not _valid_group_path(group_path))
            or (entry.get("kind") == "color" and (
                keys != {"slot", "name", "kind", "color"} or not _argb8(entry.get("color"))
            ))
            or (entry.get("kind") == "point" and (
                keys != {"slot", "name", "kind", "components"} or not _point2(entry.get("components"))
            ))
            or (entry.get("kind") not in {"color", "point"} and (
                keys not in (
                    {"slot", "name", "kind", "minimum", "maximum", "value"},
                    {"slot", "name", "kind", "minimum", "maximum", "value", "choices"},
                )
                or entry.get("kind") not in {"integer", "float", "angle"}
                or any(not _finite_scalar(entry[key]) for key in ("minimum", "maximum", "value"))
                or not entry["minimum"] <= entry["value"] <= entry["maximum"]
                or (entry["kind"] == "integer" and not float(entry["value"]).is_integer())
                or ("choices" in entry and (
                    entry["kind"] != "integer" or not _valid_choice_mapping(
                        entry["choices"], entry["minimum"], entry["maximum"],
                    )
                ))
            ))
        ):
            raise AEXCompatSessionError("worker_protocol_error")
        seen.add(entry["slot"])
    return response


def evaluate_rgba8(
    rgba: bytes,
    width: int,
    height: int,
    *,
    stride: int | None = None,
    frame_time: float = 0.0,
    alpha: str = "straight",
    color_space: str = "scene_linear",
    plugin_source: str | None = None,
    mode: str = "identity_no_aex",
    timeout_ms: int | None = None,
    render_path: str = "classic",
    parameter_override: dict[str, int | float] | None = None,
    parameter_overrides: list[dict[str, Any]] | None = None,
) -> tuple[bytes, dict[str, Any]]:
    """Validate one RGBA8 frame and return only verified worker pixels."""

    width = int(width)
    height = int(height)
    stride = width * 4 if stride is None else int(stride)
    raw = bytes(rgba)
    timeout_ms = (180_000 if mode == "render_aex" else 5000) if timeout_ms is None else timeout_ms
    if isinstance(timeout_ms, bool) or not isinstance(timeout_ms, int) or not 1 <= timeout_ms <= 180_000:
        raise AEXCompatSessionError("blender_addon_error: timeout_ms")
    if width < 1 or height < 1 or stride < width * 4 or len(raw) != stride * height:
        raise AEXCompatSessionError("blender_addon_error")
    request = {
        "schema_version": SCHEMA_VERSION,
        "request_kind": "aexcompat_blender_session",
        "mode": mode,
        "timeout_ms": timeout_ms,
        "plugin": {"source_relative_path": plugin_source},
        "frame": {
            "width": width,
            "height": height,
            "stride": stride,
            "channels": "RGBA8",
            "alpha": alpha,
            "color_space": color_space,
            "frame_time": {"seconds": frame_time},
        },
        "input": {
            "encoding": "base64-rgba8",
            "data": base64.b64encode(raw).decode("ascii"),
            "sha256": hashlib.sha256(raw).hexdigest(),
        },
    }
    if mode == "render_aex":
        if parameter_override is not None and parameter_overrides is not None:
            raise AEXCompatSessionError("choose one parameter override form")
        request["render_path"] = render_path
        if parameter_override is not None:
            request["parameter_override"] = parameter_override
        if parameter_overrides is not None:
            request["parameter_overrides"] = parameter_overrides
    elif parameter_override is not None or parameter_overrides is not None:
        raise AEXCompatSessionError("parameter overrides require render_aex")
    response = _run_session(request, timeout_ms=timeout_ms)
    expected_statuses = {"rendered"} if mode == "render_aex" else {"identity_only", "fixture_transform"}
    expected_success = mode == "render_aex"
    if (
        response.get("status") not in expected_statuses
        or response.get("host_success") is not expected_success
        or response.get("aex_render_performed") is not expected_success
        or (mode == "render_aex" and response.get("failure_class") != "none")
    ):
        raise AEXCompatSessionError(str(response.get("failure_class", "session_failed")))
    if mode != "render_aex" and ("parameter_override" in response or "parameter_overrides" in response):
        raise AEXCompatSessionError("worker_protocol_error")
    if response.get("input") != {"sha256": hashlib.sha256(raw).hexdigest(), "bytes": len(raw)}:
        raise AEXCompatSessionError("worker_protocol_error")
    if mode == "render_aex":
        identity = response.get("plugin_identity")
        render_identity = response.get("render_identity")
        post_run = render_identity.get("post_run") if isinstance(render_identity, dict) else None
        if (
            response.get("render_path") != render_path
            or not isinstance(identity, dict)
            or identity.get("state") != "loaded"
            or identity.get("source_relative_path") != plugin_source
            or not isinstance(identity.get("sha256"), str)
            or len(identity["sha256"]) != 64
            or not isinstance(render_identity, dict)
            or any(not isinstance(render_identity.get(key), str) or len(render_identity[key]) != 64 for key in ("harness_sha256", "guest_worker_sha256"))
            or not isinstance(post_run, dict)
            or any(not isinstance(post_run.get(key), str) or len(post_run[key]) != 64 for key in ("plugin_sha256", "harness_sha256", "guest_worker_sha256"))
            or not isinstance(render_identity.get("files_unchanged"), bool)
            or render_identity["files_unchanged"] is not (
                post_run["plugin_sha256"] == identity["sha256"]
                and post_run["harness_sha256"] == render_identity["harness_sha256"]
                and post_run["guest_worker_sha256"] == render_identity["guest_worker_sha256"]
            )
        ):
            raise AEXCompatSessionError("worker_protocol_error")
        applied = response.get("parameter_override")
        applied_many = response.get("parameter_overrides")
        if parameter_override is None and applied is not None or parameter_overrides is None and applied_many is not None:
            raise AEXCompatSessionError("worker_protocol_error")
        if parameter_override is not None and (
            not isinstance(applied, dict)
            or applied.get("slot") != parameter_override.get("slot")
            or applied.get("value") != parameter_override.get("value")
            or not isinstance(applied.get("kind"), str)
            or applied["kind"] not in {"integer", "float", "angle"}
            or not isinstance(applied.get("description_sha256"), str)
            or len(applied["description_sha256"]) != 64
            or not isinstance(applied.get("description_plugin_sha256"), str)
            or len(applied["description_plugin_sha256"]) != 64
            or type(applied.get("description_files_unchanged")) not in (bool, type(None))
            or applied.get("description_matches_render_plugin") is not (
                applied["description_plugin_sha256"] == identity["sha256"]
            )
        ):
            raise AEXCompatSessionError("worker_protocol_error")
        if parameter_overrides is not None:
            if not isinstance(applied_many, list) or len(applied_many) != len(parameter_overrides):
                raise AEXCompatSessionError("worker_protocol_error")
            shared_description = None
            for requested, recorded in zip(parameter_overrides, applied_many):
                color_requested = isinstance(requested, dict) and "color" in requested
                point_requested = isinstance(requested, dict) and "components" in requested
                typed_value_valid = (
                    _argb8(recorded.get("color")) and recorded.get("color") == requested.get("color")
                    and recorded.get("kind") == "color" and "value" not in recorded and "components" not in recorded
                ) if color_requested and isinstance(recorded, dict) else (
                    _point2(recorded.get("components")) and recorded.get("components") == requested.get("components")
                    and recorded.get("kind") == "point" and "value" not in recorded and "color" not in recorded
                ) if point_requested and isinstance(recorded, dict) else (
                    _finite_scalar(recorded.get("value")) and recorded.get("value") == requested.get("value")
                    and isinstance(recorded.get("kind"), str)
                    and recorded["kind"] in {"integer", "float", "angle"}
                    and "color" not in recorded and "components" not in recorded
                ) if isinstance(requested, dict) and isinstance(recorded, dict) else False
                if (
                    not isinstance(recorded, dict)
                    or not isinstance(requested, dict)
                    or type(recorded.get("slot")) is not int
                    or recorded.get("slot") != requested.get("slot")
                    or not typed_value_valid
                    or not _sha256_text(recorded.get("description_sha256"))
                    or not _sha256_text(recorded.get("description_plugin_sha256"))
                    or type(recorded.get("description_files_unchanged")) not in (bool, type(None))
                    or recorded.get("description_matches_render_plugin") is not (
                        recorded["description_plugin_sha256"] == identity["sha256"]
                    )
                ):
                    raise AEXCompatSessionError("worker_protocol_error")
                description = (
                    recorded["description_sha256"], recorded["description_plugin_sha256"],
                    recorded["description_files_unchanged"],
                )
                if shared_description is not None and description != shared_description:
                    raise AEXCompatSessionError("worker_protocol_error")
                shared_description = description
    output = response.get("output")
    if not isinstance(output, dict) or output.get("encoding") != "base64-rgba8":
        raise AEXCompatSessionError("worker_protocol_error")
    try:
        decoded = base64.b64decode(output["data"], validate=True)
    except Exception as exc:  # pragma: no cover - exact binascii type varies
        raise AEXCompatSessionError("worker_protocol_error") from exc
    if (
        len(decoded) != stride * height
        or output.get("bytes") != len(decoded)
        or output.get("sha256") != hashlib.sha256(decoded).hexdigest()
    ):
        raise AEXCompatSessionError("worker_protocol_error")
    return decoded, response


def _configure_bake_button(node: Any, button: Any) -> None:
    button.source_image_name = node.source_image_name
    button.output_image_name = node.output_image_name
    button.plugin_source = node.plugin_source
    button.mode = node.transport_mode
    button.frame_time = node.frame_time
    button.render_path = node.render_path
    button.parameter_slot = node.parameter_slot
    button.parameter_value = node.parameter_value
    button.parameter_kind = node.parameter_kind
    button.parameter_integer_value = node.parameter_integer_value
    button.parameter_value_text = node.parameter_value_text
    button.parameter_overrides_json = json.dumps([
        {"slot": item.slot, **({"color": list(item.color_argb)} if item.kind == "color" else
            {"components": [item.point_x_text, item.point_y_text]} if item.kind == "point" else {
            "value": item.integer_value if item.kind == "integer" else item.value_text,
        })}
        for item in node.selected_parameter_items
    ], separators=(",", ":")) if node.selected_parameter_items else ""
    button.selected_description_plugin_sha = node.description_plugin_sha


def _parameter_override(slot: int, kind: str, integer_value: int, value_text: str, legacy_value: float) -> dict[str, int | float]:
    if kind == "integer":
        value: int | float = integer_value
    elif kind in {"float", "angle"}:
        try:
            value = float(value_text)
        except ValueError as exc:
            raise AEXCompatSessionError("parameter value must be finite") from exc
        if not math.isfinite(value):
            raise AEXCompatSessionError("parameter value must be finite")
    else:
        value = legacy_value
    return {"slot": slot, "value": value}


def _named_parameter_overrides(node: Any) -> list[dict[str, Any]]:
    return [
        {"slot": item.slot, "color": list(item.color_argb)} if item.kind == "color" else
        {"slot": item.slot, "components": _point_text_values(item.point_x_text, item.point_y_text)} if item.kind == "point" else
        _parameter_override(item.slot, item.kind, item.integer_value, item.value_text, 0.0)
        for item in node.selected_parameter_items
    ]


def _parse_parameter_overrides(value: str) -> list[dict[str, Any]]:
    def unique(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
        result: dict[str, Any] = {}
        for key, item in pairs:
            if key in result:
                raise AEXCompatSessionError("duplicate parameter override field")
            result[key] = item
        return result
    try:
        parsed = json.loads(
            value, object_pairs_hook=unique,
            parse_constant=lambda _value: (_ for _ in ()).throw(ValueError("non-finite number")),
        )
    except (ValueError, UnicodeError) as exc:
        raise AEXCompatSessionError("invalid parameter override list") from exc
    if not isinstance(parsed, list) or not 1 <= len(parsed) <= 16:
        raise AEXCompatSessionError("parameter override list must contain 1..16 values")
    seen: set[int] = set()
    for entry in parsed:
        if (
            not isinstance(entry, dict) or set(entry) not in (
                {"slot", "value"}, {"slot", "color"}, {"slot", "components"},
            )
            or type(entry["slot"]) is not int or entry["slot"] < 1 or entry["slot"] in seen
        ):
            raise AEXCompatSessionError("invalid or duplicate parameter override")
        if "color" in entry:
            if not _argb8(entry["color"]):
                raise AEXCompatSessionError("color must contain four ARGB8 channels")
            seen.add(entry["slot"])
            continue
        if "components" in entry:
            if isinstance(entry["components"], list):
                for index, component in enumerate(entry["components"]):
                    if isinstance(component, str):
                        try:
                            entry["components"][index] = float(component)
                        except (ValueError, OverflowError) as exc:
                            raise AEXCompatSessionError("point must contain two bounded finite coordinates") from exc
            if not _point2(entry["components"]):
                raise AEXCompatSessionError("point must contain two bounded finite coordinates")
            seen.add(entry["slot"])
            continue
        if isinstance(entry["value"], str):
            try:
                entry["value"] = float(entry["value"])
            except ValueError as exc:
                raise AEXCompatSessionError("parameter value must be finite") from exc
        if not _finite_scalar(entry["value"]):
            raise AEXCompatSessionError("parameter value must be finite")
        seen.add(entry["slot"])
    return parsed


def _parameter_slot_changed(node: Any, _context: Any) -> None:
    node.parameter_kind = ""
    node.parameter_value_text = ""
    node.selected_parameter_items.clear()


def _plugin_source_changed(node: Any, _context: Any) -> None:
    node.parameter_items.clear()
    node.selected_parameter_index = 0
    node.parameter_slot = 0
    node.selected_parameter_items.clear()
    node.description_source = ""
    node.description_plugin_sha = ""


class AEXCompatScalarParameter(bpy.types.PropertyGroup):
    slot: IntProperty(name="Slot")
    name: StringProperty(name="Name")
    kind: StringProperty(name="Kind")
    group_path_json: StringProperty(name="Group path", default="")
    minimum_text: StringProperty(name="Minimum")
    maximum_text: StringProperty(name="Maximum")
    value_text: StringProperty(name="Default")
    choices_json: StringProperty(name="Named integer choices", default="")
    color_argb: IntVectorProperty(name="ARGB8 default", size=4, min=0, max=255, default=(255, 0, 0, 0))
    point_x_text: StringProperty(name="Default X", default="")
    point_y_text: StringProperty(name="Default Y", default="")


class AEXCompatChosenParameter(bpy.types.PropertyGroup):
    slot: IntProperty(name="Slot")
    name: StringProperty(name="Name")
    kind: StringProperty(name="Kind")
    group_path_json: StringProperty(name="Group path", default="")
    integer_value: IntProperty(name="Integer value")
    value_text: StringProperty(name="Decimal value")
    choices_json: StringProperty(name="Named integer choices", default="")
    color_argb: IntVectorProperty(name="ARGB8 color", size=4, min=0, max=255, default=(255, 0, 0, 0))
    point_x_text: StringProperty(name="Point X", default="")
    point_y_text: StringProperty(name="Point Y", default="")


class AEXCOMPAT_UL_scalar_parameters(bpy.types.UIList):
    def draw_item(self, _context: Any, layout: Any, _data: Any, item: Any, _icon: Any, _active_data: Any, _active_propname: Any, _index: int) -> None:
        layout.label(text=f"{_parameter_label(item)} (#{item.slot}, {item.kind})")


class AEXCOMPAT_UL_chosen_parameters(bpy.types.UIList):
    def draw_item(self, _context: Any, layout: Any, _data: Any, item: Any, _icon: Any, _active_data: Any, _active_propname: Any, _index: int) -> None:
        row = layout.row(align=True)
        row.label(text=f"{_parameter_label(item)} (#{item.slot})")
        if item.kind == "color":
            row.label(text=f"ARGB {list(item.color_argb)}")
        elif item.kind == "point":
            row.prop(item, "point_x_text", text="X")
            row.prop(item, "point_y_text", text="Y")
        elif choices := _stored_choices(item.choices_json):
            selected = next((choice["label"] for choice in choices if choice["value"] == item.integer_value), str(item.integer_value))
            row.label(text=selected)
        else:
            row.prop(item, "integer_value" if item.kind == "integer" else "value_text", text="")


class AEXCompatCompositorNode(bpy.types.CompositorNode):
    bl_idname = "AEXCompatCompositorNode"
    bl_label = "AEXCompat transport descriptor (8-bit RGBA)"
    bl_icon = "NODE"

    plugin_source: StringProperty(name="Plugin source", default="", update=_plugin_source_changed)
    source_image_name: StringProperty(name="Source image", default="")
    output_image_name: StringProperty(name="Output image", default="AEXCompat Baked")
    transport_mode: EnumProperty(
        name="Transport mode",
        items=(
            ("identity_no_aex", "Identity transport", "No AEX is loaded; validate transport only"),
            ("fixture_invert_no_aex", "Fixture invert", "Test transform only; no AEX is loaded"),
            ("render_aex", "Render AEX (macOS)", "Render one frame through the configured harness"),
        ),
        default="identity_no_aex",
    )
    frame_time: FloatProperty(name="Frame time", default=0.0)
    render_path: EnumProperty(
        name="AEX render path",
        items=(("classic", "Classic", "Classic AEX rendering"), ("smart", "SmartFX", "SmartFX rendering")),
        default="classic",
    )
    parameter_slot: IntProperty(name="Parameter number (0 uses defaults)", default=0, min=0, update=_parameter_slot_changed)
    parameter_value: FloatProperty(name="Parameter value", default=0.0)
    parameter_kind: StringProperty(name="Selected parameter kind", default="")
    parameter_integer_value: IntProperty(name="Integer value", default=0)
    parameter_value_text: StringProperty(name="Decimal value", default="")
    parameter_items: CollectionProperty(type=AEXCompatScalarParameter)
    selected_parameter_items: CollectionProperty(type=AEXCompatChosenParameter)
    selected_override_index: IntProperty(name="Selected override", default=0, min=0)
    selected_parameter_index: IntProperty(name="Selected parameter", default=0, min=0)
    description_source: StringProperty(name="Described plugin source", default="")
    description_plugin_sha: StringProperty(name="Described plugin SHA-256", default="")

    def init(self, _context: Any) -> None:
        self.inputs.new("NodeSocketColor", "Image")
        self.outputs.new("NodeSocketColor", "Image")

    def draw_buttons(self, _context: Any, layout: Any) -> None:
        layout.prop(self, "plugin_source")
        layout.prop(self, "source_image_name")
        layout.prop(self, "output_image_name")
        layout.prop(self, "transport_mode")
        layout.prop(self, "frame_time")
        if self.transport_mode == "render_aex":
            layout.prop(self, "render_path")
            refresh = layout.operator("aexcompat.refresh_parameters", text="Refresh AEX parameters")
            _configure_node_locator(self, refresh)
            if self.parameter_items and self.description_source == self.plugin_source:
                layout.template_list(
                    "AEXCOMPAT_UL_scalar_parameters", "", self, "parameter_items",
                    self, "selected_parameter_index", rows=5,
                )
                index = min(self.selected_parameter_index, len(self.parameter_items) - 1)
                selected = self.parameter_items[index]
                if group := _stored_group_path(selected.group_path_json):
                    layout.label(text="Group: " + " / ".join(group))
                if selected.kind == "color":
                    layout.label(text=f"ARGB8 default {list(selected.color_argb)}")
                elif selected.kind == "point":
                    layout.label(text=f"Default X/Y ({selected.point_x_text}, {selected.point_y_text})")
                else:
                    layout.label(text=f"Range {selected.minimum_text} .. {selected.maximum_text}; default {selected.value_text}")
                    if choices := _stored_choices(selected.choices_json):
                        layout.label(text="Choices: " + ", ".join(choice["label"] for choice in choices))
                choose = layout.operator("aexcompat.choose_parameter", text="Add/update selected parameter")
                _configure_node_locator(self, choose)
            if self.selected_parameter_items:
                layout.template_list(
                    "AEXCOMPAT_UL_chosen_parameters", "", self, "selected_parameter_items",
                    self, "selected_override_index", rows=4,
                )
                chosen = self.selected_parameter_items[min(self.selected_override_index, len(self.selected_parameter_items) - 1)]
                if chosen.kind == "color":
                    layout.prop(chosen, "color_argb", text="ARGB8 (A, R, G, B)")
                elif chosen.kind == "point":
                    row = layout.row(align=True)
                    row.prop(chosen, "point_x_text", text="X")
                    row.prop(chosen, "point_y_text", text="Y")
                elif choices := _stored_choices(chosen.choices_json):
                    for choice in choices:
                        button = layout.operator(
                            "aexcompat.set_popup_choice", text=choice["label"],
                            icon="RADIOBUT_ON" if chosen.integer_value == choice["value"] else "RADIOBUT_OFF",
                        )
                        _configure_node_locator(self, button)
                        button.slot = chosen.slot
                        button.value = choice["value"]
                remove = layout.operator("aexcompat.remove_parameter", text="Remove selected override")
                _configure_node_locator(self, remove)
            else:
                layout.prop(self, "parameter_slot")
            if self.parameter_slot and not self.selected_parameter_items:
                if self.parameter_kind == "integer":
                    layout.prop(self, "parameter_integer_value")
                elif self.parameter_kind in {"float", "angle"}:
                    layout.prop(self, "parameter_value_text")
                else:
                    layout.prop(self, "parameter_value")
        bake = layout.operator("aexcompat.bake_image", text="Bake image through worker")
        _configure_bake_button(self, bake)

    def evaluate_rgba8(self, rgba: bytes, width: int, height: int, **kwargs: Any) -> tuple[bytes, dict[str, Any]]:
        options = dict(kwargs)
        options.setdefault("render_path", self.render_path)
        if (
            self.transport_mode == "render_aex" and (self.parameter_slot or self.selected_parameter_items)
            and "parameter_override" not in options and "parameter_overrides" not in options
        ):
            if self.selected_parameter_items:
                options.setdefault("parameter_overrides", _named_parameter_overrides(self))
            else:
                options.setdefault("parameter_override", _parameter_override(
                    self.parameter_slot, self.parameter_kind, self.parameter_integer_value,
                    self.parameter_value_text, self.parameter_value,
                ))
        return evaluate_rgba8(
            rgba,
            width,
            height,
            frame_time=self.frame_time,
            plugin_source=self.plugin_source or None,
            mode=self.transport_mode,
            **options,
        )


def _configure_node_locator(node: AEXCompatCompositorNode, operator: Any) -> None:
    tree = node.id_data
    operator.tree_name = tree.name
    operator.node_name = node.name
    group = bpy.data.node_groups.get(tree.name)
    if group is not None and group.as_pointer() == tree.as_pointer():
        operator.owner_type = "GROUP"
        operator.owner_name = group.name
        return
    for scene in bpy.data.scenes:
        if scene.node_tree is not None and scene.node_tree.as_pointer() == tree.as_pointer():
            operator.owner_type = "SCENE"
            operator.owner_name = scene.name
            return
    operator.owner_type = "UNKNOWN"


def _find_descriptor_node(owner_type: str, owner_name: str, tree_name: str, node_name: str) -> AEXCompatCompositorNode | None:
    if owner_type == "GROUP":
        tree = bpy.data.node_groups.get(owner_name)
    elif owner_type == "SCENE":
        scene = bpy.data.scenes.get(owner_name)
        tree = scene.node_tree if scene is not None else None
    else:
        return None
    if tree is None or tree.name != tree_name:
        return None
    node = tree.nodes.get(node_name)
    return node if node is not None and node.bl_idname == AEXCompatCompositorNode.bl_idname else None


class AEXCompatRefreshParametersOperator(bpy.types.Operator):
    bl_idname = "aexcompat.refresh_parameters"
    bl_label = "Refresh AEX parameters"

    tree_name: StringProperty()
    node_name: StringProperty()
    owner_type: StringProperty(default="GROUP")
    owner_name: StringProperty(default="")

    def execute(self, _context: Any):
        node = _find_descriptor_node(self.owner_type, self.owner_name or self.tree_name, self.tree_name, self.node_name)
        if node is None or node.transport_mode != "render_aex":
            self.report({"ERROR"}, "AEX node not found")
            return {"CANCELLED"}
        node.parameter_items.clear()
        node.selected_parameter_index = 0
        node.parameter_slot = 0
        node.selected_parameter_items.clear()
        node.description_source = ""
        node.description_plugin_sha = ""
        try:
            response = describe_aex(node.plugin_source)
        except AEXCompatSessionError as exc:
            self.report({"ERROR"}, str(exc))
            return {"CANCELLED"}
        for record in response["parameter_catalog"]:
            item = node.parameter_items.add()
            item.slot = record["slot"]
            item.name = record["name"]
            item.kind = record["kind"]
            item.group_path_json = json.dumps(record["group_path"], ensure_ascii=False) if "group_path" in record else ""
            if record["kind"] == "color":
                item.color_argb = record["color"]
            elif record["kind"] == "point":
                item.point_x_text = str(record["components"][0])
                item.point_y_text = str(record["components"][1])
            else:
                item.minimum_text = str(record["minimum"])
                item.maximum_text = str(record["maximum"])
                item.value_text = str(int(record["value"])) if record["kind"] == "integer" else str(record["value"])
                item.choices_json = json.dumps(record["choices"], ensure_ascii=False) if "choices" in record else ""
        node.description_source = node.plugin_source
        node.description_plugin_sha = response["description_identity"]["description_plugin_sha256"]
        self.report({"INFO"}, f"Found {len(node.parameter_items)} editable parameters")
        return {"FINISHED"}


class AEXCompatChooseParameterOperator(bpy.types.Operator):
    bl_idname = "aexcompat.choose_parameter"
    bl_label = "Use selected AEX parameter"

    tree_name: StringProperty()
    node_name: StringProperty()
    owner_type: StringProperty(default="GROUP")
    owner_name: StringProperty(default="")

    def execute(self, _context: Any):
        node = _find_descriptor_node(self.owner_type, self.owner_name or self.tree_name, self.tree_name, self.node_name)
        if (
            node is None or node.transport_mode != "render_aex"
            or node.description_source != node.plugin_source
            or not 0 <= node.selected_parameter_index < len(node.parameter_items)
        ):
            self.report({"ERROR"}, "refresh AEX parameters before selection")
            return {"CANCELLED"}
        item = node.parameter_items[node.selected_parameter_index]
        if not node.selected_parameter_items:
            node.parameter_slot = item.slot
            node.parameter_kind = item.kind
            if item.kind == "integer":
                node.parameter_integer_value = int(item.value_text)
            elif item.kind != "color":
                node.parameter_value_text = item.value_text
        existing = next((selected for selected in node.selected_parameter_items if selected.slot == item.slot), None)
        if existing is None:
            if len(node.selected_parameter_items) >= 16:
                self.report({"ERROR"}, "at most 16 named parameters can be baked")
                return {"CANCELLED"}
            existing = node.selected_parameter_items.add()
        existing.slot = item.slot
        existing.name = item.name
        existing.kind = item.kind
        existing.group_path_json = item.group_path_json
        existing.choices_json = item.choices_json
        if item.kind == "color":
            existing.color_argb = item.color_argb
        elif item.kind == "point":
            existing.point_x_text = item.point_x_text
            existing.point_y_text = item.point_y_text
        elif item.kind == "integer":
            existing.integer_value = int(item.value_text)
        else:
            existing.value_text = item.value_text
        return {"FINISHED"}


class AEXCompatSetPopupChoiceOperator(bpy.types.Operator):
    bl_idname = "aexcompat.set_popup_choice"
    bl_label = "Set AEX popup choice"

    tree_name: StringProperty()
    node_name: StringProperty()
    owner_type: StringProperty(default="GROUP")
    owner_name: StringProperty(default="")
    slot: IntProperty()
    value: IntProperty()

    def execute(self, _context: Any):
        node = _find_descriptor_node(self.owner_type, self.owner_name or self.tree_name, self.tree_name, self.node_name)
        if node is None or node.transport_mode != "render_aex" or node.description_source != node.plugin_source:
            self.report({"ERROR"}, "refresh AEX parameters before selection")
            return {"CANCELLED"}
        chosen = next((item for item in node.selected_parameter_items if item.slot == self.slot), None)
        if chosen is None or chosen.kind != "integer" or not any(
            choice["value"] == self.value for choice in _stored_choices(chosen.choices_json)
        ):
            self.report({"ERROR"}, "popup choice is not declared for this parameter")
            return {"CANCELLED"}
        chosen.integer_value = self.value
        return {"FINISHED"}


class AEXCompatRemoveParameterOperator(bpy.types.Operator):
    bl_idname = "aexcompat.remove_parameter"
    bl_label = "Remove selected AEX parameter"

    tree_name: StringProperty()
    node_name: StringProperty()
    owner_type: StringProperty(default="GROUP")
    owner_name: StringProperty(default="")

    def execute(self, _context: Any):
        node = _find_descriptor_node(self.owner_type, self.owner_name or self.tree_name, self.tree_name, self.node_name)
        if node is None or not 0 <= node.selected_override_index < len(node.selected_parameter_items):
            self.report({"ERROR"}, "selected override not found")
            return {"CANCELLED"}
        node.selected_parameter_items.remove(node.selected_override_index)
        node.selected_override_index = max(0, min(node.selected_override_index, len(node.selected_parameter_items) - 1))
        if not node.selected_parameter_items:
            node.parameter_slot = 0
        return {"FINISHED"}


def _image_to_rgba8(image: Any) -> tuple[bytes, int, int, str]:
    width, height = (int(value) for value in image.size)
    if width < 1 or height < 1:
        raise AEXCompatSessionError("blender_addon_error: empty_image")
    values = list(image.pixels)
    if len(values) != width * height * 4:
        raise AEXCompatSessionError("blender_addon_error: image_pixel_count")
    raw = bytes(max(0, min(255, int(round(float(value) * 255.0)))) for value in values)
    color_space = getattr(getattr(image, "colorspace_settings", None), "name", "scene_linear") or "scene_linear"
    return raw, width, height, color_space


def _rgba8_to_image(name: str, raw: bytes, width: int, height: int) -> Any:
    image = bpy.data.images.get(name)
    if image is None:
        image = bpy.data.images.new(name, width=width, height=height, alpha=True, float_buffer=False)
    elif tuple(image.size) != (width, height):
        raise AEXCompatSessionError("blender_addon_error: output_image_dimensions")
    image.pixels = [value / 255.0 for value in raw]
    image.pack()
    image.use_fake_user = True
    return image


def _connect_native_image_compositor(scene: Any, image: Any) -> None:
    scene.use_nodes = True
    tree = scene.node_tree
    tree.nodes.clear()
    image_node = tree.nodes.new("CompositorNodeImage")
    image_node.image = image
    composite = tree.nodes.new("CompositorNodeComposite")
    tree.links.new(image_node.outputs["Image"], composite.inputs["Image"])


class AEXCompatBakeImageOperator(bpy.types.Operator):
    bl_idname = "aexcompat.bake_image"
    bl_label = "Bake AEXCompat image"

    source_image_name: StringProperty(name="Source image", default="")
    output_image_name: StringProperty(name="Output image", default="AEXCompat Baked")
    plugin_source: StringProperty(name="Plugin source", default="")
    frame_time: FloatProperty(name="Frame time", default=0.0)
    mode: EnumProperty(
        name="Transport mode",
        items=(
            ("identity_no_aex", "Identity transport", "No AEX is loaded; validate transport only"),
            ("fixture_invert_no_aex", "Fixture invert", "Test transform only; no AEX is loaded"),
            ("render_aex", "Render AEX (macOS)", "Render one frame through the configured harness"),
        ),
        default="identity_no_aex",
    )
    connect_native: BoolProperty(name="Connect native compositor", default=True)
    render_path: EnumProperty(
        name="AEX render path",
        items=(("classic", "Classic", "Classic AEX rendering"), ("smart", "SmartFX", "SmartFX rendering")),
        default="classic",
    )
    parameter_slot: IntProperty(name="Parameter number (0 uses defaults)", default=0, min=0)
    parameter_value: FloatProperty(name="Parameter value", default=0.0)
    parameter_kind: StringProperty(name="Selected parameter kind", default="")
    parameter_integer_value: IntProperty(name="Integer value", default=0)
    parameter_value_text: StringProperty(name="Decimal value", default="")
    parameter_overrides_json: StringProperty(name="Named parameter overrides", default="")
    selected_description_plugin_sha: StringProperty(name="Described plugin SHA-256", default="")

    def execute(self, context: Any):
        source = bpy.data.images.get(self.source_image_name)
        if source is None:
            self.report({"ERROR"}, "source image not found")
            return {"CANCELLED"}
        try:
            if self.mode == "render_aex" and source.alpha_mode != "STRAIGHT":
                raise AEXCompatSessionError("render_aex requires a straight-alpha source image")
            raw, width, height, color_space = _image_to_rgba8(source)
            overrides = _parse_parameter_overrides(self.parameter_overrides_json) if self.mode == "render_aex" and self.parameter_overrides_json else None
            output, response = evaluate_rgba8(
                raw,
                width,
                height,
                alpha="straight",
                color_space=color_space,
                plugin_source=self.plugin_source or None,
                mode=self.mode,
                frame_time=self.frame_time,
                render_path=self.render_path,
                parameter_override=_parameter_override(
                    self.parameter_slot, self.parameter_kind, self.parameter_integer_value,
                    self.parameter_value_text, self.parameter_value,
                ) if self.mode == "render_aex" and self.parameter_slot and overrides is None else None,
                parameter_overrides=overrides if self.mode == "render_aex" else None,
            )
            output_image = _rgba8_to_image(self.output_image_name, output, width, height)
            if self.connect_native:
                _connect_native_image_compositor(context.scene, output_image)
        except AEXCompatSessionError as exc:
            self.report({"ERROR"}, str(exc))
            return {"CANCELLED"}
        applied = response.get("parameter_override") or response.get("parameter_overrides")
        applied_records = applied if isinstance(applied, list) else [applied]
        if (
            self.selected_description_plugin_sha
            and any(isinstance(record, dict) and record.get("description_plugin_sha256") != self.selected_description_plugin_sha for record in applied_records)
        ):
            self.report({"WARNING"}, "AEX changed since parameter refresh; refresh the list")
        self.report({"INFO"}, f"{response['status']} ({response['failure_class']})")
        return {"FINISHED"}


_CLASSES = (
    AEXCompatScalarParameter, AEXCompatChosenParameter,
    AEXCOMPAT_UL_scalar_parameters, AEXCOMPAT_UL_chosen_parameters, AEXCompatCompositorNode,
    AEXCompatRefreshParametersOperator, AEXCompatChooseParameterOperator, AEXCompatSetPopupChoiceOperator,
    AEXCompatRemoveParameterOperator,
    AEXCompatBakeImageOperator,
)
_MENU = None


def _menu_add(self: Any, _context: Any) -> None:
    self.layout.operator("node.add_node", text="AEXCompat (8-bit RGBA)").type = AEXCompatCompositorNode.bl_idname


def register() -> None:
    global _MENU
    for cls in _CLASSES:
        bpy.utils.register_class(cls)
    # Blender 3.6 and 4.5 expose different public menu names.  The node
    # remains creatable from Python in either version; menu registration is a
    # convenience and is not allowed to make addon registration fail.
    _MENU = (
        getattr(bpy.types, "NODE_MT_compositor_node_add_all", None)
        or getattr(bpy.types, "NODE_MT_category_CMP_GROUP", None)
        or getattr(bpy.types, "NODE_MT_add", None)
    )
    if _MENU is not None:
        _MENU.append(_menu_add)


def unregister() -> None:
    global _MENU
    if _MENU is not None:
        _MENU.remove(_menu_add)
        _MENU = None
    for cls in reversed(_CLASSES):
        bpy.utils.unregister_class(cls)


if __name__ == "__main__":
    register()

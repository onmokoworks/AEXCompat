"""AEXCompat Blender compositor adapter.

Blender has no public ``CompositorNodeOFX`` type in the supported versions.
This addon therefore exposes an explicit, public-API Python node and uses an
out-of-process JSONL session wrapper for bounded RGBA8 transport checks.
"""

from __future__ import annotations

import base64
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any

import bpy
from bpy.props import BoolProperty, EnumProperty, FloatProperty, StringProperty


ADDON_VERSION = (1, 0, 0)
SCHEMA_VERSION = 1


class AEXCompatSessionError(RuntimeError):
    """A bounded session did not produce an explicitly valid response."""


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


def _run_session(request: dict[str, Any], timeout_ms: int = 5000) -> dict[str, Any]:
    wrapper = _wrapper_path()
    if not wrapper.is_file():
        raise AEXCompatSessionError("blender_session_wrapper_missing")
    try:
        completed = subprocess.run(
            _python_command() + [str(wrapper)],
            input=json.dumps(request, sort_keys=True) + "\n",
            text=True,
            capture_output=True,
            cwd=str(wrapper.parent),
            timeout=max(1, timeout_ms) / 1000.0,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        raise AEXCompatSessionError("session_timeout") from exc
    except OSError as exc:
        raise AEXCompatSessionError("worker_crash") from exc
    if completed.returncode != 0 or not completed.stdout.strip():
        raise AEXCompatSessionError("worker_crash")
    try:
        response = json.loads(completed.stdout.splitlines()[-1])
    except (json.JSONDecodeError, IndexError) as exc:
        raise AEXCompatSessionError("worker_protocol_error") from exc
    if not isinstance(response, dict) or response.get("schema_version") != SCHEMA_VERSION:
        raise AEXCompatSessionError("worker_protocol_error")
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
    timeout_ms: int = 5000,
) -> tuple[bytes, dict[str, Any]]:
    """Validate and transport one RGBA8 frame without claiming AEX success."""

    width = int(width)
    height = int(height)
    stride = width * 4 if stride is None else int(stride)
    raw = bytes(rgba)
    if width < 1 or height < 1 or stride < width * 4 or len(raw) != stride * height:
        raise AEXCompatSessionError("blender_addon_error")
    request = {
        "schema_version": SCHEMA_VERSION,
        "request_kind": "aexcompat_blender_session",
        "mode": mode,
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
            "sha256": __import__("hashlib").sha256(raw).hexdigest(),
        },
    }
    response = _run_session(request, timeout_ms=timeout_ms)
    if response.get("status") not in {"identity_only", "fixture_transform"} or response.get("host_success") is not False:
        raise AEXCompatSessionError(str(response.get("failure_class", "session_failed")))
    output = response.get("output")
    if not isinstance(output, dict) or output.get("encoding") != "base64-rgba8":
        raise AEXCompatSessionError("worker_protocol_error")
    try:
        return base64.b64decode(output["data"], validate=True), response
    except Exception as exc:  # pragma: no cover - exact binascii type varies
        raise AEXCompatSessionError("worker_protocol_error") from exc


class AEXCompatCompositorNode(bpy.types.CompositorNode):
    bl_idname = "AEXCompatCompositorNode"
    bl_label = "AEXCompat transport descriptor (8-bit RGBA)"
    bl_icon = "NODE"

    plugin_source: StringProperty(name="Plugin source", default="")
    source_image_name: StringProperty(name="Source image", default="")
    output_image_name: StringProperty(name="Output image", default="AEXCompat Baked")
    transport_mode: EnumProperty(
        name="Transport mode",
        items=(
            ("identity_no_aex", "Identity transport", "No AEX is loaded; validate transport only"),
            ("fixture_invert_no_aex", "Fixture invert", "Test transform only; no AEX is loaded"),
        ),
        default="identity_no_aex",
    )
    frame_time: FloatProperty(name="Frame time", default=0.0)

    def init(self, _context: Any) -> None:
        self.inputs.new("NodeSocketColor", "Image")
        self.outputs.new("NodeSocketColor", "Image")

    def draw_buttons(self, _context: Any, layout: Any) -> None:
        layout.prop(self, "plugin_source")
        layout.prop(self, "source_image_name")
        layout.prop(self, "output_image_name")
        layout.prop(self, "transport_mode")
        layout.prop(self, "frame_time")
        layout.operator("aexcompat.bake_image", text="Bake image through worker")

    def evaluate_rgba8(self, rgba: bytes, width: int, height: int, **kwargs: Any) -> tuple[bytes, dict[str, Any]]:
        return evaluate_rgba8(
            rgba,
            width,
            height,
            frame_time=self.frame_time,
            plugin_source=self.plugin_source or None,
            mode=self.transport_mode,
            **kwargs,
        )


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
    mode: EnumProperty(
        name="Transport mode",
        items=(
            ("identity_no_aex", "Identity transport", "No AEX is loaded; validate transport only"),
            ("fixture_invert_no_aex", "Fixture invert", "Test transform only; no AEX is loaded"),
        ),
        default="identity_no_aex",
    )
    connect_native: BoolProperty(name="Connect native compositor", default=True)

    def execute(self, context: Any):
        source = bpy.data.images.get(self.source_image_name)
        if source is None:
            self.report({"ERROR"}, "source image not found")
            return {"CANCELLED"}
        try:
            raw, width, height, color_space = _image_to_rgba8(source)
            output, response = evaluate_rgba8(
                raw,
                width,
                height,
                alpha="straight",
                color_space=color_space,
                plugin_source=self.plugin_source or None,
                mode=self.mode,
            )
            output_image = _rgba8_to_image(self.output_image_name, output, width, height)
            if self.connect_native:
                _connect_native_image_compositor(context.scene, output_image)
        except AEXCompatSessionError as exc:
            self.report({"ERROR"}, str(exc))
            return {"CANCELLED"}
        self.report({"INFO"}, f"{response['status']} ({response['failure_class']})")
        return {"FINISHED"}


_CLASSES = (AEXCompatCompositorNode, AEXCompatBakeImageOperator)
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

"""AEXCompat Blender compositor adapter.

Blender has no public ``CompositorNodeOFX`` type in the supported versions.
This addon therefore exposes an explicit, public-API Python node and uses an
out-of-process JSONL session wrapper for bounded RGBA8 transport checks.
"""

from __future__ import annotations

import base64
import hashlib
import json
import os
import signal
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
                timeout=(max(1, timeout_ms) + 15_000) / 1000.0,
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
    try:
        response = json.loads(stdout.splitlines()[-1])
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
    timeout_ms: int | None = None,
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
    if response.get("input") != {"sha256": hashlib.sha256(raw).hexdigest(), "bytes": len(raw)}:
        raise AEXCompatSessionError("worker_protocol_error")
    if mode == "render_aex":
        identity = response.get("plugin_identity")
        render_identity = response.get("render_identity")
        post_run = render_identity.get("post_run") if isinstance(render_identity, dict) else None
        if (
            not isinstance(identity, dict)
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
            ("render_aex", "Render AEX (macOS)", "Render one frame through the configured harness"),
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
        bake = layout.operator("aexcompat.bake_image", text="Bake image through worker")
        _configure_bake_button(self, bake)

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

    def execute(self, context: Any):
        source = bpy.data.images.get(self.source_image_name)
        if source is None:
            self.report({"ERROR"}, "source image not found")
            return {"CANCELLED"}
        try:
            if self.mode == "render_aex" and source.alpha_mode != "STRAIGHT":
                raise AEXCompatSessionError("render_aex requires a straight-alpha source image")
            raw, width, height, color_space = _image_to_rgba8(source)
            output, response = evaluate_rgba8(
                raw,
                width,
                height,
                alpha="straight",
                color_space=color_space,
                plugin_source=self.plugin_source or None,
                mode=self.mode,
                frame_time=self.frame_time,
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

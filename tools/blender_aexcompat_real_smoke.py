"""Mac Blender smoke for one license-free, real AEX baked-image render.

Run this with Blender in background mode. The Python descriptor node is never
connected to Blender's compositor executor.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import traceback
from pathlib import Path
from types import SimpleNamespace

import bpy


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def addon():
    sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "blender_addon"))
    import aexcompat_blender

    return aexcompat_blender


def initial(output: Path, source_relative_path: str) -> None:
    output.mkdir(parents=True, exist_ok=True)
    module = addon()
    module.register()
    try:
        width = height = 64
        source = bytes(
            (255 if channel == 3 else (pixel * (17 + channel * 11)) % 256)
            for pixel in range(width * height)
            for channel in range(4)
        )
        image = bpy.data.images.new("AEXCompat Real Source", width=width, height=height, alpha=True)
        image.pixels = [value / 255 for value in source]
        image.alpha_mode = "PREMUL"
        rejected_name = "AEXCompat Premultiplied Rejected"
        try:
            rejected = bpy.ops.aexcompat.bake_image(
                source_image_name=image.name,
                output_image_name=rejected_name,
                plugin_source=source_relative_path,
                mode="render_aex",
                connect_native=False,
            )
        except RuntimeError as exc:
            if "requires a straight-alpha source image" not in str(exc):
                raise
            rejected = {"CANCELLED"}
        if set(rejected) != {"CANCELLED"} or bpy.data.images.get(rejected_name) is not None:
            raise RuntimeError("premultiplied source was accepted or created an output Image")
        image.alpha_mode = "STRAIGHT"
        node_tree = bpy.data.node_groups.new("AEXCompat Real Smoke", "CompositorNodeTree")
        node_tree.use_fake_user = True
        descriptor = node_tree.nodes.new("AEXCompatCompositorNode")
        descriptor.source_image_name = image.name
        descriptor.output_image_name = "AEXCompat Real Baked"
        descriptor.plugin_source = source_relative_path
        descriptor.transport_mode = "render_aex"
        descriptor.frame_time = 0.25

        button = SimpleNamespace()
        module._configure_bake_button(descriptor, button)
        if (
            button is None
            or button.source_image_name != image.name
            or button.output_image_name != descriptor.output_image_name
            or button.plugin_source != source_relative_path
            or button.mode != "render_aex"
            or button.frame_time != descriptor.frame_time
        ):
            raise RuntimeError("descriptor bake button did not carry its configured settings")
        result = bpy.ops.aexcompat.bake_image(
            source_image_name=button.source_image_name,
            output_image_name=button.output_image_name,
            plugin_source=button.plugin_source,
            mode=button.mode,
            frame_time=button.frame_time,
            connect_native=True,
        )
        if set(result) != {"FINISHED"}:
            raise RuntimeError(f"real bake failed: {result}")
        baked = bpy.data.images.get(descriptor.output_image_name)
        if baked is None or not baked.packed_file:
            raise RuntimeError("real bake did not create a packed image")
        baked_raw, actual_width, actual_height, _ = module._image_to_rgba8(baked)
        if (actual_width, actual_height) != (width, height) or baked_raw == source:
            raise RuntimeError("real AEX bake did not produce changed 64x64 pixels")
        # Capture the verified worker packet separately; it must match what the
        # operator stored in Blender's packed Image.
        direct, response = module.evaluate_rgba8(
            source, width, height, plugin_source=source_relative_path, mode="render_aex",
            frame_time=descriptor.frame_time,
        )
        if direct != baked_raw or response["status"] != "rendered" or not response["render_identity"]["files_unchanged"]:
            raise RuntimeError("baked pixels differ from verified AEX response")
        if response["frame"]["frame_time"] != {"seconds": descriptor.frame_time}:
            raise RuntimeError("node frame time did not reach the real AEX session")
        scene = bpy.context.scene
        scene.render.resolution_x = width
        scene.render.resolution_y = height
        scene.render.resolution_percentage = 100
        rendered_png = output / "blender-real-bake.png"
        scene.render.filepath = str(rendered_png)
        render_result = bpy.ops.render.render(write_still=True)
        if not rendered_png.is_file() or set(render_result) != {"FINISHED"}:
            raise RuntimeError("native compositor did not render the baked image")
        loaded_png = bpy.data.images.load(str(rendered_png), check_existing=False)
        png_dimensions = list(loaded_png.size)
        bpy.data.images.remove(loaded_png)
        if png_dimensions != [width, height]:
            raise RuntimeError("native compositor PNG dimensions differ from baked image")
        blend_path = output / "blender-real-bake.blend"
        bpy.ops.wm.save_as_mainfile(filepath=str(blend_path))
        evidence = {
            "schema": "aexcompat.blender_real_smoke",
            "schema_version": 1,
            "blender_version": bpy.app.version_string,
            "plugin_identity": response["plugin_identity"],
            "render_identity": response["render_identity"],
            "worker_identity": response["worker_identity"],
            "status": response["status"],
            "aex_render_performed": response["aex_render_performed"],
            "host_success": response["host_success"],
            "frame": {"width": width, "height": height, "stride": width * 4, "alpha": "straight", "frame_time_seconds": descriptor.frame_time},
            "input_sha256": digest(source),
            "output_sha256": digest(baked_raw),
            "diff": response["diff"],
            "baked_image_packed": True,
            "native_compositor_png_sha256": digest(rendered_png.read_bytes()),
            "native_compositor_png_dimensions": png_dimensions,
            "custom_node_in_render_graph": False,
            "blend_file": blend_path.name,
        }
        (output / "blender-real-smoke.json").write_text(
            json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        print(json.dumps(evidence, sort_keys=True))
    finally:
        module.unregister()


def reload(output: Path) -> None:
    module = addon()
    evidence = json.loads((output / "blender-real-smoke.json").read_text(encoding="utf-8"))
    baked = bpy.data.images.get("AEXCompat Real Baked")
    if baked is None or not baked.packed_file:
        raise RuntimeError("packed real AEX image missing after reload")
    raw, width, height, _ = module._image_to_rgba8(baked)
    tree = bpy.data.node_groups.get("AEXCompat Real Smoke")
    descriptor = next((node for node in tree.nodes if node.bl_idname == "AEXCompatCompositorNode"), None) if tree else None
    if descriptor is None:
        raise RuntimeError("real AEX descriptor missing after reload")
    if (
        descriptor.transport_mode != "render_aex"
        or descriptor.plugin_source != evidence["plugin_identity"]["source_relative_path"]
        or descriptor.frame_time != evidence["frame"]["frame_time_seconds"]
    ):
        raise RuntimeError("real AEX descriptor settings changed after reload")
    if (width, height) != (64, 64) or digest(raw) != evidence["output_sha256"]:
        raise RuntimeError("reloaded real AEX pixels differ from saved evidence")
    print(json.dumps({"reload": "passed", "output_sha256": digest(raw), "packed": True}, sort_keys=True))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--plugin", default="DistanceGradation.aex")
    parser.add_argument("--reload", action="store_true")
    args = parser.parse_args(sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else sys.argv[1:])
    if args.reload:
        reload(args.output_dir)
    else:
        initial(args.output_dir, args.plugin)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        traceback.print_exc()
        sys.stdout.flush()
        sys.stderr.flush()
        os._exit(1)

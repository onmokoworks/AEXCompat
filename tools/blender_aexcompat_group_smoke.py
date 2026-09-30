"""Verify grouped AEX names in Blender with a real license-free bake."""

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


def initial(output: Path) -> None:
    output.mkdir(parents=True, exist_ok=True)
    module = addon()
    module.register()
    try:
        size = 64
        source_bytes = bytes(channel for y in range(size) for x in range(size)
                             for channel in (x * 4, y * 4, (x + y) * 2, 255))
        source = bpy.data.images.new("AEXCompat Group Source", width=size, height=size, alpha=True)
        source.pixels = [channel / 255 for channel in source_bytes]
        source.alpha_mode = "STRAIGHT"
        source.pack()
        source.use_fake_user = True
        if module._image_to_rgba8(source)[0] != source_bytes:
            raise RuntimeError("group smoke source changed")

        tree = bpy.data.node_groups.new("AEXCompat Group Smoke", "CompositorNodeTree")
        tree.use_fake_user = True
        node = tree.nodes.new("AEXCompatCompositorNode")
        node.plugin_source = "OLMColorKey.aex"
        node.source_image_name = source.name
        node.output_image_name = "AEXCompat Group Baked"
        node.transport_mode = "render_aex"
        node.render_path = "smart"
        locator = {"tree_name": tree.name, "node_name": node.name}
        if set(bpy.ops.aexcompat.refresh_parameters(**locator)) != {"FINISHED"}:
            raise RuntimeError("group parameter refresh failed")
        thin = next(item for item in node.parameter_items if item.slot == 14)
        blur = next(item for item in node.parameter_items if item.slot == 18)
        if (
            module._parameter_label(thin) != "Edge Thin / Amount"
            or module._parameter_label(blur) != "Edge Blur / Amount"
            or module._stored_group_path(thin.group_path_json) != ["Edge Thin"]
            or module._stored_group_path(blur.group_path_json) != ["Edge Blur"]
        ):
            raise RuntimeError("same-named parameters lost their group context")
        flat = module.describe_aex("OLMToonDilate.aex")
        if any("group_path" in item for item in flat["parameter_catalog"]):
            raise RuntimeError("flat AEX acquired a false group")
        described = module.describe_aex(node.plugin_source)
        forged = json.loads(json.dumps(described))
        next(item for item in forged["parameter_catalog"] if item["slot"] == 14)["group_path"] = [""]
        real_session = module._run_session
        try:
            module._run_session = lambda *_args, **_kwargs: forged
            try:
                module.describe_aex(node.plugin_source)
            except module.AEXCompatSessionError as exc:
                if str(exc) != "worker_protocol_error":
                    raise
            else:
                raise RuntimeError("malformed group path entered Blender catalog")
        finally:
            module._run_session = real_session

        node.selected_parameter_index = next(index for index, item in enumerate(node.parameter_items) if item.slot == 14)
        if set(bpy.ops.aexcompat.choose_parameter(**locator)) != {"FINISHED"}:
            raise RuntimeError("grouped parameter selection failed")
        chosen = node.selected_parameter_items[0]
        if chosen.slot != 14 or module._parameter_label(chosen) != "Edge Thin / Amount":
            raise RuntimeError("group context lost when parameter was selected")
        chosen.integer_value = 30
        button = SimpleNamespace()
        module._configure_bake_button(node, button)
        if module._parse_parameter_overrides(button.parameter_overrides_json) != [{"slot": 14, "value": 30}]:
            raise RuntimeError("group label changed the typed render request")
        result = bpy.ops.aexcompat.bake_image(
            source_image_name=button.source_image_name,
            output_image_name=button.output_image_name,
            plugin_source=button.plugin_source,
            mode=button.mode,
            frame_time=button.frame_time,
            render_path=button.render_path,
            parameter_overrides_json=button.parameter_overrides_json,
            connect_native=True,
        )
        if set(result) != {"FINISHED"}:
            raise RuntimeError("grouped real AEX bake failed")
        image = bpy.data.images.get(node.output_image_name)
        if image is None or image.packed_file is None:
            raise RuntimeError("grouped bake image is not packed")
        baked = module._image_to_rgba8(image)[0]
        direct, response = node.evaluate_rgba8(source_bytes, size, size)
        if (
            baked != direct or response["status"] != "rendered"
            or response["parameter_overrides"][0]["slot"] != 14
            or response["parameter_overrides"][0]["value"] != 30.0
            or response["render_identity"]["files_unchanged"] is not True
        ):
            raise RuntimeError("grouped bake differs from direct typed render")
        scene_nodes = bpy.context.scene.node_tree.nodes
        if not any(item.bl_idname == "CompositorNodeImage" and item.image == image for item in scene_nodes):
            raise RuntimeError("native compositor did not receive grouped bake")

        radial = tree.nodes.new("AEXCompatCompositorNode")
        radial.plugin_source = "OLMRadialBlur.aex"
        radial.source_image_name = source.name
        radial.output_image_name = "AEXCompat Group Radial"
        radial.transport_mode = "render_aex"
        radial.render_path = "smart"
        radial_locator = {"tree_name": tree.name, "node_name": radial.name}
        if set(bpy.ops.aexcompat.refresh_parameters(**radial_locator)) != {"FINISHED"}:
            raise RuntimeError("radial group parameter refresh failed")
        outer = next(item for item in radial.parameter_items if item.slot == 4)
        inner = next(item for item in radial.parameter_items if item.slot == 10)
        if (
            module._parameter_label(outer) != "Outer Blur / Strength"
            or module._parameter_label(inner) != "Inner Blur / Strength"
        ):
            raise RuntimeError("radial strength groups were flattened")
        radial.selected_parameter_index = next(index for index, item in enumerate(radial.parameter_items) if item.slot == 4)
        if set(bpy.ops.aexcompat.choose_parameter(**radial_locator)) != {"FINISHED"}:
            raise RuntimeError("outer strength selection failed")
        radial.selected_parameter_items[0].integer_value = 500
        radial_button = SimpleNamespace()
        module._configure_bake_button(radial, radial_button)
        if set(bpy.ops.aexcompat.bake_image(
            source_image_name=radial_button.source_image_name,
            output_image_name=radial_button.output_image_name,
            plugin_source=radial_button.plugin_source,
            mode=radial_button.mode,
            frame_time=radial_button.frame_time,
            render_path=radial_button.render_path,
            parameter_overrides_json=radial_button.parameter_overrides_json,
            connect_native=True,
        )) != {"FINISHED"}:
            raise RuntimeError("radial grouped bake failed")
        radial_image = bpy.data.images.get(radial.output_image_name)
        if radial_image is None or radial_image.packed_file is None:
            raise RuntimeError("radial group image is not packed")
        radial_bytes = module._image_to_rgba8(radial_image)[0]
        radial_direct, radial_response = radial.evaluate_rgba8(source_bytes, size, size)
        if (
            radial_bytes != radial_direct or radial_bytes == source_bytes
            or radial_response["parameter_overrides"][0]["value"] != 500.0
            or radial_response["render_identity"]["files_unchanged"] is not True
        ):
            raise RuntimeError("outer strength did not change real SmartFX pixels")
        blend = output / "grouped-parameter-bake.blend"
        bpy.ops.wm.save_as_mainfile(filepath=str(blend))
        evidence = {
            "schema": "aexcompat.blender_group_smoke", "schema_version": 1,
            "blender_version": bpy.app.version_string,
            "plugin_identity": response["plugin_identity"],
            "render_identity": response["render_identity"],
            "grouped_names": [module._parameter_label(thin), module._parameter_label(blur)],
            "selected_slot": chosen.slot, "selected_value": chosen.integer_value,
            "input_sha256": digest(source_bytes), "output_sha256": digest(baked),
            "changed_bytes": sum(a != b for a, b in zip(source_bytes, baked)),
            "radial_grouped_names": [module._parameter_label(outer), module._parameter_label(inner)],
            "radial_output_sha256": digest(radial_bytes),
            "radial_changed_bytes": sum(a != b for a, b in zip(source_bytes, radial_bytes)),
            "blend_file": blend.name,
        }
        (output / "grouped-parameter-smoke.json").write_text(
            json.dumps(evidence, sort_keys=True, indent=2) + "\n", encoding="utf-8",
        )
        print(json.dumps(evidence, sort_keys=True))
    finally:
        module.unregister()


def reload(output: Path) -> None:
    module = addon()
    module.register()
    try:
        evidence = json.loads((output / "grouped-parameter-smoke.json").read_text(encoding="utf-8"))
        tree = bpy.data.node_groups.get("AEXCompat Group Smoke")
        node = next((item for item in tree.nodes if item.bl_idname == "AEXCompatCompositorNode"
                     and item.plugin_source == "OLMColorKey.aex"), None) if tree else None
        radial = next((item for item in tree.nodes if item.bl_idname == "AEXCompatCompositorNode"
                       and item.plugin_source == "OLMRadialBlur.aex"), None) if tree else None
        image = bpy.data.images.get("AEXCompat Group Baked")
        radial_image = bpy.data.images.get("AEXCompat Group Radial")
        if (
            node is None or len(node.selected_parameter_items) != 1
            or module._parameter_label(node.selected_parameter_items[0]) != "Edge Thin / Amount"
            or module._named_parameter_overrides(node) != [{"slot": 14, "value": 30}]
            or image is None or image.packed_file is None
            or digest(module._image_to_rgba8(image)[0]) != evidence["output_sha256"]
            or radial is None or len(radial.selected_parameter_items) != 1
            or module._parameter_label(radial.selected_parameter_items[0]) != "Outer Blur / Strength"
            or module._named_parameter_overrides(radial) != [{"slot": 4, "value": 500}]
            or radial_image is None or radial_image.packed_file is None
            or digest(module._image_to_rgba8(radial_image)[0]) != evidence["radial_output_sha256"]
        ):
            raise RuntimeError("grouped selection or packed output changed after reload")
        print(json.dumps({"reload": "passed", "output_sha256": evidence["output_sha256"]}))
    finally:
        module.unregister()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--reload", action="store_true")
    args = parser.parse_args(sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else sys.argv[1:])
    reload(args.output_dir) if args.reload else initial(args.output_dir)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        traceback.print_exc()
        sys.stdout.flush()
        sys.stderr.flush()
        os._exit(1)

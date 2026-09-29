"""Bake one license-free OLM AEX in Blender with default and changed values."""

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


def sha(data: bytes) -> str:
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
        width = height = 64
        source = bytes(
            value
            for y in range(height)
            for x in range(width)
            for value in (255 if (x // 8 + y // 8) % 2 else 0, 0, 0,
                          255 if (x // 8 + y // 8) % 2 else 0)
        )
        image = bpy.data.images.new("AEXCompat Parameter Source", width=width, height=height, alpha=True)
        image.pixels = [value / 255 for value in source]
        image.alpha_mode = "STRAIGHT"
        image.pack()
        image.use_fake_user = True
        observed_source, _, _, _ = module._image_to_rgba8(image)
        tree = bpy.data.node_groups.new("AEXCompat Parameter Smoke", "CompositorNodeTree")
        tree.use_fake_user = True
        descriptor = tree.nodes.new("AEXCompatCompositorNode")
        descriptor.source_image_name = image.name
        descriptor.output_image_name = "AEXCompat Changed Baked"
        descriptor.plugin_source = "OLMToonDilate.aex"
        descriptor.transport_mode = "render_aex"
        descriptor.render_path = "smart"
        if set(bpy.ops.aexcompat.refresh_parameters(tree_name=tree.name, node_name=descriptor.name)) != {"FINISHED"}:
            raise RuntimeError("AEX parameter list refresh failed")
        if (
            len(descriptor.parameter_items) != 1
            or descriptor.parameter_items[0].name != "Search Radius"
            or descriptor.parameter_items[0].kind != "float"
            or (descriptor.parameter_items[0].minimum_text, descriptor.parameter_items[0].maximum_text, descriptor.parameter_items[0].value_text) != ("0.0", "100.0", "2.0")
        ):
            raise RuntimeError("AEX parameter list omitted the real scalar description")
        described = module.describe_aex(descriptor.plugin_source)
        real_run_session = module._run_session
        try:
            for corrupt in ("missing", "invalid_sha"):
                packet = dict(described)
                if corrupt == "missing":
                    packet.pop("worker_identity")
                else:
                    packet["worker_identity"] = {**packet["worker_identity"], "source_sha256": "bad"}
                module._run_session = lambda *_args, packet=packet, **_kwargs: packet
                try:
                    module.describe_aex(descriptor.plugin_source)
                except module.AEXCompatSessionError as exc:
                    if str(exc) != "worker_protocol_error":
                        raise RuntimeError("invalid worker identity got a misleading error") from exc
                else:
                    raise RuntimeError("invalid worker identity entered the parameter picker")
        finally:
            module._run_session = real_run_session
        if set(bpy.ops.aexcompat.choose_parameter(tree_name=tree.name, node_name=descriptor.name)) != {"FINISHED"}:
            raise RuntimeError("named AEX parameter selection failed")
        if (descriptor.parameter_slot, descriptor.parameter_kind, descriptor.parameter_value_text) != (1, "float", "2.0"):
            raise RuntimeError("named selection did not apply the declared default")
        descriptor.parameter_value_text = "80.0"
        button = SimpleNamespace()
        module._configure_bake_button(descriptor, button)
        if (button.render_path, button.parameter_slot, button.parameter_kind, button.parameter_value_text) != ("smart", 1, "float", "80.0"):
            raise RuntimeError("node parameter controls did not reach the bake operator")

        default_name = "AEXCompat Default Baked"
        default_result = bpy.ops.aexcompat.bake_image(
            source_image_name=image.name, output_image_name=default_name,
            plugin_source=button.plugin_source, mode=button.mode,
            render_path=button.render_path, parameter_slot=0,
            connect_native=False,
        )
        changed_result = bpy.ops.aexcompat.bake_image(
            source_image_name=button.source_image_name, output_image_name=button.output_image_name,
            plugin_source=button.plugin_source, mode=button.mode,
            render_path=button.render_path, parameter_slot=button.parameter_slot,
            parameter_kind=button.parameter_kind, parameter_value_text=button.parameter_value_text,
            connect_native=True,
        )
        if set(default_result) != {"FINISHED"} or set(changed_result) != {"FINISHED"}:
            raise RuntimeError("parameterized AEX bake did not finish")
        default_image = bpy.data.images.get(default_name)
        changed_image = bpy.data.images.get(button.output_image_name)
        if not default_image or not changed_image or not default_image.packed_file or not changed_image.packed_file:
            raise RuntimeError("baked images were not packed")
        default_raw, _, _, _ = module._image_to_rgba8(default_image)
        changed_raw, _, _, _ = module._image_to_rgba8(changed_image)
        changed_bytes = sum(left != right for left, right in zip(default_raw, changed_raw))
        if changed_bytes == 0:
            raise RuntimeError("parameter change did not change baked pixels")
        direct, response = module.evaluate_rgba8(
            observed_source, width, height, plugin_source=button.plugin_source,
            mode="render_aex", render_path="smart",
            parameter_override={"slot": 1, "value": 80.0},
        )
        if direct != changed_raw or response["parameter_override"]["value"] != 80.0:
            raise RuntimeError("packed changed image differs from verified AEX response")
        descriptor.transport_mode = "identity_no_aex"
        identity_pixels, identity_response = descriptor.evaluate_rgba8(observed_source, width, height)
        if identity_pixels != observed_source or identity_response["status"] != "identity_only":
            raise RuntimeError("non-AEX mode used a hidden parameter override")
        descriptor.transport_mode = "render_aex"
        scene = bpy.context.scene
        scene.render.resolution_x = width
        scene.render.resolution_y = height
        scene.render.resolution_percentage = 100
        png = output / "blender-parameter-bake.png"
        scene.render.filepath = str(png)
        if set(bpy.ops.render.render(write_still=True)) != {"FINISHED"} or not png.is_file():
            raise RuntimeError("native compositor did not render the changed image")
        loaded_png = bpy.data.images.load(str(png), check_existing=False)
        png_size = list(loaded_png.size)
        bpy.data.images.remove(loaded_png)
        if png_size != [width, height]:
            raise RuntimeError("native compositor PNG dimensions are wrong")
        blend = output / "blender-parameter-bake.blend"
        bpy.ops.wm.save_as_mainfile(filepath=str(blend))
        evidence = {
            "schema": "aexcompat.blender_parameter_smoke", "schema_version": 1,
            "blender_version": bpy.app.version_string,
            "plugin_identity": response["plugin_identity"],
            "render_identity": response["render_identity"],
            "parameter_override": response["parameter_override"],
            "parameter_catalog": [
                {"slot": item.slot, "name": item.name, "kind": item.kind,
                 "minimum_text": item.minimum_text, "maximum_text": item.maximum_text, "value_text": item.value_text}
                for item in descriptor.parameter_items
            ],
            "render_path": response["render_path"],
            "input_sha256": sha(observed_source),
            "default_output_sha256": sha(default_raw),
            "changed_output_sha256": sha(changed_raw),
            "changed_bytes": changed_bytes,
            "native_png_sha256": sha(png.read_bytes()),
            "native_png_dimensions": png_size,
            "packed": True,
            "blend_file": blend.name,
        }
        (output / "blender-parameter-smoke.json").write_text(
            json.dumps(evidence, sort_keys=True, indent=2) + "\n", encoding="utf-8",
        )
        descriptor.plugin_source = "another-plugin.aex"
        if descriptor.parameter_items or descriptor.parameter_slot != 0 or descriptor.description_source:
            raise RuntimeError("changing the plugin did not clear the named parameter selection")
        rejection = []
        stale_button = SimpleNamespace(
            tree_name=tree.name, node_name=descriptor.name,
            owner_type="GROUP", owner_name=tree.name,
            report=lambda level, message: rejection.append((level, message)),
        )
        if module.AEXCompatChooseParameterOperator.execute(stale_button, None) != {"CANCELLED"}:
            raise RuntimeError("stale parameter selection was accepted")
        if rejection != [({"ERROR"}, "refresh AEX parameters before selection")]:
            raise RuntimeError("stale selection lacked an explicit error")
        color_key_node = tree.nodes.new("AEXCompatCompositorNode")
        color_key_node.plugin_source = "OLMColorKey.aex"
        color_key_node.transport_mode = "render_aex"
        if set(bpy.ops.aexcompat.refresh_parameters(tree_name=tree.name, node_name=color_key_node.name)) != {"FINISHED"}:
            raise RuntimeError("second license-free AEX parameter list refresh failed")
        threshold_index = next(
            (index for index, item in enumerate(color_key_node.parameter_items)
             if item.name == "Threshold" and item.kind == "float"), None,
        )
        if len(color_key_node.parameter_items) < 100 or threshold_index is None:
            raise RuntimeError("large AEX parameter list was truncated or lost")
        color_key_node.selected_parameter_index = threshold_index
        if set(bpy.ops.aexcompat.choose_parameter(tree_name=tree.name, node_name=color_key_node.name)) != {"FINISHED"}:
            raise RuntimeError("second AEX named selection failed")
        if color_key_node.parameter_slot != color_key_node.parameter_items[threshold_index].slot:
            raise RuntimeError("second AEX selection used the wrong slot")
        precise = tree.nodes.new("AEXCompatCompositorNode")
        precise.plugin_source = "precision.aex"
        precise.transport_mode = "render_aex"
        precise.description_source = precise.plugin_source
        item = precise.parameter_items.add()
        item.slot, item.name, item.kind = 1, "Exact integer", "integer"
        item.minimum_text, item.maximum_text, item.value_text = "0", "2147483647", "16777217"
        if set(bpy.ops.aexcompat.choose_parameter(tree_name=tree.name, node_name=precise.name)) != {"FINISHED"}:
            raise RuntimeError("large integer selection failed")
        button = SimpleNamespace()
        module._configure_bake_button(precise, button)
        if (precise.parameter_integer_value, button.parameter_integer_value) != (16777217, 16777217):
            raise RuntimeError("Blender rounded a valid i32 default")
        if module._parameter_override(button.parameter_slot, button.parameter_kind,
                                      button.parameter_integer_value, button.parameter_value_text,
                                      button.parameter_value) != {"slot": 1, "value": 16777217}:
            raise RuntimeError("integer bake changed the selected value")
        item.kind, item.name, item.value_text = "float", "Exact decimal", "0.1"
        if set(bpy.ops.aexcompat.choose_parameter(tree_name=tree.name, node_name=precise.name)) != {"FINISHED"}:
            raise RuntimeError("decimal selection failed")
        if module._parameter_override(precise.parameter_slot, precise.parameter_kind,
                                      precise.parameter_integer_value, precise.parameter_value_text,
                                      precise.parameter_value) != {"slot": 1, "value": 0.1}:
            raise RuntimeError("Blender rounded a valid decimal default")
        scene = bpy.context.scene
        scene.use_nodes = True
        scene_tree = scene.node_tree
        group = bpy.data.node_groups.new(scene_tree.name, "CompositorNodeTree")
        scene_node = scene_tree.nodes.new("AEXCompatCompositorNode")
        group_node = group.nodes.new("AEXCompatCompositorNode")
        scene_node.plugin_source = "OLMToonDilate.aex"
        group_node.plugin_source = "different.aex"
        scene_node.transport_mode = group_node.transport_mode = "render_aex"
        scene_button = SimpleNamespace()
        module._configure_node_locator(scene_node, scene_button)
        if scene_button.owner_type != "SCENE" or scene_button.owner_name != scene.name:
            raise RuntimeError("scene locator did not identify the scene")
        if set(bpy.ops.aexcompat.refresh_parameters(
            owner_type=scene_button.owner_type, owner_name=scene_button.owner_name,
            tree_name=scene_button.tree_name, node_name=scene_button.node_name,
        )) != {"FINISHED"} or len(scene_node.parameter_items) != 1 or group_node.parameter_items:
            raise RuntimeError("same-name scene/group refresh targeted the wrong node")
        if set(bpy.ops.aexcompat.choose_parameter(
            owner_type=scene_button.owner_type, owner_name=scene_button.owner_name,
            tree_name=scene_button.tree_name, node_name=scene_button.node_name,
        )) != {"FINISHED"} or scene_node.parameter_slot != 1 or group_node.parameter_slot:
            raise RuntimeError("same-name scene/group selection targeted the wrong node")
        print(json.dumps(evidence, sort_keys=True))
    finally:
        module.unregister()


def reload(output: Path) -> None:
    module = addon()
    module.register()
    try:
        evidence = json.loads((output / "blender-parameter-smoke.json").read_text(encoding="utf-8"))
        for name, expected in (
            ("AEXCompat Default Baked", evidence["default_output_sha256"]),
            ("AEXCompat Changed Baked", evidence["changed_output_sha256"]),
            ("AEXCompat Parameter Source", evidence["input_sha256"]),
        ):
            image = bpy.data.images.get(name)
            if not image or not image.packed_file:
                raise RuntimeError("packed image missing after reload")
            raw, width, height, _ = module._image_to_rgba8(image)
            if (width, height) != (64, 64) or sha(raw) != expected:
                raise RuntimeError("packed image changed after reload")
        tree = bpy.data.node_groups.get("AEXCompat Parameter Smoke")
        descriptor = next((node for node in tree.nodes if node.bl_idname == "AEXCompatCompositorNode"), None) if tree else None
        if not descriptor or (descriptor.render_path, descriptor.parameter_slot, descriptor.parameter_kind, descriptor.parameter_value_text) != ("smart", 1, "float", "80.0"):
            raise RuntimeError("parameter controls changed after reload")
        if (
            [{"slot": item.slot, "name": item.name, "kind": item.kind,
              "minimum_text": item.minimum_text, "maximum_text": item.maximum_text, "value_text": item.value_text}
             for item in descriptor.parameter_items] != evidence["parameter_catalog"]
            or descriptor.description_source != descriptor.plugin_source
        ):
            raise RuntimeError("named parameter list changed after reload")
        print(json.dumps({"reload": "passed", "changed_output_sha256": evidence["changed_output_sha256"]}))
    finally:
        module.unregister()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--reload", action="store_true")
    args = parser.parse_args(sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else sys.argv[1:])
    if args.reload:
        reload(args.output_dir)
    else:
        initial(args.output_dir)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        traceback.print_exc()
        sys.stdout.flush()
        sys.stderr.flush()
        os._exit(1)

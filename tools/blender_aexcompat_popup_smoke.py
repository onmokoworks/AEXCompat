"""Exercise a named popup choice with a license-free macOS AEX in Blender."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
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
        source_bytes = bytes(
            channel
            for y in range(size)
            for x in range(size)
            for channel in ((x * 4, y * 4, (x + y) * 2, 255))
        )
        source = bpy.data.images.new("AEXCompat Popup Source", width=size, height=size, alpha=True)
        source.pixels = [channel / 255 for channel in source_bytes]
        source.alpha_mode = "STRAIGHT"
        source.pack()
        source.use_fake_user = True
        if module._image_to_rgba8(source)[0] != source_bytes:
            raise RuntimeError("source pixels changed before bake")

        tree = bpy.data.node_groups.new("AEXCompat Popup Smoke", "CompositorNodeTree")
        tree.use_fake_user = True
        node = tree.nodes.new("AEXCompatCompositorNode")
        node.plugin_source = "DistanceGradation.aex"
        node.source_image_name = source.name
        node.output_image_name = "AEXCompat Popup Baked"
        node.transport_mode = "render_aex"
        node.render_path = "classic"
        locator = {"tree_name": tree.name, "node_name": node.name}
        if set(bpy.ops.aexcompat.refresh_parameters(**locator)) != {"FINISHED"}:
            raise RuntimeError("parameter refresh failed")
        mapped = next(item for item in node.parameter_items if item.slot == 2)
        mismatched = next(item for item in node.parameter_items if item.slot == 11)
        expected = [
            {"value": 1, "label": "Inside"},
            {"value": 2, "label": "Outside"},
            {"value": 3, "label": "Both"},
        ]
        if json.loads(mapped.choices_json) != expected or mismatched.choices_json:
            raise RuntimeError("real popup mapping is missing or ambiguous range was labeled")
        described = module.describe_aex(node.plugin_source)
        forged = copy.deepcopy(described)
        forged_slot = next(item for item in forged["parameter_catalog"] if item["slot"] == 2)
        forged_slot["choices"][1]["value"] = 3
        real_session = module._run_session
        try:
            module._run_session = lambda *_args, **_kwargs: forged
            try:
                module.describe_aex(node.plugin_source)
            except module.AEXCompatSessionError as exc:
                if str(exc) != "worker_protocol_error":
                    raise
            else:
                raise RuntimeError("forged duplicate choice value entered Blender UI")
        finally:
            module._run_session = real_session
        node.selected_parameter_index = next(index for index, item in enumerate(node.parameter_items) if item.slot == 2)
        if set(bpy.ops.aexcompat.choose_parameter(**locator)) != {"FINISHED"}:
            raise RuntimeError("named parameter selection failed")
        chosen = node.selected_parameter_items[0]
        if chosen.integer_value != 1 or json.loads(chosen.choices_json) != expected:
            raise RuntimeError("popup default or choices were lost")
        try:
            bpy.ops.aexcompat.set_popup_choice(**locator, slot=2, value=4)
        except RuntimeError as exc:
            if "popup choice is not declared" not in str(exc):
                raise
        else:
            raise RuntimeError("out-of-range popup choice was accepted")
        if chosen.integer_value != 1:
            raise RuntimeError("rejected popup choice changed the selected value")
        if set(bpy.ops.aexcompat.set_popup_choice(**locator, slot=2, value=2)) != {"FINISHED"}:
            raise RuntimeError("named Outside choice was rejected")
        if chosen.integer_value != 2:
            raise RuntimeError("named choice did not set the declared integer value")

        button = SimpleNamespace()
        module._configure_bake_button(node, button)
        if module._parse_parameter_overrides(button.parameter_overrides_json) != [{"slot": 2, "value": 2}]:
            raise RuntimeError("named choice did not reach the bake request")
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
            raise RuntimeError("named popup bake failed")
        baked_image = bpy.data.images.get(node.output_image_name)
        if baked_image is None or baked_image.packed_file is None:
            raise RuntimeError("popup output was not packed")
        baked = module._image_to_rgba8(baked_image)[0]
        direct, response = module.evaluate_rgba8(
            source_bytes, size, size, mode="render_aex", plugin_source=node.plugin_source,
            render_path="classic", parameter_override={"slot": 2, "value": 2},
        )
        if (
            baked != direct or response["status"] != "rendered"
            or response["failure_class"] != "none"
            or response["parameter_override"]["value"] != 2.0
            or response["plugin_identity"]["state"] != "loaded"
            or response["render_identity"]["files_unchanged"] is not True
        ):
            raise RuntimeError("named popup bake differs from direct integer render")
        scene_nodes = bpy.context.scene.node_tree.nodes
        if not any(item.bl_idname == "CompositorNodeImage" and item.image == baked_image for item in scene_nodes):
            raise RuntimeError("native compositor did not receive the packed AEX output")

        impulse_size = 32
        impulse = bytes(
            channel for y in range(impulse_size) for x in range(impulse_size)
            for channel in ((255, 255, 255, 255) if 14 <= x < 18 and 14 <= y < 18 else (0, 0, 0, 255))
        )
        effect_node = tree.nodes.new("AEXCompatCompositorNode")
        effect_node.plugin_source = "OLMKiraKira.aex"
        effect_node.transport_mode = "render_aex"
        effect_node.render_path = "smart"
        effect_locator = {"tree_name": tree.name, "node_name": effect_node.name}
        if set(bpy.ops.aexcompat.refresh_parameters(**effect_locator)) != {"FINISHED"}:
            raise RuntimeError("effect parameter refresh failed")
        merge = next(item for item in effect_node.parameter_items if item.slot == 3)
        channel = next(item for item in effect_node.parameter_items if item.slot == 1)
        if json.loads(merge.choices_json) != [
            {"value": 1, "label": "premultiply"}, {"value": 2, "label": "add"},
        ] or channel.choices_json:
            raise RuntimeError("effect popup names were mapped onto an ambiguous range")
        effect_node.selected_parameter_index = next(index for index, item in enumerate(effect_node.parameter_items) if item.slot == 3)
        if set(bpy.ops.aexcompat.choose_parameter(**effect_locator)) != {"FINISHED"}:
            raise RuntimeError("effect popup selection failed")
        if set(bpy.ops.aexcompat.set_popup_choice(**effect_locator, slot=3, value=2)) != {"FINISHED"}:
            raise RuntimeError("named add choice was rejected")
        named_add, named_response = effect_node.evaluate_rgba8(impulse, impulse_size, impulse_size)
        direct_default, default_response = module.evaluate_rgba8(
            impulse, impulse_size, impulse_size, mode="render_aex", plugin_source="OLMKiraKira.aex",
            render_path="smart", parameter_override={"slot": 3, "value": 1},
        )
        if (
            named_add == direct_default or named_response["status"] != "rendered"
            or default_response["status"] != "rendered"
            or named_response["parameter_overrides"][0]["value"] != 2.0
            or named_response["render_identity"]["files_unchanged"] is not True
        ):
            raise RuntimeError("named add choice did not change real AEX pixels")
        blend = output / "popup-parameter-bake.blend"
        bpy.ops.wm.save_as_mainfile(filepath=str(blend))
        evidence = {
            "schema": "aexcompat.blender_popup_smoke", "schema_version": 1,
            "blender_version": bpy.app.version_string,
            "plugin_identity": response["plugin_identity"],
            "render_identity": response["render_identity"],
            "choice": {"slot": 2, "label": "Outside", "value": chosen.integer_value},
            "source_sha256": digest(source_bytes), "output_sha256": digest(baked),
            "changed_bytes": sum(before != after for before, after in zip(source_bytes, baked)),
            "effect_choice": {"slot": 3, "label": "add", "value": 2},
            "effect_default_sha256": digest(direct_default),
            "effect_add_sha256": digest(named_add),
            "blend_file": blend.name,
        }
        (output / "popup-parameter-smoke.json").write_text(
            json.dumps(evidence, indent=2, ensure_ascii=False) + "\n", encoding="utf-8",
        )
    finally:
        module.unregister()


def reload(output: Path) -> None:
    module = addon()
    module.register()
    try:
        evidence = json.loads((output / "popup-parameter-smoke.json").read_text(encoding="utf-8"))
        bpy.ops.wm.open_mainfile(filepath=str(output / evidence["blend_file"]))
        tree = bpy.data.node_groups["AEXCompat Popup Smoke"]
        node = next(item for item in tree.nodes if item.bl_idname == "AEXCompatCompositorNode" and item.plugin_source == "DistanceGradation.aex")
        effect_node = next(item for item in tree.nodes if item.bl_idname == "AEXCompatCompositorNode" and item.plugin_source == "OLMKiraKira.aex")
        chosen = node.selected_parameter_items[0]
        effect_chosen = effect_node.selected_parameter_items[0]
        image = bpy.data.images["AEXCompat Popup Baked"]
        if (
            node.plugin_source != "DistanceGradation.aex" or node.render_path != "classic"
            or chosen.slot != 2 or chosen.integer_value != 2
            or [choice["label"] for choice in module._stored_choices(chosen.choices_json)] != ["Inside", "Outside", "Both"]
            or image.packed_file is None
            or digest(module._image_to_rgba8(image)[0]) != evidence["output_sha256"]
            or effect_chosen.slot != 3 or effect_chosen.integer_value != 2
            or [choice["label"] for choice in module._stored_choices(effect_chosen.choices_json)] != ["premultiply", "add"]
        ):
            raise RuntimeError("saved Blender popup selection or packed image changed")
    finally:
        module.unregister()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--reload", action="store_true")
    args = parser.parse_args(sys.argv[sys.argv.index("--") + 1:])
    try:
        (reload if args.reload else initial)(args.output_dir.resolve())
    except Exception:
        traceback.print_exc()
        sys.exit(1)

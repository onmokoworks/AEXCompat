"""Verify two named license-free OLM parameters through Blender and one bake."""

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


def digest(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def addon():
    sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "blender_addon"))
    import aexcompat_blender

    return aexcompat_blender


def initial(output: Path) -> None:
    output.mkdir(parents=True, exist_ok=True)
    module = addon()
    module.register()
    try:
        width = height = 32
        pixels = bytes(channel for y in range(height) for x in range(width)
                       for channel in ((255, 0, 0, 255) if (x // 8 + y // 8) % 2 else (0, 0, 255, 255)))
        source = bpy.data.images.new("AEXCompat Multi Source", width=width, height=height, alpha=True)
        source.pixels = [channel / 255 for channel in pixels]
        source.alpha_mode = "STRAIGHT"
        source.pack()
        source.use_fake_user = True
        observed, _, _, _ = module._image_to_rgba8(source)
        if observed != pixels:
            raise RuntimeError("source pixels changed before bake")
        tree = bpy.data.node_groups.new("AEXCompat Multi Smoke", "CompositorNodeTree")
        tree.use_fake_user = True
        node = tree.nodes.new("AEXCompatCompositorNode")
        node.plugin_source = "OLMBlur.aex"
        node.source_image_name = source.name
        node.output_image_name = "AEXCompat Multi Baked"
        node.transport_mode = "render_aex"
        node.render_path = "smart"
        if set(bpy.ops.aexcompat.refresh_parameters(tree_name=tree.name, node_name=node.name)) != {"FINISHED"}:
            raise RuntimeError("AEX parameter refresh failed")
        if len(node.parameter_items) != 5:
            raise RuntimeError("AEX parameter catalog changed")
        for slot, name, value in ((1, "Blur Amount", "20.0"), (3, "Number of Repeat", 5)):
            index = next((index for index, item in enumerate(node.parameter_items)
                          if item.slot == slot and item.name == name), None)
            if index is None:
                raise RuntimeError("named AEX parameter missing")
            node.selected_parameter_index = index
            if set(bpy.ops.aexcompat.choose_parameter(tree_name=tree.name, node_name=node.name)) != {"FINISHED"}:
                raise RuntimeError("named AEX parameter selection failed")
            chosen = next(item for item in node.selected_parameter_items if item.slot == slot)
            if slot == 3:
                chosen.integer_value = value
            else:
                chosen.value_text = value
        if len(node.selected_parameter_items) != 2:
            raise RuntimeError("second parameter replaced the first")
        chosen_amount = next(item for item in node.selected_parameter_items if item.slot == 1)
        chosen_amount.value_text = "abc"
        invalid_button = SimpleNamespace()
        module._configure_bake_button(node, invalid_button)
        try:
            bpy.ops.aexcompat.bake_image(
                source_image_name=source.name, output_image_name="AEXCompat Invalid",
                plugin_source=node.plugin_source, mode="render_aex", render_path="smart",
                parameter_overrides_json=invalid_button.parameter_overrides_json,
                connect_native=False,
            )
        except RuntimeError as exc:
            if "parameter value must be finite" not in str(exc):
                raise
        else:
            raise RuntimeError("invalid editable value unexpectedly baked")
        if bpy.data.images.get("AEXCompat Invalid") is not None:
            raise RuntimeError("invalid editable value created an image")
        node.transport_mode = "identity_no_aex"
        identity_button = SimpleNamespace()
        module._configure_bake_button(node, identity_button)
        if set(bpy.ops.aexcompat.bake_image(
            source_image_name=source.name, output_image_name="AEXCompat Identity",
            plugin_source=node.plugin_source, mode=identity_button.mode,
            parameter_overrides_json=identity_button.parameter_overrides_json,
            connect_native=False,
        )) != {"FINISHED"}:
            raise RuntimeError("invalid AEX edit blocked non-AEX identity transport")
        identity_pixels, identity_response = node.evaluate_rgba8(observed, width, height)
        if identity_pixels != observed or identity_response["status"] != "identity_only":
            raise RuntimeError("identity transport used AEX parameter selection")
        real_run_session = module._run_session
        try:
            module._run_session = lambda *_args, **_kwargs: {
                **identity_response, "parameter_overrides": [{
                    "slot": 1, "kind": "integer", "value": 1,
                    "description_sha256": "0" * 64, "description_plugin_sha256": "0" * 64,
                    "description_files_unchanged": True, "description_matches_render_plugin": False,
                }],
            }
            try:
                module.evaluate_rgba8(observed, width, height)
            except module.AEXCompatSessionError as exc:
                if str(exc) != "worker_protocol_error":
                    raise RuntimeError("identity response pollution got misleading error") from exc
            else:
                raise RuntimeError("identity response accepted AEX parameter records")
        finally:
            module._run_session = real_run_session
        node.transport_mode = "render_aex"
        chosen_amount.value_text = "20.0"
        button = SimpleNamespace()
        module._configure_bake_button(node, button)
        expected = [{"slot": 1, "value": 20.0}, {"slot": 3, "value": 5}]
        if module._parse_parameter_overrides(button.parameter_overrides_json) != expected:
            raise RuntimeError("Blender controls changed the two override values")
        default_name = "AEXCompat Multi Default"
        if set(bpy.ops.aexcompat.bake_image(
            source_image_name=source.name, output_image_name=default_name,
            plugin_source=node.plugin_source, mode="render_aex", render_path="smart",
            connect_native=False,
        )) != {"FINISHED"}:
            raise RuntimeError("default bake failed")
        solo_names = ("AEXCompat Amount Baked", "AEXCompat Repeat Baked")
        for name, override in zip(solo_names, expected):
            if set(bpy.ops.aexcompat.bake_image(
                source_image_name=source.name, output_image_name=name,
                plugin_source=node.plugin_source, mode="render_aex", render_path="smart",
                parameter_overrides_json=json.dumps([override]), connect_native=False,
            )) != {"FINISHED"}:
                raise RuntimeError("single-control comparison bake failed")
        if set(bpy.ops.aexcompat.bake_image(
            source_image_name=button.source_image_name, output_image_name=button.output_image_name,
            plugin_source=button.plugin_source, mode=button.mode,
            render_path=button.render_path, parameter_overrides_json=button.parameter_overrides_json,
            selected_description_plugin_sha=button.selected_description_plugin_sha,
            connect_native=True,
        )) != {"FINISHED"}:
            raise RuntimeError("two-parameter bake failed")
        default_image = bpy.data.images.get(default_name)
        changed_image = bpy.data.images.get(node.output_image_name)
        if not default_image or not changed_image or not default_image.packed_file or not changed_image.packed_file:
            raise RuntimeError("baked images were not packed")
        default_raw, _, _, _ = module._image_to_rgba8(default_image)
        changed_raw, _, _, _ = module._image_to_rgba8(changed_image)
        solo_raw = [module._image_to_rgba8(bpy.data.images[name])[0] for name in solo_names]
        changed_bytes = sum(left != right for left, right in zip(default_raw, changed_raw))
        if changed_bytes == 0 or len({digest(raw) for raw in (default_raw, *solo_raw, changed_raw)}) != 4:
            raise RuntimeError("both AEX controls did not independently affect the image")
        direct, response = node.evaluate_rgba8(observed, width, height)
        if direct != changed_raw or [item["slot"] for item in response["parameter_overrides"]] != [1, 3]:
            raise RuntimeError("node and bake did not use the same two parameters")
        explicit_single, single_response = node.evaluate_rgba8(
            observed, width, height, parameter_override={"slot": 1, "value": 20.0},
        )
        if explicit_single != solo_raw[0] or "parameter_overrides" in single_response:
            raise RuntimeError("explicit legacy node override was replaced by named values")
        real_run_session = module._run_session
        try:
            for index, mutation, requested in (
                (1, {"description_sha256": "0" * 64}, expected),
                (0, {"slot": True}, expected),
                (0, {"value": True}, [{"slot": 1, "value": 1}, expected[1]]),
            ):
                forged = dict(response)
                forged["parameter_overrides"] = [dict(item) for item in response["parameter_overrides"]]
                forged["parameter_overrides"][index].update(mutation)
                module._run_session = lambda *_args, forged=forged, **_kwargs: forged
                try:
                    module.evaluate_rgba8(observed, width, height, plugin_source=node.plugin_source,
                                          mode="render_aex", render_path="smart", parameter_overrides=requested)
                except module.AEXCompatSessionError as exc:
                    if str(exc) != "worker_protocol_error":
                        raise RuntimeError("invalid multi response got misleading error") from exc
                else:
                    raise RuntimeError("invalid multi response was accepted")
        finally:
            module._run_session = real_run_session
        scene = bpy.context.scene
        scene.render.resolution_x = width
        scene.render.resolution_y = height
        scene.render.resolution_percentage = 100
        png = output / "multi-parameter-bake.png"
        scene.render.filepath = str(png)
        if set(bpy.ops.render.render(write_still=True)) != {"FINISHED"} or not png.is_file():
            raise RuntimeError("native compositor PNG render failed")
        blend = output / "multi-parameter-bake.blend"
        bpy.ops.wm.save_as_mainfile(filepath=str(blend))
        evidence = {
            "schema": "aexcompat.blender_multi_parameter_smoke", "schema_version": 1,
            "blender_version": bpy.app.version_string,
            "plugin_identity": response["plugin_identity"],
            "render_identity": response["render_identity"],
            "parameter_overrides": response["parameter_overrides"],
            "default_sha256": digest(default_raw), "changed_sha256": digest(changed_raw),
            "amount_sha256": digest(solo_raw[0]), "repeat_sha256": digest(solo_raw[1]),
            "changed_bytes": changed_bytes, "input_sha256": digest(observed),
            "native_png_sha256": digest(png.read_bytes()),
            "blend_file": blend.name,
        }
        (output / "multi-parameter-smoke.json").write_text(json.dumps(evidence, sort_keys=True, indent=2) + "\n", encoding="utf-8")
        node.selected_override_index = 1
        if set(bpy.ops.aexcompat.remove_parameter(tree_name=tree.name, node_name=node.name)) != {"FINISHED"}:
            raise RuntimeError("remove override failed")
        if [item.slot for item in node.selected_parameter_items] != [1]:
            raise RuntimeError("remove override selected wrong row")
        node.plugin_source = "another.aex"
        if node.selected_parameter_items or node.parameter_items or node.parameter_slot:
            raise RuntimeError("source change retained stale named overrides")
        print(json.dumps(evidence, sort_keys=True))
    finally:
        module.unregister()


def reload(output: Path) -> None:
    module = addon()
    module.register()
    try:
        evidence = json.loads((output / "multi-parameter-smoke.json").read_text(encoding="utf-8"))
        tree = bpy.data.node_groups.get("AEXCompat Multi Smoke")
        node = next((item for item in tree.nodes if item.bl_idname == "AEXCompatCompositorNode"), None) if tree else None
        if node is None or module._named_parameter_overrides(node) != [
            {"slot": 1, "value": 20.0}, {"slot": 3, "value": 5},
        ]:
            raise RuntimeError("two named override values changed after reload")
        for name, expected in (("AEXCompat Multi Source", evidence["input_sha256"]),
                               ("AEXCompat Multi Default", evidence["default_sha256"]),
                               ("AEXCompat Amount Baked", evidence["amount_sha256"]),
                               ("AEXCompat Repeat Baked", evidence["repeat_sha256"]),
                               ("AEXCompat Multi Baked", evidence["changed_sha256"])):
            image = bpy.data.images.get(name)
            if image is None or not image.packed_file:
                raise RuntimeError("packed image missing after reload")
            raw, width, height, _ = module._image_to_rgba8(image)
            if (width, height) != (32, 32) or digest(raw) != expected:
                raise RuntimeError("packed image changed after reload")
        print(json.dumps({"reload": "passed", "changed_sha256": evidence["changed_sha256"]}))
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

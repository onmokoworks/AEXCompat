"""Bake an edited AEX point through Blender and verify actual SmartFX pixels."""

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


def bake(module, source_name: str, output_name: str, overrides: list[dict], *, native: bool = False) -> bytes:
    result = bpy.ops.aexcompat.bake_image(
        source_image_name=source_name, output_image_name=output_name,
        plugin_source="OLMRadialBlur.aex", mode="render_aex", render_path="smart",
        parameter_overrides_json=json.dumps(overrides), connect_native=native,
    )
    if set(result) != {"FINISHED"}:
        raise RuntimeError(f"point bake failed: {output_name}")
    image = bpy.data.images.get(output_name)
    if image is None or image.packed_file is None:
        raise RuntimeError("point bake was not packed")
    return module._image_to_rgba8(image)[0]


def initial(output: Path) -> None:
    output.mkdir(parents=True, exist_ok=True)
    module = addon()
    module.register()
    try:
        size = 64
        pixels = bytes(channel for y in range(size) for x in range(size)
                       for channel in ((x * 4, y * 4, (x + y) * 2, 255)))
        source = bpy.data.images.new("AEXCompat Point Source", width=size, height=size, alpha=True)
        source.pixels = [channel / 255 for channel in pixels]
        source.alpha_mode = "STRAIGHT"
        source.pack()
        source.use_fake_user = True
        if module._image_to_rgba8(source)[0] != pixels:
            raise RuntimeError("source pixels changed")

        tree = bpy.data.node_groups.new("AEXCompat Point Smoke", "CompositorNodeTree")
        tree.use_fake_user = True
        node = tree.nodes.new("AEXCompatCompositorNode")
        node.plugin_source = "OLMRadialBlur.aex"
        node.source_image_name = source.name
        node.output_image_name = "AEXCompat Point Selected"
        node.transport_mode = "render_aex"
        node.render_path = "smart"
        locator = {"tree_name": tree.name, "node_name": node.name}
        if set(bpy.ops.aexcompat.refresh_parameters(**locator)) != {"FINISHED"}:
            raise RuntimeError("point catalog refresh failed")
        precision = tree.nodes.new("AEXCompatCompositorNode")
        precision.name = "AEXCompat Point Precision"
        precision.plugin_source = node.plugin_source
        precision.transport_mode = "render_aex"
        precise_components = [256.0 + 1.0 / 65536.0, 1.0 / 65536.0]
        described = module.describe_aex(node.plugin_source)
        forged = json.loads(json.dumps(described))
        next(entry for entry in forged["parameter_catalog"] if entry["slot"] == 2)["components"] = precise_components
        real_session = module._run_session
        try:
            module._run_session = lambda *_args, **_kwargs: forged
            if set(bpy.ops.aexcompat.refresh_parameters(tree_name=tree.name, node_name=precision.name)) != {"FINISHED"}:
                raise RuntimeError("fractional point catalog refresh failed")
        finally:
            module._run_session = real_session
        precision_index = next(index for index, item in enumerate(precision.parameter_items) if item.slot == 2)
        precision.selected_parameter_index = precision_index
        if set(bpy.ops.aexcompat.choose_parameter(tree_name=tree.name, node_name=precision.name)) != {"FINISHED"}:
            raise RuntimeError("fractional point selection failed")
        if module._named_parameter_overrides(precision) != [{"slot": 2, "components": precise_components}]:
            raise RuntimeError("fractional point default was rounded")
        precision_button = SimpleNamespace()
        module._configure_bake_button(precision, precision_button)
        if module._parse_parameter_overrides(precision_button.parameter_overrides_json) != [
            {"slot": 2, "components": precise_components},
        ]:
            raise RuntimeError("fractional point request was rounded")
        for slot, name, kind in ((4, "Strength", "integer"), (2, "Center", "point")):
            index = next((index for index, item in enumerate(node.parameter_items)
                          if item.slot == slot and item.name == name and item.kind == kind), None)
            if index is None:
                raise RuntimeError(f"named parameter missing: {name}")
            node.selected_parameter_index = index
            if set(bpy.ops.aexcompat.choose_parameter(**locator)) != {"FINISHED"}:
                raise RuntimeError(f"parameter selection failed: {name}")
        scalar = next(item for item in node.selected_parameter_items if item.slot == 4)
        point = next(item for item in node.selected_parameter_items if item.slot == 2)
        if [point.point_x_text, point.point_y_text] != ["50.0", "50.0"]:
            raise RuntimeError("real point default was not exposed")
        scalar.integer_value = 500
        point.point_x_text = "20.0"
        point.point_y_text = "80.0"
        selected = [{"slot": 4, "value": 500}, {"slot": 2, "components": [20.0, 80.0]}]
        default = [{"slot": 4, "value": 500}, {"slot": 2, "components": [50.0, 50.0]}]
        button = SimpleNamespace()
        module._configure_bake_button(node, button)
        if module._parse_parameter_overrides(button.parameter_overrides_json) != selected:
            raise RuntimeError("selected point did not reach bake request")
        for invalid in (
            '[{"slot":2,"components":[20]}]',
            '[{"slot":2,"components":[20,true]}]',
            '[{"slot":2,"components":[20,32769]}]',
            '[{"slot":2,"components":[20,80],"value":2}]',
            '[{"slot":2,"components":["1e999",80]}]',
        ):
            try:
                module._parse_parameter_overrides(invalid)
            except module.AEXCompatSessionError:
                pass
            else:
                raise RuntimeError("malformed point override was accepted")

        baseline = bake(module, source.name, "AEXCompat Point Baseline", default)
        chosen = bake(module, source.name, node.output_image_name, selected, native=True)
        direct, response = node.evaluate_rgba8(pixels, size, size)
        if chosen != direct or chosen == baseline or chosen == pixels:
            raise RuntimeError("selected point did not change real SmartFX pixels")
        if response["parameter_overrides"][1]["components"] != [20.0, 80.0]:
            raise RuntimeError("point values were not recorded")
        real_session = module._run_session
        try:
            for mutation in ({"components": [50.0, 50.0]}, {"kind": "float"}, {"components": [20.0]}):
                forged = dict(response)
                forged["parameter_overrides"] = [dict(record) for record in response["parameter_overrides"]]
                forged["parameter_overrides"][1].update(mutation)
                module._run_session = lambda *_args, forged=forged, **_kwargs: forged
                try:
                    module.evaluate_rgba8(pixels, size, size, plugin_source=node.plugin_source,
                                          mode="render_aex", render_path="smart", parameter_overrides=selected)
                except module.AEXCompatSessionError as exc:
                    if str(exc) != "worker_protocol_error":
                        raise
                else:
                    raise RuntimeError("forged point response was accepted")
        finally:
            module._run_session = real_session

        scene = bpy.context.scene
        scene.render.resolution_x = size
        scene.render.resolution_y = size
        scene.render.resolution_percentage = 100
        png = output / "point-parameter-bake.png"
        scene.render.filepath = str(png)
        if set(bpy.ops.render.render(write_still=True)) != {"FINISHED"} or not png.is_file():
            raise RuntimeError("native compositor render failed")
        blend = output / "point-parameter-bake.blend"
        bpy.ops.wm.save_as_mainfile(filepath=str(blend))
        evidence = {
            "schema": "aexcompat.blender_point_parameter_smoke", "schema_version": 1,
            "blender_version": bpy.app.version_string,
            "plugin_identity": response["plugin_identity"],
            "render_identity": response["render_identity"],
            "parameter_overrides": response["parameter_overrides"],
            "input_sha256": digest(pixels), "baseline_sha256": digest(baseline),
            "selected_sha256": digest(chosen),
            "point_changed_bytes": sum(a != b for a, b in zip(baseline, chosen)),
            "precision_components": precise_components,
            "native_png_sha256": digest(png.read_bytes()), "blend_file": blend.name,
        }
        (output / "point-parameter-smoke.json").write_text(
            json.dumps(evidence, sort_keys=True, indent=2) + "\n", encoding="utf-8",
        )
        print(json.dumps(evidence, sort_keys=True))
    finally:
        module.unregister()


def reload(output: Path) -> None:
    module = addon()
    module.register()
    try:
        evidence = json.loads((output / "point-parameter-smoke.json").read_text(encoding="utf-8"))
        tree = bpy.data.node_groups.get("AEXCompat Point Smoke")
        node = next((item for item in tree.nodes if item.bl_idname == "AEXCompatCompositorNode"), None) if tree else None
        if node is None or module._named_parameter_overrides(node) != [
            {"slot": 4, "value": 500}, {"slot": 2, "components": [20.0, 80.0]},
        ]:
            raise RuntimeError("point selection changed after reload")
        precision = tree.nodes.get("AEXCompat Point Precision")
        if precision is None or module._named_parameter_overrides(precision) != [
            {"slot": 2, "components": evidence["precision_components"]},
        ]:
            raise RuntimeError("fractional point changed after reload")
        for name, expected in (
            ("AEXCompat Point Source", evidence["input_sha256"]),
            ("AEXCompat Point Baseline", evidence["baseline_sha256"]),
            ("AEXCompat Point Selected", evidence["selected_sha256"]),
        ):
            image = bpy.data.images.get(name)
            if image is None or image.packed_file is None:
                raise RuntimeError("packed point image missing after reload")
            raw, width, height, _ = module._image_to_rgba8(image)
            if (width, height) != (64, 64) or digest(raw) != expected:
                raise RuntimeError("packed point image changed after reload")
        print(json.dumps({"reload": "passed", "selected_sha256": evidence["selected_sha256"]}))
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

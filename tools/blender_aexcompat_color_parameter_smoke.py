"""Bake named color and scalar values through a license-free OLM AEX in Blender."""

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


def bake(module, node, name: str, source: str, overrides: list[dict] | None = None, *, native: bool = False) -> bytes:
    result = bpy.ops.aexcompat.bake_image(
        source_image_name=source, output_image_name=name,
        plugin_source=node.plugin_source, mode="render_aex", render_path="smart",
        parameter_overrides_json=json.dumps(overrides) if overrides else "",
        connect_native=native,
    )
    if set(result) != {"FINISHED"}:
        raise RuntimeError(f"bake failed: {name}")
    image = bpy.data.images.get(name)
    if image is None or image.packed_file is None:
        raise RuntimeError(f"bake was not packed: {name}")
    return module._image_to_rgba8(image)[0]


def initial(output: Path) -> None:
    output.mkdir(parents=True, exist_ok=True)
    module = addon()
    module.register()
    try:
        size = 32
        pixels = bytes(channel for y in range(size) for x in range(size)
                       for channel in ((255, 255, 255, 255) if 14 <= x < 18 and 14 <= y < 18
                                       else (0, 0, 0, 255)))
        source = bpy.data.images.new("AEXCompat Color Source", width=size, height=size, alpha=True)
        source.pixels = [channel / 255 for channel in pixels]
        source.alpha_mode = "STRAIGHT"
        source.pack()
        source.use_fake_user = True
        observed, _, _, _ = module._image_to_rgba8(source)
        if observed != pixels:
            raise RuntimeError("source image changed before bake")

        tree = bpy.data.node_groups.new("AEXCompat Color Smoke", "CompositorNodeTree")
        tree.use_fake_user = True
        node = tree.nodes.new("AEXCompatCompositorNode")
        node.plugin_source = "OLMKiraKira.aex"
        node.source_image_name = source.name
        node.output_image_name = "AEXCompat Color Mixed"
        node.transport_mode = "render_aex"
        node.render_path = "smart"
        if set(bpy.ops.aexcompat.refresh_parameters(tree_name=tree.name, node_name=node.name)) != {"FINISHED"}:
            raise RuntimeError("parameter catalog refresh failed")
        described = module.describe_aex(node.plugin_source)
        real_session = module._run_session
        try:
            for invalid_kind in ([], {}):
                forged = dict(described)
                forged["parameter_catalog"] = [dict(entry) for entry in described["parameter_catalog"]]
                forged["parameter_catalog"][0]["kind"] = invalid_kind
                module._run_session = lambda *_args, forged=forged, **_kwargs: forged
                try:
                    module.describe_aex(node.plugin_source)
                except module.AEXCompatSessionError as exc:
                    if str(exc) != "worker_protocol_error":
                        raise RuntimeError("invalid catalog kind got misleading error") from exc
                else:
                    raise RuntimeError("invalid catalog kind entered Blender UI")
        finally:
            module._run_session = real_session
        for slot, name in ((10, "Vertical Length"), (11, "Vertical Color")):
            index = next((index for index, item in enumerate(node.parameter_items)
                          if item.slot == slot and item.name == name), None)
            if index is None:
                raise RuntimeError(f"named parameter missing: {name}")
            node.selected_parameter_index = index
            if set(bpy.ops.aexcompat.choose_parameter(tree_name=tree.name, node_name=node.name)) != {"FINISHED"}:
                raise RuntimeError(f"parameter selection failed: {name}")
        scalar = next(item for item in node.selected_parameter_items if item.slot == 10)
        color = next(item for item in node.selected_parameter_items if item.slot == 11)
        if list(color.color_argb) != [255, 255, 255, 255]:
            raise RuntimeError("color default was not populated")
        scalar.integer_value = 100
        color.color_argb = (255, 255, 0, 0)
        expected = [{"slot": 10, "value": 100}, {"slot": 11, "color": [255, 255, 0, 0]}]
        button = SimpleNamespace()
        module._configure_bake_button(node, button)
        if module._parse_parameter_overrides(button.parameter_overrides_json) != expected:
            raise RuntimeError("Blender controls changed ARGB8 or scalar value")
        for invalid in (
            '[{"slot":11,"color":[255,0,0]}]',
            '[{"slot":11,"color":[255,true,0,0]}]',
            '[{"slot":11,"color":[255,256,0,0]}]',
            '[{"slot":11,"color":[255,0,0,0],"value":1}]',
        ):
            try:
                module._parse_parameter_overrides(invalid)
            except module.AEXCompatSessionError:
                pass
            else:
                raise RuntimeError("malformed ARGB8 operator value was accepted")

        names = ("AEXCompat Color Default", "AEXCompat Color Scalar", "AEXCompat Color Only", node.output_image_name)
        raw = (
            bake(module, node, names[0], source.name),
            bake(module, node, names[1], source.name, expected[:1]),
            bake(module, node, names[2], source.name, expected[1:]),
            bake(module, node, names[3], source.name, expected, native=True),
        )
        shas = [digest(item) for item in raw]
        if len(set(shas)) != 4:
            raise RuntimeError("color and scalar did not independently change real AEX pixels")
        direct, response = node.evaluate_rgba8(observed, size, size)
        if direct != raw[3] or response["parameter_overrides"][1]["color"] != [255, 255, 0, 0]:
            raise RuntimeError("node evaluation differs from mixed bake")
        real_session = module._run_session
        try:
            for index, mutation in (
                (1, {"color": [255, 255, 0, 256]}),
                (1, {"color": [255, True, 0, 0]}),
                (1, {"description_sha256": "0" * 64}),
                (1, {"kind": "float"}),
                (0, {"kind": []}),
                (0, {"kind": {}}),
            ):
                forged = dict(response)
                forged["parameter_overrides"] = [dict(record) for record in response["parameter_overrides"]]
                forged["parameter_overrides"][index].update(mutation)
                module._run_session = lambda *_args, forged=forged, **_kwargs: forged
                try:
                    module.evaluate_rgba8(observed, size, size, plugin_source=node.plugin_source,
                                          mode="render_aex", render_path="smart", parameter_overrides=expected)
                except module.AEXCompatSessionError as exc:
                    if str(exc) != "worker_protocol_error":
                        raise RuntimeError("invalid color response got misleading error") from exc
                else:
                    raise RuntimeError("invalid color response was accepted")
            for invalid_kind in ([], {}):
                forged = dict(response)
                forged["parameter_override"] = dict(response["parameter_overrides"][0], kind=invalid_kind)
                forged.pop("parameter_overrides")
                module._run_session = lambda *_args, forged=forged, **_kwargs: forged
                try:
                    module.evaluate_rgba8(observed, size, size, plugin_source=node.plugin_source,
                                          mode="render_aex", render_path="smart", parameter_override=expected[0])
                except module.AEXCompatSessionError as exc:
                    if str(exc) != "worker_protocol_error":
                        raise RuntimeError("invalid legacy kind got misleading error") from exc
                else:
                    raise RuntimeError("invalid legacy kind was accepted")
        finally:
            module._run_session = real_session

        scene = bpy.context.scene
        scene.render.resolution_x = size
        scene.render.resolution_y = size
        scene.render.resolution_percentage = 100
        png = output / "color-parameter-bake.png"
        scene.render.filepath = str(png)
        if set(bpy.ops.render.render(write_still=True)) != {"FINISHED"} or not png.is_file():
            raise RuntimeError("native compositor PNG failed")
        blend = output / "color-parameter-bake.blend"
        bpy.ops.wm.save_as_mainfile(filepath=str(blend))
        evidence = {
            "schema": "aexcompat.blender_color_parameter_smoke", "schema_version": 1,
            "blender_version": bpy.app.version_string,
            "plugin_identity": response["plugin_identity"],
            "render_identity": response["render_identity"],
            "parameter_overrides": response["parameter_overrides"],
            "input_sha256": digest(observed),
            "image_sha256": dict(zip(names, shas)),
            "mixed_changed_bytes": sum(left != right for left, right in zip(raw[0], raw[3])),
            "native_png_sha256": digest(png.read_bytes()),
            "blend_file": blend.name,
        }
        (output / "color-parameter-smoke.json").write_text(json.dumps(evidence, sort_keys=True, indent=2) + "\n", encoding="utf-8")
        print(json.dumps(evidence, sort_keys=True))
    finally:
        module.unregister()


def reload(output: Path) -> None:
    module = addon()
    module.register()
    try:
        evidence = json.loads((output / "color-parameter-smoke.json").read_text(encoding="utf-8"))
        tree = bpy.data.node_groups.get("AEXCompat Color Smoke")
        node = next((item for item in tree.nodes if item.bl_idname == "AEXCompatCompositorNode"), None) if tree else None
        if node is None or module._named_parameter_overrides(node) != [
            {"slot": 10, "value": 100}, {"slot": 11, "color": [255, 255, 0, 0]},
        ]:
            raise RuntimeError("named color/scalar overrides changed after reload")
        for name, expected in (("AEXCompat Color Source", evidence["input_sha256"]), *evidence["image_sha256"].items()):
            image = bpy.data.images.get(name)
            if image is None or image.packed_file is None:
                raise RuntimeError("packed image missing after reload")
            raw, width, height, _ = module._image_to_rgba8(image)
            if (width, height) != (32, 32) or digest(raw) != expected:
                raise RuntimeError("packed image changed after reload")
        print(json.dumps({"reload": "passed", "mixed_sha256": evidence["image_sha256"]["AEXCompat Color Mixed"]}))
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

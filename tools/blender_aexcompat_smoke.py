"""Background Blender smoke for the public addon contract.

The smoke records Blender API support, addon lifecycle, direct transport
evaluation, save/reload, and the separate compositor background-render
boundary.  A successful Blender render is never reported as an AEX render.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

import bpy


def _sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _addon_import():
    root = Path(__file__).resolve().parents[1]
    sys.path.insert(0, str(root / "blender_addon"))
    import aexcompat_blender

    return aexcompat_blender


def _find_node(node_tree):
    return next((node for node in node_tree.nodes if node.bl_idname == "AEXCompatCompositorNode"), None)


def _background_render_probe() -> dict:
    scene = bpy.context.scene
    scene.use_nodes = True
    tree = scene.node_tree
    tree.nodes.clear()
    render_layers = tree.nodes.new("CompositorNodeRLayers")
    composite = tree.nodes.new("CompositorNodeComposite")
    # Do not put the Python node in Blender's compositor executor at all. A
    # prior direct host probe crashed both installed versions with an access
    # violation; this repeatable smoke keeps the host graph native-only.
    tree.links.new(render_layers.outputs["Image"], composite.inputs["Image"])
    try:
        result = bpy.ops.render.render(write_still=False)
    except Exception as exc:  # Blender's exact exception varies by version
        return {
            "status": "blocked",
            "classification": "blender_compositor_execution_error",
            "operator_result": None,
            "error": str(exc),
            "custom_node_evaluation_observed": False,
        }
    return {
        "status": "completed",
        "classification": "custom_python_node_execution_unverified",
        "operator_result": list(result),
        "custom_node_present_in_render_tree": False,
        "custom_node_evaluation_observed": False,
        "note": "Blender completed a safe background render with the custom node unconnected. A connected custom-node probe crashed the installed hosts with EXCEPTION_ACCESS_VIOLATION and is not repeated.",
    }


def run(output_dir: Path) -> Path:
    output_dir.mkdir(parents=True, exist_ok=True)
    addon = _addon_import()
    addon.register()
    try:
        api = {
            "version": bpy.app.version_string,
            "CompositorNode": hasattr(bpy.types, "CompositorNode"),
            "CompositorNodeOFX": hasattr(bpy.types, "CompositorNodeOFX"),
            "CompositorNodeTree": hasattr(bpy.types, "CompositorNodeTree"),
            "public_api_only": True,
        }
        tree = bpy.data.node_groups.new("AEXCompatSmoke", "CompositorNodeTree")
        tree.use_fake_user = True
        node = tree.nodes.new("AEXCompatCompositorNode")
        node.plugin_source = "fixtures/not-loaded.aex"
        sample = bytes((index * 17) % 256 for index in range(4 * 4 * 4))
        output, first = node.evaluate_rgba8(sample, 4, 4, color_space="scene_linear")
        first_ok = output == sample

        source_image = bpy.data.images.new("AEXCompat Source", width=4, height=4, alpha=True, float_buffer=False)
        source_image.pixels = [value / 255.0 for value in sample]
        node.source_image_name = source_image.name
        node.output_image_name = "AEXCompat Baked"
        node.transport_mode = "fixture_invert_no_aex"
        bake_result = bpy.ops.aexcompat.bake_image(
            source_image_name=source_image.name,
            output_image_name=node.output_image_name,
            plugin_source=node.plugin_source,
            mode="fixture_invert_no_aex",
            connect_native=True,
        )
        baked_image = bpy.data.images.get(node.output_image_name)
        if baked_image is None:
            raise RuntimeError("baked_image_missing")
        baked_raw, baked_width, baked_height, _ = addon._image_to_rgba8(baked_image)
        expected_baked = bytes(value if index % 4 == 3 else 255 - value for index, value in enumerate(sample))
        native_png = output_dir / "aexcompat_native_bake.png"
        scene = bpy.context.scene
        scene.render.resolution_x = 4
        scene.render.resolution_y = 4
        scene.render.resolution_percentage = 100
        scene.render.filepath = str(native_png)
        native_render_result = bpy.ops.render.render(write_still=True)
        native_png_dimensions = None
        if native_png.is_file():
            loaded_native_png = bpy.data.images.load(str(native_png), check_existing=False)
            native_png_dimensions = list(loaded_native_png.size)
            bpy.data.images.remove(loaded_native_png)

        blend_path = output_dir / "aexcompat_blender_smoke.blend"
        bpy.ops.wm.save_as_mainfile(filepath=str(blend_path))

        evidence = {
            "schema_version": 1,
            "report_kind": "aexcompat_blender_smoke",
            "result_state": "baked_image_bridge_smoke_passed_aex_not_loaded" if first_ok and baked_raw == expected_baked else "addon_smoke_failed",
            "blender_api": api,
            "lifecycle": {
                "registered": True,
                "created": True,
                "direct_evaluate": first_ok,
                "saved": blend_path.is_file(),
                "reloaded": "separate_process_required",
            },
            "frame": {
                "width": 4,
                "height": 4,
                "stride": 16,
                "channels": "RGBA8",
                "alpha": "straight",
                "color_space": "scene_linear",
                "frame_time": {"seconds": 0.0},
            },
            "input": {"sha256": _sha256(sample), "bytes": len(sample)},
            "output": {"sha256": _sha256(output), "bytes": len(output)},
            "diff": {"changed_bytes": sum(left != right for left, right in zip(sample, output)), "max_abs_delta": max(abs(left - right) for left, right in zip(sample, output))},
            "baked_fixture": {
                "operator_result": list(bake_result),
                "transport_status": "fixture_transform",
                "failure_class": "aex_not_loaded",
                "aex_render_performed": False,
                "host_success": False,
                "input_sha256": _sha256(sample),
                "output_sha256": _sha256(baked_raw),
                "width": baked_width,
                "height": baked_height,
                "expected_pixel_transform": baked_raw == expected_baked,
                "changed_bytes": sum(left != right for left, right in zip(sample, baked_raw)),
            },
            "session": {
                "status": first.get("status"),
                "failure_class": first.get("failure_class"),
                "aex_render_performed": first.get("aex_render_performed"),
                "host_success": first.get("host_success"),
                "worker_identity": first.get("worker_identity"),
                "plugin_identity": first.get("plugin_identity"),
            },
            "reload_session": {"status": "separate_process_required"},
            "background_render": _background_render_probe(),
            "native_bake_render": {
                "operator_result": list(native_render_result),
                "status": "completed" if native_png.is_file() else "artifact_missing",
                "png_sha256": _sha256(native_png.read_bytes()) if native_png.is_file() else None,
                "dimensions": native_png_dimensions,
                "graph": ["CompositorNodeImage", "CompositorNodeComposite"],
                "custom_node_present": False,
            },
            "artifacts": {"blend_relative": blend_path.name},
        }
        evidence_path = output_dir / "aexcompat_blender_smoke.json"
        evidence_path.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(evidence_path)
        return evidence_path
    finally:
        addon.unregister()


def run_reload(output_dir: Path) -> Path:
    """Inspect a blend loaded by an earlier command-line open-mainfile step."""

    output_dir.mkdir(parents=True, exist_ok=True)
    addon = _addon_import()
    if not hasattr(bpy.types, "AEXCompatCompositorNode"):
        try:
            addon.register()
        except ValueError as exc:
            if "already registered" not in str(exc):
                raise
    tree = bpy.data.node_groups.get("AEXCompatSmoke")
    node = _find_node(tree) if tree else None
    if node is None:
        raise RuntimeError("addon_node_missing_after_reload")
    sample = bytes((index * 17) % 256 for index in range(4 * 4 * 4))
    output, response = node.evaluate_rgba8(sample, 4, 4, color_space="scene_linear")
    expected_output = (
        bytes(value if index % 4 == 3 else 255 - value for index, value in enumerate(sample))
        if node.transport_mode == "fixture_invert_no_aex"
        else sample
    )
    evaluate_after_reload = output == expected_output
    baked_image = bpy.data.images.get("AEXCompat Baked")
    native_nodes = [item.bl_idname for item in bpy.context.scene.node_tree.nodes] if bpy.context.scene.use_nodes else []
    evidence = {
        "schema_version": 1,
        "report_kind": "aexcompat_blender_reload_smoke",
        "result_state": "save_reload_transport_smoke_passed_aex_not_loaded" if evaluate_after_reload else "reload_smoke_failed",
        "blender_api": {"version": bpy.app.version_string, "CompositorNodeOFX": hasattr(bpy.types, "CompositorNodeOFX")},
        "lifecycle": {
            "loaded": True,
            "node_found": True,
            "evaluate_after_reload": evaluate_after_reload,
            "transport_mode": node.transport_mode,
        },
        "baked_image": {
            "loaded": baked_image is not None,
            "native_compositor_graph": native_nodes,
        },
        "input_sha256": _sha256(sample),
        "output_sha256": _sha256(output),
        "session": {
            "status": response.get("status"),
            "failure_class": response.get("failure_class"),
            "aex_render_performed": response.get("aex_render_performed"),
            "host_success": response.get("host_success"),
        },
    }
    evidence_path = output_dir / "aexcompat_blender_reload_smoke.json"
    evidence_path.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(evidence_path)
    return evidence_path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--reload", action="store_true")
    args = parser.parse_args(sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else sys.argv[1:])
    if args.reload:
        run_reload(args.output_dir)
    else:
        run(args.output_dir)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

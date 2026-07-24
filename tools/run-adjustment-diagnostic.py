"""Pairwise Adjustment Layer diagnostics built on the Issue #4 runner.

This module intentionally does not render an AEX itself.  It generates the two
standard scenes, invokes ``run-conformance-bundle.py`` once per application
mode, and then compares the resulting bundle-local raw artifacts.  A supplied
adapter is test-only evidence; it is never labelled as native AEX evidence.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

from jsonschema import Draft202012Validator
from PIL import Image
from referencing import Registry, Resource


ROOT = Path(__file__).resolve().parents[1]
SCHEMAS = ROOT / "schemas"
CONFORMANCE_RUNNER = ROOT / "tools" / "run-conformance-bundle.py"
DEPTHS = ("argb8", "argb16", "argb32f")
PIXEL_BYTES = {"argb8": 4, "argb16": 8, "argb32f": 16}
CHANNEL_BYTES = {"argb8": 1, "argb16": 2, "argb32f": 4}
CAUSES = {
    "layer_flag_or_admission", "composite_input", "effect_stack_order", "extent_roi",
    "alpha_premultiplication", "resolution_downsample", "selector_suite",
    "session_transport", "identity_mismatch", "external_unavailable",
}


def _json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def _digest(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def _artifact(path: Path, relative: str) -> dict[str, Any]:
    return {"path": relative.replace("\\", "/"), "sha256": _digest(path), "size_bytes": path.stat().st_size}


def _safe_source(root: Path, artifact: dict[str, Any]) -> Path:
    raw = artifact["path"]
    if "\\" in raw or Path(raw).is_absolute() or any(part in {"", ".", ".."} for part in raw.split("/")):
        raise ValueError(f"unsafe artifact path: {raw}")
    path = (root / Path(*raw.split("/"))).resolve()
    if root.resolve() not in path.parents:
        raise ValueError(f"artifact escapes manifest root: {raw}")
    if not path.is_file() or path.stat().st_size != artifact["size_bytes"] or _digest(path) != artifact["sha256"]:
        raise ValueError(f"artifact identity mismatch: {raw}")
    return path


def _manifest_validator() -> Draft202012Validator:
    manifest = _json(SCHEMAS / "adjustment-diagnostic-manifest.schema.json")
    conformance = _json(SCHEMAS / "conformance-manifest.schema.json")
    registry = Registry().with_resource(conformance["$id"], Resource.from_contents(conformance))
    return Draft202012Validator(manifest, registry=registry)


def _validate_manifest(manifest: dict[str, Any]) -> None:
    errors = sorted(_manifest_validator().iter_errors(manifest), key=lambda error: list(error.absolute_path))
    if errors:
        raise ValueError("; ".join(f"{'.'.join(map(str, e.absolute_path))}: {e.message}" for e in errors))
    if manifest["execution"]["premultiplication"] != manifest["execution"]["alpha_policy"]:
        raise ValueError("premultiplication and alpha_policy must agree")


def _rect(width: int, height: int) -> dict[str, int]:
    return {"left": 0, "top": 0, "right": width, "bottom": height}


def _layer(layer_id: str, kind: str, order: int, flags: list[str], coverage: str, width: int, height: int, roi: dict[str, int] | None = None) -> dict[str, Any]:
    return {"layer_id": layer_id, "kind": kind, "order": order, "flags": flags, "coverage": coverage, "extent": _rect(width, height), "roi": roi or _rect(width, height)}


def _load_rgba(path: Path, width: int, height: int) -> Image.Image:
    with Image.open(path) as source:
        image = source.convert("RGBA")
    if image.size != (width, height):
        raise ValueError(f"source image {path} is {image.size}, expected {(width, height)}")
    return image


def _generate_scenes(manifest: dict[str, Any], source_root: Path, output_root: Path) -> tuple[list[dict[str, Any]], dict[str, Path]]:
    width = manifest["execution"]["resolution"]["width"]
    height = manifest["execution"]["resolution"]["height"]
    sources = {source["role"]: _safe_source(source_root, source["artifact"]) for source in manifest["sources"]}
    if "opaque_full_frame" not in sources or "alpha_material" not in sources:
        raise ValueError("sources must contain opaque_full_frame and alpha_material")
    opaque = _load_rgba(sources["opaque_full_frame"], width, height)
    alpha = _load_rgba(sources["alpha_material"], width, height)
    inputs = output_root / "inputs"
    inputs.mkdir(parents=True, exist_ok=True)
    scenes: list[dict[str, Any]] = []
    paths: dict[str, Path] = {}
    opaque_path = inputs / "opaque_full_frame.png"
    opaque.save(opaque_path, format="PNG")
    paths["opaque_full_frame"] = opaque_path
    scenes.append({
        "scene_id": "opaque_full_frame", "kind": "opaque_full_frame", "alpha_policy": "opaque",
        "layers": [_layer("background", "source", 0, ["source", "opaque"], "full_frame", width, height),
                    _layer("effect", "effect", 1, ["effect", "direct_or_adjustment"], "full_frame", width, height)],
        "composite_input": _artifact(opaque_path, "inputs/opaque_full_frame.png"),
    })
    composite = Image.alpha_composite(opaque, alpha)
    composite_path = inputs / "alpha_extent_roi.png"
    composite.save(composite_path, format="PNG")
    paths["alpha_extent_roi"] = composite_path
    alpha_bbox = alpha.getchannel("A").getbbox()
    roi = _rect(width, height) if alpha_bbox is None else {"left": alpha_bbox[0], "top": alpha_bbox[1], "right": alpha_bbox[2], "bottom": alpha_bbox[3]}
    scenes.append({
        "scene_id": "alpha_extent_roi", "kind": "alpha_extent_roi", "alpha_policy": manifest["execution"]["alpha_policy"],
        "layers": [_layer("background", "source", 0, ["source", "alpha_capable"], "partial", width, height),
                    _layer("foreground", "source", 1, ["source", "transparent_regions"], "transparent", width, height, roi),
                    _layer("effect", "effect", 2, ["effect", "direct_or_adjustment"], "partial", width, height, roi)],
        "composite_input": _artifact(composite_path, "inputs/alpha_extent_roi.png"),
    })
    return scenes, paths


def _copy_to_stage(source_root: Path, stage: Path, artifact: dict[str, Any]) -> dict[str, Any]:
    source = _safe_source(source_root, artifact)
    destination = stage / Path(*artifact["path"].split("/"))
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)
    return _artifact(destination, artifact["path"])


def _common_identity(manifest: dict[str, Any], scene: dict[str, Any]) -> dict[str, Any]:
    execution = manifest["execution"]
    return {
        "aex": manifest["plugin"]["aex"], "parameters": execution["parameters"], "time": execution["time"],
        "resolution": execution["resolution"], "depths": manifest["requested_depths"], "renderer": execution["renderer"],
        "alpha_policy": scene["alpha_policy"], "premultiplication": execution["premultiplication"], "downsample": execution["downsample"],
    }



def _application_layers(scene: dict[str, Any], mode: str) -> list[dict[str, Any]]:
    layers = []
    for layer in scene["layers"]:
        value = dict(layer)
        value["flags"] = list(layer["flags"])
        if layer["layer_id"] == "effect":
            if mode == "adjustment":
                value["kind"] = "adjustment"
                value["flags"] = ["adjustment", "adjustment_layer"]
            else:
                value["kind"] = "effect"
                value["flags"] = ["effect", "direct_application"]
        layers.append(value)
    return layers


def _conformance_runner_module():
    spec = importlib.util.spec_from_file_location("adjustment_conformance_runner", CONFORMANCE_RUNNER)
    module = importlib.util.module_from_spec(spec)
    if spec.loader is None:
        raise RuntimeError("cannot load Issue #4 conformance runner")
    spec.loader.exec_module(module)
    return module


def _expected_raw_input(source: Path, output_root: Path, scene_id: str, depth: str, premultiplication: str) -> dict[str, Any]:
    suffix = {"argb8": "rgba8", "argb16": "rgba16le", "argb32f": "rgba32f-le"}[depth]
    destination = output_root / "inputs" / f"{scene_id}-expected.{depth}.{suffix}"
    _conformance_runner_module().write_native_input(source, destination, depth, premultiplication)
    return _artifact(destination, f"inputs/{destination.name}")


def _base_manifest(manifest: dict[str, Any], aex: dict[str, Any], dependencies: list[dict[str, Any]], runner: dict[str, Any], input_artifact: dict[str, Any]) -> dict[str, Any]:
    execution = manifest["execution"]
    return {
        "schema_version": 1, "fixture_id": f"{manifest['diagnostic_id']}-pair",
        "plugin": {"aex": aex, "dependencies": dependencies}, "input": input_artifact, "runner": runner,
        "requested_depths": manifest["requested_depths"],
        "execution": {key: execution[key] for key in ("render_path", "time", "parameters", "premultiplication", "color_management", "linear_light", "renderer")},
        "oracle": {"state": "not_requested", "identity_match": False},
    }


def _remap_artifact(value: dict[str, Any] | None, prefix: str) -> dict[str, Any] | None:
    if value is None:
        return None
    mapped = dict(value)
    mapped["path"] = f"{prefix}/{value['path']}"
    return mapped


def _outcome(bundle: Path, top_root: Path, mode: str, returncode: int) -> dict[str, Any]:
    report_path = bundle / "report.json"
    manifest_path = bundle / "manifest.json"
    prefix = f"pairs/{mode}"
    if not report_path.is_file():
        return {"status": "blocked_external", "manifest_path": f"{prefix}/manifest.json", "report_path": f"{prefix}/report.json", "results": [{
            "depth": "argb8", "classification": "blocked_external", "selector": {"render_path": "classic", "completed": False, "error_code": None},
            "input_world": None, "world": None, "raw_input": None, "raw_output": None, "output_sha256": None, "suite_timeline": None,
        }], "diagnostics": {"selector": [], "suite_timeline": [], "session": {"application_mode": mode, "status": "blocked_external"}, "worker": None}}
    report = _json(report_path)
    results = []
    for result in report.get("results", []):
        item = {key: result.get(key) for key in ("depth", "classification", "selector", "input_world", "world", "raw_input", "raw_output", "output_sha256", "suite_timeline")}
        item["raw_input"] = _remap_artifact(item["raw_input"], prefix)
        item["raw_output"] = _remap_artifact(item["raw_output"], prefix)
        results.append(item)
    status = "completed" if returncode == 0 and not any(item["classification"] not in {"ok", "empty_result"} for item in results) else "failed"
    return {"status": status, "manifest_path": f"{prefix}/manifest.json", "report_path": f"{prefix}/report.json", "results": results, "diagnostics": {"selector": [item.get("selector") for item in results], "suite_timeline": [item.get("suite_timeline") for item in results], "session": {"application_mode": mode, "status": status}, "worker": report.get("identities", {}).get("workers", [])}}


def _raw_path(root: Path, artifact: dict[str, Any] | None) -> Path | None:
    if artifact is None:
        return None
    return root / Path(*artifact["path"].split("/"))


def _diff_depth(top_root: Path, direct: dict[str, Any], adjustment: dict[str, Any], depth: str) -> dict[str, Any]:
    left = next((item for item in direct["results"] if item["depth"] == depth), None)
    right = next((item for item in adjustment["results"] if item["depth"] == depth), None)
    if not left or not right or left["classification"] != "ok" or right["classification"] != "ok":
        return {"depth": depth, "comparable": False, "mismatched_pixels": 0, "alpha_mismatched_pixels": 0, "premultiplication_mismatch": bool(left and right and left.get("world") and right.get("world") and left["world"].get("premultiplication") != right["world"].get("premultiplication"))}
    left_path = _raw_path(top_root, left["raw_output"])
    right_path = _raw_path(top_root, right["raw_output"])
    if left_path is None or right_path is None or not left_path.is_file() or not right_path.is_file():
        return {"depth": depth, "comparable": False, "mismatched_pixels": 0, "alpha_mismatched_pixels": 0, "premultiplication_mismatch": False}
    left_bytes, right_bytes = left_path.read_bytes(), right_path.read_bytes()
    step, alpha_bytes = PIXEL_BYTES[depth], CHANNEL_BYTES[depth]
    pixels = max((len(left_bytes) + step - 1) // step, (len(right_bytes) + step - 1) // step)
    mismatched = alpha_mismatched = 0
    for index in range(pixels):
        l = left_bytes[index * step:(index + 1) * step]
        r = right_bytes[index * step:(index + 1) * step]
        if l != r:
            mismatched += 1
        if l[:alpha_bytes] != r[:alpha_bytes]:
            alpha_mismatched += 1
    worlds_differ = bool(left.get("world") and right.get("world") and (left["world"].get("extent_hint") != right["world"].get("extent_hint") or left["world"].get("premultiplication") != right["world"].get("premultiplication")))
    return {"depth": depth, "comparable": True, "mismatched_pixels": mismatched, "alpha_mismatched_pixels": alpha_mismatched, "premultiplication_mismatch": worlds_differ}


def _classify(direct: dict[str, Any], adjustment: dict[str, Any], diffs: list[dict[str, Any]] | None, blocked: bool, input_identity_match: bool) -> tuple[str, list[str]]:
    if blocked:
        return "blocked_external", ["external_unavailable"]
    if not input_identity_match:
        return "identity_mismatch", ["composite_input", "identity_mismatch"]
    direct_ok = all(item["classification"] in {"ok", "empty_result"} for item in direct["results"])
    adjustment_ok = all(item["classification"] in {"ok", "empty_result"} for item in adjustment["results"])
    if direct_ok and adjustment_ok:
        if diffs is not None and any(item["comparable"] and item["mismatched_pixels"] for item in diffs):
            causes = ["composite_input"]
            if any(item["alpha_mismatched_pixels"] or item["premultiplication_mismatch"] for item in diffs):
                causes.append("alpha_premultiplication")
            return "pixel_diverged", causes
        return "both_succeeded", []
    if direct_ok and not adjustment_ok:
        return "direct_ok_adjustment_failed", ["layer_flag_or_admission", "effect_stack_order"]
    if not direct_ok and adjustment_ok:
        return "direct_failed_adjustment_ok", ["selector_suite"]
    return "both_failed", ["selector_suite"]


def _write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def run(manifest_path: Path, output_root: Path, adapter: Path | None, include_pixel_diff: bool = False) -> int:
    manifest = _json(manifest_path)
    _validate_manifest(manifest)
    source_root = manifest_path.parent.resolve()
    for item in [manifest["plugin"]["aex"], *manifest["plugin"]["dependencies"], manifest["runner"]]:
        _safe_source(source_root, item)
    output_root = output_root.resolve()
    if output_root.exists():
        raise ValueError("diagnostic output must not already exist")
    output_root.mkdir(parents=True)
    stage = Path(tempfile.mkdtemp(prefix=f"{manifest['diagnostic_id']}-", dir=str(output_root.parent)))
    try:
        scenes, input_paths = _generate_scenes(manifest, source_root, output_root)
        staged_aex = _copy_to_stage(source_root, stage, manifest["plugin"]["aex"])
        staged_dependencies = [_copy_to_stage(source_root, stage, item) for item in manifest["plugin"]["dependencies"]]
        staged_runner = _copy_to_stage(source_root, stage, manifest["runner"])
        common_identity = {scene["scene_id"]: _common_identity(manifest, scene) for scene in scenes}
        native_available = adapter is not None
        pairs: list[dict[str, Any]] = []
        for scene in scenes:
            scene_id = scene["scene_id"]
            pair_root = output_root / "pairs"
            direct_bundle = pair_root / f"{scene_id}-direct"
            adjustment_bundle = pair_root / f"{scene_id}-adjustment"
            modes = (("direct", direct_bundle), ("adjustment", adjustment_bundle))
            outcomes: dict[str, dict[str, Any]] = {}
            for mode, bundle in modes:
                pair_stage_input = stage / "inputs" / f"{scene_id}-{mode}.png"
                pair_stage_input.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(input_paths[scene_id], pair_stage_input)
                input_artifact = _artifact(pair_stage_input, f"inputs/{scene_id}-{mode}.png")
                base = _base_manifest(manifest, staged_aex, staged_dependencies, staged_runner, input_artifact)
                pair_manifest = stage / f"{scene_id}-{mode}.manifest.json"
                _write_json(pair_manifest, base)
                application = {"schema_version": 1, "scene_id": scene_id, "application_mode": mode, "layers": _application_layers(scene, mode), "composite_input": scene["composite_input"], "input_artifact": input_artifact}
                application_path = stage / f"{scene_id}-{mode}.application.json"
                _write_json(application_path, application)
                _write_json(output_root / "diagnostics" / f"{scene_id}-{mode}-application.json", application)
                if not native_available:
                    outcomes[mode] = {"status": "blocked_external", "manifest_path": f"pairs/{scene_id}-{mode}/manifest.json", "report_path": f"pairs/{scene_id}-{mode}/report.json", "results": [{"depth": manifest["requested_depths"][0], "classification": "blocked_external", "selector": {"render_path": manifest["execution"]["render_path"], "completed": False, "error_code": None}, "input_world": None, "world": None, "raw_input": None, "raw_output": None, "output_sha256": None, "suite_timeline": None}], "diagnostics": {"selector": [], "suite_timeline": [], "session": {"application_mode": mode, "status": "blocked_external"}, "worker": None}}
                    continue
                environment = dict(os.environ)
                environment["AEXCOMPAT_ADJUSTMENT_APPLICATION"] = mode
                environment["AEXCOMPAT_ADJUSTMENT_SCENE"] = str(application_path)
                command = [sys.executable, str(CONFORMANCE_RUNNER), "--manifest", str(pair_manifest), "--out", str(bundle), "--allow-failures"]
                if adapter is not None:
                    command.extend(["--adapter-command", str(adapter.resolve())])
                completed = subprocess.run(command, cwd=ROOT, env=environment, capture_output=True, text=True)
                _write_json(output_root / "diagnostics" / f"{scene_id}-{mode}-process.json", {"returncode": completed.returncode, "stdout": completed.stdout[-4096:], "stderr": completed.stderr[-4096:], "application_mode": mode})
                outcomes[mode] = _outcome(bundle, output_root, f"{scene_id}-{mode}", completed.returncode)
            diffs = [_diff_depth(output_root, outcomes["direct"], outcomes["adjustment"], depth) for depth in manifest["requested_depths"]] if include_pixel_diff else None
            raw_inputs = []
            for depth in manifest["requested_depths"]:
                expected = _expected_raw_input(input_paths[scene_id], output_root, scene_id, depth, manifest["execution"]["premultiplication"])
                adjustment_result = next((item for item in outcomes["adjustment"]["results"] if item["depth"] == depth), None)
                actual = adjustment_result.get("raw_input") if adjustment_result else None
                actual_path = _raw_path(output_root, actual)
                identity_match = bool(actual and actual_path and actual_path.is_file() and _digest(actual_path) == expected["sha256"] and actual_path.stat().st_size == expected["size_bytes"])
                raw_inputs.append({"depth": depth, "expected": expected, "adjustment": actual, "identity_match": identity_match})
            input_identity_match = all(item["identity_match"] for item in raw_inputs)
            blocked = outcomes["direct"]["status"] == "blocked_external" or outcomes["adjustment"]["status"] == "blocked_external"
            classification, causes = _classify(outcomes["direct"], outcomes["adjustment"], diffs, blocked, input_identity_match)
            pairs.append({"pair_id": f"{manifest['diagnostic_id']}-{scene_id}", "scene_id": scene_id, "application_modes": ["direct", "adjustment"], "common_identity": common_identity[scene_id], "layer_stacks": {"direct": _application_layers(scene, "direct"), "adjustment": _application_layers(scene, "adjustment")}, "composite_input": {"source_ids": ["opaque"] if scene_id == "opaque_full_frame" else ["opaque", "alpha"], "png": scene["composite_input"], "recomputed_sha256": _digest(input_paths[scene_id]), "raw_inputs": raw_inputs}, "direct": outcomes["direct"], "adjustment": outcomes["adjustment"], "classification": classification, "cause_candidates": causes, "diff": {"depths": diffs} if include_pixel_diff else None})
        top_status = "blocked_external" if all(pair["classification"] == "blocked_external" for pair in pairs) else ("completed" if all(pair["classification"] == "both_succeeded" for pair in pairs) else "completed_with_failures")
        report = {"schema_version": 1, "diagnostic_id": manifest["diagnostic_id"], "status": top_status, "execution_evidence": "adapter" if adapter else "none", "blocked_reason": None if adapter else "no scene-capable native AEX execution was available", "identity": {"aex": manifest["plugin"]["aex"], "dependencies": manifest["plugin"]["dependencies"], "runner": manifest["runner"]}, "scenes": scenes, "pairs": pairs}
        _write_json(output_root / "manifest.json", manifest)
        _write_json(output_root / "report.json", report)
        return 0 if top_status == "completed" else 3
    finally:
        shutil.rmtree(stage, ignore_errors=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--adapter-command", type=Path, help="test-only adapter; evidence is labelled adapter")
    parser.add_argument("--pixel-diff", action="store_true", help="optional detailed raw-pixel/alpha diff artifact")
    args = parser.parse_args()
    try:
        return run(args.manifest.resolve(), args.out, args.adapter_command, args.pixel_diff)
    except Exception as error:
        if args.out:
            args.out.mkdir(parents=True, exist_ok=True)
            _write_json(args.out / "report.json", {"schema_version": 1, "diagnostic_id": "invalid", "status": "failed", "execution_evidence": "none", "blocked_reason": str(error), "identity": {"aex": {"path": "invalid", "sha256": "0" * 64, "size_bytes": 0}, "dependencies": [], "runner": {"path": "invalid", "sha256": "0" * 64, "size_bytes": 0}}, "scenes": [], "pairs": []})
        raise


if __name__ == "__main__":
    raise SystemExit(main())

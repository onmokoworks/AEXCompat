#!/usr/bin/env python3
"""Create a self-contained AEX conformance evidence bundle."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import struct
import subprocess
import sys
from pathlib import Path
from typing import Any

from jsonschema import Draft202012Validator
from PIL import Image
from referencing import Registry, Resource

from conformance_bundle_validator import validate_bundle


ROOT = Path(__file__).resolve().parents[1]
SCHEMAS = ROOT / "schemas"
MAX_DIAGNOSTIC_BYTES = 64 * 1024
DEPTH_COMMANDS = {
    "argb8": "--render-experimental-smart",
    "argb16": "--render-experimental-smart-16",
    "argb32f": "--render-experimental-smart-32-cpu",
}


def load_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def validator(name: str) -> Draft202012Validator:
    schema = load_json(SCHEMAS / name)
    manifest = load_json(SCHEMAS / "conformance-manifest.schema.json")
    registry = Registry().with_resource(
        manifest["$id"], Resource.from_contents(manifest)
    )
    return Draft202012Validator(schema, registry=registry)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def resolve_artifact(root: Path, artifact: dict[str, Any]) -> Path:
    path = (root / artifact["path"]).resolve()
    try:
        path.relative_to(root.resolve())
    except ValueError as error:
        raise ValueError(f"artifact escapes manifest root: {artifact['path']}") from error
    if not path.is_file():
        raise ValueError(f"artifact is unavailable: {artifact['path']}")
    size = path.stat().st_size
    actual_hash = sha256(path)
    if size != artifact["size_bytes"] or actual_hash != artifact["sha256"]:
        raise ValueError(
            f"artifact identity mismatch: {artifact['path']} "
            f"expected={artifact['size_bytes']}/{artifact['sha256']} "
            f"actual={size}/{actual_hash}"
        )
    return path


def bounded(text: str) -> dict[str, Any]:
    raw = text.encode("utf-8", errors="replace")
    truncated = len(raw) > MAX_DIAGNOSTIC_BYTES
    raw = raw[:MAX_DIAGNOSTIC_BYTES]
    return {"text": raw.decode("utf-8", errors="replace"), "truncated": truncated}


def world(width: int, height: int, depth: str, premultiplication: str) -> dict[str, Any]:
    bytes_per_pixel = {"argb8": 4, "argb16": 8, "argb32f": 16}[depth]
    return {
        "width": width,
        "height": height,
        "row_bytes": width * bytes_per_pixel,
        "pixel_format": depth,
        "premultiplication": premultiplication,
        "extent_hint": {"left": 0, "top": 0, "right": width, "bottom": height},
    }


def artifact_for(path: Path, root: Path) -> dict[str, Any]:
    return {
        "path": path.relative_to(root).as_posix(),
        "sha256": sha256(path),
        "size_bytes": path.stat().st_size,
    }


def write_native_input(source: Path, destination: Path, depth: str) -> tuple[int, int]:
    with Image.open(source) as image:
        rgba = image.convert("RGBA")
        width, height = rgba.size
        samples = rgba.tobytes()
    destination.parent.mkdir(parents=True, exist_ok=True)
    if depth == "argb8":
        destination.write_bytes(samples)
    elif depth == "argb16":
        with destination.open("wb") as stream:
            for sample in samples:
                stream.write(struct.pack("<H", (sample * 32768 + 127) // 255))
    else:
        with destination.open("wb") as stream:
            for sample in samples:
                stream.write(struct.pack("<f", sample / 255.0))
    return width, height


def failed_result(
    depth: str, input_world: dict[str, Any], classification: str = "nonzero_exit"
) -> dict[str, Any]:
    return {
        "depth": depth,
        "classification": classification,
        "selector": {"render_path": "smartfx", "completed": False, "error_code": None},
        "input_world": input_world,
        "world": None,
        "raw_input": None,
        "raw_output": None,
        "output_sha256": None,
        "suite_timeline": [],
        "oracle": {"state": "not_captured", "identity_match": False, "exact": False},
    }


def normalize_harness_report(
    depth: str,
    value: dict[str, Any],
    output: Path,
    input_world: dict[str, Any],
    premultiplication: str,
) -> dict[str, Any]:
    if not value.get("passed") or not output.is_file():
        return failed_result(depth, input_world, "invalid_output")
    width = int(value["width"])
    height = int(value["height"])
    result = {
        "depth": depth,
        "classification": "ok",
        "selector": {
            "render_path": value.get("render_path", "smartfx"),
            "completed": True,
            "error_code": 0,
        },
        "input_world": input_world,
        "world": world(width, height, depth, premultiplication),
        "raw_input": None,
        "raw_output": None,
        "output_sha256": sha256(output),
        "suite_timeline": value.get("suite_timeline", [])[:65536],
        "oracle": {"state": "not_captured", "identity_match": False, "exact": False},
    }
    missing = value.get("missing_suites")
    if isinstance(missing, list) and missing:
        result["missing_suites"] = missing[:16]
    return result


def run_depth(
    depth: str,
    harness: Path,
    plugin: Path,
    input_path: Path,
    output: Path,
    dump_dir: Path,
    adapter: Path | None,
    input_world: dict[str, Any],
    premultiplication: str,
) -> tuple[dict[str, Any], dict[str, Any]]:
    if adapter:
        command = [
            sys.executable, str(adapter), "--depth", depth, "--runner", str(harness),
            "--plugin", str(plugin), "--input", str(input_path), "--output", str(output),
            "--world-dump-dir", str(dump_dir),
        ]
    else:
        command = [str(harness), DEPTH_COMMANDS[depth], str(plugin), str(input_path), str(output)]
    environment = dict(os.environ)
    environment["AEXCOMPAT_DUMP_WORLDS_DIR"] = dump_dir.relative_to(ROOT).as_posix()
    environment["AEXCOMPAT_CHECKSUM_DETAIL"] = "1"
    try:
        completed = subprocess.run(
            command, cwd=ROOT, env=environment, capture_output=True, text=True,
            timeout=120, check=False,
        )
        diagnostic = {
            "command": [Path(command[0]).name, *command[1:2]],
            "exit_code": completed.returncode,
            "stdout": bounded(completed.stdout),
            "stderr": bounded(completed.stderr),
        }
        try:
            value = json.loads(completed.stdout)
        except json.JSONDecodeError:
            return failed_result(depth, input_world), diagnostic
        if adapter:
            if completed.returncode != 0:
                return failed_result(depth, input_world), diagnostic
            if value.get("classification") == "ok":
                if not output.is_file() or value.get("output_sha256") != sha256(output):
                    return failed_result(depth, input_world, "invalid_output"), diagnostic
            return value, diagnostic
        if completed.returncode != 0:
            return failed_result(depth, input_world), diagnostic
        return normalize_harness_report(
            depth, value, output, input_world, premultiplication
        ), diagnostic
    except subprocess.TimeoutExpired as error:
        diagnostic = {
            "command": [Path(command[0]).name, *command[1:2]],
            "exit_code": None,
            "stdout": bounded(error.stdout or ""),
            "stderr": bounded(error.stderr or ""),
        }
        return failed_result(depth, input_world, "timeout_killed"), diagnostic


def attach_raw_artifacts(
    result: dict[str, Any],
    depth: str,
    source_input: Path,
    output: Path,
    dump_dir: Path,
    bundle_root: Path,
) -> None:
    raw_root = bundle_root / "raw" / depth
    raw_root.mkdir(parents=True, exist_ok=True)
    input_candidates = sorted(dump_dir.glob("*input*")) if dump_dir.exists() else []
    output_candidates = sorted(dump_dir.glob("*output*")) if dump_dir.exists() else []
    raw_input = raw_root / f"input.{ {'argb8':'rgba8','argb16':'rgba16le','argb32f':'rgba32f-le'}[depth] }"
    if input_candidates:
        shutil.copyfile(input_candidates[-1], raw_input)
    else:
        write_native_input(source_input, raw_input, depth)
    result["raw_input"] = artifact_for(raw_input, bundle_root)

    if result["classification"] != "ok":
        result["raw_output"] = None
        return
    raw_output = raw_root / f"output.{ {'argb8':'rgba8','argb16':'rgba16le','argb32f':'rgba32f-le'}[depth] }"
    if output_candidates:
        shutil.copyfile(output_candidates[-1], raw_output)
    elif depth != "argb8" and output.with_suffix("." + {"argb16": "rgba16le", "argb32f": "rgba32f-le"}[depth]).is_file():
        shutil.copyfile(output.with_suffix("." + {"argb16": "rgba16le", "argb32f": "rgba32f-le"}[depth]), raw_output)
    elif depth == "argb8":
        try:
            write_native_input(output, raw_output, depth)
        except Exception:
            result.update({"classification": "invalid_output", "world": None, "raw_output": None, "output_sha256": None})
            return
    else:
        result.update({"classification": "invalid_output", "world": None, "raw_output": None, "output_sha256": None})
        return
    result["raw_output"] = artifact_for(raw_output, bundle_root)
    result["output_sha256"] = result["raw_output"]["sha256"]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--adapter-command", type=Path)
    args = parser.parse_args()

    manifest_path = args.manifest.resolve()
    manifest = load_json(manifest_path)
    validator("conformance-manifest.schema.json").validate(manifest)
    source_root = manifest_path.parent

    artifacts = [manifest["plugin"]["aex"], *manifest["plugin"]["dependencies"], manifest["input"], manifest["runner"]]
    if "artifact" in manifest["oracle"]:
        artifacts.append(manifest["oracle"]["artifact"])
    resolved = {item["path"]: resolve_artifact(source_root, item) for item in artifacts}

    output_root = args.out.resolve()
    output_root.mkdir(parents=True, exist_ok=False)
    try:
        for item in artifacts:
            destination = output_root / item["path"]
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(resolved[item["path"]], destination)
        (output_root / "manifest.json").write_text(
            json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        outputs = output_root / "outputs"
        diagnostics_dir = output_root / "diagnostics"
        outputs.mkdir()
        diagnostics_dir.mkdir()
        results = []
        diagnostics = {}
        with Image.open(output_root / manifest["input"]["path"]) as source_image:
            input_width, input_height = source_image.size
        for depth in manifest["requested_depths"]:
            output = outputs / f"{depth}.png"
            dump_dir = ROOT / "target" / f"conformance-{manifest['fixture_id']}-{os.getpid()}-{depth}"
            if dump_dir.exists():
                shutil.rmtree(dump_dir)
            input_world = world(
                input_width,
                input_height,
                depth,
                manifest["execution"]["premultiplication"],
            )
            result, detail = run_depth(
                depth,
                output_root / manifest["runner"]["path"],
                output_root / manifest["plugin"]["aex"]["path"],
                output_root / manifest["input"]["path"],
                output,
                dump_dir,
                args.adapter_command.resolve() if args.adapter_command else None,
                input_world,
                manifest["execution"]["premultiplication"],
            )
            attach_raw_artifacts(
                result,
                depth,
                output_root / manifest["input"]["path"],
                output,
                dump_dir,
                output_root,
            )
            if dump_dir.exists():
                shutil.move(str(dump_dir), str(outputs / f"{depth}-worlds"))
            results.append(result)
            diagnostics[depth] = detail
        report = {
            "schema_version": 1,
            "fixture_id": manifest["fixture_id"],
            "identities": {
                "aex": manifest["plugin"]["aex"],
                "dependencies": manifest["plugin"]["dependencies"],
                "input": manifest["input"],
                "runner": manifest["runner"],
            },
            "parameters": [
                {
                    "index": parameter["index"],
                    "type": parameter["type"],
                    "initial_value": parameter["value"],
                    "host_range": None,
                    "user_range": None,
                }
                for parameter in manifest["execution"]["parameters"]
            ],
            "results": results,
        }
        validate_bundle(manifest, report, output_root)
        (output_root / "report.json").write_text(
            json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        metadata = {
            "schema_version": 1,
            "bundle_runner": {
                "path": "tools/run-conformance-bundle.py",
                "sha256": sha256(Path(__file__)),
                "size_bytes": Path(__file__).stat().st_size,
            },
            "diagnostics": diagnostics,
        }
        (diagnostics_dir / "run.json").write_text(
            json.dumps(metadata, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
    except Exception:
        # Preserve the create-new directory and any bounded evidence already written.
        raise
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Run declarative AEXCompat no-load tool pipelines safely and sequentially."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TOOLS_ROOT = LAB_ROOT / "tools"
TARGET_ROOT = LAB_ROOT / "target"
ARTIFACT_INDEX_ROOT = TARGET_ROOT / "artifact-index"
PIPELINE_ROOT = LAB_ROOT / "pipelines"
STAGE_KEYS = {"stage_id", "tool", "args", "inputs", "output_root"}


def read_object(path: Path) -> dict[str, Any]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        raise ValueError("JSON must be an object")
    return payload


def validate_tool_name(name: Any) -> Path:
    if not isinstance(name, str) or not name or Path(name).name != name or "/" in name or "\\" in name or ".." in name:
        raise ValueError("tool must be a filename directly under tools")
    if not name.endswith(".py"):
        raise ValueError("tool must be a Python file")
    path = TOOLS_ROOT / name
    if not path.is_file():
        raise ValueError(f"tool does not exist: {name}")
    return path


def validate_output_root(value: Any) -> str:
    if not isinstance(value, str) or not value:
        raise ValueError("output_root must be a non-empty string")
    path = Path(value)
    if any(part in {".", ".."} for part in path.parts):
        raise ValueError("output_root must not contain traversal")
    resolved = (path if path.is_absolute() else LAB_ROOT / path).resolve(strict=False)
    if not resolved.is_relative_to(TARGET_ROOT.resolve(strict=True)):
        raise ValueError("output_root must stay under target")
    return value


def validate_manifest(payload: dict[str, Any]) -> dict[str, Any]:
    if set(payload) != {"pipeline_name", "schema_version", "stages"}:
        raise ValueError("pipeline manifest fields are invalid")
    if payload.get("schema_version") != 1 or not isinstance(payload.get("pipeline_name"), str) or not payload["pipeline_name"]:
        raise ValueError("pipeline metadata is invalid")
    stages = payload.get("stages")
    if not isinstance(stages, list) or not stages:
        raise ValueError("pipeline stages must be non-empty")
    ids: set[str] = set()
    for stage in stages:
        if not isinstance(stage, dict) or set(stage) != STAGE_KEYS:
            raise ValueError("stage fields are invalid")
        stage_id = stage.get("stage_id")
        if not isinstance(stage_id, str) or not stage_id or stage_id in ids:
            raise ValueError("stage_id must be non-empty and unique")
        ids.add(stage_id)
        validate_tool_name(stage.get("tool"))
        if not isinstance(stage.get("args"), list) or not all(isinstance(arg, str) for arg in stage["args"]):
            raise ValueError("stage args must be strings")
        inputs = stage.get("inputs")
        if not isinstance(inputs, dict) or not all(isinstance(k, str) and isinstance(v, str) for k, v in inputs.items()):
            raise ValueError("stage inputs must be a string map")
        for source in inputs.values():
            if not (source.startswith("artifact:") or source.startswith("literal:")):
                raise ValueError("input must use artifact: or literal:")
        validate_output_root(stage.get("output_root"))
    return payload


def load_manifest(path: Path) -> dict[str, Any]:
    resolved = path.resolve(strict=True)
    if not resolved.is_relative_to(PIPELINE_ROOT.resolve(strict=True)) or resolved.suffix.lower() != ".json":
        raise ValueError("manifest must be JSON under pipelines")
    return validate_manifest(read_object(resolved))


def parse_artifacts(values: list[str]) -> dict[str, Path]:
    result: dict[str, Path] = {}
    for value in values:
        if "=" not in value:
            raise ValueError("artifact override must be kind=path")
        kind, raw_path = value.split("=", 1)
        if not kind or kind in result:
            raise ValueError("artifact override kinds must be non-empty and unique")
        path = Path(raw_path).resolve(strict=True)
        if not path.is_file():
            raise ValueError("artifact override must resolve to a file")
        result[kind] = path
    return result


def latest_artifact_index() -> tuple[dict[str, Any], Path]:
    candidates = sorted(ARTIFACT_INDEX_ROOT.glob("*-readiness-index.local.json"), key=lambda p: (p.stat().st_mtime_ns, p.name), reverse=True)
    for path in candidates:
        try:
            payload = read_object(path)
        except (OSError, ValueError, json.JSONDecodeError):
            continue
        if payload.get("report_kind") == "aex_artifact_index" and isinstance(payload.get("artifacts"), list):
            return payload, path.resolve(strict=True)
    raise ValueError("no usable artifact readiness index found")


def resolve_artifact(kind: str, overrides: dict[str, Path], index: tuple[dict[str, Any], Path] | None) -> Path:
    if kind in overrides:
        return overrides[kind]
    if index is None:
        index = latest_artifact_index()
    payload, index_path = index
    if kind == "artifact_index":
        return index_path
    matches = [row for row in payload["artifacts"] if isinstance(row, dict) and row.get("label") == kind and row.get("found") is True]
    if len(matches) != 1 or not isinstance(matches[0].get("path"), str):
        raise ValueError(f"artifact cannot be resolved: {kind}")
    path = Path(matches[0]["path"]).resolve(strict=True)
    if not path.is_file():
        raise ValueError(f"artifact path is unavailable: {kind}")
    return path


def input_flag(name: str) -> str:
    if not name or name.startswith("-") or any(c.isspace() for c in name):
        raise ValueError("input arg name must be an unprefixed CLI name")
    return "--" + name.replace("_", "-")


def build_command(stage: dict[str, Any], overrides: dict[str, Path], index: tuple[dict[str, Any], Path] | None) -> list[str]:
    command = [sys.executable, str(validate_tool_name(stage["tool"])), *stage["args"]]
    for name in sorted(stage["inputs"]):
        source = stage["inputs"][name]
        if source.startswith("artifact:"):
            value = str(resolve_artifact(source.removeprefix("artifact:"), overrides, index))
        else:
            value = source.removeprefix("literal:")
        command.extend([input_flag(name), value])
    return command


def resolved_commands(manifest: dict[str, Any], overrides: dict[str, Path]) -> list[dict[str, Any]]:
    needs_index = any(source.startswith("artifact:") and source.removeprefix("artifact:") not in overrides for stage in manifest["stages"] for source in stage["inputs"].values())
    index = latest_artifact_index() if needs_index else None
    return [{"stage_id": stage["stage_id"], "command": build_command(stage, overrides, index)} for stage in manifest["stages"]]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("list", "dry-run", "run"))
    parser.add_argument("--pipeline", required=True, type=Path)
    parser.add_argument("--artifact", action="append", default=[])
    args = parser.parse_args(argv)
    try:
        manifest = load_manifest(args.pipeline)
        overrides = parse_artifacts(args.artifact)
        if args.command == "list":
            result: Any = [{"stage_id": s["stage_id"], "tool": s["tool"]} for s in manifest["stages"]]
        else:
            commands = resolved_commands(manifest, overrides)
            result = commands
            if args.command == "run":
                for item in commands:
                    completed = subprocess.run(item["command"], shell=False, check=False)
                    if completed.returncode != 0:
                        print(json.dumps({"failed_stage": item["stage_id"], "returncode": completed.returncode}), file=sys.stderr)
                        return completed.returncode or 1
        print(json.dumps(result, indent=2, ensure_ascii=False))
        return 0
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print(f"labctl: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Compare two AE startup probe manifests without treating failures as success."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


def load(path: Path) -> dict[str, Any]:
    document = json.loads(path.read_text(encoding="utf-8-sig"))
    if document.get("kind") != "aexcompat.ae-startup-crash-probe":
        raise ValueError(f"not an AE startup probe manifest: {path}")
    return document


def compare(left: dict[str, Any], right: dict[str, Any]) -> dict[str, Any]:
    fields = (
        "evidence_origin", "status", "failure_classes", "target", "preflight",
        "launch", "telemetry", "dump", "cleanup", "limitations",
    )
    changed = [field for field in fields if left.get(field) != right.get(field)]
    left_artifacts = {item["path"]: item["sha256"] for item in left.get("artifacts", [])}
    right_artifacts = {item["path"]: item["sha256"] for item in right.get("artifacts", [])}
    paths = sorted(set(left_artifacts) | set(right_artifacts))
    artifact_diff = [
        {"path": path, "left": left_artifacts.get(path), "right": right_artifacts.get(path)}
        for path in paths
        if left_artifacts.get(path) != right_artifacts.get(path)
    ]
    return {
        "kind": "aexcompat.ae-startup-crash-probe-diff",
        "schema_version": 1,
        "changed_fields": changed,
        "artifact_hash_diff": artifact_diff,
        "identical": not changed and not artifact_diff,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("left", type=Path)
    parser.add_argument("right", type=Path)
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    result = compare(load(args.left), load(args.right))
    text = json.dumps(result, indent=2, sort_keys=True) + "\n"
    if args.out:
        args.out.write_text(text, encoding="utf-8", newline="\n")
    else:
        print(text, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Validate promoted local contract documents.

The imported AviUtlas "schema" files are contract documents, not always strict
JSON Schema draft documents. This validator starts with stable checks that are
useful today: JSON validity, required contract metadata, path safety for
promoted examples, and obviously unsafe payload-bearing keys.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any, Iterable


WINDOWS_ABSOLUTE_PATH = re.compile(r"^[A-Za-z]:\\")
FORBIDDEN_KEY_PARTS = (
    "raw_payload",
    "binary_payload",
    "payload_bytes",
    "private_image_contents",
)


def iter_json_files(paths: Iterable[Path]) -> list[Path]:
    found: list[Path] = []
    for path in paths:
        if path.is_dir():
            found.extend(sorted(path.rglob("*.json")))
        elif path.suffix.lower() == ".json":
            found.append(path)
    return found


def walk_values(value: Any, path: str = "$") -> Iterable[tuple[str, Any]]:
    yield path, value
    if isinstance(value, dict):
        for key, child in value.items():
            yield from walk_values(child, f"{path}.{key}")
    elif isinstance(value, list):
        for index, child in enumerate(value):
            yield from walk_values(child, f"{path}[{index}]")


def validate_contract(path: Path) -> list[str]:
    issues: list[str] = []
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        return [f"{path}: invalid JSON: {exc}"]

    if not isinstance(data, dict):
        issues.append(f"{path}: top-level JSON value must be an object")
        return issues

    json_schema_version = (
        isinstance(data.get("$schema"), str)
        and isinstance(data.get("properties"), dict)
        and "schema_version" in data["properties"]
    )
    if "schema_version" not in data and "schema_name" not in data and not json_schema_version:
        issues.append(f"{path}: missing schema_version or schema_name")

    for value_path, value in walk_values(data):
        leaf = value_path.rsplit(".", 1)[-1].lower()
        if any(part in leaf for part in FORBIDDEN_KEY_PARTS):
            issues.append(f"{path}: forbidden payload-bearing key at {value_path}")
        if isinstance(value, str) and WINDOWS_ABSOLUTE_PATH.match(value):
            issues.append(f"{path}: local absolute path leaked at {value_path}")

    return issues


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "paths",
        nargs="*",
        default=["contracts"],
        help="JSON files or directories to validate; defaults to contracts",
    )
    parser.add_argument(
        "--strict",
        action="store_true",
        help="Return a non-zero exit code when issues are found",
    )
    args = parser.parse_args(argv)

    json_files = iter_json_files(Path(p) for p in args.paths)
    if not json_files:
        print("contract_schema_validator: no JSON files found", file=sys.stderr)
        return 1 if args.strict else 0

    issues: list[str] = []
    for json_file in json_files:
        issues.extend(validate_contract(json_file))

    result = {
        "schema_version": 1,
        "checked_files": len(json_files),
        "issue_count": len(issues),
        "strict_mode": args.strict,
        "mode": "strict" if args.strict else "warning",
        "issues": issues,
    }
    print(json.dumps(result, indent=2, ensure_ascii=False))
    return 1 if args.strict and issues else 0


if __name__ == "__main__":
    raise SystemExit(main())

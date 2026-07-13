#!/usr/bin/env python3
"""Validate host parameter assignments before any native selector dispatch."""

from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path
from typing import Any


NUMERIC_TYPES = {1: "integer", 4: "boolean", 7: "choice", 10: "float"}


def _number(value: Any) -> float | None:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        return None
    number = float(value)
    return number if math.isfinite(number) else None


def validate_assignments(parameters: Any, assignments: Any) -> dict[str, Any]:
    errors: list[dict[str, Any]] = []
    descriptors: dict[str, dict[str, Any]] = {}
    if not isinstance(parameters, list):
        errors.append({"code": "invalid_parameter_schema"})
        parameters = []
    for descriptor in parameters:
        if not isinstance(descriptor, dict) or not isinstance(descriptor.get("name"), str):
            errors.append({"code": "invalid_parameter_descriptor"})
            continue
        name = descriptor["name"]
        if name in descriptors:
            errors.append({"code": "duplicate_parameter", "parameter": name})
        descriptors[name] = descriptor

    if not isinstance(assignments, dict):
        errors.append({"code": "invalid_assignments"})
        assignments = {}
    for name, value in assignments.items():
        descriptor = descriptors.get(name)
        if descriptor is None:
            errors.append({"code": "unknown_parameter", "parameter": name})
            continue
        parameter_type = descriptor.get("type")
        kind = NUMERIC_TYPES.get(parameter_type)
        if kind is None:
            errors.append({"code": "unsupported_parameter_type", "parameter": name})
            continue
        minimum = _number(descriptor.get("valid_min"))
        maximum = _number(descriptor.get("valid_max"))
        number = _number(value)
        if minimum is None or maximum is None or minimum > maximum:
            errors.append({"code": "invalid_parameter_range", "parameter": name})
            continue
        if number is None or (kind != "float" and not number.is_integer()):
            errors.append({"code": "invalid_parameter_value", "parameter": name})
            continue
        if number < minimum or number > maximum:
            errors.append({
                "code": "parameter_out_of_range",
                "parameter": name,
                "valid_min": minimum,
                "valid_max": maximum,
            })

    return {
        "schema_version": 1,
        "gate": "pre_dispatch_parameter_validation",
        "assignment_count": len(assignments),
        "accepted": not errors,
        "native_dispatch_permitted": not errors,
        "errors": errors,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("request", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    request = json.loads(args.request.read_text(encoding="utf-8"))
    report = validate_assignments(request.get("parameters"), request.get("assignments"))
    try:
        with args.output.open("x", encoding="utf-8", newline="\n") as output:
            json.dump(report, output, indent=2, ensure_ascii=True)
            output.write("\n")
    except FileExistsError:
        print("output already exists", file=sys.stderr)
        return 4
    return 0 if report["accepted"] else 3


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Verify AE's fixed ScatterMap out-of-range parameter rejection contract."""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path


EXPECTED = (
    ("Scatter Amount", -1, 5, 0, 500),
    ("Scatter Amount", 501, 5, 0, 500),
    ("Direction", 0, 3, 1, 3),
    ("Direction", 4, 3, 1, 3),
    ("Random Seed", -1, 0, 0, 10000),
    ("Random Seed", 10001, 0, 0, 10000),
    ("Mix with Original", -0.1, 100, 0, 100),
    ("Mix with Original", 100.1, 100, 0, 100),
    ("Invert Map", -1, 0, 0, 1),
    ("Invert Map", 2, 0, 0, 1),
)


def same_number(actual: object, expected: float) -> bool:
    return isinstance(actual, (int, float)) and not isinstance(actual, bool) and math.isclose(
        float(actual), float(expected), rel_tol=0.0, abs_tol=1e-9
    )


def verify(source: dict[str, object]) -> dict[str, object]:
    attempts = source.get("attempts")
    failures: list[str] = []
    if source.get("schema_version") != 1:
        failures.append("schema_version must be 1")
    if source.get("error") != "":
        failures.append("probe-level error must be empty")
    if not isinstance(attempts, list) or len(attempts) != len(EXPECTED):
        failures.append("exactly ten ordered attempts are required")
        attempts = []
    for index, expected in enumerate(EXPECTED):
        if index >= len(attempts) or not isinstance(attempts[index], dict):
            continue
        item = attempts[index]
        name, attempted, unchanged, minimum, maximum = expected
        if item.get("property") != name or not same_number(item.get("attempted"), attempted):
            failures.append(f"attempt {index} identity mismatch")
        if not same_number(item.get("before"), unchanged) or not same_number(item.get("after"), unchanged):
            failures.append(f"attempt {index} changed the property value")
        error = item.get("error")
        if not isinstance(error, str) or "out of range" not in error:
            failures.append(f"attempt {index} did not report out of range")
        elif f"{minimum} to {maximum}" not in error:
            failures.append(f"attempt {index} reported the wrong valid range")
    return {
        "schema_version": 1,
        "case_id": "scattermap_ae_parameter_bounds",
        "app_version": source.get("app_version"),
        "attempt_count": len(attempts),
        "rejected_count": len(EXPECTED) if not failures else None,
        "values_unchanged": not failures,
        "failures": failures,
        "passed": not failures,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    source = json.loads(args.input.read_text(encoding="utf-8"))
    report = verify(source)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(report, handle, indent=2, sort_keys=True)
        handle.write("\n")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())

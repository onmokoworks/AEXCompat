#!/usr/bin/env python3
"""Normalize a validated host trace into deterministic comparison JSON."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

try:
    from tools.trace_contract_validator import validate_event
except ModuleNotFoundError:
    from trace_contract_validator import validate_event


LAB_ROOT = Path(__file__).resolve().parents[1]
OUTPUT_ROOT = LAB_ROOT / "target" / "trace-normalized"
VOLATILE_FIELDS = {"timestamp", "session_id"}


def strip_volatile(value: Any) -> Any:
    if isinstance(value, dict):
        return {key: strip_volatile(child) for key, child in value.items() if key not in VOLATILE_FIELDS}
    if isinstance(value, list):
        return [strip_volatile(child) for child in value]
    return value


def normalize(events: list[Any]) -> dict[str, Any]:
    normalized = []
    errors = []
    for index, source in enumerate(events):
        event = strip_volatile(source)
        if isinstance(event, dict):
            event["event_index"] = index
        errors.extend(f"event {index}: {error}" for error in validate_event(event))
        normalized.append(event)
    if errors:
        raise ValueError("; ".join(errors))
    return {"schema_version": 1, "report_kind": "normalized_host_trace", "events": normalized}


def read_jsonl(path: Path) -> list[Any]:
    if path.suffix.lower() != ".jsonl":
        raise ValueError("input must have .jsonl extension")
    events = []
    for number, line in enumerate(path.resolve(strict=True).read_text(encoding="utf-8").splitlines(), 1):
        if not line:
            raise ValueError(f"line {number} is blank")
        events.append(json.loads(line))
    if not events:
        raise ValueError("input trace is empty")
    return events


def output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("output must have .json extension")
    OUTPUT_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = (path if path.is_absolute() else LAB_ROOT / path).resolve(strict=False)
    if not resolved.is_relative_to(OUTPUT_ROOT.resolve(strict=True)):
        raise ValueError("output must stay under trace-normalized root")
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if resolved.exists():
        raise FileExistsError("refusing to overwrite normalized trace")
    return resolved


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        destination = output_path(args.out)
        payload = normalize(read_jsonl(args.input))
        destination.write_text(json.dumps(payload, indent=2, ensure_ascii=False, sort_keys=True) + "\n", encoding="utf-8")
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print(f"trace_normalizer: {type(exc).__name__}", file=sys.stderr)
        return 2
    print(json.dumps({"normalized": True, "event_count": len(payload["events"])}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

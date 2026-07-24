#!/usr/bin/env python3
"""Compare normalized AE and minihost traces using explicit contract rules."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
RULES_ROOT = LAB_ROOT / "contracts" / "trace"
OUTPUT_ROOT = LAB_ROOT / "target" / "trace-conformance"
LEVELS = {"must_match", "should_match", "informational"}
COMPARISONS = {"ordered_sequence", "set", "value", "presence"}


def read_object(path: Path) -> dict[str, Any]:
    payload = json.loads(path.resolve(strict=True).read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        raise ValueError("JSON input must be an object")
    return payload


def load_trace(path: Path) -> list[dict[str, Any]]:
    payload = read_object(path)
    if payload.get("schema_version") != 1 or payload.get("report_kind") != "normalized_host_trace":
        raise ValueError("input is not a normalized host trace")
    events = payload.get("events")
    if not isinstance(events, list) or not all(isinstance(event, dict) for event in events):
        raise ValueError("normalized events must be an array of objects")
    return events


def load_rules(path: Path) -> list[dict[str, str]]:
    resolved = path.resolve(strict=True)
    if not resolved.is_relative_to(RULES_ROOT.resolve(strict=True)):
        raise ValueError("rules must stay under contracts/trace")
    payload = read_object(resolved)
    if payload.get("schema_version") != 1 or payload.get("rules_kind") != "host_trace_conformance":
        raise ValueError("unsupported rules contract")
    rules = payload.get("rules")
    if not isinstance(rules, list) or not rules:
        raise ValueError("rules must be non-empty")
    required = {("selector_dispatch", "selector"), ("suite_acquire", "suite.name")}
    found = set()
    for rule in rules:
        if not isinstance(rule, dict) or set(rule) != {"event_kind", "field", "comparison", "level"}:
            raise ValueError("invalid rule shape")
        if rule["level"] not in LEVELS or rule["comparison"] not in COMPARISONS:
            raise ValueError("unsupported rule value")
        found.add((rule["event_kind"], rule["field"]))
    if not required.issubset(found):
        raise ValueError("required must-match rules are missing")
    return rules


def _field(event: dict[str, Any], dotted: str) -> Any:
    value: Any = event
    for part in dotted.split("."):
        if not isinstance(value, dict) or part not in value:
            return None
        value = value[part]
    return value


def _summary(events: list[dict[str, Any]], rule: dict[str, str]) -> Any:
    matching = [event for event in events if event.get("event_kind") == rule["event_kind"]]
    if rule["comparison"] == "ordered_sequence":
        return [_field(event, rule["field"]) for event in matching]
    if rule["comparison"] == "set" and rule["event_kind"] == "suite_acquire":
        return sorted({(event.get("suite", {}).get("name"), event.get("suite", {}).get("version")) for event in matching})
    if rule["comparison"] == "value":
        return [_field(event, rule["field"]) for event in matching]
    return [event.get("event_kind") for event in events]


def compare(reference: list[dict[str, Any]], candidate: list[dict[str, Any]], rules: list[dict[str, str]]) -> dict[str, Any]:
    must = []
    should = []
    informational = 0
    for rule in rules:
        ref = _summary(reference, rule)
        cand = _summary(candidate, rule)
        if ref == cand:
            continue
        mismatch = {"rule": rule, "reference_summary": ref, "candidate_summary": cand}
        if rule["level"] == "must_match":
            must.append(mismatch)
        elif rule["level"] == "should_match":
            should.append(mismatch)
        else:
            informational += 1
    return {
        "schema_version": 1,
        "report_kind": "trace_conformance",
        "must_match_failures": must,
        "should_match_mismatches": should,
        "informational_diff_count": informational,
        "conformance_state": "nonconformant" if must else "conformant",
        "ae_invoked": False,
        "native_load_performed": False,
    }


def output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("output must have .json extension")
    OUTPUT_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = (path if path.is_absolute() else LAB_ROOT / path).resolve(strict=False)
    if not resolved.is_relative_to(OUTPUT_ROOT.resolve(strict=True)):
        raise ValueError("output must stay under trace-conformance root")
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if resolved.exists():
        raise FileExistsError("refusing to overwrite conformance report")
    return resolved


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", required=True, type=Path)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--rules", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        destination = output_path(args.out)
        report = compare(load_trace(args.reference), load_trace(args.candidate), load_rules(args.rules))
        with destination.open("x", encoding="utf-8", newline="\n") as handle:
            handle.write(json.dumps(report, indent=2, ensure_ascii=False, sort_keys=True) + "\n")
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print(f"trace_conformance_diff: {type(exc).__name__}", file=sys.stderr)
        return 2
    print(json.dumps(report, indent=2, ensure_ascii=False))
    return 0 if report["conformance_state"] == "conformant" else 1


if __name__ == "__main__":
    raise SystemExit(main())

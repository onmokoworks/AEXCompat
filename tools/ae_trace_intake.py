#!/usr/bin/env python3
"""Validate and sanitize a manually captured After Effects host trace."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any

try:
    from tools.trace_contract_validator import validate_event
except ModuleNotFoundError:  # Direct execution places tools/ rather than the repo root on sys.path.
    from trace_contract_validator import validate_event


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
REPORT_ROOT = TARGET_ROOT / "ae-trace-intake"
CORPUS_ROOT = TARGET_ROOT / "ae-trace-corpus"
WINDOWS_ABSOLUTE_PATH = re.compile(r"(^|[^A-Za-z])[A-Za-z]:[\\/]")


def _has_traversal(path: Path) -> bool:
    return any(part in {".", ".."} for part in path.parts)


def resolve_create_new(path: Path, root: Path, suffix: str) -> Path:
    if path.suffix.lower() != suffix:
        raise ValueError(f"output must have {suffix} extension")
    if _has_traversal(path):
        raise ValueError("output path must not contain traversal components")
    root.mkdir(parents=True, exist_ok=True)
    absolute = path if path.is_absolute() else LAB_ROOT / path
    resolved = absolute.resolve(strict=False)
    if not resolved.is_relative_to(root.resolve(strict=True)):
        raise ValueError(f"output must stay under {root}")
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if resolved.exists():
        raise FileExistsError(f"refusing to overwrite existing output: {resolved.name}")
    return resolved


def load_jsonl(path: Path) -> tuple[list[Any], list[str]]:
    if path.suffix.lower() != ".jsonl":
        raise ValueError("raw trace must have .jsonl extension")
    events: list[Any] = []
    errors: list[str] = []
    with path.resolve(strict=True).open("r", encoding="utf-8") as handle:
        for line_number, line in enumerate(handle, 1):
            if not line.strip():
                errors.append(f"line {line_number}: blank lines are not allowed")
                continue
            try:
                events.append(json.loads(line))
            except json.JSONDecodeError as exc:
                errors.append(f"line {line_number}: invalid JSON: {exc.msg}")
    if not events and not errors:
        errors.append("trace must contain at least one event")
    return events, errors


def redact_absolute_paths(value: Any) -> tuple[Any, int]:
    if isinstance(value, str):
        if WINDOWS_ABSOLUTE_PATH.search(value):
            return "<redacted-path>", 1
        return value, 0
    if isinstance(value, list):
        result: list[Any] = []
        count = 0
        for child in value:
            redacted, child_count = redact_absolute_paths(child)
            result.append(redacted)
            count += child_count
        return result, count
    if isinstance(value, dict):
        result = {}
        count = 0
        for key, child in value.items():
            redacted, child_count = redact_absolute_paths(child)
            result[key] = redacted
            count += child_count
        return result, count
    return value, 0


def intake(events: list[Any], parse_errors: list[str], *, redact: bool) -> tuple[list[Any], dict[str, Any]]:
    sanitized: list[Any] = []
    rejection_reasons = list(parse_errors)
    redaction_count = 0
    for index, event in enumerate(events, 1):
        candidate = event
        if redact:
            candidate, count = redact_absolute_paths(candidate)
            redaction_count += count
        # Boundary gate: native_observation traces carry no provenance and must
        # never enter the AE-equivalence corpus, even though the shared validator
        # accepts the host_kind. Keep them out of evidence intake.
        if isinstance(candidate, dict) and candidate.get("host_kind") == "native_observation":
            rejection_reasons.append(
                f"line {index}: native_observation events are not admissible to the AE trace corpus"
            )
        event_errors = validate_event(candidate)
        rejection_reasons.extend(f"line {index}: {error}" for error in event_errors)
        sanitized.append(candidate)

    accepted = not rejection_reasons
    report = {
        "schema_version": 1,
        "report_kind": "ae_trace_intake",
        "publication_status": "local-only",
        "line_count": len(events) + len(parse_errors),
        "accepted": accepted,
        "redaction_count": redaction_count,
        "rejection_reasons": rejection_reasons,
        "sanitized_trace_written": accepted,
        "raw_trace_copied": False,
        "ae_invoked": False,
        "native_load_performed": False,
    }
    return sanitized, report


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.write_text(json.dumps(payload, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def write_jsonl(path: Path, events: list[Any]) -> None:
    content = "".join(json.dumps(event, ensure_ascii=False, separators=(",", ":")) + "\n" for event in events)
    path.write_text(content, encoding="utf-8")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--raw-trace", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--sanitized-out", required=True, type=Path)
    parser.add_argument("--redact", action="store_true")
    args = parser.parse_args(argv)

    try:
        report_path = resolve_create_new(args.out, REPORT_ROOT, ".json")
        sanitized_path = resolve_create_new(args.sanitized_out, CORPUS_ROOT, ".jsonl")
        events, parse_errors = load_jsonl(args.raw_trace)
        sanitized, report = intake(events, parse_errors, redact=args.redact)
        if report["accepted"]:
            write_jsonl(sanitized_path, sanitized)
        try:
            write_json(report_path, report)
        except OSError:
            sanitized_path.unlink(missing_ok=True)
            report_path.unlink(missing_ok=True)
            raise
    except (OSError, ValueError) as exc:
        if isinstance(exc, FileExistsError):
            message = str(exc)
        else:
            message = "input or output path validation failed"
        print(f"ae_trace_intake: {message}", file=sys.stderr)
        return 2

    print(json.dumps(report, indent=2, ensure_ascii=False))
    return 0 if report["accepted"] else 1


if __name__ == "__main__":
    raise SystemExit(main())

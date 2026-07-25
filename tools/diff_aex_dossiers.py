#!/usr/bin/env python3
"""Produce a bounded semantic diff between two AEXCompat render dossiers."""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


def event_key(event: dict[str, Any]) -> str:
    parts = (
        event.get("kind"),
        event.get("depth"),
        event.get("function_rva"),
        event.get("pc_rva"),
        event.get("target_rva"),
        event.get("name"),
        event.get("call_kind"),
    )
    return "|".join("" if part is None else str(part) for part in parts)


def trace_index(dossier: dict[str, Any]) -> dict[str, dict[str, Any]]:
    traces = dossier.get("execution_traces", [])
    occurrences: Counter[str] = Counter()
    indexed = {}
    for trace in traces:
        selector = str(trace.get("selector", "unknown"))
        occurrence = occurrences[selector]
        occurrences[selector] += 1
        indexed[f"{selector}#{occurrence}"] = trace
    return indexed


def count_events(trace: dict[str, Any]) -> Counter[str]:
    counts: Counter[str] = Counter()
    for event in trace.get("events", []):
        counts[event_key(event)] += int(event.get("observed_count", 1))
    return counts


def bounded_counter_delta(
    before: Counter[str], after: Counter[str], limit: int = 256
) -> dict[str, Any]:
    changed = []
    for key in sorted(set(before) | set(after)):
        left = before[key]
        right = after[key]
        if left != right:
            changed.append({"key": key, "before": left, "after": right, "delta": right - left})
    entries = changed[:limit]
    return {
        "entries": entries,
        "limit": limit,
        "truncated": len(changed) > limit,
        "dropped_count": max(0, len(changed) - len(entries)),
    }


def build_diff(before: dict[str, Any], after: dict[str, Any]) -> dict[str, Any]:
    before_traces = trace_index(before)
    after_traces = trace_index(after)
    selectors = sorted(set(before_traces) | set(after_traces))
    trace_diffs = []
    for selector in selectors:
        left = before_traces.get(selector, {})
        right = after_traces.get(selector, {})
        event_delta = bounded_counter_delta(count_events(left), count_events(right))
        trace_diffs.append(
            {
                "selector": selector,
                "present_before": selector in before_traces,
                "present_after": selector in after_traces,
                "return_value": {
                    "before": left.get("return_value"),
                    "after": right.get("return_value"),
                },
                "event_count": {
                    "before": len(left.get("events", [])),
                    "after": len(right.get("events", [])),
                },
                "event_observation_deltas": event_delta["entries"],
                "event_observation_delta_truncation": {
                    "limit": event_delta["limit"],
                    "truncated": event_delta["truncated"],
                    "dropped_count": event_delta["dropped_count"],
                },
                "memory_witness_count": {
                    "before": len(left.get("memory_witnesses", [])),
                    "after": len(right.get("memory_witnesses", [])),
                },
                "truncation": {
                    "before": left.get("truncation", []),
                    "after": right.get("truncation", []),
                },
            }
        )
    return {
        "schema": "aexcompat.aex-dossier-diff",
        "schema_version": 1,
        "same_plugin_image": _first_image_sha(before) == _first_image_sha(after),
        "input_png_sha256": {
            "before": before.get("input_png_sha256"),
            "after": after.get("input_png_sha256"),
        },
        "parameter_values": {
            "before": before.get("parameter_values", []),
            "after": after.get("parameter_values", []),
        },
        "render_error": {
            "before": before.get("render_error"),
            "after": after.get("render_error"),
        },
        "trace_diffs": trace_diffs,
    }


def _first_image_sha(dossier: dict[str, Any]) -> str | None:
    traces = dossier.get("execution_traces", [])
    return traces[0].get("image_sha256") if traces else None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    parser.add_argument("-o", "--output", type=Path)
    arguments = parser.parse_args()
    before = json.loads(arguments.before.read_text(encoding="utf-8"))
    after = json.loads(arguments.after.read_text(encoding="utf-8"))
    result = json.dumps(build_diff(before, after), indent=2, sort_keys=True) + "\n"
    if arguments.output:
        arguments.output.write_text(result, encoding="utf-8")
    else:
        print(result, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

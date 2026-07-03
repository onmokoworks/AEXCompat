#!/usr/bin/env python3
"""Build a dependency review packet from no-load availability preflight JSON.

This turns filesystem-only dependency availability evidence into loader-gate
review items. It does not load DLLs, open AEX files, start AE, render, or route
OFX.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
DEPENDENCY_PREFLIGHT_ROOT = TARGET_ROOT / "dependency-preflight"
DEPENDENCY_REVIEW_ROOT = TARGET_ROOT / "dependency-review"

SAFETY_FLAGS = (
    "native_load_enabled",
    "native_load_performed",
    "dll_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
    "aex_file_opened",
)


def path_has_traversal(path: Path) -> bool:
    return any(part in ("..", ".") for part in path.parts)


def resolve_under_root(path: Path, root: Path, *, must_exist: bool) -> Path:
    if path_has_traversal(path):
        raise ValueError("path must not contain traversal components")
    absolute = path if path.is_absolute() else LAB_ROOT / path
    resolved_root = root.resolve(strict=True)
    resolved = absolute.resolve(strict=must_exist)
    if not resolved.is_relative_to(resolved_root):
        raise ValueError(f"path must stay under {root}")
    return resolved


def validate_preflight_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("dependency preflight must have .json extension")
    return resolve_under_root(path, DEPENDENCY_PREFLIGHT_ROOT, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("dependency review output must have .json extension")
    DEPENDENCY_REVIEW_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, DEPENDENCY_REVIEW_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(DEPENDENCY_REVIEW_ROOT.resolve(strict=True)):
        raise ValueError(f"dependency review parent must stay under {DEPENDENCY_REVIEW_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_preflight(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_preflight_path(path)
    return read_json_object(resolved), resolved


def validate_preflight(preflight: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if preflight.get("report_kind") != "aex_dependency_availability_preflight":
        errors.append("source report_kind must be aex_dependency_availability_preflight")
    if preflight.get("preflight_state") != "dependency_availability_preflight_ready_no_load":
        errors.append("source preflight_state must be dependency_availability_preflight_ready_no_load")
    if preflight.get("availability_check") != "filesystem_exists_only_no_load":
        errors.append("source availability_check must be filesystem_exists_only_no_load")
    if preflight.get("publication_status") != "local-only":
        errors.append("source publication_status must be local-only")
    rows = preflight.get("dependency_rows")
    if not isinstance(rows, list):
        errors.append("source dependency_rows must be a list")
    for flag in SAFETY_FLAGS:
        if preflight.get(flag) is not False:
            errors.append(f"source {flag} must be false")
    if isinstance(rows, list):
        for index, row in enumerate(rows):
            if not isinstance(row, dict):
                errors.append(f"dependency row {index} must be an object")
                continue
            for key in ("dll_name", "category", "availability_status", "policy_review_state"):
                if not isinstance(row.get(key), str):
                    errors.append(f"dependency row {index} {key} must be a string")
    return errors


def review_state(row: dict[str, Any]) -> str:
    policy_state = row.get("policy_review_state")
    availability = row.get("availability_status")
    if policy_state == "default_deny_dependency":
        return "native_load_blocker"
    if availability == "not_found_needs_review":
        return "availability_missing_review_required"
    if availability == "api_set_virtual_or_not_found_review":
        return "api_set_resolution_review_required"
    if policy_state == "manual_review_required":
        return "manual_policy_review_required"
    return "available_dependency"


def review_severity(state: str) -> str:
    if state in {"native_load_blocker", "availability_missing_review_required"}:
        return "blocker"
    if state in {"api_set_resolution_review_required", "manual_policy_review_required"}:
        return "review"
    return "informational"


def review_reason(row: dict[str, Any], state: str) -> str:
    category = row.get("category")
    if state == "native_load_blocker":
        return "debug/default-deny runtime dependency must not open the first native load gate"
    if state == "availability_missing_review_required":
        return "dependency filename was not found in the no-load filesystem search"
    if state == "api_set_resolution_review_required":
        return "Windows API-set dependency may be virtualized and needs loader-environment review"
    if state == "manual_policy_review_required":
        return f"{category} dependency requires manual sandbox/host-policy review"
    return "dependency is available in the no-load filesystem preflight"


def build_review_items(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    items: list[dict[str, Any]] = []
    for row in rows:
        state = review_state(row)
        item = {
            "dll_name": row.get("dll_name"),
            "category": row.get("category"),
            "candidate_count": row.get("candidate_count"),
            "availability_status": row.get("availability_status"),
            "policy_review_state": row.get("policy_review_state"),
            "review_state": state,
            "review_severity": review_severity(state),
            "review_reason": review_reason(row, state),
            "example_candidates": row.get("example_candidates", []),
        }
        items.append(item)
    items.sort(
        key=lambda item: (
            item["review_severity"] != "blocker",
            item["review_severity"] != "review",
            str(item["category"]),
            str(item["dll_name"]),
        )
    )
    return items


def count_by(items: list[dict[str, Any]], key: str) -> dict[str, int]:
    counts: dict[str, int] = {}
    for item in items:
        value = str(item.get(key))
        counts[value] = counts.get(value, 0) + 1
    return dict(sorted(counts.items()))


def summarize(items: list[dict[str, Any]], preflight: dict[str, Any]) -> dict[str, Any]:
    state_counts = count_by(items, "review_state")
    severity_counts = count_by(items, "review_severity")
    return {
        "unique_dependency_count": len(items),
        "source_found_count": preflight.get("summary", {}).get("found_count"),
        "available_dependency_count": state_counts.get("available_dependency", 0),
        "native_load_blocker_count": state_counts.get("native_load_blocker", 0),
        "availability_missing_review_required_count": state_counts.get(
            "availability_missing_review_required", 0
        ),
        "api_set_resolution_review_required_count": state_counts.get(
            "api_set_resolution_review_required", 0
        ),
        "manual_policy_review_required_count": state_counts.get("manual_policy_review_required", 0),
        "review_state_counts": state_counts,
        "review_severity_counts": severity_counts,
        "category_counts": count_by(items, "category"),
    }


def review_state_for_summary(summary: dict[str, Any]) -> str:
    if int(summary.get("native_load_blocker_count") or 0) > 0:
        return "dependency_review_pending_native_load_blocked"
    if int(summary.get("availability_missing_review_required_count") or 0) > 0:
        return "dependency_review_pending_availability_review"
    if int(summary.get("api_set_resolution_review_required_count") or 0) > 0:
        return "dependency_review_pending_api_set_review"
    if int(summary.get("manual_policy_review_required_count") or 0) > 0:
        return "dependency_review_pending_manual_policy_review"
    return "dependency_review_ready_for_manual_loader_design_no_load"


def native_load_recommendation(summary: dict[str, Any]) -> str:
    if summary.get("review_severity_counts", {}).get("blocker", 0):
        return "do_not_open_native_load_gate"
    if summary.get("review_severity_counts", {}).get("review", 0):
        return "hold_native_load_until_dependency_review_complete"
    return "manual_loader_design_review_only_no_auto_approval"


def build_dependency_review_packet(preflight: dict[str, Any], source_path: Path) -> dict[str, Any]:
    errors = validate_preflight(preflight)
    if errors:
        raise ValueError("; ".join(errors))
    items = build_review_items(preflight.get("dependency_rows", []))
    summary = summarize(items, preflight)
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aex_dependency_review_packet",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_dependency_preflight": str(source_path),
        "source_preflight_state": preflight.get("preflight_state"),
        "review_state": review_state_for_summary(summary),
        "native_load_recommendation": native_load_recommendation(summary),
        "review_items": items,
        "summary": summary,
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "notes": [
            "Review packet reads dependency preflight JSON only.",
            "Found DLL filenames do not grant native load approval.",
            "Default-deny and manual-review dependency rows keep the loader gate closed.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build local AEX dependency review packet")
    parser.add_argument("--dependency-preflight", required=True, help="Preflight JSON under target/dependency-preflight")
    parser.add_argument("--out", required=True, help="Create-new review JSON under target/dependency-review")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    preflight, source_path = load_preflight(Path(args.dependency_preflight))
    packet = build_dependency_review_packet(preflight, source_path)
    written = write_json_create_new(Path(args.out), packet)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

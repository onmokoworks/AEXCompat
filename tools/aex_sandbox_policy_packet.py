#!/usr/bin/env python3
"""Build a future sandbox policy packet from dependency matrix JSON.

The packet is a design artifact only. It does not inspect local DLL
availability, open AEX files, load libraries, invoke AE, render, or route OFX.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
DEPENDENCY_MATRIX_ROOT = TARGET_ROOT / "dependency-matrix"
SANDBOX_POLICY_ROOT = TARGET_ROOT / "sandbox-policy"

SAFETY_FLAGS = (
    "native_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
    "aex_file_opened",
)

DEFAULT_ALLOW_CATEGORIES = {
    "core_windows",
    "windows_api_set",
    "windows_crt_api_set",
    "release_crt_runtime",
}

MANUAL_REVIEW_CATEGORIES = {
    "graphics_or_gpu",
    "windows_gui",
    "com_ole",
    "manual_review",
}

DEFAULT_DENY_CATEGORIES = {
    "debug_crt_runtime",
}


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


def validate_dependency_matrix_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("dependency matrix must have .json extension")
    return resolve_under_root(path, DEPENDENCY_MATRIX_ROOT, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("sandbox policy output must have .json extension")
    SANDBOX_POLICY_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, SANDBOX_POLICY_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(SANDBOX_POLICY_ROOT.resolve(strict=True)):
        raise ValueError(f"sandbox policy parent must stay under {SANDBOX_POLICY_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_dependency_matrix(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_dependency_matrix_path(path)
    return read_json_object(resolved), resolved


def validate_dependency_matrix(matrix: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if matrix.get("report_kind") != "aex_dependency_matrix":
        errors.append("source report_kind must be aex_dependency_matrix")
    if matrix.get("dependency_matrix_state") != "dependency_matrix_ready":
        errors.append("source dependency_matrix_state must be dependency_matrix_ready")
    if matrix.get("publication_status") != "local-only":
        errors.append("source publication_status must be local-only")
    if matrix.get("availability_check") != "not_performed":
        errors.append("source availability_check must be not_performed")
    if not isinstance(matrix.get("dependency_rows"), list):
        errors.append("source dependency_rows must be a list")
    if not isinstance(matrix.get("candidate_rows"), list):
        errors.append("source candidate_rows must be a list")
    for flag in SAFETY_FLAGS:
        if matrix.get(flag) is not False:
            errors.append(f"source {flag} must be false")
    return errors


def policy_action_for_category(category: str) -> str:
    if category in DEFAULT_ALLOW_CATEGORIES:
        return "default_allow_for_first_sandbox_design"
    if category in MANUAL_REVIEW_CATEGORIES:
        return "manual_review_required"
    if category in DEFAULT_DENY_CATEGORIES:
        return "default_deny_for_first_native_load"
    return "manual_review_required"


def policy_category_rows(dependency_rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    categories: dict[str, dict[str, Any]] = {}
    for row in dependency_rows:
        if not isinstance(row, dict):
            continue
        category = str(row.get("category", "manual_review"))
        item = categories.setdefault(
            category,
            {
                "category": category,
                "policy_action": policy_action_for_category(category),
                "dependency_count": 0,
                "candidate_reference_count": 0,
                "example_dependencies": [],
            },
        )
        item["dependency_count"] += 1
        item["candidate_reference_count"] += int(row.get("candidate_count") or 0)
        if len(item["example_dependencies"]) < 8:
            item["example_dependencies"].append(row.get("dll_name"))
    return sorted(categories.values(), key=lambda item: (item["policy_action"], item["category"]))


def candidate_policy_state(row: dict[str, Any]) -> str:
    categories = set(str(category) for category in row.get("dependency_categories", []))
    if categories & DEFAULT_DENY_CATEGORIES:
        return "blocked_by_default_deny_dependency"
    if categories & MANUAL_REVIEW_CATEGORIES:
        return "manual_dependency_review_required"
    if categories and categories <= DEFAULT_ALLOW_CATEGORIES:
        return "eligible_for_manual_policy_review"
    return "manual_dependency_review_required"


def candidate_policy_rows(candidate_rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for source_order, row in enumerate(candidate_rows):
        if not isinstance(row, dict):
            continue
        state = candidate_policy_state(row)
        rows.append(
            {
                "source_order": source_order,
                "relative_path": row.get("relative_path"),
                "review_bucket": row.get("review_bucket"),
                "compatibility_class": row.get("compatibility_class"),
                "fixture_candidate_score": row.get("fixture_candidate_score"),
                "dependency_categories": row.get("dependency_categories", []),
                "dependency_risk_flags": row.get("dependency_risk_flags", []),
                "candidate_policy_state": state,
                "native_load_approval": "not_granted",
            }
        )
    rows.sort(
        key=lambda item: (
            item["candidate_policy_state"] != "eligible_for_manual_policy_review",
            int(item.get("source_order") or 0),
        )
    )
    return rows


def summarize_candidate_policy(rows: list[dict[str, Any]]) -> dict[str, Any]:
    state_counts: dict[str, int] = {}
    for row in rows:
        state = str(row.get("candidate_policy_state"))
        state_counts[state] = state_counts.get(state, 0) + 1
    return {
        "candidate_count": len(rows),
        "state_counts": dict(sorted(state_counts.items())),
        "eligible_for_manual_policy_review_count": state_counts.get("eligible_for_manual_policy_review", 0),
        "manual_dependency_review_required_count": state_counts.get("manual_dependency_review_required", 0),
        "blocked_by_default_deny_dependency_count": state_counts.get("blocked_by_default_deny_dependency", 0),
    }


def build_sandbox_policy_packet(matrix: dict[str, Any], source_path: Path) -> dict[str, Any]:
    errors = validate_dependency_matrix(matrix)
    if errors:
        raise ValueError("; ".join(errors))
    dependency_rows = matrix.get("dependency_rows", [])
    candidate_rows = candidate_policy_rows(matrix.get("candidate_rows", []))
    first_candidate = candidate_rows[0] if candidate_rows else None
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aex_sandbox_policy_packet",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_dependency_matrix": str(source_path),
        "source_dependency_matrix_state": matrix.get("dependency_matrix_state"),
        "sandbox_policy_state": "policy_ready_no_native_load",
        "availability_check": "not_performed",
        "policy_categories": policy_category_rows(dependency_rows),
        "candidate_policy_summary": summarize_candidate_policy(candidate_rows),
        "candidate_policy_rows": candidate_rows,
        "primary_policy_candidate": first_candidate,
        "default_allow_categories": sorted(DEFAULT_ALLOW_CATEGORIES),
        "manual_review_categories": sorted(MANUAL_REVIEW_CATEGORIES),
        "default_deny_categories": sorted(DEFAULT_DENY_CATEGORIES),
        "required_before_native_load": [
            "explicit user fixture approval artifact",
            "passing load gate using the approved fixture",
            "local dependency availability check in a separate no-load preflight",
            "worker process isolation design review",
            "crash/timeout containment plan",
        ],
        "native_load_enabled": False,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "notes": [
            "Policy packet reads dependency matrix JSON only.",
            "Policy actions are design labels, not approval decisions.",
            "No AEX file is opened and no library is loaded.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build local AEX sandbox policy packet")
    parser.add_argument("--dependency-matrix", required=True, help="Dependency matrix JSON under target/dependency-matrix")
    parser.add_argument("--out", required=True, help="Create-new policy JSON under target/sandbox-policy")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    matrix, source_path = load_dependency_matrix(Path(args.dependency_matrix))
    packet = build_sandbox_policy_packet(matrix, source_path)
    written = write_json_create_new(Path(args.out), packet)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

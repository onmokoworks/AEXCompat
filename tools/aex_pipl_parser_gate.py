#!/usr/bin/env python3
"""Build a no-real-payload gate for future PiPL parser work.

The gate reads PiPL/resource catalog and synthetic parser selftest JSON only.
It derives parser review budgets from metadata sizes, but does not open AEX
files, parse PiPL payloads, serialize raw payloads, or emit parameter schemas.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
PIPL_CATALOG_ROOT = TARGET_ROOT / "pipl-resource-catalog"
SYNTHETIC_SELFTEST_ROOT = TARGET_ROOT / "synthetic-pipl-parser-selftest"
GATE_ROOT = TARGET_ROOT / "pipl-parser-gate"

MIN_PROPOSED_LIMIT_BYTES = 4096
MAX_PROPOSED_LIMIT_BYTES = 65536

SAFETY_FLAGS = (
    "native_load_enabled",
    "native_load_performed",
    "dll_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
    "aex_file_opened",
    "aepx_file_modified",
    "aep_binary_modified",
    "ae_project_write_performed",
    "ofx_plugin_built",
    "ofx_describe_performed",
    "ofx_render_performed",
    "aex_render_performed",
    "render_validation_performed",
    "pipl_payload_parsed",
    "parameter_schema_emitted",
    "redacted_schema_emitted",
    "real_pipl_payload_parser_enabled",
    "real_pipl_payload_parsed",
    "raw_payload_serialized",
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


def validate_json_input(path: Path, root: Path, label: str) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError(f"{label} must have .json extension")
    return resolve_under_root(path, root, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("PiPL parser gate report must have .json extension")
    GATE_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, GATE_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(GATE_ROOT.resolve(strict=True)):
        raise ValueError(f"PiPL parser gate parent must stay under {GATE_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_pipl_catalog(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, PIPL_CATALOG_ROOT, "PiPL catalog")
    return read_json_object(resolved), resolved


def load_synthetic_selftest(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, SYNTHETIC_SELFTEST_ROOT, "synthetic parser selftest")
    return read_json_object(resolved), resolved


def require_local_only(payload: dict[str, Any], label: str) -> list[str]:
    if payload.get("publication_status") != "local-only":
        return [f"{label} publication_status must be local-only"]
    return []


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def validate_pipl_catalog(catalog: dict[str, Any]) -> list[str]:
    errors = require_local_only(catalog, "PiPL catalog")
    if catalog.get("report_kind") != "aex_pipl_resource_catalog":
        errors.append("PiPL catalog report_kind must be aex_pipl_resource_catalog")
    if catalog.get("catalog_state") != "pipl_resource_catalog_ready_no_payload":
        errors.append("PiPL catalog catalog_state must be pipl_resource_catalog_ready_no_payload")
    if catalog.get("payload_policy") != "metadata_only_no_resource_payload":
        errors.append("PiPL catalog payload_policy must be metadata_only_no_resource_payload")
    if catalog.get("resource_payload_extracted") is not False:
        errors.append("PiPL catalog resource_payload_extracted must be false")
    rows = catalog.get("rows")
    if not isinstance(rows, list) or not rows:
        errors.append("PiPL catalog rows must be a non-empty list")
    errors.extend(safety_errors(catalog, "PiPL catalog"))
    return errors


def validate_synthetic_selftest(report: dict[str, Any]) -> list[str]:
    errors = require_local_only(report, "synthetic parser selftest")
    if report.get("report_kind") != "aex_synthetic_pipl_parser_selftest":
        errors.append("synthetic parser selftest report_kind must be aex_synthetic_pipl_parser_selftest")
    if report.get("selftest_state") != "synthetic_pipl_parser_selftest_passed_no_real_payload":
        errors.append("synthetic parser selftest must have passed no-real-payload state")
    if report.get("synthetic_parser_ready") is not True:
        errors.append("synthetic parser selftest synthetic_parser_ready must be true")
    if report.get("real_pipl_payload_parser_enabled") is not False:
        errors.append("synthetic parser selftest real parser must be disabled")
    if report.get("real_pipl_payload_parsed") is not False:
        errors.append("synthetic parser selftest real payload parsed must be false")
    if report.get("raw_payload_serialized") is not False:
        errors.append("synthetic parser selftest raw payload serialized must be false")
    summary = report.get("summary")
    if isinstance(summary, dict):
        if summary.get("failed_count") not in (0, None):
            errors.append("synthetic parser selftest failed_count must be 0")
        if summary.get("raw_payload_serialized_count") not in (0, None):
            errors.append("synthetic parser selftest raw_payload_serialized_count must be 0")
    else:
        errors.append("synthetic parser selftest summary must be an object")
    errors.extend(safety_errors(report, "synthetic parser selftest"))
    return errors


def int_or_zero(value: Any) -> int:
    if isinstance(value, bool):
        return 0
    if isinstance(value, int):
        return value
    return 0


def proposed_limit(max_observed_size: int) -> int:
    if max_observed_size <= 0:
        return MIN_PROPOSED_LIMIT_BYTES
    padded = max(max_observed_size * 4, max_observed_size + 1024, MIN_PROPOSED_LIMIT_BYTES)
    return min(padded, MAX_PROPOSED_LIMIT_BYTES)


def parser_action(row: dict[str, Any], limit_bytes: int) -> str:
    entry_count = int_or_zero(row.get("pipl_resource_data_entry_count"))
    total_size = int_or_zero(row.get("pipl_resource_total_size"))
    if row.get("metadata_state") != "pipl_resource_metadata_ready_no_payload":
        return "hold_until_effect_or_host_contract_review"
    if entry_count <= 0:
        return "hold_missing_pipl_resource_entry_metadata"
    if total_size > limit_bytes:
        return "hold_exceeds_proposed_parser_limit"
    return "eligible_for_future_real_payload_parser_review"


def budget_rows(rows: Any, limit_bytes: int) -> list[dict[str, Any]]:
    if not isinstance(rows, list):
        return []
    result: list[dict[str, Any]] = []
    for row in rows:
        if not isinstance(row, dict):
            continue
        total_size = int_or_zero(row.get("pipl_resource_total_size"))
        result.append(
            {
                "relative_path": row.get("relative_path"),
                "metadata_state": row.get("metadata_state"),
                "pipl_resource_data_entry_count": row.get("pipl_resource_data_entry_count"),
                "pipl_resource_total_size": total_size,
                "effect_main_export_present": row.get("effect_main_export_present"),
                "parser_gate_action": parser_action(row, limit_bytes),
                "within_proposed_parser_limit": total_size <= limit_bytes,
                "resource_payload_opened": False,
                "resource_payload_serialized": False,
            }
        )
    result.sort(
        key=lambda item: (
            item["parser_gate_action"] != "eligible_for_future_real_payload_parser_review",
            -int_or_zero(item.get("pipl_resource_total_size")),
            str(item.get("relative_path")),
        )
    )
    return result


def count_by(rows: list[dict[str, Any]], key: str) -> dict[str, int]:
    counts: dict[str, int] = {}
    for row in rows:
        value = str(row.get(key))
        counts[value] = counts.get(value, 0) + 1
    return dict(sorted(counts.items()))


def build_gate_checks(rows: list[dict[str, Any]], limit_bytes: int, selftest: dict[str, Any]) -> list[dict[str, Any]]:
    eligible_count = sum(1 for row in rows if row["parser_gate_action"] == "eligible_for_future_real_payload_parser_review")
    return [
        {
            "check_id": "synthetic_parser_selftest_passed",
            "passed": selftest.get("synthetic_parser_ready") is True,
        },
        {
            "check_id": "raw_payload_output_blocked",
            "passed": selftest.get("raw_payload_serialized") is False,
        },
        {
            "check_id": "metadata_budget_rows_available",
            "passed": len(rows) > 0,
            "row_count": len(rows),
        },
        {
            "check_id": "eligible_future_parser_candidates_identified",
            "passed": eligible_count > 0,
            "eligible_count": eligible_count,
        },
        {
            "check_id": "proposed_parser_limit_in_range",
            "passed": MIN_PROPOSED_LIMIT_BYTES <= limit_bytes <= MAX_PROPOSED_LIMIT_BYTES,
            "limit_bytes": limit_bytes,
        },
    ]


def build_parser_gate(
    *,
    pipl_catalog: dict[str, Any],
    pipl_catalog_path: Path,
    synthetic_selftest: dict[str, Any],
    synthetic_selftest_path: Path,
) -> dict[str, Any]:
    errors = validate_pipl_catalog(pipl_catalog) + validate_synthetic_selftest(synthetic_selftest)
    if errors:
        raise ValueError("; ".join(errors))

    catalog_summary = pipl_catalog.get("summary", {}) if isinstance(pipl_catalog.get("summary"), dict) else {}
    max_size = int_or_zero(catalog_summary.get("pipl_resource_max_size"))
    total_size = int_or_zero(catalog_summary.get("pipl_resource_total_size"))
    limit_bytes = proposed_limit(max_size)
    rows = budget_rows(pipl_catalog.get("rows"), limit_bytes)
    checks = build_gate_checks(rows, limit_bytes, synthetic_selftest)
    checks_passed = all(check["passed"] for check in checks)
    action_counts = count_by(rows, "parser_gate_action")
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_pipl_parser_gate",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_pipl_resource_catalog": str(pipl_catalog_path),
        "source_synthetic_pipl_parser_selftest": str(synthetic_selftest_path),
        "gate_state": "pipl_parser_gate_closed_no_real_payload",
        "gate_ready_for_review": checks_passed,
        "real_pipl_payload_parser_enabled": False,
        "real_pipl_payload_parsed": False,
        "resource_payload_opened": False,
        "raw_payload_serialized": False,
        "metadata_budget_ready": checks_passed,
        "parser_input_contract": {
            "state": "metadata_size_budget_ready",
            "source": "pipl_resource_catalog_metadata_only",
            "observed_plugin_count": catalog_summary.get("plugin_count"),
            "observed_pipl_resource_entry_count": catalog_summary.get("pipl_resource_entry_count"),
            "observed_pipl_resource_total_size": total_size,
            "observed_pipl_resource_max_size": max_size,
            "proposed_real_parser_limit_bytes": limit_bytes,
            "real_payload_input_allowed_now": False,
            "raw_payload_output_allowed": False,
        },
        "parser_gate_checks": checks,
        "candidate_budget_rows": rows,
        "summary": {
            "candidate_count": len(rows),
            "eligible_future_parser_candidate_count": action_counts.get(
                "eligible_for_future_real_payload_parser_review", 0
            ),
            "hold_candidate_count": len(rows)
            - action_counts.get("eligible_for_future_real_payload_parser_review", 0),
            "parser_gate_action_counts": action_counts,
            "observed_pipl_resource_total_size": total_size,
            "observed_pipl_resource_max_size": max_size,
            "proposed_real_parser_limit_bytes": limit_bytes,
        },
        "blockers": [
            "real_pipl_payload_parser_disabled",
            "real_pipl_payload_not_parsed",
            "fixture_not_approved",
            "load_gate_closed",
            "schema_emission_not_approved",
        ],
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "aepx_file_modified": False,
        "aep_binary_modified": False,
        "ae_project_write_performed": False,
        "ofx_plugin_built": False,
        "ofx_describe_performed": False,
        "ofx_render_performed": False,
        "aex_render_performed": False,
        "render_validation_performed": False,
        "pipl_payload_parsed": False,
        "parameter_schema_emitted": False,
        "redacted_schema_emitted": False,
        "notes": [
            "Gate reads PiPL catalog and synthetic parser selftest JSON only.",
            "Observed PiPL sizes are metadata from static reports, not payload bytes.",
            "No real PiPL resource payload is opened, parsed, copied, or serialized.",
            "No AEX, AE, OFX, render, schema emission, or project-write action is performed.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build no-real-payload PiPL parser gate")
    parser.add_argument("--pipl-catalog", required=True, help="PiPL catalog JSON under target/pipl-resource-catalog")
    parser.add_argument(
        "--synthetic-selftest",
        required=True,
        help="Synthetic parser selftest JSON under target/synthetic-pipl-parser-selftest",
    )
    parser.add_argument("--out", required=True, help="Create-new gate report under target/pipl-parser-gate")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    pipl_catalog, pipl_catalog_path = load_pipl_catalog(Path(args.pipl_catalog))
    synthetic_selftest, synthetic_selftest_path = load_synthetic_selftest(Path(args.synthetic_selftest))
    report = build_parser_gate(
        pipl_catalog=pipl_catalog,
        pipl_catalog_path=pipl_catalog_path,
        synthetic_selftest=synthetic_selftest,
        synthetic_selftest_path=synthetic_selftest_path,
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

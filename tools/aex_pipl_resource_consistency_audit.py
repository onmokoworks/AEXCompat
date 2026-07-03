#!/usr/bin/env python3
"""Audit PiPL static/catalog/gate consistency without opening payloads.

The audit reads static probe, PiPL catalog, and PiPL parser gate JSON only. It
recomputes metadata-derived catalog and gate rows, verifies source-chain paths,
and keeps real PiPL payload parsing, AEX access, native load, schema emission,
rendering, and OFX routes closed.
"""

from __future__ import annotations

import argparse
import json
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TOOLS_ROOT = LAB_ROOT / "tools"
if str(TOOLS_ROOT) not in sys.path:
    sys.path.insert(0, str(TOOLS_ROOT))

import aex_pipl_parser_gate
import aex_pipl_resource_catalog


TARGET_ROOT = LAB_ROOT / "target"
STATIC_REPORT_ROOT = TARGET_ROOT / "aex-static-probe"
PIPL_CATALOG_ROOT = TARGET_ROOT / "pipl-resource-catalog"
PIPL_GATE_ROOT = TARGET_ROOT / "pipl-parser-gate"
AUDIT_ROOT = TARGET_ROOT / "pipl-resource-consistency-audit"

SAFETY_FLAGS = (
    "native_load_enabled",
    "native_load_performed",
    "dll_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
    "aex_file_opened",
    "resource_payload_extracted",
    "resource_payload_opened",
    "real_pipl_payload_parser_enabled",
    "real_pipl_payload_parsed",
    "pipl_payload_parsed",
    "parameter_schema_emitted",
    "redacted_schema_emitted",
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
        raise ValueError("PiPL resource consistency audit report must have .json extension")
    AUDIT_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, AUDIT_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(AUDIT_ROOT.resolve(strict=True)):
        raise ValueError(f"PiPL resource consistency audit parent must stay under {AUDIT_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_static_report(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, STATIC_REPORT_ROOT, "static report")
    return read_json_object(resolved), resolved


def load_pipl_catalog(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, PIPL_CATALOG_ROOT, "PiPL resource catalog")
    return read_json_object(resolved), resolved


def load_pipl_parser_gate(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, PIPL_GATE_ROOT, "PiPL parser gate")
    return read_json_object(resolved), resolved


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def int_or_zero(value: Any) -> int:
    if isinstance(value, bool):
        return 0
    if isinstance(value, int):
        return value
    return 0


def source_path_matches(value: Any, expected: Path) -> bool:
    if not isinstance(value, str):
        return False
    actual = Path(value).resolve(strict=False)
    return actual == expected.resolve(strict=False)


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false when present")
    return errors


def validate_catalog_source(catalog: dict[str, Any]) -> list[str]:
    errors = aex_pipl_parser_gate.validate_pipl_catalog(catalog)
    errors.extend(safety_errors(catalog, "PiPL catalog"))
    rows = catalog.get("rows")
    if isinstance(rows, list):
        for index, row in enumerate(rows):
            if not isinstance(row, dict):
                errors.append(f"PiPL catalog row {index} must be an object")
                continue
            if row.get("payload_policy") != "metadata_only_no_resource_payload":
                errors.append(f"PiPL catalog row {index} payload_policy must be metadata_only_no_resource_payload")
            if row.get("resource_payload_opened") is not None and row.get("resource_payload_opened") is not False:
                errors.append(f"PiPL catalog row {index} resource_payload_opened must be false when present")
            if row.get("resource_payload_serialized") is not None and row.get("resource_payload_serialized") is not False:
                errors.append(f"PiPL catalog row {index} resource_payload_serialized must be false when present")
    return errors


def validate_gate_source(gate: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if gate.get("publication_status") != "local-only":
        errors.append("PiPL parser gate publication_status must be local-only")
    if gate.get("report_kind") != "aex_pipl_parser_gate":
        errors.append("PiPL parser gate report_kind must be aex_pipl_parser_gate")
    if gate.get("gate_state") != "pipl_parser_gate_closed_no_real_payload":
        errors.append("PiPL parser gate must stay closed to real payloads")
    if gate.get("gate_ready_for_review") is not True:
        errors.append("PiPL parser gate gate_ready_for_review must be true")
    if gate.get("metadata_budget_ready") is not True:
        errors.append("PiPL parser gate metadata_budget_ready must be true")
    if gate.get("real_pipl_payload_parser_enabled") is not False:
        errors.append("PiPL parser gate real_pipl_payload_parser_enabled must be false")
    if gate.get("real_pipl_payload_parsed") is not False:
        errors.append("PiPL parser gate real_pipl_payload_parsed must be false")
    if gate.get("resource_payload_opened") is not False:
        errors.append("PiPL parser gate resource_payload_opened must be false")
    if gate.get("raw_payload_serialized") is not False:
        errors.append("PiPL parser gate raw_payload_serialized must be false")
    rows = gate.get("candidate_budget_rows")
    if not isinstance(rows, list) or not rows:
        errors.append("PiPL parser gate candidate_budget_rows must be a non-empty list")
    elif any(not isinstance(row, dict) for row in rows):
        errors.append("PiPL parser gate candidate_budget_rows must contain objects")
    errors.extend(safety_errors(gate, "PiPL parser gate"))
    return errors


def sorted_json(value: Any) -> str:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def values_match(actual: Any, expected: Any) -> bool:
    return sorted_json(actual) == sorted_json(expected)


def recompute_gate_from_catalog(catalog: dict[str, Any]) -> dict[str, Any]:
    summary = catalog.get("summary", {}) if isinstance(catalog.get("summary"), dict) else {}
    max_size = int_or_zero(summary.get("pipl_resource_max_size"))
    total_size = int_or_zero(summary.get("pipl_resource_total_size"))
    limit_bytes = aex_pipl_parser_gate.proposed_limit(max_size)
    rows = aex_pipl_parser_gate.budget_rows(catalog.get("rows"), limit_bytes)
    action_counts = aex_pipl_parser_gate.count_by(rows, "parser_gate_action")
    return {
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
    }


def audit_check(check_id: str, passed: bool, note: str, details: dict[str, Any] | None = None) -> dict[str, Any]:
    check: dict[str, Any] = {"check_id": check_id, "passed": passed, "note": note}
    if details:
        check["details"] = details
    return check


def build_consistency_audit(
    *,
    static_report: dict[str, Any],
    static_report_path: Path,
    pipl_catalog: dict[str, Any],
    pipl_catalog_path: Path,
    pipl_parser_gate: dict[str, Any],
    pipl_parser_gate_path: Path,
) -> dict[str, Any]:
    validation_errors = (
        aex_pipl_resource_catalog.validate_static_report(static_report)
        + validate_catalog_source(pipl_catalog)
        + validate_gate_source(pipl_parser_gate)
    )
    if validation_errors:
        raise ValueError("; ".join(validation_errors))

    recomputed_catalog = aex_pipl_resource_catalog.build_catalog(static_report, static_report_path)
    recomputed_gate = recompute_gate_from_catalog(pipl_catalog)
    catalog_static_source_matches = source_path_matches(pipl_catalog.get("source_static_report"), static_report_path)
    gate_catalog_source_matches = source_path_matches(
        pipl_parser_gate.get("source_pipl_resource_catalog"),
        pipl_catalog_path,
    )
    catalog_summary_matches = values_match(pipl_catalog.get("summary"), recomputed_catalog.get("summary"))
    catalog_rows_match = values_match(pipl_catalog.get("rows"), recomputed_catalog.get("rows"))
    gate_rows_match = values_match(pipl_parser_gate.get("candidate_budget_rows"), recomputed_gate["candidate_budget_rows"])
    gate_summary_matches = values_match(pipl_parser_gate.get("summary"), recomputed_gate["summary"])
    checks = [
        audit_check(
            "catalog_source_static_report_matches",
            catalog_static_source_matches,
            "PiPL catalog source_static_report must equal the explicit static report path.",
        ),
        audit_check(
            "gate_source_catalog_matches",
            gate_catalog_source_matches,
            "PiPL parser gate source_pipl_resource_catalog must equal the explicit catalog path.",
        ),
        audit_check(
            "catalog_summary_recomputed_from_static",
            catalog_summary_matches,
            "PiPL catalog summary must recompute from static report metadata.",
            {
                "actual_plugin_count": (pipl_catalog.get("summary") or {}).get("plugin_count")
                if isinstance(pipl_catalog.get("summary"), dict)
                else None,
                "expected_plugin_count": (recomputed_catalog.get("summary") or {}).get("plugin_count"),
            },
        ),
        audit_check(
            "catalog_rows_recomputed_from_static",
            catalog_rows_match,
            "PiPL catalog rows must recompute from static report metadata.",
            {
                "actual_row_count": len(pipl_catalog.get("rows", [])) if isinstance(pipl_catalog.get("rows"), list) else 0,
                "expected_row_count": len(recomputed_catalog.get("rows", [])),
            },
        ),
        audit_check(
            "gate_budget_rows_recomputed_from_catalog",
            gate_rows_match,
            "PiPL parser gate budget rows must recompute from catalog metadata.",
            {
                "actual_row_count": len(pipl_parser_gate.get("candidate_budget_rows", []))
                if isinstance(pipl_parser_gate.get("candidate_budget_rows"), list)
                else 0,
                "expected_row_count": len(recomputed_gate["candidate_budget_rows"]),
            },
        ),
        audit_check(
            "gate_summary_recomputed_from_catalog",
            gate_summary_matches,
            "PiPL parser gate summary/action counts must recompute from catalog metadata.",
            {
                "actual_action_counts": (pipl_parser_gate.get("summary") or {}).get("parser_gate_action_counts")
                if isinstance(pipl_parser_gate.get("summary"), dict)
                else None,
                "expected_action_counts": recomputed_gate["summary"].get("parser_gate_action_counts"),
            },
        ),
    ]
    audit_passed = all(check["passed"] for check in checks)
    catalog_summary = pipl_catalog.get("summary") if isinstance(pipl_catalog.get("summary"), dict) else {}
    gate_summary = pipl_parser_gate.get("summary") if isinstance(pipl_parser_gate.get("summary"), dict) else {}
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_pipl_resource_consistency_audit",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_static_report": str(static_report_path),
        "source_pipl_resource_catalog": str(pipl_catalog_path),
        "source_pipl_parser_gate": str(pipl_parser_gate_path),
        "audit_state": (
            "pipl_resource_consistency_audit_passed_no_payload"
            if audit_passed
            else "pipl_resource_consistency_audit_failed"
        ),
        "audit_passed": audit_passed,
        "source_chain_valid": catalog_static_source_matches and gate_catalog_source_matches,
        "catalog_summary_recomputed": catalog_summary_matches,
        "catalog_rows_recomputed": catalog_rows_match,
        "gate_budget_rows_recomputed": gate_rows_match,
        "gate_summary_recomputed": gate_summary_matches,
        "metadata_consistency_ready": audit_passed,
        "real_payload_input_allowed_now": False,
        "real_pipl_payload_parser_enabled": False,
        "real_pipl_payload_parsed": False,
        "resource_payload_opened": False,
        "resource_payload_extracted": False,
        "raw_payload_serialized": False,
        "checks": checks,
        "summary": {
            "static_entry_count": len(static_report.get("entries", []))
            if isinstance(static_report.get("entries"), list)
            else 0,
            "catalog_row_count": len(pipl_catalog.get("rows", [])) if isinstance(pipl_catalog.get("rows"), list) else 0,
            "gate_budget_row_count": len(pipl_parser_gate.get("candidate_budget_rows", []))
            if isinstance(pipl_parser_gate.get("candidate_budget_rows"), list)
            else 0,
            "pipl_resource_entry_count": catalog_summary.get("pipl_resource_entry_count"),
            "pipl_resource_total_size": catalog_summary.get("pipl_resource_total_size"),
            "pipl_resource_max_size": catalog_summary.get("pipl_resource_max_size"),
            "resource_parse_truncated_count": catalog_summary.get("resource_parse_truncated_count"),
            "effect_main_export_count": catalog_summary.get("effect_main_export_count"),
            "resource_type_counts": catalog_summary.get("resource_type_counts"),
            "parser_gate_action_counts": gate_summary.get("parser_gate_action_counts"),
            "eligible_future_parser_candidate_count": gate_summary.get("eligible_future_parser_candidate_count"),
            "hold_candidate_count": gate_summary.get("hold_candidate_count"),
        },
        "recomputed_summary": {
            "catalog": recomputed_catalog.get("summary"),
            "gate": recomputed_gate.get("summary"),
        },
        "blockers": [
            "real_payload_adapter_not_enabled",
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
        "pipl_payload_parsed": False,
        "parameter_schema_emitted": False,
        "redacted_schema_emitted": False,
        "notes": [
            "Audit reads static probe, PiPL catalog, and PiPL parser gate JSON only.",
            "Catalog and gate rows are recomputed from metadata without opening payloads.",
            "No AEX file, PiPL payload, AE, OFX, render, schema, or approval action is performed.",
        ],
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Audit PiPL static/catalog/gate consistency without payload access")
    parser.add_argument("--static-report", required=True, help="Static probe JSON under target/aex-static-probe")
    parser.add_argument("--pipl-catalog", required=True, help="PiPL catalog JSON under target/pipl-resource-catalog")
    parser.add_argument("--pipl-parser-gate", required=True, help="PiPL parser gate JSON under target/pipl-parser-gate")
    parser.add_argument("--out", required=True, help="Create-new audit report under target/pipl-resource-consistency-audit")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    static_report, static_report_path = load_static_report(Path(args.static_report))
    pipl_catalog, pipl_catalog_path = load_pipl_catalog(Path(args.pipl_catalog))
    pipl_parser_gate, pipl_parser_gate_path = load_pipl_parser_gate(Path(args.pipl_parser_gate))
    report = build_consistency_audit(
        static_report=static_report,
        static_report_path=static_report_path,
        pipl_catalog=pipl_catalog,
        pipl_catalog_path=pipl_catalog_path,
        pipl_parser_gate=pipl_parser_gate,
        pipl_parser_gate_path=pipl_parser_gate_path,
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

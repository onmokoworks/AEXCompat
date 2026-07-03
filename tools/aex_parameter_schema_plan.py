#!/usr/bin/env python3
"""Build a no-payload AEX parameter schema mapping plan.

The plan reads PiPL/resource catalog, candidate matrix, and render contract
JSON only. It does not open AEX files, parse or copy PiPL/resource payloads,
load DLLs, invoke AE/OFX, describe effects, render, or route pixels.
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
CANDIDATE_MATRIX_ROOT = TARGET_ROOT / "candidate-matrix"
RENDER_CONTRACT_ROOT = TARGET_ROOT / "render-validation-contract"
SCHEMA_PLAN_ROOT = TARGET_ROOT / "parameter-schema-plan"

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
        raise ValueError("parameter schema plan must have .json extension")
    SCHEMA_PLAN_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, SCHEMA_PLAN_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(SCHEMA_PLAN_ROOT.resolve(strict=True)):
        raise ValueError(f"parameter schema plan parent must stay under {SCHEMA_PLAN_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_pipl_catalog(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, PIPL_CATALOG_ROOT, "PiPL resource catalog")
    return read_json_object(resolved), resolved


def load_candidate_matrix(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, CANDIDATE_MATRIX_ROOT, "candidate matrix")
    return read_json_object(resolved), resolved


def load_render_contract(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, RENDER_CONTRACT_ROOT, "render validation contract")
    return read_json_object(resolved), resolved


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def require_local_only(payload: dict[str, Any], label: str) -> list[str]:
    if payload.get("publication_status") != "local-only":
        return [f"{label} publication_status must be local-only"]
    return []


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


def validate_candidate_matrix(matrix: dict[str, Any]) -> list[str]:
    errors = require_local_only(matrix, "candidate matrix")
    if matrix.get("report_kind") != "aex_candidate_matrix":
        errors.append("candidate matrix report_kind must be aex_candidate_matrix")
    if matrix.get("matrix_state") != "candidate_matrix_ready":
        errors.append("candidate matrix matrix_state must be candidate_matrix_ready")
    rows = matrix.get("rows")
    if not isinstance(rows, list) or not rows:
        errors.append("candidate matrix rows must be a non-empty list")
    errors.extend(safety_errors(matrix, "candidate matrix"))
    return errors


def validate_render_contract(contract: dict[str, Any]) -> list[str]:
    errors = require_local_only(contract, "render contract")
    if contract.get("report_kind") != "aex_render_validation_contract":
        errors.append("render contract report_kind must be aex_render_validation_contract")
    if contract.get("contract_state") != "render_validation_contract_ready_render_closed":
        errors.append("render contract contract_state must be render_validation_contract_ready_render_closed")
    if contract.get("real_render_open") is not False:
        errors.append("render contract real_render_open must be false")
    if contract.get("no_load_validation_ready") is not True:
        errors.append("render contract no_load_validation_ready must be true")
    blockers = contract.get("blockers")
    if not isinstance(blockers, list) or "no_aex_parameter_schema_mapping" not in blockers:
        errors.append("render contract must still record no_aex_parameter_schema_mapping blocker")
    errors.extend(safety_errors(contract, "render contract"))
    return errors


def int_or_zero(value: Any) -> int:
    if isinstance(value, bool):
        return 0
    if isinstance(value, int):
        return value
    return 0


def rows_by_path(rows: Any) -> dict[str, dict[str, Any]]:
    if not isinstance(rows, list):
        return {}
    result: dict[str, dict[str, Any]] = {}
    for row in rows:
        if isinstance(row, dict) and isinstance(row.get("relative_path"), str):
            result[row["relative_path"]] = row
    return result


def mapping_state(candidate: dict[str, Any], catalog: dict[str, Any] | None) -> str:
    if catalog is None:
        return "blocked_missing_pipl_catalog_row"
    if catalog.get("metadata_state") not in {
        "pipl_resource_metadata_ready_no_payload",
        "pipl_resource_metadata_present_effect_main_missing",
    }:
        return "blocked_missing_static_pipl_metadata"
    if int_or_zero(catalog.get("pipl_resource_data_entry_count")) <= 0:
        return "blocked_missing_pipl_resource_entry"
    risk_flags = set(candidate.get("risk_flags", []) if isinstance(candidate.get("risk_flags"), list) else [])
    if "aegp_markers_present" in risk_flags or candidate.get("compatibility_class") == "classic_pf_effect_with_aegp_markers":
        return "host_contract_review_before_schema_mapping"
    if candidate.get("review_bucket") == "primary_fixture_candidate":
        return "primary_schema_mapping_candidate_pending_payload_parser"
    return "schema_mapping_candidate_pending_payload_parser"


def schema_row(candidate: dict[str, Any], catalog: dict[str, Any] | None) -> dict[str, Any]:
    pipl_entries = []
    if isinstance(catalog, dict) and isinstance(catalog.get("pipl_resource_entries"), list):
        pipl_entries = [
            {
                "name": entry.get("name"),
                "language": entry.get("language"),
                "data_rva": entry.get("data_rva"),
                "size_bytes": entry.get("size_bytes"),
                "codepage": entry.get("codepage"),
            }
            for entry in catalog.get("pipl_resource_entries", [])[:8]
            if isinstance(entry, dict)
        ]
    return {
        "relative_path": candidate.get("relative_path"),
        "file_name": candidate.get("file_name"),
        "review_bucket": candidate.get("review_bucket"),
        "compatibility_class": candidate.get("compatibility_class"),
        "fixture_candidate_score": candidate.get("fixture_candidate_score"),
        "mapping_state": mapping_state(candidate, catalog),
        "risk_flags": candidate.get("risk_flags", []),
        "metadata_state": catalog.get("metadata_state") if isinstance(catalog, dict) else None,
        "pipl_resource_data_entry_count": catalog.get("pipl_resource_data_entry_count") if isinstance(catalog, dict) else None,
        "pipl_resource_total_size": catalog.get("pipl_resource_total_size") if isinstance(catalog, dict) else None,
        "pipl_resource_entries": pipl_entries,
        "effect_main_export_present": catalog.get("effect_main_export_present") if isinstance(catalog, dict) else None,
        "aegp_marker_count": catalog.get("aegp_marker_count") if isinstance(catalog, dict) else None,
        "planned_schema_surfaces": [
            "effect_identity",
            "parameter_groups",
            "parameter_names",
            "parameter_defaults",
            "parameter_value_ranges",
            "pixel_format_and_extent_hints",
        ],
        "payload_policy": "do_not_parse_or_copy_pipl_payload",
        "schema_output_state": "not_emitted_no_payload_parser",
    }


def count_by(rows: list[dict[str, Any]], key: str) -> dict[str, int]:
    counts: dict[str, int] = {}
    for row in rows:
        value = str(row.get(key))
        counts[value] = counts.get(value, 0) + 1
    return dict(sorted(counts.items()))


def summarize(rows: list[dict[str, Any]]) -> dict[str, Any]:
    return {
        "candidate_count": len(rows),
        "primary_mapping_candidate_count": sum(
            1 for row in rows if row.get("mapping_state") == "primary_schema_mapping_candidate_pending_payload_parser"
        ),
        "payload_parser_required_count": sum(
            1 for row in rows if str(row.get("mapping_state", "")).endswith("pending_payload_parser")
        ),
        "host_contract_review_count": sum(
            1 for row in rows if row.get("mapping_state") == "host_contract_review_before_schema_mapping"
        ),
        "blocked_missing_metadata_count": sum(
            1 for row in rows if str(row.get("mapping_state", "")).startswith("blocked_")
        ),
        "mapping_state_counts": count_by(rows, "mapping_state"),
        "review_bucket_counts": count_by(rows, "review_bucket"),
    }


def build_schema_surfaces() -> list[dict[str, Any]]:
    return [
        {
            "surface": "effect_identity",
            "source": "PiPL resource metadata plus future payload parser",
            "current_state": "metadata_anchor_only",
            "write_policy": "no_output_schema_values_yet",
        },
        {
            "surface": "parameters",
            "source": "future PiPL payload parser",
            "current_state": "blocked_payload_parser_disabled",
            "write_policy": "do_not_emit_names_defaults_ranges",
        },
        {
            "surface": "ofx_describe_mapping",
            "source": "future redacted parameter schema",
            "current_state": "blocked_until_parameter_schema_exists",
            "write_policy": "do_not_build_ofx_describe_from_aex",
        },
        {
            "surface": "render_validation_inputs",
            "source": "existing image fixtures and smoke tool",
            "current_state": "generated_ppm_identity_only",
            "write_policy": "no_aex_backed_render_baseline",
        },
    ]


def build_parameter_schema_plan(
    *,
    pipl_catalog: dict[str, Any],
    pipl_catalog_path: Path,
    candidate_matrix: dict[str, Any],
    candidate_matrix_path: Path,
    render_contract: dict[str, Any],
    render_contract_path: Path,
) -> dict[str, Any]:
    errors = (
        validate_pipl_catalog(pipl_catalog)
        + validate_candidate_matrix(candidate_matrix)
        + validate_render_contract(render_contract)
    )
    if errors:
        raise ValueError("; ".join(errors))
    catalog_by_path = rows_by_path(pipl_catalog.get("rows"))
    candidate_rows = candidate_matrix.get("rows", []) if isinstance(candidate_matrix.get("rows"), list) else []
    rows = [
        schema_row(candidate, catalog_by_path.get(candidate.get("relative_path")))
        for candidate in candidate_rows
        if isinstance(candidate, dict)
    ]
    rows.sort(
        key=lambda row: (
            row["mapping_state"] != "primary_schema_mapping_candidate_pending_payload_parser",
            -int_or_zero(row.get("fixture_candidate_score")),
            str(row.get("relative_path")),
        )
    )
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_parameter_schema_plan",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_pipl_resource_catalog": str(pipl_catalog_path),
        "source_candidate_matrix": str(candidate_matrix_path),
        "source_render_validation_contract": str(render_contract_path),
        "plan_state": "parameter_schema_plan_ready_no_payload",
        "schema_plan_ready": True,
        "real_parameter_schema_available": False,
        "payload_parser_enabled": False,
        "payload_policy": "metadata_only_no_pipl_payload",
        "schema_surfaces": build_schema_surfaces(),
        "candidate_schema_rows": rows,
        "summary": summarize(rows),
        "render_contract_link": {
            "source_contract_state": render_contract.get("contract_state"),
            "real_render_open": render_contract.get("real_render_open"),
            "remaining_blocker": "no_aex_parameter_schema_mapping",
            "this_plan_resolves_blocker": False,
            "this_plan_reduces_blocker_to": "needs_reviewed_payload_parser_and_redacted_schema",
        },
        "blockers": [
            "pipl_payload_parser_disabled",
            "no_parameter_names_defaults_or_ranges",
            "no_redacted_public_schema",
            "no_ofx_describe_mapping",
            "no_real_render_harness",
            "load_gate_closed",
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
        "notes": [
            "Plan reads local JSON artifacts only.",
            "PiPL/resource references are metadata anchors only.",
            "No PiPL/resource payload is parsed or copied.",
            "No parameter names, defaults, ranges, OFX describe data, or render baseline is emitted.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build no-payload AEX parameter schema mapping plan")
    parser.add_argument("--pipl-catalog", required=True, help="PiPL resource catalog JSON under target/pipl-resource-catalog")
    parser.add_argument("--candidate-matrix", required=True, help="Candidate matrix JSON under target/candidate-matrix")
    parser.add_argument("--render-contract", required=True, help="Render validation contract JSON under target/render-validation-contract")
    parser.add_argument("--out", required=True, help="Create-new schema plan JSON under target/parameter-schema-plan")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    pipl_catalog, pipl_catalog_path = load_pipl_catalog(Path(args.pipl_catalog))
    candidate_matrix, candidate_matrix_path = load_candidate_matrix(Path(args.candidate_matrix))
    render_contract, render_contract_path = load_render_contract(Path(args.render_contract))
    plan = build_parameter_schema_plan(
        pipl_catalog=pipl_catalog,
        pipl_catalog_path=pipl_catalog_path,
        candidate_matrix=candidate_matrix,
        candidate_matrix_path=candidate_matrix_path,
        render_contract=render_contract,
        render_contract_path=render_contract_path,
    )
    written = write_json_create_new(Path(args.out), plan)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

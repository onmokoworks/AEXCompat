#!/usr/bin/env python3
"""Build a no-load PiPL/resource metadata catalog from static probe JSON.

The catalog reads an existing AEX static probe report only. It never opens AEX
files, extracts resource payloads, hashes binaries, loads DLLs, invokes AE/OFX,
renders, or routes pixels.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
STATIC_REPORT_ROOT = TARGET_ROOT / "aex-static-probe"
CATALOG_ROOT = TARGET_ROOT / "pipl-resource-catalog"

REPORT_SAFETY_FLAGS = (
    "native_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
)

ENTRY_SAFETY_FLAGS = REPORT_SAFETY_FLAGS + ("private_payload_copied",)


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


def validate_static_report_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("static probe report must have .json extension")
    return resolve_under_root(path, STATIC_REPORT_ROOT, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("PiPL/resource catalog report must have .json extension")
    CATALOG_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, CATALOG_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(CATALOG_ROOT.resolve(strict=True)):
        raise ValueError(f"PiPL/resource catalog parent must stay under {CATALOG_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_static_report(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_static_report_path(path)
    return read_json_object(resolved), resolved


def validate_static_report(report: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if report.get("report_kind") != "aex_static_probe":
        errors.append("source report_kind must be aex_static_probe")
    if int(report.get("schema_version") or 0) < 3:
        errors.append("source schema_version must be >= 3")
    if report.get("publication_status") != "local-only":
        errors.append("source publication_status must be local-only")
    entries = report.get("entries")
    if not isinstance(entries, list):
        errors.append("source entries must be a list")
    for flag in REPORT_SAFETY_FLAGS:
        if report.get(flag) is not False:
            errors.append(f"source {flag} must be false")
    if isinstance(entries, list):
        for index, entry in enumerate(entries):
            if not isinstance(entry, dict):
                errors.append(f"entry {index} must be an object")
                continue
            for flag in ENTRY_SAFETY_FLAGS:
                if entry.get(flag) is not False:
                    errors.append(f"entry {index} {flag} must be false")
            if not isinstance(entry.get("relative_path"), str):
                errors.append(f"entry {index} relative_path must be a string")
    return errors


def int_or_zero(value: Any) -> int:
    if isinstance(value, bool):
        return 0
    if isinstance(value, int):
        return value
    return 0


def bounded_resource_type_details(details: Any) -> list[dict[str, Any]]:
    if not isinstance(details, list):
        return []
    rows: list[dict[str, Any]] = []
    for detail in details[:64]:
        if not isinstance(detail, dict):
            continue
        rows.append(
            {
                "type": detail.get("type"),
                "entry_count": int_or_zero(detail.get("entry_count")),
            }
        )
    return rows


def bounded_pipl_entries(entries: Any) -> list[dict[str, Any]]:
    if not isinstance(entries, list):
        return []
    rows: list[dict[str, Any]] = []
    for entry in entries[:64]:
        if not isinstance(entry, dict):
            continue
        rows.append(
            {
                "type": entry.get("type"),
                "name": entry.get("name"),
                "language": entry.get("language"),
                "data_rva": entry.get("data_rva"),
                "size_bytes": entry.get("size_bytes"),
                "codepage": entry.get("codepage"),
                "reserved": entry.get("reserved"),
            }
        )
    return rows


def metadata_state(entry: dict[str, Any], resource_summary: dict[str, Any], export_summary: dict[str, Any]) -> str:
    pipl_count = int_or_zero(resource_summary.get("pipl_resource_data_entry_count"))
    effect_main = export_summary.get("effect_main_export_present") is True
    if pipl_count > 0 and effect_main:
        return "pipl_resource_metadata_ready_no_payload"
    if pipl_count > 0:
        return "pipl_resource_metadata_present_effect_main_missing"
    if entry.get("pipl_signal_present") is True:
        return "pipl_marker_present_resource_metadata_missing"
    return "pipl_resource_metadata_missing"


def catalog_row(entry: dict[str, Any]) -> dict[str, Any]:
    pe = entry.get("pe") if isinstance(entry.get("pe"), dict) else {}
    resource_summary = pe.get("resource_summary") if isinstance(pe.get("resource_summary"), dict) else {}
    export_summary = pe.get("export_summary") if isinstance(pe.get("export_summary"), dict) else {}
    import_summary = pe.get("import_summary") if isinstance(pe.get("import_summary"), dict) else {}
    markers = entry.get("markers") if isinstance(entry.get("markers"), dict) else {}
    return {
        "relative_path": entry.get("relative_path"),
        "file_name": entry.get("file_name"),
        "size_bytes": entry.get("size_bytes"),
        "compatibility_class": entry.get("compatibility_class"),
        "fixture_candidate_score": entry.get("fixture_candidate_score"),
        "metadata_state": metadata_state(entry, resource_summary, export_summary),
        "machine_label": pe.get("machine_label"),
        "section_count": pe.get("section_count"),
        "resource_dir_present": resource_summary.get("resource_dir_present"),
        "resource_parse_truncated": resource_summary.get("resource_parse_truncated"),
        "resource_type_count": resource_summary.get("type_count"),
        "resource_type_details": bounded_resource_type_details(resource_summary.get("type_details")),
        "resource_data_entry_count": resource_summary.get("resource_data_entry_count"),
        "pipl_signal_present": entry.get("pipl_signal_present"),
        "pipl_ascii_marker_present": markers.get("pipl_ascii_marker_present"),
        "pipl_ascii_marker_count": markers.get("pipl_ascii_marker_count"),
        "pipl_resource_type_present": resource_summary.get("pipl_resource_type_present"),
        "pipl_resource_data_entry_count": resource_summary.get("pipl_resource_data_entry_count"),
        "pipl_resource_total_size": resource_summary.get("pipl_resource_total_size"),
        "pipl_resource_entries": bounded_pipl_entries(resource_summary.get("pipl_resource_entries")),
        "effect_main_export_present": export_summary.get("effect_main_export_present"),
        "effect_main_marker_present": markers.get("effect_main_marker_present"),
        "effect_main_marker_count": markers.get("effect_main_marker_count"),
        "aegp_marker_count": markers.get("ae_plugin_marker_count"),
        "imported_dll_count": import_summary.get("dll_count"),
        "payload_policy": "metadata_only_no_resource_payload",
    }


def count_by(rows: list[dict[str, Any]], key: str) -> dict[str, int]:
    counts: dict[str, int] = {}
    for row in rows:
        value = str(row.get(key))
        counts[value] = counts.get(value, 0) + 1
    return dict(sorted(counts.items()))


def resource_type_counts(rows: list[dict[str, Any]]) -> dict[str, int]:
    counts: dict[str, int] = {}
    for row in rows:
        for detail in row.get("resource_type_details", []):
            if not isinstance(detail, dict):
                continue
            label = str(detail.get("type"))
            counts[label] = counts.get(label, 0) + int_or_zero(detail.get("entry_count"))
    return dict(sorted(counts.items()))


def summarize(rows: list[dict[str, Any]], static_summary: dict[str, Any]) -> dict[str, Any]:
    pipl_sizes = [int_or_zero(row.get("pipl_resource_total_size")) for row in rows]
    ready_count = sum(1 for row in rows if row.get("metadata_state") == "pipl_resource_metadata_ready_no_payload")
    truncated_count = sum(1 for row in rows if row.get("resource_parse_truncated") is True)
    return {
        "plugin_count": len(rows),
        "pipl_resource_metadata_ready_count": ready_count,
        "pipl_resource_entry_count": sum(int_or_zero(row.get("pipl_resource_data_entry_count")) for row in rows),
        "pipl_resource_total_size": sum(pipl_sizes),
        "pipl_resource_max_size": max(pipl_sizes) if pipl_sizes else 0,
        "resource_parse_truncated_count": truncated_count,
        "effect_main_export_count": sum(1 for row in rows if row.get("effect_main_export_present") is True),
        "pipl_signal_count": sum(1 for row in rows if row.get("pipl_signal_present") is True),
        "metadata_state_counts": count_by(rows, "metadata_state"),
        "compatibility_class_counts": count_by(rows, "compatibility_class"),
        "resource_type_counts": resource_type_counts(rows),
        "source_summary": {
            "aex_count": static_summary.get("aex_count"),
            "pipl_resource_entry_count": static_summary.get("pipl_resource_entry_count"),
            "pipl_resource_total_size": static_summary.get("pipl_resource_total_size"),
            "effect_main_export_count": static_summary.get("effect_main_export_count"),
        },
    }


def build_catalog(report: dict[str, Any], source_path: Path) -> dict[str, Any]:
    errors = validate_static_report(report)
    if errors:
        raise ValueError("; ".join(errors))
    entries = report.get("entries", [])
    rows = [catalog_row(entry) for entry in entries if isinstance(entry, dict)]
    summary = summarize(rows, report.get("summary", {}) if isinstance(report.get("summary"), dict) else {})
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_pipl_resource_catalog",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_static_report": str(source_path),
        "source_schema_version": report.get("schema_version"),
        "catalog_state": "pipl_resource_catalog_ready_no_payload",
        "payload_policy": "metadata_only_no_resource_payload",
        "rows": rows,
        "summary": summary,
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "resource_payload_extracted": False,
        "notes": [
            "Catalog reads static probe JSON only.",
            "PiPL/resource entries are metadata only: type/name/language/RVA/size/codepage.",
            "No AEX file is opened and no resource payload is extracted or copied.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build local PiPL/resource metadata catalog from static probe JSON")
    parser.add_argument("--static-report", required=True, help="Static probe JSON under target/aex-static-probe")
    parser.add_argument("--out", required=True, help="Create-new catalog JSON under target/pipl-resource-catalog")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    report, source_path = load_static_report(Path(args.static_report))
    catalog = build_catalog(report, source_path)
    written = write_json_create_new(Path(args.out), catalog)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Build a no-load candidate matrix from an AEX static probe report.

The matrix reads static-probe JSON only. It never opens AEX files, extracts
payloads, hashes binaries, loads DLLs, invokes AE, renders, or routes OFX.
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
MATRIX_ROOT = TARGET_ROOT / "candidate-matrix"

SAFETY_FLAGS = (
    "native_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
)

DEBUG_RUNTIME_IMPORT_MARKERS = (
    "ucrtbased.dll",
    "msvcp140d.dll",
    "vcruntime140d.dll",
    "vcruntime140_1d.dll",
)

GRAPHICS_IMPORT_MARKERS = (
    "opencl.dll",
    "opengl32.dll",
    "d3d11.dll",
    "d3d12.dll",
    "dxgi.dll",
    "gdi32.dll",
    "user32.dll",
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


def validate_report_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("static probe report must have .json extension")
    return resolve_under_root(path, STATIC_REPORT_ROOT, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("candidate matrix output must have .json extension")
    MATRIX_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, MATRIX_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(MATRIX_ROOT.resolve(strict=True)):
        raise ValueError(f"candidate matrix parent must stay under {MATRIX_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_static_report(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_report_path(path)
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
    for flag in SAFETY_FLAGS[:-1]:
        if report.get(flag) is not False:
            errors.append(f"source {flag} must be false")
    if isinstance(entries, list):
        for index, entry in enumerate(entries):
            if not isinstance(entry, dict):
                errors.append(f"entry {index} must be an object")
                continue
            for flag in SAFETY_FLAGS:
                if entry.get(flag) is not False:
                    errors.append(f"entry {index} {flag} must be false")
    return errors


def normalized_imports(entry: dict[str, Any]) -> list[str]:
    dll_names = entry.get("pe", {}).get("import_summary", {}).get("dll_names", [])
    if not isinstance(dll_names, list):
        return []
    return [str(name).lower() for name in dll_names]


def display_imports(entry: dict[str, Any]) -> list[str]:
    dll_names = entry.get("pe", {}).get("import_summary", {}).get("dll_names", [])
    if not isinstance(dll_names, list):
        return []
    return [str(name) for name in dll_names[:32]]


def resource_types(entry: dict[str, Any]) -> list[str]:
    details = entry.get("pe", {}).get("resource_summary", {}).get("type_details", [])
    if not isinstance(details, list):
        return []
    return [str(detail.get("type")) for detail in details if isinstance(detail, dict) and detail.get("type")][:32]


def pipl_entries(entry: dict[str, Any]) -> list[dict[str, Any]]:
    entries = entry.get("pe", {}).get("resource_summary", {}).get("pipl_resource_entries", [])
    if not isinstance(entries, list):
        return []
    result: list[dict[str, Any]] = []
    for item in entries[:8]:
        if not isinstance(item, dict):
            continue
        result.append(
            {
                "name": item.get("name"),
                "language": item.get("language"),
                "data_rva": item.get("data_rva"),
                "size_bytes": item.get("size_bytes"),
                "codepage": item.get("codepage"),
            }
        )
    return result


def risk_flags(entry: dict[str, Any]) -> list[str]:
    flags: list[str] = []
    imports = normalized_imports(entry)
    compatibility_class = entry.get("compatibility_class")
    pe = entry.get("pe", {})
    resource_summary = pe.get("resource_summary", {})
    markers = entry.get("markers", {})
    if compatibility_class != "classic_pf_effect_candidate":
        flags.append("not_classic_pf_effect_candidate")
    if entry.get("pipl_signal_present") is not True:
        flags.append("pipl_signal_missing")
    if int(resource_summary.get("pipl_resource_data_entry_count") or 0) <= 0:
        flags.append("pipl_resource_metadata_missing")
    if pe.get("export_summary", {}).get("effect_main_export_present") is not True:
        flags.append("effect_main_export_missing")
    if int(markers.get("ae_plugin_marker_count") or 0) > 0:
        flags.append("aegp_markers_present")
    if int(entry.get("size_bytes") or 0) > 2_000_000:
        flags.append("large_fixture_candidate")
    if any(marker in imports for marker in DEBUG_RUNTIME_IMPORT_MARKERS):
        flags.append("debug_runtime_imports_present")
    if any(marker in imports for marker in GRAPHICS_IMPORT_MARKERS):
        flags.append("graphics_or_gpu_imports_present")
    if pe.get("machine_label") != "x64":
        flags.append("non_x64_machine")
    return flags


def review_bucket(entry: dict[str, Any], flags: list[str]) -> str:
    compatibility_class = entry.get("compatibility_class")
    score = int(entry.get("fixture_candidate_score") or 0)
    if compatibility_class == "aegp_or_helper_candidate":
        return "hold_for_aegp_or_helper_contract_review"
    if "aegp_markers_present" in flags:
        return "hold_for_host_contract_review"
    if "effect_main_export_missing" in flags or "pipl_resource_metadata_missing" in flags:
        return "hold_for_missing_required_static_metadata"
    if "debug_runtime_imports_present" in flags or "graphics_or_gpu_imports_present" in flags:
        return "dependency_or_environment_review"
    if "large_fixture_candidate" in flags:
        return "large_fixture_review"
    if compatibility_class == "classic_pf_effect_candidate" and score >= 90 and not flags:
        return "primary_fixture_candidate"
    if compatibility_class == "classic_pf_effect_candidate":
        return "secondary_fixture_candidate"
    return "not_recommended_for_first_fixture"


def next_action_for_bucket(bucket: str) -> str:
    actions = {
        "primary_fixture_candidate": "manual provenance/license review before any approval artifact",
        "secondary_fixture_candidate": "manual comparison against primary candidates",
        "large_fixture_review": "review binary size and dependency surface before considering fixture use",
        "dependency_or_environment_review": "review runtime imports and future sandbox availability",
        "hold_for_host_contract_review": "review AEGP/helper markers and host contract before load planning",
        "hold_for_aegp_or_helper_contract_review": "treat as AEGP/helper candidate, not first PF effect fixture",
        "hold_for_missing_required_static_metadata": "do not approve until static metadata is complete",
        "not_recommended_for_first_fixture": "keep for inventory only",
    }
    return actions.get(bucket, "manual review required")


def matrix_row(entry: dict[str, Any]) -> dict[str, Any]:
    pe = entry.get("pe", {})
    resource_summary = pe.get("resource_summary", {})
    markers = entry.get("markers", {})
    flags = risk_flags(entry)
    bucket = review_bucket(entry, flags)
    return {
        "relative_path": entry.get("relative_path"),
        "file_name": entry.get("file_name"),
        "size_bytes": entry.get("size_bytes"),
        "mtime_utc": entry.get("mtime_utc"),
        "review_bucket": bucket,
        "suggested_next_action": next_action_for_bucket(bucket),
        "risk_flags": flags,
        "compatibility_class": entry.get("compatibility_class"),
        "fixture_candidate_score": entry.get("fixture_candidate_score"),
        "fixture_candidate_reasons": entry.get("fixture_candidate_reasons", []),
        "machine_label": pe.get("machine_label"),
        "dll_image": pe.get("characteristics_flags", {}).get("dll"),
        "pipl_signal_present": entry.get("pipl_signal_present"),
        "pipl_resource_data_entry_count": resource_summary.get("pipl_resource_data_entry_count"),
        "pipl_resource_total_size": resource_summary.get("pipl_resource_total_size"),
        "pipl_resource_entries": pipl_entries(entry),
        "resource_types": resource_types(entry),
        "effect_main_export_present": pe.get("export_summary", {}).get("effect_main_export_present"),
        "effect_main_marker_present": markers.get("effect_main_marker_present"),
        "aegp_marker_count": markers.get("ae_plugin_marker_count", 0),
        "import_dll_names": display_imports(entry),
    }


def summarize_rows(rows: list[dict[str, Any]]) -> dict[str, Any]:
    bucket_counts: dict[str, int] = {}
    class_counts: dict[str, int] = {}
    risk_counts: dict[str, int] = {}
    for row in rows:
        bucket = str(row.get("review_bucket"))
        bucket_counts[bucket] = bucket_counts.get(bucket, 0) + 1
        compatibility_class = str(row.get("compatibility_class"))
        class_counts[compatibility_class] = class_counts.get(compatibility_class, 0) + 1
        for flag in row.get("risk_flags", []):
            risk_counts[flag] = risk_counts.get(flag, 0) + 1
    return {
        "candidate_count": len(rows),
        "primary_candidate_count": bucket_counts.get("primary_fixture_candidate", 0),
        "bucket_counts": dict(sorted(bucket_counts.items())),
        "class_counts": dict(sorted(class_counts.items())),
        "risk_flag_counts": dict(sorted(risk_counts.items())),
    }


def build_candidate_matrix(report: dict[str, Any], source_path: Path) -> dict[str, Any]:
    errors = validate_static_report(report)
    if errors:
        raise ValueError("; ".join(errors))
    rows = [matrix_row(entry) for entry in report.get("entries", [])]
    rows.sort(
        key=lambda row: (
            row["review_bucket"] != "primary_fixture_candidate",
            -int(row.get("fixture_candidate_score") or 0),
            int(row.get("size_bytes") or 0),
            str(row.get("relative_path")),
        )
    )
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_matrix",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_static_report": str(source_path),
        "source_static_schema_version": report.get("schema_version"),
        "source_summary": report.get("summary", {}),
        "matrix_state": "candidate_matrix_ready",
        "summary": summarize_rows(rows),
        "rows": rows,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "notes": [
            "Matrix reads static probe JSON only.",
            "No AEX file is opened by this tool.",
            "Review buckets are not approval decisions.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build local AEX candidate matrix from static report")
    parser.add_argument("--report", required=True, help="Static probe JSON under target/aex-static-probe")
    parser.add_argument("--out", required=True, help="Create-new matrix JSON under target/candidate-matrix")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    report, source_path = load_static_report(Path(args.report))
    matrix = build_candidate_matrix(report, source_path)
    written = write_json_create_new(Path(args.out), matrix)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

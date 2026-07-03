#!/usr/bin/env python3
"""Build a no-load dependency matrix from an AEX candidate matrix.

This reads candidate-matrix JSON only. It does not probe the local system DLL
state, open AEX files, load libraries, start AE, render, or route OFX.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
CANDIDATE_MATRIX_ROOT = TARGET_ROOT / "candidate-matrix"
DEPENDENCY_MATRIX_ROOT = TARGET_ROOT / "dependency-matrix"

SAFETY_FLAGS = (
    "native_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
    "aex_file_opened",
)

DEBUG_RUNTIME_DLLS = {
    "ucrtbased.dll",
    "msvcp140d.dll",
    "vcruntime140d.dll",
    "vcruntime140_1d.dll",
}

RELEASE_RUNTIME_DLLS = {
    "msvcp140.dll",
    "vcruntime140.dll",
    "vcruntime140_1.dll",
    "ucrtbase.dll",
}

CORE_WINDOWS_DLLS = {
    "kernel32.dll",
    "ntdll.dll",
    "bcryptprimitives.dll",
}

GRAPHICS_DLLS = {
    "opencl.dll",
    "opengl32.dll",
    "d3d11.dll",
    "d3d12.dll",
    "dxgi.dll",
}

GUI_DLLS = {
    "gdi32.dll",
    "user32.dll",
}

COM_DLLS = {
    "ole32.dll",
    "oleaut32.dll",
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


def validate_candidate_matrix_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("candidate matrix must have .json extension")
    return resolve_under_root(path, CANDIDATE_MATRIX_ROOT, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("dependency matrix output must have .json extension")
    DEPENDENCY_MATRIX_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, DEPENDENCY_MATRIX_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(DEPENDENCY_MATRIX_ROOT.resolve(strict=True)):
        raise ValueError(f"dependency matrix parent must stay under {DEPENDENCY_MATRIX_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_candidate_matrix(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_candidate_matrix_path(path)
    return read_json_object(resolved), resolved


def validate_candidate_matrix(matrix: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if matrix.get("report_kind") != "aex_candidate_matrix":
        errors.append("source report_kind must be aex_candidate_matrix")
    if matrix.get("matrix_state") != "candidate_matrix_ready":
        errors.append("source matrix_state must be candidate_matrix_ready")
    if matrix.get("publication_status") != "local-only":
        errors.append("source publication_status must be local-only")
    rows = matrix.get("rows")
    if not isinstance(rows, list):
        errors.append("source rows must be a list")
    for flag in SAFETY_FLAGS:
        if matrix.get(flag) is not False:
            errors.append(f"source {flag} must be false")
    if isinstance(rows, list):
        for index, row in enumerate(rows):
            if not isinstance(row, dict):
                errors.append(f"row {index} must be an object")
                continue
            if not isinstance(row.get("import_dll_names"), list):
                errors.append(f"row {index} import_dll_names must be a list")
    return errors


def normalize_dll_name(name: Any) -> str:
    return str(name).strip().lower()


def dependency_category(name: str) -> str:
    lowered = normalize_dll_name(name)
    if lowered.startswith("api-ms-win-crt-"):
        return "windows_crt_api_set"
    if lowered.startswith("api-ms-win-"):
        return "windows_api_set"
    if lowered in DEBUG_RUNTIME_DLLS:
        return "debug_crt_runtime"
    if lowered in RELEASE_RUNTIME_DLLS:
        return "release_crt_runtime"
    if lowered in CORE_WINDOWS_DLLS:
        return "core_windows"
    if lowered in GRAPHICS_DLLS:
        return "graphics_or_gpu"
    if lowered in GUI_DLLS:
        return "windows_gui"
    if lowered in COM_DLLS:
        return "com_ole"
    return "manual_review"


def dependency_risk_flags(categories: set[str]) -> list[str]:
    flags: list[str] = []
    if "debug_crt_runtime" in categories:
        flags.append("debug_runtime_dependency")
    if "graphics_or_gpu" in categories:
        flags.append("graphics_or_gpu_dependency")
    if "windows_gui" in categories:
        flags.append("gui_dependency")
    if "com_ole" in categories:
        flags.append("com_or_ole_dependency")
    if "manual_review" in categories:
        flags.append("unknown_dependency_review")
    return flags


def row_dependencies(row: dict[str, Any]) -> list[dict[str, str]]:
    result: list[dict[str, str]] = []
    seen: set[str] = set()
    for name in row.get("import_dll_names", []):
        normalized = normalize_dll_name(name)
        if not normalized or normalized in seen:
            continue
        seen.add(normalized)
        result.append({"dll_name": normalized, "category": dependency_category(normalized)})
    result.sort(key=lambda item: (item["category"], item["dll_name"]))
    return result


def build_candidate_dependency_row(row: dict[str, Any]) -> dict[str, Any]:
    dependencies = row_dependencies(row)
    categories = {item["category"] for item in dependencies}
    return {
        "relative_path": row.get("relative_path"),
        "review_bucket": row.get("review_bucket"),
        "compatibility_class": row.get("compatibility_class"),
        "fixture_candidate_score": row.get("fixture_candidate_score"),
        "dependency_count": len(dependencies),
        "dependency_categories": sorted(categories),
        "dependency_risk_flags": dependency_risk_flags(categories),
        "dependencies": dependencies,
    }


def build_dependency_rows(candidate_rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    by_dll: dict[str, dict[str, Any]] = {}
    for row in candidate_rows:
        relative_path = row.get("relative_path")
        review_bucket = row.get("review_bucket")
        for dependency in row_dependencies(row):
            dll_name = dependency["dll_name"]
            item = by_dll.setdefault(
                dll_name,
                {
                    "dll_name": dll_name,
                    "category": dependency["category"],
                    "candidate_count": 0,
                    "review_buckets": set(),
                    "example_candidates": [],
                },
            )
            item["candidate_count"] += 1
            item["review_buckets"].add(str(review_bucket))
            if len(item["example_candidates"]) < 8:
                item["example_candidates"].append(relative_path)
    rows: list[dict[str, Any]] = []
    for item in by_dll.values():
        category = item["category"]
        rows.append(
            {
                "dll_name": item["dll_name"],
                "category": category,
                "candidate_count": item["candidate_count"],
                "review_buckets": sorted(item["review_buckets"]),
                "dependency_risk_flags": dependency_risk_flags({category}),
                "example_candidates": item["example_candidates"],
            }
        )
    rows.sort(key=lambda item: (-int(item["candidate_count"]), item["category"], item["dll_name"]))
    return rows


def summarize_dependency_rows(rows: list[dict[str, Any]], candidate_rows: list[dict[str, Any]]) -> dict[str, Any]:
    category_counts: dict[str, int] = {}
    risk_counts: dict[str, int] = {}
    for row in rows:
        category = str(row.get("category"))
        category_counts[category] = category_counts.get(category, 0) + 1
        for flag in row.get("dependency_risk_flags", []):
            risk_counts[flag] = risk_counts.get(flag, 0) + 1
    candidate_risk_counts: dict[str, int] = {}
    for row in candidate_rows:
        for flag in row.get("dependency_risk_flags", []):
            candidate_risk_counts[flag] = candidate_risk_counts.get(flag, 0) + 1
    return {
        "unique_dependency_count": len(rows),
        "candidate_count": len(candidate_rows),
        "category_counts": dict(sorted(category_counts.items())),
        "dependency_risk_flag_counts": dict(sorted(risk_counts.items())),
        "candidate_dependency_risk_flag_counts": dict(sorted(candidate_risk_counts.items())),
    }


def build_dependency_matrix(matrix: dict[str, Any], source_path: Path) -> dict[str, Any]:
    errors = validate_candidate_matrix(matrix)
    if errors:
        raise ValueError("; ".join(errors))
    candidate_rows = [build_candidate_dependency_row(row) for row in matrix.get("rows", [])]
    dependency_rows = build_dependency_rows(matrix.get("rows", []))
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_dependency_matrix",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_candidate_matrix": str(source_path),
        "source_candidate_matrix_state": matrix.get("matrix_state"),
        "dependency_matrix_state": "dependency_matrix_ready",
        "availability_check": "not_performed",
        "summary": summarize_dependency_rows(dependency_rows, candidate_rows),
        "dependency_rows": dependency_rows,
        "candidate_rows": candidate_rows,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "notes": [
            "Dependency matrix reads candidate matrix JSON only.",
            "Local system DLL availability is not checked in this tool.",
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
    parser = argparse.ArgumentParser(description="Build local AEX dependency matrix from candidate matrix")
    parser.add_argument("--candidate-matrix", required=True, help="Candidate matrix JSON under target/candidate-matrix")
    parser.add_argument("--out", required=True, help="Create-new dependency matrix JSON under target/dependency-matrix")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    matrix, source_path = load_candidate_matrix(Path(args.candidate_matrix))
    payload = build_dependency_matrix(matrix, source_path)
    written = write_json_create_new(Path(args.out), payload)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Check imported DLL availability with filesystem existence only.

This reads the dependency matrix JSON and checks whether dependency filenames
exist in configured search directories. It does not open AEX files, call
LoadLibrary, start AE, render, or route OFX.
"""

from __future__ import annotations

import argparse
import json
import os
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable, Mapping


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
DEPENDENCY_MATRIX_ROOT = TARGET_ROOT / "dependency-matrix"
DEPENDENCY_PREFLIGHT_ROOT = TARGET_ROOT / "dependency-preflight"

SAFETY_FLAGS = (
    "native_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
    "aex_file_opened",
)

API_SET_CATEGORIES = {
    "windows_api_set",
    "windows_crt_api_set",
}

DEFAULT_ALLOW_CATEGORIES = {
    "core_windows",
    "release_crt_runtime",
    "windows_api_set",
    "windows_crt_api_set",
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
        raise ValueError("dependency preflight output must have .json extension")
    DEPENDENCY_PREFLIGHT_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, DEPENDENCY_PREFLIGHT_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(DEPENDENCY_PREFLIGHT_ROOT.resolve(strict=True)):
        raise ValueError(f"dependency preflight parent must stay under {DEPENDENCY_PREFLIGHT_ROOT}")
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


def safe_dll_name(value: Any) -> str:
    name = str(value).strip().lower()
    if not name:
        raise ValueError("dependency dll_name must not be empty")
    if "\x00" in name or "\\" in name or "/" in name or name in {".", ".."}:
        raise ValueError(f"dependency dll_name must be a filename only: {value!r}")
    return name


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
    rows = matrix.get("dependency_rows")
    if not isinstance(rows, list):
        errors.append("source dependency_rows must be a list")
    for flag in SAFETY_FLAGS:
        if matrix.get(flag) is not False:
            errors.append(f"source {flag} must be false")
    if isinstance(rows, list):
        for index, row in enumerate(rows):
            if not isinstance(row, dict):
                errors.append(f"dependency row {index} must be an object")
                continue
            try:
                safe_dll_name(row.get("dll_name"))
            except ValueError as exc:
                errors.append(str(exc))
            if not isinstance(row.get("category"), str):
                errors.append(f"dependency row {index} category must be a string")
    return errors


def normalize_search_dirs(paths: Iterable[Path | str]) -> list[Path]:
    result: list[Path] = []
    seen: set[str] = set()
    for raw in paths:
        text = str(raw).strip().strip('"')
        if not text:
            continue
        path = Path(text).expanduser()
        if path_has_traversal(path):
            continue
        try:
            resolved = path.resolve(strict=False)
            if not resolved.exists() or not resolved.is_dir():
                continue
        except OSError:
            continue
        key = str(resolved).lower()
        if key in seen:
            continue
        seen.add(key)
        result.append(resolved)
    return result


def default_search_dirs(environ: Mapping[str, str] | None = None) -> list[Path]:
    env = os.environ if environ is None else environ
    candidates: list[Path | str] = []
    system_root = env.get("SystemRoot") or env.get("WINDIR") or r"C:\Windows"
    candidates.extend(
        [
            Path(system_root) / "System32",
            Path(system_root) / "SysWOW64",
        ]
    )
    candidates.extend(entry for entry in env.get("PATH", "").split(os.pathsep) if entry.strip())
    return normalize_search_dirs(candidates)


def find_dependency_paths(dll_name: str, search_dirs: list[Path]) -> list[str]:
    found_paths: list[str] = []
    for directory in search_dirs:
        candidate = directory / dll_name
        try:
            if candidate.exists() and candidate.is_file():
                found_paths.append(str(candidate.resolve(strict=True)))
        except OSError:
            continue
    return found_paths


def availability_status(category: str, found_paths: list[str]) -> str:
    if found_paths:
        return "found_in_search_path"
    if category in API_SET_CATEGORIES:
        return "api_set_virtual_or_not_found_review"
    return "not_found_needs_review"


def policy_review_state(category: str, status: str) -> str:
    if category in DEFAULT_DENY_CATEGORIES:
        return "default_deny_dependency"
    if status == "api_set_virtual_or_not_found_review":
        return "api_set_resolution_review"
    if category in MANUAL_REVIEW_CATEGORIES:
        return "manual_review_required"
    if category in DEFAULT_ALLOW_CATEGORIES and status == "found_in_search_path":
        return "available_for_first_sandbox_design_review"
    return "manual_review_required"


def build_preflight_row(source_row: dict[str, Any], search_dirs: list[Path]) -> dict[str, Any]:
    dll_name = safe_dll_name(source_row.get("dll_name"))
    category = str(source_row.get("category", "manual_review"))
    found_paths = find_dependency_paths(dll_name, search_dirs)
    status = availability_status(category, found_paths)
    return {
        "dll_name": dll_name,
        "category": category,
        "candidate_count": source_row.get("candidate_count"),
        "review_buckets": source_row.get("review_buckets", []),
        "dependency_risk_flags": source_row.get("dependency_risk_flags", []),
        "availability_status": status,
        "policy_review_state": policy_review_state(category, status),
        "found_paths": found_paths,
        "checked_search_directory_count": len(search_dirs),
        "example_candidates": source_row.get("example_candidates", []),
    }


def count_by(rows: list[dict[str, Any]], key: str) -> dict[str, int]:
    counts: dict[str, int] = {}
    for row in rows:
        value = str(row.get(key))
        counts[value] = counts.get(value, 0) + 1
    return dict(sorted(counts.items()))


def summarize(rows: list[dict[str, Any]], matrix: dict[str, Any], search_dirs: list[Path]) -> dict[str, Any]:
    status_counts = count_by(rows, "availability_status")
    policy_counts = count_by(rows, "policy_review_state")
    return {
        "unique_dependency_count": len(rows),
        "source_candidate_count": matrix.get("summary", {}).get("candidate_count"),
        "search_directory_count": len(search_dirs),
        "found_count": status_counts.get("found_in_search_path", 0),
        "not_found_needs_review_count": status_counts.get("not_found_needs_review", 0),
        "api_set_virtual_or_not_found_review_count": status_counts.get(
            "api_set_virtual_or_not_found_review", 0
        ),
        "default_deny_dependency_count": policy_counts.get("default_deny_dependency", 0),
        "manual_review_dependency_count": policy_counts.get("manual_review_required", 0),
        "status_counts": status_counts,
        "policy_review_state_counts": policy_counts,
        "category_counts": count_by(rows, "category"),
    }


def build_dependency_preflight(
    matrix: dict[str, Any],
    source_path: Path,
    search_dirs: Iterable[Path | str] | None = None,
) -> dict[str, Any]:
    errors = validate_dependency_matrix(matrix)
    if errors:
        raise ValueError("; ".join(errors))
    normalized_dirs = default_search_dirs() if search_dirs is None else normalize_search_dirs(search_dirs)
    rows = [build_preflight_row(row, normalized_dirs) for row in matrix.get("dependency_rows", [])]
    rows.sort(key=lambda item: (item["availability_status"], item["category"], item["dll_name"]))
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_dependency_availability_preflight",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_dependency_matrix": str(source_path),
        "source_dependency_matrix_state": matrix.get("dependency_matrix_state"),
        "preflight_state": "dependency_availability_preflight_ready_no_load",
        "availability_check": "filesystem_exists_only_no_load",
        "search_directories": [str(path) for path in normalized_dirs],
        "summary": summarize(rows, matrix, normalized_dirs),
        "dependency_rows": rows,
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "notes": [
            "Preflight reads dependency matrix JSON and filesystem metadata only.",
            "DLL existence checks use Path.exists/is_file; no LoadLibrary or AEX load is performed.",
            "API-set DLLs may be virtualized by Windows and require review when no file exists.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build local no-load dependency availability preflight")
    parser.add_argument("--dependency-matrix", required=True, help="Dependency matrix JSON under target/dependency-matrix")
    parser.add_argument("--out", required=True, help="Create-new preflight JSON under target/dependency-preflight")
    parser.add_argument(
        "--search-dir",
        action="append",
        default=[],
        help="Optional DLL search directory. Defaults to System32/SysWOW64/PATH when omitted.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    matrix, source_path = load_dependency_matrix(Path(args.dependency_matrix))
    explicit_search_dirs = [Path(item) for item in args.search_dir] if args.search_dir else None
    payload = build_dependency_preflight(matrix, source_path, explicit_search_dirs)
    written = write_json_create_new(Path(args.out), payload)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

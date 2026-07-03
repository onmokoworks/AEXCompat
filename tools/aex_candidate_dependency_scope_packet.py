#!/usr/bin/env python3
"""Build a no-load candidate-scoped dependency review packet.

The packet reads fixture manual-review, dependency review, and load-gate JSON
only. It separates global dependency blockers from dependencies imported by the
selected fixture candidate, without opening AEX files or loading DLLs.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
FIXTURE_MANUAL_REVIEW_ROOT = TARGET_ROOT / "fixture-manual-review"
DEPENDENCY_REVIEW_ROOT = TARGET_ROOT / "dependency-review"
DEPENDENCY_PREFLIGHT_ROOT = TARGET_ROOT / "dependency-preflight"
LOAD_GATE_ROOT = TARGET_ROOT / "load-gate"
CANDIDATE_SCOPE_ROOT = TARGET_ROOT / "candidate-dependency-scope"

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


def validate_json_input(path: Path, root: Path, label: str) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError(f"{label} must have .json extension")
    return resolve_under_root(path, root, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("candidate dependency scope packet must have .json extension")
    CANDIDATE_SCOPE_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, CANDIDATE_SCOPE_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(CANDIDATE_SCOPE_ROOT.resolve(strict=True)):
        raise ValueError(f"candidate dependency scope parent must stay under {CANDIDATE_SCOPE_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_fixture_manual_review(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, FIXTURE_MANUAL_REVIEW_ROOT, "fixture manual-review packet")
    return read_json_object(resolved), resolved


def load_dependency_review(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, DEPENDENCY_REVIEW_ROOT, "dependency review")
    return read_json_object(resolved), resolved


def load_dependency_preflight(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, DEPENDENCY_PREFLIGHT_ROOT, "dependency preflight")
    return read_json_object(resolved), resolved


def load_load_gate(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, LOAD_GATE_ROOT, "load gate")
    return read_json_object(resolved), resolved


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def validate_manual_review(packet: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if packet.get("publication_status") != "local-only":
        errors.append("fixture manual-review publication_status must be local-only")
    if packet.get("report_kind") != "aex_fixture_manual_review_packet":
        errors.append("fixture manual-review report_kind must be aex_fixture_manual_review_packet")
    if packet.get("review_packet_state") != "fixture_manual_review_packet_ready_no_load":
        errors.append("fixture manual-review packet must be ready no-load")
    if not isinstance(packet.get("candidate"), dict):
        errors.append("fixture manual-review candidate must be an object")
    if not isinstance(packet.get("candidate_relative_path"), str):
        errors.append("fixture manual-review candidate_relative_path must be a string")
    errors.extend(safety_errors(packet, "fixture manual-review"))
    return errors


def validate_dependency_review(review: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if review.get("publication_status") != "local-only":
        errors.append("dependency review publication_status must be local-only")
    if review.get("packet_kind") != "aex_dependency_review_packet":
        errors.append("dependency review packet_kind must be aex_dependency_review_packet")
    if not isinstance(review.get("review_items"), list):
        errors.append("dependency review review_items must be a list")
    if not isinstance(review.get("summary"), dict):
        errors.append("dependency review summary must be an object")
    errors.extend(safety_errors(review, "dependency review"))
    return errors


def validate_dependency_preflight(preflight: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if preflight.get("publication_status") != "local-only":
        errors.append("dependency preflight publication_status must be local-only")
    if preflight.get("report_kind") != "aex_dependency_availability_preflight":
        errors.append("dependency preflight report_kind must be aex_dependency_availability_preflight")
    if preflight.get("preflight_state") != "dependency_availability_preflight_ready_no_load":
        errors.append("dependency preflight state must be ready no-load")
    if not isinstance(preflight.get("dependency_rows"), list):
        errors.append("dependency preflight dependency_rows must be a list")
    errors.extend(safety_errors(preflight, "dependency preflight"))
    return errors


def validate_load_gate(gate: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if gate.get("publication_status") != "local-only":
        errors.append("load gate publication_status must be local-only")
    if gate.get("report_kind") != "aex_load_gate_check":
        errors.append("load gate report_kind must be aex_load_gate_check")
    if not isinstance(gate.get("gate_state"), str):
        errors.append("load gate gate_state must be a string")
    errors.extend(safety_errors(gate, "load gate"))
    return errors


def normalize_dll_name(value: Any) -> str:
    return str(value).strip().lower()


def review_items_by_dll(items: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    result: dict[str, dict[str, Any]] = {}
    for item in items:
        if not isinstance(item, dict):
            continue
        dll_name = normalize_dll_name(item.get("dll_name"))
        if dll_name:
            result[dll_name] = item
    return result


def rows_by_dll(rows: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    result: dict[str, dict[str, Any]] = {}
    for row in rows:
        if not isinstance(row, dict):
            continue
        dll_name = normalize_dll_name(row.get("dll_name"))
        if dll_name:
            result[dll_name] = row
    return result


def candidate_imports(candidate: dict[str, Any]) -> list[str]:
    result: list[str] = []
    seen: set[str] = set()
    imports = candidate.get("import_dll_names", [])
    if not isinstance(imports, list):
        return []
    for name in imports:
        normalized = normalize_dll_name(name)
        if normalized and normalized not in seen:
            seen.add(normalized)
            result.append(normalized)
    return sorted(result)


def item_mentions_candidate(item: dict[str, Any], candidate_relative_path: str) -> bool:
    candidate_key = candidate_relative_path.lower()
    examples = item.get("example_candidates", [])
    if not isinstance(examples, list):
        return False
    return any(str(example).lower() == candidate_key for example in examples)


def build_candidate_dependency_rows(
    *,
    candidate_dlls: list[str],
    review_by_dll: dict[str, dict[str, Any]],
    preflight_by_dll: dict[str, dict[str, Any]] | None = None,
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    preflight_by_dll = preflight_by_dll or {}
    for dll_name in candidate_dlls:
        item = review_by_dll.get(dll_name, {})
        preflight = preflight_by_dll.get(dll_name, {})
        found_paths = preflight.get("found_paths", [])
        found_paths_count = len(found_paths) if isinstance(found_paths, list) else None
        rows.append(
            {
                "dll_name": dll_name,
                "matched_global_review": bool(item),
                "matched_preflight": bool(preflight),
                "category": item.get("category"),
                "availability_status": item.get("availability_status") or preflight.get("availability_status"),
                "policy_review_state": item.get("policy_review_state") or preflight.get("policy_review_state"),
                "found_paths_count": found_paths_count,
                "found_paths_exported": False,
                "checked_search_directory_count": preflight.get("checked_search_directory_count"),
                "review_state": item.get("review_state", "not_found_in_global_dependency_review"),
                "review_severity": item.get("review_severity", "review"),
                "review_reason": item.get("review_reason", "candidate import was not present in dependency review rows"),
                "candidate_count": item.get("candidate_count"),
            }
        )
    rows.sort(key=lambda row: (row["review_severity"] != "blocker", row["review_severity"], row["dll_name"]))
    return rows


def summarize_candidate_rows(rows: list[dict[str, Any]]) -> dict[str, Any]:
    severity_counts: dict[str, int] = {}
    state_counts: dict[str, int] = {}
    category_counts: dict[str, int] = {}
    for row in rows:
        severity = str(row.get("review_severity"))
        state = str(row.get("review_state"))
        category = str(row.get("category"))
        severity_counts[severity] = severity_counts.get(severity, 0) + 1
        state_counts[state] = state_counts.get(state, 0) + 1
        category_counts[category] = category_counts.get(category, 0) + 1
    return {
        "candidate_dependency_count": len(rows),
        "candidate_dependency_blocker_count": severity_counts.get("blocker", 0),
        "candidate_dependency_review_count": severity_counts.get("review", 0),
        "candidate_dependency_available_count": state_counts.get("available_dependency", 0),
        "candidate_dependency_missing_or_api_set_review_count": sum(
            1
            for row in rows
            if row.get("availability_status")
            in {"not_found_needs_review", "api_set_virtual_or_not_found_review"}
        ),
        "candidate_dependency_found_paths_exported": False,
        "review_severity_counts": dict(sorted(severity_counts.items())),
        "review_state_counts": dict(sorted(state_counts.items())),
        "category_counts": dict(sorted(category_counts.items())),
    }


def global_blocker_rows(review_items: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [
        item
        for item in review_items
        if isinstance(item, dict) and item.get("review_severity") == "blocker"
    ]


def build_candidate_dependency_scope_packet(
    *,
    fixture_manual_review: dict[str, Any],
    fixture_manual_review_path: Path,
    dependency_review: dict[str, Any],
    dependency_review_path: Path,
    dependency_preflight: dict[str, Any] | None = None,
    dependency_preflight_path: Path | None = None,
    load_gate: dict[str, Any],
    load_gate_path: Path,
) -> dict[str, Any]:
    errors = (
        validate_manual_review(fixture_manual_review)
        + validate_dependency_review(dependency_review)
        + validate_load_gate(load_gate)
    )
    if dependency_preflight is not None:
        errors.extend(validate_dependency_preflight(dependency_preflight))
    if errors:
        raise ValueError("; ".join(errors))

    candidate = fixture_manual_review["candidate"]
    candidate_relative_path = fixture_manual_review["candidate_relative_path"]
    gate_candidate = load_gate.get("primary_review_candidate", {})
    if isinstance(gate_candidate, dict) and gate_candidate.get("relative_path") != candidate_relative_path:
        raise ValueError("load gate primary candidate must match fixture manual-review candidate")

    review_items = dependency_review.get("review_items", [])
    by_dll = review_items_by_dll(review_items)
    preflight_by_dll = (
        rows_by_dll(dependency_preflight.get("dependency_rows", []))
        if isinstance(dependency_preflight, dict)
        else {}
    )
    rows = build_candidate_dependency_rows(
        candidate_dlls=candidate_imports(candidate),
        review_by_dll=by_dll,
        preflight_by_dll=preflight_by_dll,
    )
    candidate_summary = summarize_candidate_rows(rows)
    blockers = global_blocker_rows(review_items)
    blocker_mentions = [
        item
        for item in blockers
        if item_mentions_candidate(item, candidate_relative_path)
        or normalize_dll_name(item.get("dll_name")) in {row["dll_name"] for row in rows}
    ]
    candidate_has_dependency_blockers = bool(blocker_mentions) or candidate_summary["candidate_dependency_blocker_count"] > 0
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_dependency_scope_packet",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_fixture_manual_review": str(fixture_manual_review_path),
        "source_dependency_review": str(dependency_review_path),
        "source_dependency_preflight": str(dependency_preflight_path) if dependency_preflight_path else None,
        "source_load_gate": str(load_gate_path),
        "candidate_dependency_scope_state": "candidate_dependency_scope_ready_no_load",
        "candidate_scope_ready": True,
        "candidate_relative_path": candidate_relative_path,
        "candidate_dependency_blockers_present": candidate_has_dependency_blockers,
        "global_dependency_blockers_present": bool(blockers),
        "global_dependency_blockers_apply_to_candidate": bool(blocker_mentions),
        "candidate_dependency_blocker_count": candidate_summary["candidate_dependency_blocker_count"],
        "candidate_dependency_review_count": candidate_summary["candidate_dependency_review_count"],
        "candidate_dependency_missing_or_api_set_review_count": candidate_summary[
            "candidate_dependency_missing_or_api_set_review_count"
        ],
        "candidate_dependency_found_paths_exported": False,
        "candidate_dependency_summary": candidate_summary,
        "candidate_dependency_rows": rows,
        "global_dependency_summary": {
            "review_state": dependency_review.get("review_state"),
            "native_load_recommendation": dependency_review.get("native_load_recommendation"),
            "summary": dependency_review.get("summary", {}),
            "global_blocker_count": len(blockers),
            "global_blockers_matching_candidate_count": len(blocker_mentions),
        },
        "global_blocker_rows": [
            {
                "dll_name": item.get("dll_name"),
                "category": item.get("category"),
                "review_state": item.get("review_state"),
                "review_severity": item.get("review_severity"),
                "example_candidates": item.get("example_candidates", []),
            }
            for item in blockers[:12]
        ],
        "load_gate_summary": {
            "gate_state": load_gate.get("gate_state"),
            "dependency_native_load_recommendation": load_gate.get("dependency_native_load_recommendation"),
            "gate_errors": load_gate.get("gate_errors", []),
        },
        "scoped_gate_recommendation": (
            "keep_gate_closed_candidate_dependency_blockers_present"
            if candidate_has_dependency_blockers
            else "candidate_dependencies_clear_global_gate_still_closed"
        ),
        "blocked_actions": [
            "open_aex_file",
            "hash_aex_file",
            "load_aex_dll",
            "load_dependency_dll",
            "call_EffectMain",
            "start_after_effects",
            "render_with_aex",
            "route_through_ofx",
        ],
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "notes": [
            "Packet reads JSON evidence only.",
            "Candidate imports come from existing fixture manual-review metadata.",
            "Dependency preflight found paths are reduced to counts when provided.",
            "No AEX file or DLL is opened, copied, hashed, loaded, or executed.",
            "Candidate-scoped dependency clarity does not approve native loading.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build no-load candidate-scoped dependency packet")
    parser.add_argument(
        "--fixture-manual-review",
        required=True,
        help="Fixture manual-review packet under target/fixture-manual-review",
    )
    parser.add_argument("--dependency-review", required=True, help="Dependency review JSON under target/dependency-review")
    parser.add_argument("--dependency-preflight", help="Optional dependency preflight JSON under target/dependency-preflight")
    parser.add_argument("--load-gate", required=True, help="Load gate JSON under target/load-gate")
    parser.add_argument("--out", required=True, help="Create-new packet under target/candidate-dependency-scope")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    fixture_manual_review, fixture_manual_review_path = load_fixture_manual_review(
        Path(args.fixture_manual_review)
    )
    dependency_review, dependency_review_path = load_dependency_review(Path(args.dependency_review))
    dependency_preflight = None
    dependency_preflight_path = None
    if args.dependency_preflight:
        dependency_preflight, dependency_preflight_path = load_dependency_preflight(Path(args.dependency_preflight))
    load_gate, load_gate_path = load_load_gate(Path(args.load_gate))
    packet = build_candidate_dependency_scope_packet(
        fixture_manual_review=fixture_manual_review,
        fixture_manual_review_path=fixture_manual_review_path,
        dependency_review=dependency_review,
        dependency_review_path=dependency_review_path,
        dependency_preflight=dependency_preflight,
        dependency_preflight_path=dependency_preflight_path,
        load_gate=load_gate,
        load_gate_path=load_gate_path,
    )
    written = write_json_create_new(Path(args.out), packet)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

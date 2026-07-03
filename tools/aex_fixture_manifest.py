#!/usr/bin/env python3
"""Build a no-load fixture review manifest from an AEX static probe report."""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
STATIC_REPORT_ROOT = LAB_ROOT / "target" / "aex-static-probe"
MANIFEST_ROOT = LAB_ROOT / "target" / "fixture-review"

SOURCE_SAFETY_FLAGS = (
    "native_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
)

ENTRY_SAFETY_FLAGS = (
    "native_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
)

BLOCKED_ACTIONS = [
    "load_aex_dll",
    "call_EffectMain",
    "start_after_effects",
    "render_with_aex",
    "route_through_ofx",
    "copy_binary_payload",
]


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


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("output path must have .json extension")
    MANIFEST_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, MANIFEST_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(MANIFEST_ROOT.resolve(strict=True)):
        raise ValueError(f"output parent must stay under {MANIFEST_ROOT}")
    return resolved


def validate_report_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("source report must have .json extension")
    return resolve_under_root(path, STATIC_REPORT_ROOT, must_exist=True)


def load_source_report(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_report_path(path)
    with resolved.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source report must be a JSON object")
    return payload, resolved


def validate_source_report(report: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if report.get("report_kind") != "aex_static_probe":
        errors.append("source report_kind must be aex_static_probe")
    if int(report.get("schema_version") or 0) < 2:
        errors.append("source schema_version must be >= 2")
    if report.get("publication_status") != "local-only":
        errors.append("source publication_status must be local-only")
    for flag in SOURCE_SAFETY_FLAGS:
        if report.get(flag) is not False:
            errors.append(f"source {flag} must be false")
    entries = report.get("entries")
    if not isinstance(entries, list):
        errors.append("source entries must be a list")
        return errors
    for index, entry in enumerate(entries):
        if not isinstance(entry, dict):
            errors.append(f"entry {index} must be an object")
            continue
        for flag in ENTRY_SAFETY_FLAGS:
            if entry.get(flag) is not False:
                errors.append(f"entry {index} {flag} must be false")
    return errors


def entry_by_relative_path(report: dict[str, Any]) -> dict[str, dict[str, Any]]:
    result: dict[str, dict[str, Any]] = {}
    for entry in report.get("entries", []):
        if isinstance(entry, dict) and isinstance(entry.get("relative_path"), str):
            result[entry["relative_path"]] = entry
    return result


def import_dll_names(entry: dict[str, Any]) -> list[str]:
    names = entry.get("pe", {}).get("import_summary", {}).get("dll_names", [])
    if not isinstance(names, list):
        return []
    return [str(name) for name in names[:32]]


def resource_types(entry: dict[str, Any]) -> list[str]:
    details = entry.get("pe", {}).get("resource_summary", {}).get("type_details", [])
    if not isinstance(details, list):
        return []
    return [str(detail.get("type")) for detail in details if isinstance(detail, dict) and detail.get("type")][:32]


def pipl_resource_entries(entry: dict[str, Any]) -> list[dict[str, Any]]:
    entries = entry.get("pe", {}).get("resource_summary", {}).get("pipl_resource_entries", [])
    if not isinstance(entries, list):
        return []
    compact: list[dict[str, Any]] = []
    for resource in entries[:16]:
        if not isinstance(resource, dict):
            continue
        compact.append(
            {
                "type": resource.get("type"),
                "name": resource.get("name"),
                "language": resource.get("language"),
                "data_rva": resource.get("data_rva"),
                "size_bytes": resource.get("size_bytes"),
                "codepage": resource.get("codepage"),
            }
        )
    return compact


def compact_candidate(entry: dict[str, Any], *, review_status: str, notes: list[str]) -> dict[str, Any]:
    resource_summary = entry.get("pe", {}).get("resource_summary", {})
    return {
        "relative_path": entry.get("relative_path"),
        "file_name": entry.get("file_name"),
        "size_bytes": entry.get("size_bytes"),
        "mtime_utc": entry.get("mtime_utc"),
        "compatibility_class": entry.get("compatibility_class"),
        "fixture_candidate_score": entry.get("fixture_candidate_score"),
        "fixture_candidate_reasons": entry.get("fixture_candidate_reasons", []),
        "machine_label": entry.get("pe", {}).get("machine_label"),
        "dll_image": entry.get("pe", {}).get("characteristics_flags", {}).get("dll"),
        "pipl_signal_present": entry.get("pipl_signal_present"),
        "effect_main_export_present": entry.get("pe", {})
        .get("export_summary", {})
        .get("effect_main_export_present"),
        "effect_main_marker_present": entry.get("markers", {}).get("effect_main_marker_present"),
        "aegp_marker_count": entry.get("markers", {}).get("ae_plugin_marker_count", 0),
        "resource_types": resource_types(entry),
        "pipl_resource_data_entry_count": resource_summary.get("pipl_resource_data_entry_count"),
        "pipl_resource_total_size": resource_summary.get("pipl_resource_total_size"),
        "pipl_resource_entries": pipl_resource_entries(entry),
        "import_dll_names": import_dll_names(entry),
        "review_status": review_status,
        "review_notes": notes,
    }


def select_approved_review_candidates(report: dict[str, Any], limit: int) -> list[dict[str, Any]]:
    entries = entry_by_relative_path(report)
    seed_paths = [
        candidate.get("relative_path")
        for candidate in report.get("fixture_candidates", [])
        if isinstance(candidate, dict) and isinstance(candidate.get("relative_path"), str)
    ]
    candidates: list[dict[str, Any]] = [entries[path] for path in seed_paths if path in entries]
    if not candidates:
        candidates = [
            entry
            for entry in entries.values()
            if entry.get("compatibility_class") == "classic_pf_effect_candidate"
            and int(entry.get("fixture_candidate_score") or 0) > 0
        ]
    candidates.sort(
        key=lambda entry: (
            -int(entry.get("fixture_candidate_score") or 0),
            int(entry.get("size_bytes") or 0),
            str(entry.get("relative_path")),
        )
    )
    return [
        compact_candidate(
            entry,
            review_status="static_review_candidate",
            notes=[
                "Static metadata suggests a classic PF effect candidate.",
                "Manual provenance/license review is still required before any load gate opens.",
            ],
        )
        for entry in candidates[:limit]
    ]


def select_hold_candidates(report: dict[str, Any], limit: int) -> list[dict[str, Any]]:
    hold_classes = {
        "classic_pf_effect_with_aegp_markers": "EffectMain is present, but AEGP markers require host-contract review.",
        "aegp_or_helper_candidate": "AEGP/helper-looking entry is not a first classic PF effect fixture.",
        "pipl_present_unclassified": "PiPL is present, but static evidence is not enough for first fixture approval.",
    }
    candidates = [
        entry
        for entry in report.get("entries", [])
        if isinstance(entry, dict) and entry.get("compatibility_class") in hold_classes
    ]
    candidates.sort(
        key=lambda entry: (
            str(entry.get("compatibility_class")),
            -int(entry.get("fixture_candidate_score") or 0),
            int(entry.get("size_bytes") or 0),
            str(entry.get("relative_path")),
        )
    )
    return [
        compact_candidate(
            entry,
            review_status="hold_for_later_review",
            notes=[hold_classes[str(entry.get("compatibility_class"))]],
        )
        for entry in candidates[:limit]
    ]


def build_manifest_payload(
    report: dict[str, Any],
    source_report_path: Path,
    *,
    candidate_limit: int = 8,
    hold_limit: int = 8,
) -> dict[str, Any]:
    errors = validate_source_report(report)
    if errors:
        raise ValueError("; ".join(errors))
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "manifest_kind": "aex_fixture_review_manifest",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_report": str(source_report_path),
        "source_report_schema_version": report.get("schema_version"),
        "source_input_root": report.get("input_root"),
        "source_summary": report.get("summary", {}),
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "safety_gate": {
            "state": "review_manifest_only",
            "allowed_next_action": "manual_static_fixture_review",
            "blocked_actions": BLOCKED_ACTIONS,
            "requires_explicit_user_approval_before": [
                "copying selected AEX fixture",
                "loading selected AEX fixture",
                "calling EffectMain",
                "rendering with selected AEX fixture",
                "opening any OFX route",
            ],
        },
        "selected_candidates": select_approved_review_candidates(report, candidate_limit),
        "hold_candidates": select_hold_candidates(report, hold_limit),
        "review_checklist": [
            "Confirm the candidate is local/provenance-safe for fixture use.",
            "Confirm license/publication boundary before sharing any metadata beyond local-only reports.",
            "Confirm the candidate remains in classic_pf_effect_candidate class after a fresh static probe.",
            "Design and review an isolated worker process before any native load gate opens.",
        ],
        "notes": [
            "Manifest is derived from static probe JSON only.",
            "No AEX file is opened by this tool.",
            "No DLL load, EffectMain call, After Effects launch, render, OFX route, binary copy, or hash is performed.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build no-load AEX fixture review manifest")
    parser.add_argument("--report", required=True, help="Static probe JSON under target/aex-static-probe")
    parser.add_argument("--out", required=True, help="Create-new manifest JSON under target/fixture-review")
    parser.add_argument("--limit", type=int, default=8, help="Selected fixture candidate limit")
    parser.add_argument("--hold-limit", type=int, default=8, help="Hold-for-review candidate limit")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    report, source_path = load_source_report(Path(args.report))
    manifest = build_manifest_payload(
        report,
        source_path,
        candidate_limit=max(0, args.limit),
        hold_limit=max(0, args.hold_limit),
    )
    written = write_json_create_new(Path(args.out), manifest)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

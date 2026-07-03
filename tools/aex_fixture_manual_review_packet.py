#!/usr/bin/env python3
"""Build a no-load manual-review packet for the first AEX fixture candidate.

The packet reads fixture dossier, dependency review, load gate JSON, and an
optional WizTree CSV snapshot. It does not open, copy, hash, load, or execute
AEX files; the CSV is used only for size/inventory context.
"""

from __future__ import annotations

import argparse
import csv
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TOOLS_ROOT = LAB_ROOT.parent
TARGET_ROOT = LAB_ROOT / "target"
DOSSIER_ROOT = TARGET_ROOT / "fixture-dossier"
DEPENDENCY_REVIEW_ROOT = TARGET_ROOT / "dependency-review"
LOAD_GATE_ROOT = TARGET_ROOT / "load-gate"
MANUAL_REVIEW_ROOT = TARGET_ROOT / "fixture-manual-review"
WIZTREE_EXPORT_ROOT = TOOLS_ROOT / "WizTree MCP" / "exports"

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


def validate_wiztree_csv(path: Path) -> Path:
    if path.suffix.lower() != ".csv":
        raise ValueError("WizTree snapshot must have .csv extension")
    return resolve_under_root(path, WIZTREE_EXPORT_ROOT, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("fixture manual-review packet must have .json extension")
    MANUAL_REVIEW_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, MANUAL_REVIEW_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(MANUAL_REVIEW_ROOT.resolve(strict=True)):
        raise ValueError(f"fixture manual-review packet parent must stay under {MANUAL_REVIEW_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_fixture_dossier(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, DOSSIER_ROOT, "fixture dossier")
    return read_json_object(resolved), resolved


def load_dependency_review(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, DEPENDENCY_REVIEW_ROOT, "dependency review")
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


def validate_dossier(dossier: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if dossier.get("publication_status") != "local-only":
        errors.append("fixture dossier publication_status must be local-only")
    if dossier.get("report_kind") != "aex_fixture_review_dossier":
        errors.append("fixture dossier report_kind must be aex_fixture_review_dossier")
    if not isinstance(dossier.get("candidate"), dict):
        errors.append("fixture dossier candidate must be an object")
    if not isinstance(dossier.get("candidate_relative_path"), str):
        errors.append("fixture dossier candidate_relative_path must be a string")
    if not isinstance(dossier.get("review_items"), list):
        errors.append("fixture dossier review_items must be a list")
    if not isinstance(dossier.get("risk_flags"), list):
        errors.append("fixture dossier risk_flags must be a list")
    errors.extend(safety_errors(dossier, "fixture dossier"))
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


def validate_load_gate(gate: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if gate.get("publication_status") != "local-only":
        errors.append("load gate publication_status must be local-only")
    if gate.get("report_kind") != "aex_load_gate_check":
        errors.append("load gate report_kind must be aex_load_gate_check")
    if not isinstance(gate.get("gate_state"), str):
        errors.append("load gate gate_state must be a string")
    if not isinstance(gate.get("gate_errors"), list):
        errors.append("load gate gate_errors must be a list")
    errors.extend(safety_errors(gate, "load gate"))
    return errors


def normalize_path_text(value: str) -> str:
    return value.replace("/", "\\").lower()


def int_or_none(value: Any) -> int | None:
    try:
        return int(str(value).replace(",", "").strip())
    except (TypeError, ValueError):
        return None


def tools_relative_path(path_text: str) -> str:
    path = Path(path_text)
    try:
        return str(path.resolve(strict=False).relative_to(TOOLS_ROOT.resolve(strict=True)))
    except ValueError:
        return path_text


def compact_wiztree_row(row: dict[str, Any]) -> dict[str, Any]:
    return {
        "tools_relative_path": tools_relative_path(str(row.get("path", ""))),
        "file_name": Path(str(row.get("path", ""))).name,
        "size_bytes": row.get("size_bytes"),
        "allocated_bytes": row.get("allocated_bytes"),
        "modified": row.get("modified"),
    }


def read_wiztree_csv(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    with path.open("r", encoding="utf-8-sig", newline="") as handle:
        lines = handle.readlines()
    if not lines:
        return rows
    header_index = 0
    if "ファイル名" not in lines[0] and "File" not in lines[0]:
        header_index = 1
    reader = csv.DictReader(lines[header_index:])
    for raw in reader:
        file_path = raw.get("ファイル名") or raw.get("File Name") or raw.get("Filename") or raw.get("Path")
        if not file_path:
            continue
        if Path(file_path).suffix.lower() != ".aex":
            continue
        size = int_or_none(raw.get("サイズ") or raw.get("Size"))
        allocated = int_or_none(raw.get("割り当て") or raw.get("Allocated"))
        rows.append(
            {
                "path": str(file_path),
                "size_bytes": size,
                "allocated_bytes": allocated,
                "modified": raw.get("更新日時") or raw.get("Modified"),
            }
        )
    return rows


def wiztree_inventory_summary(
    *,
    csv_path: Path | None,
    candidate_relative_path: str,
    candidate_size_bytes: int | None,
) -> dict[str, Any]:
    if csv_path is None:
        return {
            "inventory_state": "not_provided",
            "aex_file_count": None,
            "total_size_bytes": None,
            "candidate_match_count": None,
            "candidate_size_match": None,
            "candidate_size_rank_smallest": None,
            "largest_aex_files": [],
            "candidate_rows": [],
        }

    rows = read_wiztree_csv(csv_path)
    candidate_norm = normalize_path_text(candidate_relative_path)
    matching = [
        row
        for row in rows
        if normalize_path_text(str(row.get("path", ""))).endswith(candidate_norm)
        or normalize_path_text(Path(str(row.get("path", ""))).name) == normalize_path_text(Path(candidate_relative_path).name)
    ]
    total_size = sum(int(row.get("size_bytes") or 0) for row in rows)
    sorted_by_size = sorted(rows, key=lambda row: (int(row.get("size_bytes") or 0), str(row.get("path"))))
    candidate_rank = None
    if matching:
        match_paths = {row.get("path") for row in matching}
        for index, row in enumerate(sorted_by_size, start=1):
            if row.get("path") in match_paths:
                candidate_rank = index
                break
    candidate_size_match = None
    if matching and candidate_size_bytes is not None:
        candidate_size_match = any(row.get("size_bytes") == candidate_size_bytes for row in matching)
    largest = sorted(rows, key=lambda row: int(row.get("size_bytes") or 0), reverse=True)[:10]
    return {
        "inventory_state": "wiztree_csv_read_metadata_only",
        "source_wiztree_csv": str(csv_path),
        "aex_file_count": len(rows),
        "total_size_bytes": total_size,
        "candidate_match_count": len(matching),
        "candidate_size_match": candidate_size_match,
        "candidate_size_rank_smallest": candidate_rank,
        "candidate_size_percentile_smallest": (
            round((candidate_rank / len(rows)) * 100, 2) if candidate_rank and rows else None
        ),
        "largest_aex_files": [compact_wiztree_row(row) for row in largest],
        "candidate_rows": [compact_wiztree_row(row) for row in matching[:8]],
    }


def count_review_statuses(items: list[dict[str, Any]]) -> dict[str, int]:
    counts: dict[str, int] = {}
    for item in items:
        status = str(item.get("status") or item.get("review_severity") or item.get("review_state") or "unknown")
        counts[status] = counts.get(status, 0) + 1
    return dict(sorted(counts.items()))


def approval_blockers(
    *,
    dossier: dict[str, Any],
    dependency_review: dict[str, Any],
    load_gate: dict[str, Any],
) -> list[dict[str, Any]]:
    blockers: list[dict[str, Any]] = []
    if dossier.get("dossier_state") == "manual_review_pending":
        blockers.append(
            {
                "id": "manual_fixture_review_pending",
                "severity": "blocker",
                "evidence": dossier.get("dossier_state"),
                "next_action": "Complete provenance/license/safety review.",
            }
        )
    if dossier.get("load_approval_recommendation") != "approval_manifest_present_gate_check_required":
        blockers.append(
            {
                "id": "fixture_not_approved",
                "severity": "blocker",
                "evidence": dossier.get("load_approval_recommendation"),
                "next_action": "Keep or revise the fixture decision; do not open the load gate without explicit approval.",
            }
        )
    if dependency_review.get("native_load_recommendation") != "manual_loader_design_review_only_no_auto_approval":
        blockers.append(
            {
                "id": "dependency_review_blocks_native_load",
                "severity": "blocker",
                "evidence": dependency_review.get("native_load_recommendation"),
                "next_action": "Resolve default-deny/manual dependency rows before loader design.",
            }
        )
    if load_gate.get("gate_state") != "preconditions_satisfied_no_load_performed":
        blockers.append(
            {
                "id": "load_gate_closed",
                "severity": "blocker",
                "evidence": load_gate.get("gate_state"),
                "gate_errors": load_gate.get("gate_errors", []),
                "next_action": "Rerun the load gate only after review/approval inputs change.",
            }
        )
    risk_flags = dossier.get("risk_flags") if isinstance(dossier.get("risk_flags"), list) else []
    if risk_flags:
        blockers.append(
            {
                "id": "static_risk_flags_present",
                "severity": "review",
                "evidence": risk_flags,
                "next_action": "Resolve or document static risk flags before approval.",
            }
        )
    return blockers


def recommended_next_decision(blockers: list[dict[str, Any]]) -> str:
    blocker_ids = {str(item.get("id")) for item in blockers}
    if "static_risk_flags_present" in blocker_ids:
        return "hold_or_reject_pending_static_risk_review"
    if {"manual_fixture_review_pending", "fixture_not_approved"} & blocker_ids:
        return "keep_hold_pending_manual_review"
    if "dependency_review_blocks_native_load" in blocker_ids:
        return "keep_hold_pending_dependency_review"
    if "load_gate_closed" in blocker_ids:
        return "keep_hold_pending_gate_review"
    return "approval_ready_not_approved"


def build_manual_review_packet(
    *,
    fixture_dossier: dict[str, Any],
    fixture_dossier_path: Path,
    dependency_review: dict[str, Any],
    dependency_review_path: Path,
    load_gate: dict[str, Any],
    load_gate_path: Path,
    wiztree_csv_path: Path | None = None,
) -> dict[str, Any]:
    errors = (
        validate_dossier(fixture_dossier)
        + validate_dependency_review(dependency_review)
        + validate_load_gate(load_gate)
    )
    if errors:
        raise ValueError("; ".join(errors))

    candidate = fixture_dossier.get("candidate", {})
    candidate_relative_path = fixture_dossier.get("candidate_relative_path")
    gate_candidate = load_gate.get("primary_review_candidate", {})
    if isinstance(gate_candidate, dict) and gate_candidate.get("relative_path") != candidate_relative_path:
        raise ValueError("load gate primary candidate must match fixture dossier candidate")

    candidate_size = int_or_none(candidate.get("size_bytes")) if isinstance(candidate, dict) else None
    wiztree_summary = wiztree_inventory_summary(
        csv_path=wiztree_csv_path,
        candidate_relative_path=str(candidate_relative_path),
        candidate_size_bytes=candidate_size,
    )
    blockers = approval_blockers(
        dossier=fixture_dossier,
        dependency_review=dependency_review,
        load_gate=load_gate,
    )
    approval_ready = not any(item.get("severity") == "blocker" for item in blockers)
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_fixture_manual_review_packet",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_fixture_dossier": str(fixture_dossier_path),
        "source_dependency_review": str(dependency_review_path),
        "source_load_gate": str(load_gate_path),
        "source_wiztree_csv": str(wiztree_csv_path) if wiztree_csv_path else None,
        "review_packet_state": "fixture_manual_review_packet_ready_no_load",
        "manual_review_ready": True,
        "approval_ready": approval_ready,
        "candidate_relative_path": candidate_relative_path,
        "candidate": {
            "relative_path": candidate.get("relative_path"),
            "file_name": candidate.get("file_name"),
            "size_bytes": candidate.get("size_bytes"),
            "compatibility_class": candidate.get("compatibility_class"),
            "fixture_candidate_score": candidate.get("fixture_candidate_score"),
            "fixture_candidate_reasons": candidate.get("fixture_candidate_reasons", []),
            "pipl_signal_present": candidate.get("pipl_signal_present"),
            "effect_main_export_present": candidate.get("effect_main_export_present"),
            "aegp_marker_count": candidate.get("aegp_marker_count"),
            "import_dll_names": candidate.get("import_dll_names", []),
        },
        "decision_summary": {
            "dossier_state": fixture_dossier.get("dossier_state"),
            "load_approval_recommendation": fixture_dossier.get("load_approval_recommendation"),
            "risk_flags": fixture_dossier.get("risk_flags", []),
            "review_item_status_counts": count_review_statuses(fixture_dossier.get("review_items", [])),
        },
        "dependency_summary": {
            "review_state": dependency_review.get("review_state"),
            "native_load_recommendation": dependency_review.get("native_load_recommendation"),
            "summary": dependency_review.get("summary", {}),
            "blocker_items": [
                item
                for item in dependency_review.get("review_items", [])
                if isinstance(item, dict) and item.get("review_severity") == "blocker"
            ][:8],
        },
        "load_gate_summary": {
            "gate_state": load_gate.get("gate_state"),
            "approval_state": load_gate.get("approval_state"),
            "dependency_review_state": load_gate.get("dependency_review_state"),
            "dependency_native_load_recommendation": load_gate.get("dependency_native_load_recommendation"),
            "gate_errors": load_gate.get("gate_errors", []),
        },
        "wiztree_aex_inventory": wiztree_summary,
        "approval_blockers": blockers,
        "approval_blocker_count": len([item for item in blockers if item.get("severity") == "blocker"]),
        "recommended_next_decision": recommended_next_decision(blockers),
        "manual_review_questions": fixture_dossier.get("manual_review_questions", []),
        "blocked_actions": [
            "copy_selected_aex_fixture",
            "open_aex_file",
            "hash_aex_file",
            "load_aex_dll",
            "call_EffectMain",
            "start_after_effects",
            "render_with_aex",
            "route_through_ofx",
        ],
        "allowed_next_actions": [
            "review provenance/license/safety notes",
            "resolve dependency-review blockers",
            "emit a revised hold/reject decision if needed",
            "rerun load-gate check after inputs change",
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
            "Packet reads existing JSON evidence and optional WizTree CSV metadata only.",
            "No AEX file is opened, copied, hashed, loaded, or executed.",
            "This packet is decision support, not fixture approval.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build no-load AEX fixture manual-review packet")
    parser.add_argument("--fixture-dossier", required=True, help="Fixture dossier JSON under target/fixture-dossier")
    parser.add_argument("--dependency-review", required=True, help="Dependency review JSON under target/dependency-review")
    parser.add_argument("--load-gate", required=True, help="Load gate JSON under target/load-gate")
    parser.add_argument("--wiztree-csv", help="Optional WizTree AEX CSV under D:/Projects/01_Project/04_Tools/WizTree MCP/exports")
    parser.add_argument("--out", required=True, help="Create-new packet under target/fixture-manual-review")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    dossier, dossier_path = load_fixture_dossier(Path(args.fixture_dossier))
    dependency_review, dependency_review_path = load_dependency_review(Path(args.dependency_review))
    load_gate, load_gate_path = load_load_gate(Path(args.load_gate))
    wiztree_csv_path = validate_wiztree_csv(Path(args.wiztree_csv)) if args.wiztree_csv else None
    packet = build_manual_review_packet(
        fixture_dossier=dossier,
        fixture_dossier_path=dossier_path,
        dependency_review=dependency_review,
        dependency_review_path=dependency_review_path,
        load_gate=load_gate,
        load_gate_path=load_gate_path,
        wiztree_csv_path=wiztree_csv_path,
    )
    written = write_json_create_new(Path(args.out), packet)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

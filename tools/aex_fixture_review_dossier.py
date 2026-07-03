#!/usr/bin/env python3
"""Build a local-only fixture review dossier from manifest/decision JSON.

The dossier is a manual-review aid. It does not approve, copy, hash, open, load,
or execute AEX files.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
FIXTURE_REVIEW_ROOT = TARGET_ROOT / "fixture-review"
FIXTURE_APPROVAL_ROOT = TARGET_ROOT / "fixture-approval"
DOSSIER_ROOT = TARGET_ROOT / "fixture-dossier"

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


def validate_manifest_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("fixture manifest must have .json extension")
    return resolve_under_root(path, FIXTURE_REVIEW_ROOT, must_exist=True)


def validate_decision_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("fixture decision must have .json extension")
    return resolve_under_root(path, FIXTURE_APPROVAL_ROOT, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("fixture dossier output must have .json extension")
    DOSSIER_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, DOSSIER_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(DOSSIER_ROOT.resolve(strict=True)):
        raise ValueError(f"fixture dossier parent must stay under {DOSSIER_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_manifest(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_manifest_path(path)
    return read_json_object(resolved), resolved


def load_decision(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_decision_path(path)
    return read_json_object(resolved), resolved


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def validate_manifest(manifest: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if manifest.get("manifest_kind") != "aex_fixture_review_manifest":
        errors.append("manifest_kind must be aex_fixture_review_manifest")
    if manifest.get("publication_status") != "local-only":
        errors.append("manifest publication_status must be local-only")
    if not isinstance(manifest.get("selected_candidates"), list):
        errors.append("manifest selected_candidates must be a list")
    if not isinstance(manifest.get("hold_candidates"), list):
        errors.append("manifest hold_candidates must be a list")
    errors.extend(safety_errors(manifest, "manifest"))
    return errors


def validate_decision(decision: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if decision.get("manifest_kind") not in {
        "aex_fixture_decision_manifest",
        "aex_fixture_approval_manifest",
    }:
        errors.append("decision manifest_kind must be a fixture decision/approval kind")
    if decision.get("publication_status") != "local-only":
        errors.append("decision publication_status must be local-only")
    if not isinstance(decision.get("candidate_relative_path"), str):
        errors.append("decision candidate_relative_path must be present")
    errors.extend(safety_errors(decision, "decision"))
    return errors


def candidate_index(manifest: dict[str, Any]) -> dict[str, dict[str, Any]]:
    result: dict[str, dict[str, Any]] = {}
    for bucket in ("selected_candidates", "hold_candidates"):
        for candidate in manifest.get(bucket, []):
            if isinstance(candidate, dict) and isinstance(candidate.get("relative_path"), str):
                result[candidate["relative_path"]] = {**candidate, "_source_bucket": bucket}
    return result


def select_candidate(
    manifest: dict[str, Any],
    decision: dict[str, Any],
    candidate_relative_path: str | None,
) -> dict[str, Any]:
    relative_path = candidate_relative_path or decision.get("candidate_relative_path")
    if not isinstance(relative_path, str) or not relative_path:
        raise ValueError("candidate relative path is required")
    candidates = candidate_index(manifest)
    if relative_path not in candidates:
        raise ValueError(f"candidate not found in fixture manifest: {relative_path}")
    return candidates[relative_path]


def normalized_imports(candidate: dict[str, Any]) -> list[str]:
    imports = candidate.get("import_dll_names", [])
    if not isinstance(imports, list):
        return []
    return [str(name).lower() for name in imports]


def risk_flags(candidate: dict[str, Any]) -> list[str]:
    flags: list[str] = []
    imports = normalized_imports(candidate)
    if candidate.get("compatibility_class") != "classic_pf_effect_candidate":
        flags.append("not_classic_pf_effect_candidate")
    if candidate.get("pipl_signal_present") is not True:
        flags.append("pipl_signal_missing")
    if int(candidate.get("pipl_resource_data_entry_count") or 0) <= 0:
        flags.append("pipl_resource_metadata_missing")
    if candidate.get("effect_main_export_present") is not True:
        flags.append("effect_main_export_missing")
    if int(candidate.get("aegp_marker_count") or 0) > 0:
        flags.append("aegp_markers_present")
    if int(candidate.get("size_bytes") or 0) > 2_000_000:
        flags.append("large_fixture_candidate")
    if any(marker in imports for marker in DEBUG_RUNTIME_IMPORT_MARKERS):
        flags.append("debug_runtime_imports_present")
    if any(marker in imports for marker in GRAPHICS_IMPORT_MARKERS):
        flags.append("graphics_or_gpu_imports_present")
    if candidate.get("_source_bucket") != "selected_candidates":
        flags.append("not_in_selected_candidate_bucket")
    return flags


def review_item(item_id: str, status: str, evidence: Any, note: str) -> dict[str, Any]:
    return {"id": item_id, "status": status, "evidence": evidence, "note": note}


def build_review_items(candidate: dict[str, Any], decision: dict[str, Any], flags: list[str]) -> list[dict[str, Any]]:
    decision_state = decision.get("decision_state")
    approval_state = decision.get("approval_state")
    return [
        review_item(
            "classic_pf_static_classification",
            "pass" if candidate.get("compatibility_class") == "classic_pf_effect_candidate" else "needs_review",
            candidate.get("compatibility_class"),
            "First native-load fixture should be a classic PF effect candidate.",
        ),
        review_item(
            "pipl_signal",
            "pass" if candidate.get("pipl_signal_present") is True else "fail",
            candidate.get("pipl_signal_present"),
            "PiPL resource/static signal is required for AE effect metadata review.",
        ),
        review_item(
            "pipl_resource_metadata",
            "pass" if int(candidate.get("pipl_resource_data_entry_count") or 0) > 0 else "needs_review",
            {
                "pipl_resource_data_entry_count": candidate.get("pipl_resource_data_entry_count"),
                "pipl_resource_total_size": candidate.get("pipl_resource_total_size"),
                "pipl_resource_entries": candidate.get("pipl_resource_entries", []),
            },
            "Schema 3 static probe should provide PiPL resource entry metadata without payload extraction.",
        ),
        review_item(
            "effect_main_export",
            "pass" if candidate.get("effect_main_export_present") is True else "fail",
            candidate.get("effect_main_export_present"),
            "EffectMain export is expected for classic PF effect load experiments.",
        ),
        review_item(
            "aegp_marker_absence",
            "pass" if int(candidate.get("aegp_marker_count") or 0) == 0 else "needs_review",
            candidate.get("aegp_marker_count", 0),
            "AEGP marker presence changes the host-contract risk profile.",
        ),
        review_item(
            "runtime_import_risk",
            "pass" if not {"debug_runtime_imports_present", "graphics_or_gpu_imports_present"} & set(flags) else "needs_review",
            candidate.get("import_dll_names", []),
            "Debug runtime or GPU/graphics imports need manual environment review.",
        ),
        review_item(
            "fixture_decision",
            "pending" if decision_state == "hold_for_manual_review" else str(decision.get("decision", "unknown")),
            {"decision_state": decision_state, "approval_state": approval_state},
            "Current decision must be explicit user approval before any load gate can open.",
        ),
        review_item(
            "no_runtime_action",
            "pass",
            {flag: False for flag in SAFETY_FLAGS},
            "This dossier path keeps native load, render, AE, OFX, and private payload copying false.",
        ),
    ]


def compact_candidate(candidate: dict[str, Any]) -> dict[str, Any]:
    return {
        "relative_path": candidate.get("relative_path"),
        "file_name": candidate.get("file_name"),
        "size_bytes": candidate.get("size_bytes"),
        "compatibility_class": candidate.get("compatibility_class"),
        "fixture_candidate_score": candidate.get("fixture_candidate_score"),
        "fixture_candidate_reasons": candidate.get("fixture_candidate_reasons", []),
        "machine_label": candidate.get("machine_label"),
        "pipl_signal_present": candidate.get("pipl_signal_present"),
        "effect_main_export_present": candidate.get("effect_main_export_present"),
        "effect_main_marker_present": candidate.get("effect_main_marker_present"),
        "aegp_marker_count": candidate.get("aegp_marker_count", 0),
        "resource_types": candidate.get("resource_types", []),
        "pipl_resource_data_entry_count": candidate.get("pipl_resource_data_entry_count"),
        "pipl_resource_total_size": candidate.get("pipl_resource_total_size"),
        "pipl_resource_entries": candidate.get("pipl_resource_entries", []),
        "import_dll_names": candidate.get("import_dll_names", []),
        "source_bucket": candidate.get("_source_bucket"),
    }


def approval_recommendation(decision: dict[str, Any], flags: list[str]) -> str:
    if decision.get("decision") == "reject":
        return "rejected_for_load_gate"
    if decision.get("approval_state") == "user_approved_for_load_gate":
        return "approval_manifest_present_gate_check_required"
    if flags:
        return "do_not_approve_yet_static_risks_or_review_items_present"
    return "do_not_approve_yet_manual_review_pending"


def dossier_state(decision: dict[str, Any]) -> str:
    if decision.get("decision") == "reject":
        return "rejected_for_load_gate"
    if decision.get("approval_state") == "user_approved_for_load_gate":
        return "approval_manifest_present_native_gate_still_required"
    return "manual_review_pending"


def build_dossier(
    manifest: dict[str, Any],
    manifest_path: Path,
    decision: dict[str, Any],
    decision_path: Path,
    *,
    candidate_relative_path: str | None = None,
) -> dict[str, Any]:
    errors = validate_manifest(manifest) + validate_decision(decision)
    if errors:
        raise ValueError("; ".join(errors))
    candidate = select_candidate(manifest, decision, candidate_relative_path)
    if candidate.get("relative_path") != decision.get("candidate_relative_path"):
        raise ValueError("candidate-relative-path must match decision candidate_relative_path")
    flags = risk_flags(candidate)
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_fixture_review_dossier",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_fixture_manifest": str(manifest_path),
        "source_fixture_decision": str(decision_path),
        "candidate": compact_candidate(candidate),
        "candidate_relative_path": candidate.get("relative_path"),
        "dossier_state": dossier_state(decision),
        "load_approval_recommendation": approval_recommendation(decision, flags),
        "risk_flags": flags,
        "review_items": build_review_items(candidate, decision, flags),
        "manual_review_questions": [
            "Is the candidate provenance-safe to use as a local fixture?",
            "Is the candidate license-compatible with the intended local-only or future publication scope?",
            "Is the host contract classic PF only, with no AEGP/helper behavior requiring a different harness?",
            "Is the runtime dependency set available inside the future sandbox worker?",
        ],
        "blocked_actions": [
            "copy_selected_aex_fixture",
            "load_aex_dll",
            "call_EffectMain",
            "start_after_effects",
            "render_with_aex",
            "route_through_ofx",
        ],
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "notes": [
            "Dossier reads fixture manifest and decision JSON only.",
            "No AEX file is opened, copied, hashed, loaded, or executed.",
            "This report is a review aid; it is not an approval artifact.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build local AEX fixture review dossier")
    parser.add_argument("--fixture-manifest", required=True, help="Fixture review manifest under target/fixture-review")
    parser.add_argument("--fixture-decision", required=True, help="Fixture decision JSON under target/fixture-approval")
    parser.add_argument("--candidate-relative-path", help="Candidate relative path; defaults to decision candidate")
    parser.add_argument("--out", required=True, help="Create-new dossier JSON under target/fixture-dossier")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    manifest, manifest_path = load_manifest(Path(args.fixture_manifest))
    decision, decision_path = load_decision(Path(args.fixture_decision))
    dossier = build_dossier(
        manifest,
        manifest_path,
        decision,
        decision_path,
        candidate_relative_path=args.candidate_relative_path,
    )
    written = write_json_create_new(Path(args.out), dossier)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

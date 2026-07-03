#!/usr/bin/env python3
"""Create local-only fixture approval/hold/rejection decisions.

The tool reads fixture review manifest JSON only. It never opens, copies,
hashes, loads, or executes AEX files.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
FIXTURE_REVIEW_ROOT = LAB_ROOT / "target" / "fixture-review"
FIXTURE_APPROVAL_ROOT = LAB_ROOT / "target" / "fixture-approval"

SAFETY_FLAGS = (
    "native_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
)

APPROVAL_TOKEN = "APPROVE_AEX_LOAD_GATE"


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


def validate_review_manifest_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("fixture review manifest must have .json extension")
    return resolve_under_root(path, FIXTURE_REVIEW_ROOT, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("fixture decision output must have .json extension")
    FIXTURE_APPROVAL_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, FIXTURE_APPROVAL_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(FIXTURE_APPROVAL_ROOT.resolve(strict=True)):
        raise ValueError(f"fixture decision parent must stay under {FIXTURE_APPROVAL_ROOT}")
    return resolved


def load_review_manifest(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_review_manifest_path(path)
    with resolved.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("fixture review manifest must be a JSON object")
    return payload, resolved


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def validate_review_manifest(manifest: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if manifest.get("manifest_kind") != "aex_fixture_review_manifest":
        errors.append("source manifest_kind must be aex_fixture_review_manifest")
    if manifest.get("publication_status") != "local-only":
        errors.append("source publication_status must be local-only")
    errors.extend(safety_errors(manifest, "source"))
    if not isinstance(manifest.get("selected_candidates"), list):
        errors.append("source selected_candidates must be a list")
    if not isinstance(manifest.get("hold_candidates"), list):
        errors.append("source hold_candidates must be a list")
    return errors


def candidate_index(manifest: dict[str, Any]) -> dict[str, dict[str, Any]]:
    result: dict[str, dict[str, Any]] = {}
    for bucket in ("selected_candidates", "hold_candidates"):
        for candidate in manifest.get(bucket, []):
            if isinstance(candidate, dict) and isinstance(candidate.get("relative_path"), str):
                result[candidate["relative_path"]] = {**candidate, "_source_bucket": bucket}
    return result


def select_candidate(manifest: dict[str, Any], relative_path: str | None) -> dict[str, Any]:
    candidates = candidate_index(manifest)
    if relative_path:
        if relative_path not in candidates:
            raise ValueError(f"candidate not found in fixture manifest: {relative_path}")
        return candidates[relative_path]
    selected = [candidate for candidate in manifest.get("selected_candidates", []) if isinstance(candidate, dict)]
    if not selected:
        raise ValueError("fixture manifest has no selected candidates")
    first = selected[0]
    if not isinstance(first.get("relative_path"), str):
        raise ValueError("first selected candidate has no relative_path")
    return {**first, "_source_bucket": "selected_candidates"}


def validate_approval_candidate(candidate: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if candidate.get("_source_bucket") != "selected_candidates":
        errors.append("approval candidate must come from selected_candidates")
    if candidate.get("review_status") != "static_review_candidate":
        errors.append("approval candidate review_status must be static_review_candidate")
    if candidate.get("compatibility_class") != "classic_pf_effect_candidate":
        errors.append("approval candidate must be classic_pf_effect_candidate")
    if candidate.get("pipl_signal_present") is not True:
        errors.append("approval candidate must have PiPL signal")
    if candidate.get("effect_main_export_present") is not True:
        errors.append("approval candidate must export EffectMain")
    if int(candidate.get("aegp_marker_count") or 0) != 0:
        errors.append("approval candidate must not include AEGP markers")
    return errors


def compact_candidate(candidate: dict[str, Any]) -> dict[str, Any]:
    return {
        "relative_path": candidate.get("relative_path"),
        "file_name": candidate.get("file_name"),
        "size_bytes": candidate.get("size_bytes"),
        "compatibility_class": candidate.get("compatibility_class"),
        "fixture_candidate_score": candidate.get("fixture_candidate_score"),
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


def build_decision_manifest(
    source_manifest: dict[str, Any],
    source_manifest_path: Path,
    *,
    decision: str,
    candidate_relative_path: str | None = None,
    reason: str = "",
    explicit_user_approval: bool = False,
    approval_token: str | None = None,
) -> dict[str, Any]:
    errors = validate_review_manifest(source_manifest)
    if errors:
        raise ValueError("; ".join(errors))
    if decision not in {"hold", "reject", "approve"}:
        raise ValueError("decision must be hold, reject, or approve")

    candidate = select_candidate(source_manifest, candidate_relative_path)
    if decision == "approve":
        approval_errors = validate_approval_candidate(candidate)
        if approval_errors:
            raise ValueError("; ".join(approval_errors))
        if not explicit_user_approval:
            raise ValueError("approve requires --explicit-user-approval")
        if approval_token != APPROVAL_TOKEN:
            raise ValueError(f"approve requires --approval-token {APPROVAL_TOKEN}")

    base: dict[str, Any] = {
        "schema_version": 1,
        "publication_status": "local-only",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_fixture_review_manifest": str(source_manifest_path),
        "source_fixture_review_schema_version": source_manifest.get("schema_version"),
        "candidate": compact_candidate(candidate),
        "candidate_relative_path": candidate.get("relative_path"),
        "decision": decision,
        "reason": reason,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "notes": [
            "Decision is derived from fixture review manifest JSON only.",
            "No AEX file is opened, copied, hashed, loaded, or executed.",
        ],
    }
    if decision == "approve":
        base.update(
            {
                "manifest_kind": "aex_fixture_approval_manifest",
                "approval_state": "user_approved_for_load_gate",
                "explicit_user_approval": True,
                "approved_actions": ["prepare_native_load_gate"],
                "blocked_actions": [
                    "load_aex_dll",
                    "call_EffectMain",
                    "render_with_aex",
                    "route_through_ofx",
                ],
                "notes": base["notes"]
                + [
                    "Approval only permits preparing a separate loader gate; this tool still performs no load.",
                ],
            }
        )
    else:
        base.update(
            {
                "manifest_kind": "aex_fixture_decision_manifest",
                "approval_state": "not_approved_for_load_gate",
                "explicit_user_approval": False,
                "approved_actions": [],
                "decision_state": "hold_for_manual_review" if decision == "hold" else "rejected_for_load_gate",
                "blocked_actions": [
                    "copy_selected_aex_fixture",
                    "load_aex_dll",
                    "call_EffectMain",
                    "render_with_aex",
                    "route_through_ofx",
                ],
            }
        )
    return base


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Create no-load AEX fixture decision manifest")
    parser.add_argument("--fixture-manifest", required=True, help="Fixture review manifest under target/fixture-review")
    parser.add_argument("--candidate-relative-path", help="Candidate relative path; defaults to first selected candidate")
    parser.add_argument("--decision", choices=["hold", "reject", "approve"], required=True)
    parser.add_argument("--reason", default="")
    parser.add_argument("--explicit-user-approval", action="store_true")
    parser.add_argument("--approval-token")
    parser.add_argument("--out", required=True, help="Create-new decision JSON under target/fixture-approval")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    manifest, source_path = load_review_manifest(Path(args.fixture_manifest))
    decision = build_decision_manifest(
        manifest,
        source_path,
        decision=args.decision,
        candidate_relative_path=args.candidate_relative_path,
        reason=args.reason,
        explicit_user_approval=args.explicit_user_approval,
        approval_token=args.approval_token,
    )
    written = write_json_create_new(Path(args.out), decision)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

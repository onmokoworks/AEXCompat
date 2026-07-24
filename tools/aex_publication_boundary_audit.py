#!/usr/bin/env python3
"""Audit publication boundary for local AEX compatibility artifacts.

The audit reads the safety-chain audit JSON only. It reports whether artifacts
are suitable for publication and emits a redacted local summary.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
SAFETY_AUDIT_ROOT = LAB_ROOT / "target" / "safety-audit"
PUBLICATION_ROOT = LAB_ROOT / "target" / "publication-boundary"

SAFETY_FLAGS = (
    "native_load_performed",
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


def validate_safety_audit_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("safety audit must have .json extension")
    return resolve_under_root(path, SAFETY_AUDIT_ROOT, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("publication boundary report must have .json extension")
    PUBLICATION_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, PUBLICATION_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(PUBLICATION_ROOT.resolve(strict=True)):
        raise ValueError(f"publication boundary report parent must stay under {PUBLICATION_ROOT}")
    return resolved


def load_safety_audit(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_safety_audit_path(path)
    with resolved.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("safety audit must be a JSON object")
    return payload, resolved


def validate_safety_audit(audit: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if audit.get("report_kind") != "aex_no_load_safety_chain_audit":
        errors.append("safety audit report_kind must be aex_no_load_safety_chain_audit")
    if audit.get("publication_status") != "local-only":
        errors.append("safety audit publication_status must be local-only")
    if audit.get("audit_passed") is not True:
        errors.append("safety audit must have audit_passed=true")
    for flag in SAFETY_FLAGS:
        if audit.get(flag) is not False:
            errors.append(f"safety audit {flag} must be false")
    artifacts = audit.get("artifacts")
    if not isinstance(artifacts, list) or not artifacts:
        errors.append("safety audit artifacts must be a non-empty list")
    return errors


def path_redaction_count(audit: dict[str, Any]) -> int:
    count = 0
    for artifact in audit.get("artifacts", []):
        if isinstance(artifact, dict) and isinstance(artifact.get("path"), str):
            count += 1
    return count


def artifact_labels(audit: dict[str, Any]) -> list[str]:
    labels: list[str] = []
    for artifact in audit.get("artifacts", []):
        if isinstance(artifact, dict) and isinstance(artifact.get("label"), str):
            labels.append(artifact["label"])
    return labels


def redacted_summary(audit: dict[str, Any]) -> dict[str, Any]:
    return {
        "audit_state": audit.get("audit_state"),
        "artifact_count": audit.get("artifact_count"),
        "artifact_labels": artifact_labels(audit),
        "runtime_safety": {flag: audit.get(flag) for flag in SAFETY_FLAGS},
        "redacted_fields": [
            "absolute local paths",
            "candidate relative paths",
            "source input roots",
            "generated artifact filenames",
            "local timestamps",
        ],
        "non_claims": [
            "No public license/provenance approval is implied.",
            "No AEX compatibility or render compatibility is claimed.",
            "No OFX route availability is claimed.",
        ],
    }


def build_publication_report(audit: dict[str, Any], audit_path: Path) -> dict[str, Any]:
    evidence_errors = validate_safety_audit(audit)
    redaction_count = path_redaction_count(audit)
    publication_blockers = [
        "all source artifacts are local-only",
        "manual provenance/license review is not complete",
        "fixture decision is hold/review, not approval",
        "native loader and OFX route remain closed",
        "artifact chain contains local filesystem paths that require redaction",
    ]
    if evidence_errors:
        boundary_state = "invalid_evidence_not_publishable"
    else:
        boundary_state = "local_only_not_publishable"
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_publication_boundary_audit",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_safety_audit": str(audit_path),
        "source_audit_state": audit.get("audit_state"),
        "source_audit_passed": audit.get("audit_passed"),
        "boundary_state": boundary_state,
        "publishable_now": False,
        "public_summary_available": False,
        "evidence_errors": evidence_errors,
        "publication_blockers": publication_blockers if not evidence_errors else evidence_errors + publication_blockers,
        "redaction_required": True,
        "local_path_reference_count": redaction_count,
        "redacted_local_summary": redacted_summary(audit),
        "required_before_publication": [
            "complete manual provenance/license review for every named candidate",
            "remove or redact all local filesystem paths",
            "remove candidate names if publication scope does not allow them",
            "review cleanroom boundary for all derived notes",
            "keep binary payloads, hashes, and source-private metadata out of public artifacts",
            "write a separate publication-approved summary artifact",
        ],
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "notes": [
            "This report is local-only and is not itself publication approval.",
            "It reads safety-audit JSON only.",
            "It performs no AEX, AE, OFX, image runtime, or binary-payload operation.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Audit local-only AEX publication boundary")
    parser.add_argument("--safety-audit", required=True, help="Safety audit JSON under target/safety-audit")
    parser.add_argument("--out", required=True, help="Create-new report under target/publication-boundary")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    audit, audit_path = load_safety_audit(Path(args.safety_audit))
    report = build_publication_report(audit, audit_path)
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0 if not report["evidence_errors"] else 1


if __name__ == "__main__":
    raise SystemExit(main())

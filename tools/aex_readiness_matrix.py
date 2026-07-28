#!/usr/bin/env python3
"""Build a readiness matrix from the canonical AEX Compat Lab artifact index."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

try:
    from aex_readiness_matrix_core import (
        INDEX_ROOT,
        LAB_ROOT,
        MATRIX_ROOT,
        REQUIRED_ARTIFACT_LABELS,
        SAFETY_FLAGS,
        artifact_found,
        artifact_state,
        artifacts_by_label,
        evidence_errors,
        labels_found,
        load_artifact_index,
        path_has_traversal,
        read_json,
        requirement,
        resolve_under_root,
        safety_errors,
        validate_output_path,
        write_json_create_new,
    )
    from aex_readiness_matrix_requirements_a import build_requirement_group_a
    from aex_readiness_matrix_requirements_b import build_requirement_group_b
    from aex_readiness_matrix_states import evaluate_readiness_states
except ModuleNotFoundError:
    from tools.aex_readiness_matrix_core import (
        INDEX_ROOT,
        LAB_ROOT,
        MATRIX_ROOT,
        REQUIRED_ARTIFACT_LABELS,
        SAFETY_FLAGS,
        artifact_found,
        artifact_state,
        artifacts_by_label,
        evidence_errors,
        labels_found,
        load_artifact_index,
        path_has_traversal,
        read_json,
        requirement,
        resolve_under_root,
        safety_errors,
        validate_output_path,
        write_json_create_new,
    )
    from tools.aex_readiness_matrix_requirements_a import build_requirement_group_a
    from tools.aex_readiness_matrix_requirements_b import build_requirement_group_b
    from tools.aex_readiness_matrix_states import evaluate_readiness_states


def build_requirements(artifacts: dict[str, dict[str, Any]], clean_evidence: bool) -> list[dict[str, Any]]:
    state = evaluate_readiness_states(artifacts, clean_evidence)
    state["artifacts"] = artifacts
    state["clean_evidence"] = clean_evidence
    return build_requirement_group_a(state) + build_requirement_group_b(state)

def summarize_requirements(requirements: list[dict[str, Any]]) -> dict[str, int]:
    counts = {
        "satisfied_count": 0,
        "pending_count": 0,
        "intentionally_closed_count": 0,
        "failed_count": 0,
    }
    for item in requirements:
        status = item["status"]
        if status.startswith("satisfied"):
            counts["satisfied_count"] += 1
        elif status.startswith("pending"):
            counts["pending_count"] += 1
        elif status == "intentionally_closed":
            counts["intentionally_closed_count"] += 1
        elif status == "failed":
            counts["failed_count"] += 1
    return counts


def build_readiness_matrix(index: dict[str, Any], source_path: Path) -> dict[str, Any]:
    artifacts = artifacts_by_label(index)
    errors = evidence_errors(index, artifacts)
    clean_evidence = not errors
    requirements = build_requirements(artifacts, clean_evidence)
    counts = summarize_requirements(requirements)
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_readiness_matrix",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_artifact_index": str(source_path),
        "readiness_state": (
            "no_load_foundation_ready_pending_manual_approval"
            if clean_evidence
            else "evidence_invalid_no_load_foundation_not_ready"
        ),
        "overall_ready_for_no_load_tooling": clean_evidence,
        "overall_ready_for_native_load": False,
        "overall_ready_for_publication": False,
        "requirements": requirements,
        "summary": counts,
        "evidence_errors": errors,
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "aepx_file_modified": False,
        "aep_binary_modified": False,
        "ae_project_write_performed": False,
        "ofx_plugin_built": False,
        "ofx_describe_performed": False,
        "ofx_render_performed": False,
        "aex_render_performed": False,
        "render_validation_performed": False,
        "pipl_payload_parsed": False,
        "parameter_schema_emitted": False,
        "redacted_schema_emitted": False,
        "real_pipl_payload_parser_enabled": False,
        "real_pipl_payload_parsed": False,
        "resource_payload_opened": False,
        "raw_payload_serialized": False,
        "next_required_actions": [
            "Author a separate local-only user-answer artifact and run it through the provenance answer validator rules.",
            "Explicit user approval artifact before any native loader accepts an AEX path.",
            "Keep the native-loader broker pathless until fixture approval and an explicit path-acceptance approval exist.",
            "Review dependency availability preflight misses/default-deny rows before native loader work.",
            "Use the PiPL payload adapter review packet before any approved real-payload adapter implementation.",
            "Feed proposed redacted output through the verifier before schema emission.",
            "Review classified AEPX text inventory buckets before any project write tooling.",
            "Keep OFX work on the deferred/no-op path until native loader evidence exists.",
            "Create a redacted public summary only after publication review is complete.",
        ],
        "notes": [
            "Readiness matrix reads the artifact index JSON only.",
            "No AEX, AE, OFX, image runtime, or binary-payload operation is performed.",
            "Native load and publication readiness are intentionally false in this slice.",
        ],
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build local AEX Compat Lab readiness matrix")
    parser.add_argument("--artifact-index", required=True, help="Artifact index JSON under target/artifact-index")
    parser.add_argument("--out", required=True, help="Create-new readiness matrix under target/readiness-matrix")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    index, source_path = load_artifact_index(Path(args.artifact_index))
    matrix = build_readiness_matrix(index, source_path)
    written = write_json_create_new(Path(args.out), matrix)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

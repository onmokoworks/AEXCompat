#!/usr/bin/env python3
"""Create a no-write AEPX edit-plan packet from static probe metadata.

The packet is planning evidence only. It reads the AEPX static probe JSON and
does not modify AEPX/AEP files, start After Effects, load AEX, render, or route
OFX.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
AEPX_PROBE_ROOT = TARGET_ROOT / "aepx-static-probe"
AEPX_EDIT_PLAN_ROOT = TARGET_ROOT / "aepx-edit-plan"

SAFETY_FLAGS = (
    "native_load_enabled",
    "native_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
    "aex_file_opened",
    "aepx_file_modified",
    "aep_binary_modified",
    "ae_project_write_performed",
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


def validate_probe_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("AEPX probe must have .json extension")
    return resolve_under_root(path, AEPX_PROBE_ROOT, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("AEPX edit-plan output must have .json extension")
    AEPX_EDIT_PLAN_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, AEPX_EDIT_PLAN_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(AEPX_EDIT_PLAN_ROOT.resolve(strict=True)):
        raise ValueError(f"AEPX edit-plan parent must stay under {AEPX_EDIT_PLAN_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_probe(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_probe_path(path)
    return read_json_object(resolved), resolved


def validate_probe(probe: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if probe.get("report_kind") != "aepx_static_probe":
        errors.append("source report_kind must be aepx_static_probe")
    if probe.get("probe_state") != "aepx_static_probe_ready_no_write":
        errors.append("source probe_state must be aepx_static_probe_ready_no_write")
    if probe.get("xml_parse_state") != "parsed":
        errors.append("source xml_parse_state must be parsed")
    if probe.get("publication_status") != "local-only":
        errors.append("source publication_status must be local-only")
    if not isinstance(probe.get("summary"), dict):
        errors.append("source summary must be an object")
    if not isinstance(probe.get("root"), dict):
        errors.append("source root must be an object")
    if not isinstance(probe.get("top_tags"), list):
        errors.append("source top_tags must be a list")
    edit_surface = probe.get("edit_surface")
    if not isinstance(edit_surface, dict):
        errors.append("source edit_surface must be an object")
    else:
        if edit_surface.get("aepx_write_approved") is not False:
            errors.append("source edit_surface aepx_write_approved must be false")
        if edit_surface.get("text_payload_exported") is not False:
            errors.append("source edit_surface text_payload_exported must be false")
    for flag in SAFETY_FLAGS:
        if probe.get(flag) is not False:
            errors.append(f"source {flag} must be false")
    return errors


def tag_count(probe: dict[str, Any], tag: str, key: str = "count") -> int:
    for row in probe.get("top_tags", []):
        if isinstance(row, dict) and row.get("tag") == tag:
            return int(row.get(key) or 0)
    return 0


def edit_surfaces(probe: dict[str, Any]) -> list[dict[str, Any]]:
    summary = probe.get("summary", {})
    root = probe.get("root", {})
    string_count = tag_count(probe, "string")
    cdat_count = tag_count(probe, "cdat")
    bdata_count = int(summary.get("bdata_attribute_count") or 0)
    text_count = int(summary.get("nonempty_text_node_count") or 0)
    return [
        {
            "surface_id": "root_project_version_metadata",
            "surface_kind": "xml_root_metadata",
            "current_evidence": {
                "root_tag": root.get("tag"),
                "namespace": root.get("namespace"),
                "attributes": root.get("attributes", {}),
            },
            "edit_candidate_state": "read_only_schema_anchor",
            "write_risk": "medium",
            "blocked_until": [
                "schema review identifies stable version fields",
                "round-trip validator proves unchanged project semantics",
            ],
        },
        {
            "surface_id": "string_text_nodes",
            "surface_kind": "xml_text_payload",
            "current_evidence": {
                "top_tag_string_count": string_count,
                "nonempty_text_node_count": text_count,
                "text_payload_values_exported": False,
            },
            "edit_candidate_state": "potential_mapping_only_no_payload_export",
            "write_risk": "high",
            "blocked_until": [
                "redacted text-node inventory is approved",
                "schema identifies which strings are user-editable labels versus internal IDs",
                "round-trip validator exists",
            ],
        },
        {
            "surface_id": "bdata_binary_attributes",
            "surface_kind": "binary_hex_payload",
            "current_evidence": {
                "bdata_attribute_count": bdata_count,
                "bdata_total_decoded_bytes_if_hex": summary.get("bdata_total_decoded_bytes_if_hex"),
                "bdata_invalid_hex_count": summary.get("bdata_invalid_hex_count"),
            },
            "edit_candidate_state": "do_not_edit_binary_payloads",
            "write_risk": "critical",
            "blocked_until": [
                "binary field schema is known",
                "byte-level fixture and AE round-trip tests exist",
                "explicit user approval for project-write experiments exists",
            ],
        },
        {
            "surface_id": "structural_xml_tree",
            "surface_kind": "xml_structure",
            "current_evidence": {
                "element_count": summary.get("element_count"),
                "unique_tag_count": summary.get("unique_tag_count"),
                "max_depth": summary.get("max_depth"),
                "cdat_top_tag_count": cdat_count,
            },
            "edit_candidate_state": "read_only_mapping_required",
            "write_risk": "critical",
            "blocked_until": [
                "schema-aware element model exists",
                "no-write diff/round-trip plan exists",
                "AE invocation policy is reviewed separately",
            ],
        },
    ]


def review_blockers(probe: dict[str, Any]) -> list[dict[str, Any]]:
    summary = probe.get("summary", {})
    blockers = [
        {
            "blocker_id": "project_write_not_approved",
            "severity": "blocker",
            "reason": "AEPX/AEP write actions are not approved in the current goal slice.",
        },
        {
            "blocker_id": "round_trip_validator_missing",
            "severity": "blocker",
            "reason": "No tool exists yet to prove an edited AEPX can round-trip safely.",
        },
        {
            "blocker_id": "ae_host_validation_closed",
            "severity": "blocker",
            "reason": "After Effects launch and project render validation remain closed.",
        },
        {
            "blocker_id": "binary_bdata_schema_unknown",
            "severity": "blocker",
            "reason": "bdata attributes are binary-like payloads and must not be edited without schema knowledge.",
            "count": summary.get("bdata_attribute_count"),
        },
        {
            "blocker_id": "text_payloads_not_exported",
            "severity": "review",
            "reason": "Text node values were intentionally not exported; a redacted inventory is needed before text edits.",
            "count": summary.get("nonempty_text_node_count"),
        },
    ]
    if int(summary.get("bdata_invalid_hex_count") or 0) > 0:
        blockers.append(
            {
                "blocker_id": "invalid_bdata_hex_present",
                "severity": "blocker",
                "reason": "Some bdata attributes were not valid even-length hex in the static probe.",
                "count": summary.get("bdata_invalid_hex_count"),
            }
        )
    return blockers


def build_edit_plan_packet(probe: dict[str, Any], source_path: Path) -> dict[str, Any]:
    errors = validate_probe(probe)
    if errors:
        raise ValueError("; ".join(errors))
    blockers = review_blockers(probe)
    surfaces = edit_surfaces(probe)
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aepx_edit_plan_packet",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_aepx_probe": str(source_path),
        "source_probe_state": probe.get("probe_state"),
        "source_xml_parse_state": probe.get("xml_parse_state"),
        "edit_plan_state": "aepx_edit_plan_ready_no_write",
        "write_recommendation": "do_not_write_project_files",
        "edit_surfaces": surfaces,
        "review_blockers": blockers,
        "summary": {
            "surface_count": len(surfaces),
            "blocker_count": sum(1 for blocker in blockers if blocker.get("severity") == "blocker"),
            "review_count": sum(1 for blocker in blockers if blocker.get("severity") == "review"),
            "source_element_count": probe.get("summary", {}).get("element_count"),
            "source_unique_tag_count": probe.get("summary", {}).get("unique_tag_count"),
            "source_bdata_attribute_count": probe.get("summary", {}).get("bdata_attribute_count"),
            "source_nonempty_text_node_count": probe.get("summary", {}).get("nonempty_text_node_count"),
        },
        "allowed_current_actions": [
            "read-only schema mapping",
            "redacted text-node inventory planning",
            "synthetic AEPX fixture generation for parser tests",
            "round-trip validator design",
        ],
        "blocked_actions": [
            "modify_aepx",
            "write_aep",
            "start_after_effects",
            "load_aex",
            "render_project",
            "claim_project_edit_compatibility",
        ],
        "native_load_enabled": False,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "aepx_file_modified": False,
        "aep_binary_modified": False,
        "ae_project_write_performed": False,
        "notes": [
            "Edit plan reads AEPX static probe JSON only.",
            "No text payload values, AEPX writes, AEP writes, AE invocation, AEX load, or render action is performed.",
            "The packet is a planning artifact, not approval to edit project files.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build local no-write AEPX edit-plan packet")
    parser.add_argument("--aepx-probe", required=True, help="AEPX static probe JSON under target/aepx-static-probe")
    parser.add_argument("--out", required=True, help="Create-new edit plan JSON under target/aepx-edit-plan")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    probe, source_path = load_probe(Path(args.aepx_probe))
    packet = build_edit_plan_packet(probe, source_path)
    written = write_json_create_new(Path(args.out), packet)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

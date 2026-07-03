#!/usr/bin/env python3
"""Build a no-payload review packet for AEX parameter schema work.

The packet reads the parameter schema plan, publication boundary, and closed
OFX route contract JSON only. It does not parse PiPL payloads, emit real or
redacted parameter schemas, invoke OFX describe, open AEX files, or render.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
SCHEMA_PLAN_ROOT = TARGET_ROOT / "parameter-schema-plan"
PUBLICATION_BOUNDARY_ROOT = TARGET_ROOT / "publication-boundary"
OFX_ROUTE_CONTRACT_ROOT = TARGET_ROOT / "ofx-route-contract"
REVIEW_ROOT = TARGET_ROOT / "parameter-schema-review"

SAFETY_FLAGS = (
    "native_load_enabled",
    "native_load_performed",
    "dll_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
    "aex_file_opened",
    "aepx_file_modified",
    "aep_binary_modified",
    "ae_project_write_performed",
    "ofx_plugin_built",
    "ofx_describe_performed",
    "ofx_render_performed",
    "aex_render_performed",
    "render_validation_performed",
    "pipl_payload_parsed",
    "parameter_schema_emitted",
    "redacted_schema_emitted",
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
        raise ValueError("parameter schema review packet must have .json extension")
    REVIEW_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, REVIEW_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(REVIEW_ROOT.resolve(strict=True)):
        raise ValueError(f"parameter schema review packet parent must stay under {REVIEW_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_schema_plan(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, SCHEMA_PLAN_ROOT, "parameter schema plan")
    return read_json_object(resolved), resolved


def load_publication_boundary(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, PUBLICATION_BOUNDARY_ROOT, "publication boundary")
    return read_json_object(resolved), resolved


def load_ofx_route_contract(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, OFX_ROUTE_CONTRACT_ROOT, "OFX route contract")
    return read_json_object(resolved), resolved


def require_local_only(payload: dict[str, Any], label: str) -> list[str]:
    if payload.get("publication_status") != "local-only":
        return [f"{label} publication_status must be local-only"]
    return []


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def validate_schema_plan(plan: dict[str, Any]) -> list[str]:
    errors = require_local_only(plan, "parameter schema plan")
    if plan.get("report_kind") != "aex_parameter_schema_plan":
        errors.append("parameter schema plan report_kind must be aex_parameter_schema_plan")
    if plan.get("plan_state") != "parameter_schema_plan_ready_no_payload":
        errors.append("parameter schema plan plan_state must be parameter_schema_plan_ready_no_payload")
    if plan.get("schema_plan_ready") is not True:
        errors.append("parameter schema plan schema_plan_ready must be true")
    if plan.get("real_parameter_schema_available") is not False:
        errors.append("parameter schema plan real_parameter_schema_available must be false")
    if plan.get("payload_parser_enabled") is not False:
        errors.append("parameter schema plan payload_parser_enabled must be false")
    blockers = plan.get("blockers")
    if not isinstance(blockers, list):
        errors.append("parameter schema plan blockers must be a list")
    else:
        for blocker in ("pipl_payload_parser_disabled", "no_redacted_public_schema", "no_ofx_describe_mapping"):
            if blocker not in blockers:
                errors.append(f"parameter schema plan must record {blocker}")
    rows = plan.get("candidate_schema_rows")
    if not isinstance(rows, list) or not rows:
        errors.append("parameter schema plan candidate_schema_rows must be a non-empty list")
    errors.extend(safety_errors(plan, "parameter schema plan"))
    return errors


def validate_publication_boundary(boundary: dict[str, Any]) -> list[str]:
    errors = require_local_only(boundary, "publication boundary")
    if boundary.get("report_kind") != "aex_publication_boundary_audit":
        errors.append("publication boundary report_kind must be aex_publication_boundary_audit")
    if boundary.get("publishable_now") is not False:
        errors.append("publication boundary publishable_now must be false")
    blockers = boundary.get("publication_blockers")
    if not isinstance(blockers, list) or not blockers:
        errors.append("publication boundary publication_blockers must be a non-empty list")
    errors.extend(safety_errors(boundary, "publication boundary"))
    return errors


def validate_ofx_route_contract(contract: dict[str, Any]) -> list[str]:
    errors = require_local_only(contract, "OFX route contract")
    if contract.get("report_kind") != "aex_ofx_route_contract_probe":
        errors.append("OFX route contract report_kind must be aex_ofx_route_contract_probe")
    if contract.get("contract_state") != "ofx_route_contract_ready_route_closed":
        errors.append("OFX route contract contract_state must be ofx_route_contract_ready_route_closed")
    if contract.get("real_route_open") is not False:
        errors.append("OFX route contract real_route_open must be false")
    if contract.get("mock_route_ready") is not True:
        errors.append("OFX route contract mock_route_ready must be true")
    errors.extend(safety_errors(contract, "OFX route contract"))
    return errors


def int_or_zero(value: Any) -> int:
    if isinstance(value, bool):
        return 0
    if isinstance(value, int):
        return value
    return 0


def mapping_state_counts(rows: list[dict[str, Any]]) -> dict[str, int]:
    counts: dict[str, int] = {}
    for row in rows:
        state = str(row.get("mapping_state"))
        counts[state] = counts.get(state, 0) + 1
    return dict(sorted(counts.items()))


def parser_action_for(mapping_state: str) -> str:
    if mapping_state == "primary_schema_mapping_candidate_pending_payload_parser":
        return "first_parser_contract_candidate_after_approval"
    if mapping_state == "schema_mapping_candidate_pending_payload_parser":
        return "secondary_parser_contract_candidate"
    if mapping_state == "host_contract_review_before_schema_mapping":
        return "hold_until_host_contract_review"
    return "hold_until_static_metadata_gap_resolved"


def candidate_review_rows(plan_rows: Any) -> list[dict[str, Any]]:
    if not isinstance(plan_rows, list):
        return []
    rows: list[dict[str, Any]] = []
    for row in plan_rows:
        if not isinstance(row, dict):
            continue
        mapping_state = str(row.get("mapping_state"))
        rows.append(
            {
                "relative_path": row.get("relative_path"),
                "review_bucket": row.get("review_bucket"),
                "mapping_state": mapping_state,
                "fixture_candidate_score": row.get("fixture_candidate_score"),
                "parser_action": parser_action_for(mapping_state),
                "redaction_action": "do_not_emit_public_schema_values",
                "ofx_describe_action": "defer_until_redacted_schema_and_gate_review",
                "payload_policy": "metadata_anchor_only_no_pipl_payload",
            }
        )
    rows.sort(
        key=lambda item: (
            item["parser_action"] != "first_parser_contract_candidate_after_approval",
            -int_or_zero(item.get("fixture_candidate_score")),
            str(item.get("relative_path")),
        )
    )
    return rows


def build_parser_review_items(summary: dict[str, Any], rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [
        {
            "item_id": "pipl_payload_reader_bounds_checks",
            "severity": "blocker_before_parser_enablement",
            "current_state": "not_implemented",
            "required_evidence": "bounded parser tests over synthetic/minimal payload fixtures",
        },
        {
            "item_id": "private_payload_copy_prevention",
            "severity": "blocker_before_schema_output",
            "current_state": "policy_defined_no_payload_copy",
            "required_evidence": "tests proving parser reports metadata/schema only and never serializes raw payload bytes",
        },
        {
            "item_id": "candidate_scope_review",
            "severity": "review_before_parser_enablement",
            "current_state": "candidate_scope_from_no_payload_plan",
            "candidate_count": len(rows),
            "primary_candidate_count": summary.get("primary_mapping_candidate_count"),
        },
        {
            "item_id": "pf_vs_aegp_host_contract_split",
            "severity": "review_before_ofx_mapping",
            "current_state": "host_contract_rows_held",
            "host_contract_review_count": summary.get("host_contract_review_count"),
        },
        {
            "item_id": "redacted_schema_round_trip_contract",
            "severity": "blocker_before_publication_or_ofx_describe",
            "current_state": "not_implemented",
            "required_evidence": "schema verifier for allowed public fields and forbidden value/payload fields",
        },
    ]


def build_redaction_policy() -> dict[str, Any]:
    return {
        "state": "redaction_policy_ready_no_schema_output",
        "public_schema_output_state": "not_emitted",
        "allowed_after_review": [
            "schema_version",
            "compatibility_class",
            "mapping_state_counts",
            "parameter_count_if_non_identifying",
            "parameter_type_categories_if_non_identifying",
            "range_presence_flags_if_non_identifying",
        ],
        "forbidden_without_additional_approval": [
            "raw_pipl_payload_bytes",
            "private_resource_payload",
            "unredacted_parameter_names",
            "unredacted_default_values",
            "unredacted_value_ranges",
            "binary_hashes",
            "absolute_source_paths",
        ],
        "current_packet_output": "local_only_review_metadata",
    }


def build_ofx_describe_policy() -> dict[str, Any]:
    return {
        "state": "ofx_describe_mapping_deferred_until_redacted_schema",
        "mapping_ready": False,
        "required_evidence_before_mapping": [
            "reviewed payload parser with no raw payload copy",
            "redacted parameter schema verifier",
            "explicit fixture approval",
            "native load gate review if describe depends on runtime data",
            "closed-route OFX contract update",
        ],
        "blocked_actions": [
            "build_ofx_describe_from_aex",
            "call_ofx_describe",
            "route_aex_parameters_through_ofx",
            "emit_unredacted_ofx_parameters",
        ],
    }


def build_review_packet(
    *,
    schema_plan: dict[str, Any],
    schema_plan_path: Path,
    publication_boundary: dict[str, Any],
    publication_boundary_path: Path,
    ofx_route_contract: dict[str, Any],
    ofx_route_contract_path: Path,
) -> dict[str, Any]:
    errors = (
        validate_schema_plan(schema_plan)
        + validate_publication_boundary(publication_boundary)
        + validate_ofx_route_contract(ofx_route_contract)
    )
    if errors:
        raise ValueError("; ".join(errors))

    plan_summary = schema_plan.get("summary", {}) if isinstance(schema_plan.get("summary"), dict) else {}
    rows = candidate_review_rows(schema_plan.get("candidate_schema_rows"))
    state_counts = mapping_state_counts(rows)
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aex_parameter_schema_review_packet",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_parameter_schema_plan": str(schema_plan_path),
        "source_publication_boundary": str(publication_boundary_path),
        "source_ofx_route_contract": str(ofx_route_contract_path),
        "review_state": "parameter_schema_review_ready_no_payload",
        "parser_design_state": "payload_parser_design_review_ready_parser_disabled",
        "redaction_policy_state": "redaction_policy_ready_no_schema_output",
        "ofx_describe_policy_state": "ofx_describe_mapping_deferred_until_redacted_schema",
        "payload_parser_enabled": False,
        "real_parameter_schema_available": False,
        "redacted_schema_available": False,
        "ofx_describe_mapping_ready": False,
        "parser_review_items": build_parser_review_items(plan_summary, rows),
        "redaction_policy": build_redaction_policy(),
        "ofx_describe_policy": build_ofx_describe_policy(),
        "candidate_review_rows": rows,
        "summary": {
            "candidate_count": len(rows),
            "primary_mapping_candidate_count": plan_summary.get("primary_mapping_candidate_count"),
            "payload_parser_required_count": plan_summary.get("payload_parser_required_count"),
            "host_contract_review_count": plan_summary.get("host_contract_review_count"),
            "mapping_state_counts": state_counts,
            "publication_boundary_publishable_now": publication_boundary.get("publishable_now"),
            "ofx_route_contract_state": ofx_route_contract.get("contract_state"),
        },
        "schema_plan_link": {
            "source_plan_state": schema_plan.get("plan_state"),
            "source_payload_policy": schema_plan.get("payload_policy"),
            "this_packet_enables_payload_parser": False,
            "this_packet_emits_redacted_schema": False,
            "this_packet_enables_ofx_describe": False,
            "this_packet_reduces_blocker_to": "needs_reviewed_parser_implementation_and_redacted_schema_verifier",
        },
        "blockers": [
            "payload_parser_not_implemented",
            "redacted_schema_not_emitted",
            "ofx_describe_mapping_deferred",
            "fixture_not_approved",
            "load_gate_closed",
            "real_render_closed",
        ],
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
        "notes": [
            "Packet reads local JSON artifacts only.",
            "No PiPL payload parser is enabled.",
            "No real or redacted parameter schema is emitted.",
            "No OFX describe, AEX load, AE invocation, or render action is performed.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build no-payload AEX parameter schema review packet")
    parser.add_argument("--schema-plan", required=True, help="Parameter schema plan JSON under target/parameter-schema-plan")
    parser.add_argument("--publication-boundary", required=True, help="Publication boundary JSON under target/publication-boundary")
    parser.add_argument("--ofx-route-contract", required=True, help="OFX route contract JSON under target/ofx-route-contract")
    parser.add_argument("--out", required=True, help="Create-new review packet JSON under target/parameter-schema-review")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    schema_plan, schema_plan_path = load_schema_plan(Path(args.schema_plan))
    publication_boundary, publication_boundary_path = load_publication_boundary(Path(args.publication_boundary))
    ofx_route_contract, ofx_route_contract_path = load_ofx_route_contract(Path(args.ofx_route_contract))
    packet = build_review_packet(
        schema_plan=schema_plan,
        schema_plan_path=schema_plan_path,
        publication_boundary=publication_boundary,
        publication_boundary_path=publication_boundary_path,
        ofx_route_contract=ofx_route_contract,
        ofx_route_contract_path=ofx_route_contract_path,
    )
    written = write_json_create_new(Path(args.out), packet)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

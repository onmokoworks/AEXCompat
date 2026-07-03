#!/usr/bin/env python3
"""Prepare a real PiPL payload adapter review packet without payload access.

The packet reads JSON evidence only: the closed PiPL parser gate, the synthetic
payload parser report, and the static/catalog/gate consistency audit. It does
not accept AEX paths, open resources, parse real PiPL bytes, emit schemas, load
native code, start AE, invoke OFX, or render.
"""

from __future__ import annotations

import argparse
import json
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
PIPL_GATE_ROOT = TARGET_ROOT / "pipl-parser-gate"
SYNTHETIC_PAYLOAD_PARSER_ROOT = TARGET_ROOT / "synthetic-pipl-payload-parser"
PIPL_CONSISTENCY_AUDIT_ROOT = TARGET_ROOT / "pipl-resource-consistency-audit"
PARAMETER_SCHEMA_REVIEW_ROOT = TARGET_ROOT / "parameter-schema-review"
ADAPTER_REVIEW_ROOT = TARGET_ROOT / "pipl-payload-adapter-review"

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
    "real_pipl_payload_parser_enabled",
    "real_pipl_payload_parsed",
    "resource_payload_opened",
    "resource_payload_extracted",
    "raw_payload_serialized",
)

FORBIDDEN_CLI_INPUTS = (
    "--aex",
    "--aex-path",
    "--pipl",
    "--pipl-payload",
    "--resource",
    "--resource-payload",
    "--payload",
    "--payload-file",
    "--raw-payload",
    "--bytes",
    "--load",
    "--render",
    "--ofx",
    "--approve",
    "APPROVE_AEX_LOAD_GATE",
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
        raise ValueError("PiPL payload adapter review packet must have .json extension")
    ADAPTER_REVIEW_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, ADAPTER_REVIEW_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(ADAPTER_REVIEW_ROOT.resolve(strict=True)):
        raise ValueError(f"PiPL payload adapter review parent must stay under {ADAPTER_REVIEW_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_pipl_parser_gate(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, PIPL_GATE_ROOT, "PiPL parser gate")
    return read_json_object(resolved), resolved


def load_synthetic_payload_parser(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, SYNTHETIC_PAYLOAD_PARSER_ROOT, "synthetic PiPL payload parser")
    return read_json_object(resolved), resolved


def load_consistency_audit(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, PIPL_CONSISTENCY_AUDIT_ROOT, "PiPL consistency audit")
    return read_json_object(resolved), resolved


def load_parameter_schema_review(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, PARAMETER_SCHEMA_REVIEW_ROOT, "parameter schema review")
    return read_json_object(resolved), resolved


def reject_forbidden_cli_inputs(argv: list[str]) -> None:
    forbidden = set(FORBIDDEN_CLI_INPUTS)
    for raw_arg in argv:
        token = raw_arg.split("=", 1)[0]
        if token in forbidden or raw_arg in forbidden:
            raise ValueError(f"forbidden CLI input for PiPL adapter review: {raw_arg}")


def source_path_matches(value: Any, expected: Path) -> bool:
    return isinstance(value, str) and Path(value).resolve(strict=False) == expected.resolve(strict=False)


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def validate_pipl_parser_gate(report: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if report.get("publication_status") != "local-only":
        errors.append("PiPL parser gate publication_status must be local-only")
    if report.get("report_kind") != "aex_pipl_parser_gate":
        errors.append("PiPL parser gate report_kind must be aex_pipl_parser_gate")
    if report.get("gate_state") != "pipl_parser_gate_closed_no_real_payload":
        errors.append("PiPL parser gate must stay closed to real payloads")
    if report.get("gate_ready_for_review") is not True:
        errors.append("PiPL parser gate gate_ready_for_review must be true")
    if report.get("metadata_budget_ready") is not True:
        errors.append("PiPL parser gate metadata_budget_ready must be true")
    if report.get("real_pipl_payload_parser_enabled") is not False:
        errors.append("PiPL parser gate real_pipl_payload_parser_enabled must be false")
    if report.get("real_pipl_payload_parsed") is not False:
        errors.append("PiPL parser gate real_pipl_payload_parsed must be false")
    if report.get("resource_payload_opened") is not False:
        errors.append("PiPL parser gate resource_payload_opened must be false")
    if report.get("raw_payload_serialized") is not False:
        errors.append("PiPL parser gate raw_payload_serialized must be false")
    contract = report.get("parser_input_contract")
    if not isinstance(contract, dict):
        errors.append("PiPL parser gate parser_input_contract must be an object")
    elif contract.get("real_payload_input_allowed_now") is not False:
        errors.append("PiPL parser gate real payload input must stay closed")
    checks = report.get("parser_gate_checks")
    if not isinstance(checks, list) or not checks:
        errors.append("PiPL parser gate parser_gate_checks must be a non-empty list")
    elif not all(isinstance(check, dict) and check.get("passed") is True for check in checks):
        errors.append("PiPL parser gate parser_gate_checks must all pass")
    rows = report.get("candidate_budget_rows")
    if not isinstance(rows, list) or not rows:
        errors.append("PiPL parser gate candidate_budget_rows must be a non-empty list")
    errors.extend(safety_errors(report, "PiPL parser gate"))
    return errors


def validate_synthetic_payload_parser(report: dict[str, Any], gate_path: Path) -> list[str]:
    errors: list[str] = []
    if report.get("publication_status") != "local-only":
        errors.append("synthetic PiPL payload parser publication_status must be local-only")
    if report.get("report_kind") != "aex_synthetic_pipl_payload_parser":
        errors.append("synthetic PiPL payload parser report_kind must be aex_synthetic_pipl_payload_parser")
    if report.get("synthetic_payload_parser_state") != "synthetic_pipl_payload_parser_ready_real_payload_closed":
        errors.append("synthetic PiPL payload parser state must be ready with real payload closed")
    if report.get("synthetic_payload_parser_ready") is not True:
        errors.append("synthetic PiPL payload parser ready must be true")
    if report.get("synthetic_parser_implemented") is not True:
        errors.append("synthetic PiPL payload parser synthetic_parser_implemented must be true")
    if report.get("synthetic_bounds_harness_reused") is not True:
        errors.append("synthetic PiPL payload parser synthetic_bounds_harness_reused must be true")
    if report.get("synthetic_payload_cases_passed") is not True:
        errors.append("synthetic PiPL payload parser cases must pass")
    if report.get("synthetic_payloads_serialized") is not False:
        errors.append("synthetic PiPL payload parser synthetic_payloads_serialized must be false")
    if report.get("real_payload_input_allowed_now") is not False:
        errors.append("synthetic PiPL payload parser real_payload_input_allowed_now must be false")
    if report.get("output_metadata_only") is not True:
        errors.append("synthetic PiPL payload parser output_metadata_only must be true")
    if report.get("real_pipl_payload_parser_enabled") is not False:
        errors.append("synthetic PiPL payload parser real parser must be disabled")
    if report.get("real_pipl_payload_parsed") is not False:
        errors.append("synthetic PiPL payload parser real payload parsed must be false")
    if report.get("resource_payload_opened") is not False:
        errors.append("synthetic PiPL payload parser resource_payload_opened must be false")
    if report.get("raw_payload_serialized") is not False:
        errors.append("synthetic PiPL payload parser raw_payload_serialized must be false")
    if report.get("parser_case_count", 0) <= 0:
        errors.append("synthetic PiPL payload parser parser_case_count must be positive")
    if report.get("parser_case_passed_count", 0) <= 0:
        errors.append("synthetic PiPL payload parser parser_case_passed_count must be positive")
    if report.get("parser_case_failed_count") != 0:
        errors.append("synthetic PiPL payload parser parser_case_failed_count must be 0")
    contract = report.get("parser_api_contract")
    if not isinstance(contract, dict):
        errors.append("synthetic PiPL payload parser parser_api_contract must be an object")
    else:
        if contract.get("real_pipl_payload_input_allowed") is not False:
            errors.append("synthetic PiPL payload parser contract must not allow real PiPL input")
        if contract.get("resource_payload_file_input_allowed") is not False:
            errors.append("synthetic PiPL payload parser contract must not allow resource payload files")
        if contract.get("aex_path_input_allowed") is not False:
            errors.append("synthetic PiPL payload parser contract must not allow AEX paths")
    if not source_path_matches(report.get("source_pipl_parser_gate"), gate_path):
        errors.append("synthetic PiPL payload parser source_pipl_parser_gate must match gate input")
    errors.extend(safety_errors(report, "synthetic PiPL payload parser"))
    return errors


def validate_consistency_audit(report: dict[str, Any], gate_path: Path) -> list[str]:
    errors: list[str] = []
    if report.get("publication_status") != "local-only":
        errors.append("PiPL consistency audit publication_status must be local-only")
    if report.get("report_kind") != "aex_pipl_resource_consistency_audit":
        errors.append("PiPL consistency audit report_kind must be aex_pipl_resource_consistency_audit")
    if report.get("audit_state") != "pipl_resource_consistency_audit_passed_no_payload":
        errors.append("PiPL consistency audit must have passed no-payload state")
    if report.get("audit_passed") is not True:
        errors.append("PiPL consistency audit audit_passed must be true")
    if report.get("source_chain_valid") is not True:
        errors.append("PiPL consistency audit source_chain_valid must be true")
    for field in (
        "catalog_summary_recomputed",
        "catalog_rows_recomputed",
        "gate_budget_rows_recomputed",
        "gate_summary_recomputed",
        "metadata_consistency_ready",
    ):
        if report.get(field) is not True:
            errors.append(f"PiPL consistency audit {field} must be true")
    for field in (
        "real_payload_input_allowed_now",
        "real_pipl_payload_parser_enabled",
        "real_pipl_payload_parsed",
        "resource_payload_opened",
        "resource_payload_extracted",
        "raw_payload_serialized",
    ):
        if report.get(field) is not False:
            errors.append(f"PiPL consistency audit {field} must be false")
    checks = report.get("checks")
    if not isinstance(checks, list) or not checks:
        errors.append("PiPL consistency audit checks must be a non-empty list")
    elif not all(isinstance(check, dict) and check.get("passed") is True for check in checks):
        errors.append("PiPL consistency audit checks must all pass")
    if not source_path_matches(report.get("source_pipl_parser_gate"), gate_path):
        errors.append("PiPL consistency audit source_pipl_parser_gate must match gate input")
    errors.extend(safety_errors(report, "PiPL consistency audit"))
    return errors


def validate_parameter_schema_review(report: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if report.get("publication_status") != "local-only":
        errors.append("parameter schema review publication_status must be local-only")
    if report.get("packet_kind") != "aex_parameter_schema_review_packet":
        errors.append("parameter schema review packet_kind must be aex_parameter_schema_review_packet")
    if report.get("review_state") != "parameter_schema_review_ready_no_payload":
        errors.append("parameter schema review review_state must be ready no-payload")
    if report.get("parser_design_state") != "payload_parser_design_review_ready_parser_disabled":
        errors.append("parameter schema review parser_design_state must keep parser disabled")
    if report.get("redaction_policy_state") != "redaction_policy_ready_no_schema_output":
        errors.append("parameter schema review redaction_policy_state must keep schema output closed")
    if report.get("ofx_describe_policy_state") != "ofx_describe_mapping_deferred_until_redacted_schema":
        errors.append("parameter schema review ofx_describe_policy_state must defer OFX describe mapping")
    if report.get("payload_parser_enabled") is not False:
        errors.append("parameter schema review payload_parser_enabled must be false")
    if report.get("real_parameter_schema_available") is not False:
        errors.append("parameter schema review real_parameter_schema_available must be false")
    if report.get("redacted_schema_available") is not False:
        errors.append("parameter schema review redacted_schema_available must be false")
    if report.get("ofx_describe_mapping_ready") is not False:
        errors.append("parameter schema review ofx_describe_mapping_ready must be false")
    for field in ("parser_review_items", "candidate_review_rows", "blockers"):
        if not isinstance(report.get(field), list):
            errors.append(f"parameter schema review {field} must be a list")
    if not isinstance(report.get("redaction_policy"), dict):
        errors.append("parameter schema review redaction_policy must be an object")
    if not isinstance(report.get("ofx_describe_policy"), dict):
        errors.append("parameter schema review ofx_describe_policy must be an object")
    errors.extend(safety_errors(report, "parameter schema review"))
    return errors


def int_or_zero(value: Any) -> int:
    if isinstance(value, bool):
        return 0
    if isinstance(value, int):
        return value
    return 0


def review_item(item_id: str, title: str, status: str, evidence: list[str], requirement: str) -> dict[str, Any]:
    return {
        "item_id": item_id,
        "title": title,
        "status": status,
        "evidence": evidence,
        "requirement": requirement,
        "real_payload_access_allowed_now": False,
        "blocks_real_adapter_enablement": True,
    }


def build_review_items(
    gate: dict[str, Any],
    synthetic_payload_parser: dict[str, Any],
    consistency_audit: dict[str, Any],
    parameter_schema_review: dict[str, Any],
) -> list[dict[str, Any]]:
    return [
        review_item(
            "source_chain_consistency",
            "Static/catalog/gate consistency is verified",
            "satisfied_for_review",
            ["pipl_resource_consistency_audit"],
            "Keep catalog and gate rows reproducible from metadata before any payload adapter work.",
        ),
        review_item(
            "synthetic_parser_contract",
            "Synthetic parser contract is ready",
            "satisfied_for_review",
            ["synthetic_pipl_payload_parser"],
            "Future adapter must reuse case coverage and metadata-only output policy before accepting real bytes.",
        ),
        review_item(
            "metadata_budget_limit",
            "Metadata-derived size budget is available",
            "satisfied_for_review",
            ["pipl_parser_gate"],
            "Future adapter must enforce the proposed parser byte limit and reject oversized payloads.",
        ),
        review_item(
            "real_payload_input_boundary",
            "Real payload input remains closed",
            "blocked_until_approval",
            ["pipl_parser_gate", "synthetic_pipl_payload_parser", "pipl_resource_consistency_audit"],
            "Real payload bytes require explicit approval, approved fixture scope, and worker containment.",
        ),
        review_item(
            "output_redaction_boundary",
            "Raw payload and schema outputs remain closed",
            "blocked_until_schema_review",
            ["synthetic_pipl_payload_parser", "parameter_schema_review"],
            "Future adapter output must remain metadata-only until redacted schema review and verifier gates pass.",
        ),
        review_item(
            "parameter_schema_policy_boundary",
            "Parameter schema and OFX describe policies remain deferred",
            "blocked_until_redacted_schema_verifier",
            ["parameter_schema_review"],
            "Real payload adapter work cannot imply parameter schema emission or OFX describe mapping.",
        ),
        review_item(
            "native_loader_boundary",
            "Native loader and AEX path acceptance remain closed",
            "blocked_until_manual_fixture_approval",
            ["pipl_parser_gate"],
            "Payload adapter review cannot imply DLL load, AEX path acceptance, AE startup, OFX, or render approval.",
        ),
    ]


def build_adapter_review_packet(
    *,
    pipl_parser_gate: dict[str, Any],
    pipl_parser_gate_path: Path,
    synthetic_payload_parser: dict[str, Any],
    synthetic_payload_parser_path: Path,
    consistency_audit: dict[str, Any],
    consistency_audit_path: Path,
    parameter_schema_review: dict[str, Any],
    parameter_schema_review_path: Path,
) -> dict[str, Any]:
    errors = (
        validate_pipl_parser_gate(pipl_parser_gate)
        + validate_synthetic_payload_parser(synthetic_payload_parser, pipl_parser_gate_path)
        + validate_consistency_audit(consistency_audit, pipl_parser_gate_path)
        + validate_parameter_schema_review(parameter_schema_review)
    )
    if errors:
        raise ValueError("; ".join(errors))

    gate_summary = pipl_parser_gate.get("summary", {}) if isinstance(pipl_parser_gate.get("summary"), dict) else {}
    audit_summary = consistency_audit.get("summary", {}) if isinstance(consistency_audit.get("summary"), dict) else {}
    schema_summary = (
        parameter_schema_review.get("summary")
        if isinstance(parameter_schema_review.get("summary"), dict)
        else {}
    )
    gate_contract = (
        pipl_parser_gate.get("parser_input_contract")
        if isinstance(pipl_parser_gate.get("parser_input_contract"), dict)
        else {}
    )
    review_items = build_review_items(
        pipl_parser_gate,
        synthetic_payload_parser,
        consistency_audit,
        parameter_schema_review,
    )
    packet_ready = all(item["real_payload_access_allowed_now"] is False for item in review_items)
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aex_pipl_payload_adapter_review_packet",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_pipl_parser_gate": str(pipl_parser_gate_path),
        "source_synthetic_pipl_payload_parser": str(synthetic_payload_parser_path),
        "source_pipl_resource_consistency_audit": str(consistency_audit_path),
        "source_parameter_schema_review": str(parameter_schema_review_path),
        "adapter_review_state": (
            "pipl_payload_adapter_review_ready_real_payload_closed"
            if packet_ready
            else "pipl_payload_adapter_review_failed"
        ),
        "adapter_review_ready": packet_ready,
        "source_chain_valid": True,
        "synthetic_parser_contract_reviewed": True,
        "metadata_consistency_reviewed": True,
        "metadata_budget_reviewed": True,
        "parameter_schema_reviewed": True,
        "redaction_policy_reviewed": True,
        "ofx_describe_policy_reviewed": True,
        "real_payload_adapter_allowed_now": False,
        "real_payload_input_allowed_now": False,
        "real_pipl_payload_parser_enabled": False,
        "real_pipl_payload_parsed": False,
        "resource_payload_opened": False,
        "resource_payload_extracted": False,
        "raw_payload_serialized": False,
        "output_metadata_only": True,
        "parameter_schema_emission_allowed_now": False,
        "parameter_schema_emitted": False,
        "redacted_schema_emitted": False,
        "review_item_count": len(review_items),
        "blocking_review_item_count": sum(1 for item in review_items if item["blocks_real_adapter_enablement"]),
        "adapter_review_contract": {
            "state": "review_packet_ready_real_payload_closed",
            "accepted_current_inputs": [
                "pipl_parser_gate_json",
                "synthetic_pipl_payload_parser_json",
                "pipl_resource_consistency_audit_json",
                "parameter_schema_review_json",
            ],
            "future_adapter_required_gates": [
                "explicit_user_approval_for_payload_access",
                "approved_fixture_scope",
                "worker_containment_for_payload_parser",
                "byte_limit_enforcement",
                "metadata_only_output_review",
                "parameter_schema_review_policy",
                "redacted_schema_verifier_before_schema_emission",
            ],
            "current_forbidden_inputs": list(FORBIDDEN_CLI_INPUTS),
            "blocked_actions": [
                "open_aex",
                "extract_resource_payload",
                "parse_real_pipl_payload",
                "serialize_raw_payload",
                "emit_parameter_schema",
                "emit_redacted_schema",
                "load_aex",
                "start_after_effects",
                "route_ofx",
                "render_project",
            ],
        },
        "review_items": review_items,
        "candidate_review_budget": {
            "candidate_count": gate_summary.get("candidate_count"),
            "eligible_future_parser_candidate_count": gate_summary.get(
                "eligible_future_parser_candidate_count"
            ),
            "hold_candidate_count": gate_summary.get("hold_candidate_count"),
            "parser_gate_action_counts": gate_summary.get("parser_gate_action_counts"),
            "observed_pipl_resource_entry_count": audit_summary.get("pipl_resource_entry_count"),
            "observed_pipl_resource_total_size": audit_summary.get("pipl_resource_total_size"),
            "observed_pipl_resource_max_size": audit_summary.get("pipl_resource_max_size"),
            "proposed_real_parser_limit_bytes": gate_contract.get("proposed_real_parser_limit_bytes"),
            "parser_case_count": synthetic_payload_parser.get("parser_case_count"),
            "parser_case_failed_count": synthetic_payload_parser.get("parser_case_failed_count"),
            "schema_candidate_count": schema_summary.get("candidate_count"),
            "schema_payload_parser_required_count": schema_summary.get("payload_parser_required_count"),
            "schema_host_contract_review_count": schema_summary.get("host_contract_review_count"),
            "schema_mapping_state_counts": schema_summary.get("mapping_state_counts"),
        },
        "blockers": [
            "real_payload_adapter_not_enabled",
            "real_payload_access_not_approved",
            "fixture_not_approved",
            "load_gate_closed",
            "native_loader_path_policy_closed",
            "schema_emission_not_approved",
            "ae_host_validation_closed",
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
        "notes": [
            "Review packet reads JSON evidence only.",
            "No real PiPL payload, resource payload, AEX file, native load, AE, OFX, render, or schema output is accessed.",
            "This packet prepares review requirements; it does not approve or enable a real payload adapter.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    reject_forbidden_cli_inputs(sys.argv[1:] if argv is None else argv)
    parser = argparse.ArgumentParser(description="Build real PiPL payload adapter review packet")
    parser.add_argument("--pipl-parser-gate", required=True, help="PiPL parser gate JSON under target/pipl-parser-gate")
    parser.add_argument(
        "--synthetic-payload-parser",
        required=True,
        help="Synthetic PiPL payload parser JSON under target/synthetic-pipl-payload-parser",
    )
    parser.add_argument(
        "--consistency-audit",
        required=True,
        help="PiPL consistency audit JSON under target/pipl-resource-consistency-audit",
    )
    parser.add_argument(
        "--parameter-schema-review",
        required=True,
        help="Parameter schema review JSON under target/parameter-schema-review",
    )
    parser.add_argument("--out", required=True, help="Create-new review packet under target/pipl-payload-adapter-review")
    return parser.parse_args(argv)


def main() -> int:
    args = parse_args()
    pipl_parser_gate, pipl_parser_gate_path = load_pipl_parser_gate(Path(args.pipl_parser_gate))
    synthetic_payload_parser, synthetic_payload_parser_path = load_synthetic_payload_parser(
        Path(args.synthetic_payload_parser)
    )
    consistency_audit, consistency_audit_path = load_consistency_audit(Path(args.consistency_audit))
    parameter_schema_review, parameter_schema_review_path = load_parameter_schema_review(
        Path(args.parameter_schema_review)
    )
    packet = build_adapter_review_packet(
        pipl_parser_gate=pipl_parser_gate,
        pipl_parser_gate_path=pipl_parser_gate_path,
        synthetic_payload_parser=synthetic_payload_parser,
        synthetic_payload_parser_path=synthetic_payload_parser_path,
        consistency_audit=consistency_audit,
        consistency_audit_path=consistency_audit_path,
        parameter_schema_review=parameter_schema_review,
        parameter_schema_review_path=parameter_schema_review_path,
    )
    written = write_json_create_new(Path(args.out), packet)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

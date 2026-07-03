#!/usr/bin/env python3
"""Build a no-real-schema verifier report for redacted AEX schema output.

The verifier reads the parameter schema review packet JSON only. It validates
the redaction allow/deny contract with synthetic in-memory fixtures, but emits
no real parameter schema, no redacted schema, no PiPL payload, and no OFX
describe data.
"""

from __future__ import annotations

import argparse
import json
import re
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
REVIEW_ROOT = TARGET_ROOT / "parameter-schema-review"
VERIFIER_ROOT = TARGET_ROOT / "redacted-schema-verifier"

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

REQUIRED_ALLOWED_FIELDS = (
    "schema_version",
    "compatibility_class",
    "mapping_state_counts",
    "parameter_count_if_non_identifying",
    "parameter_type_categories_if_non_identifying",
    "range_presence_flags_if_non_identifying",
)

REQUIRED_FORBIDDEN_FIELDS = (
    "raw_pipl_payload_bytes",
    "private_resource_payload",
    "unredacted_parameter_names",
    "unredacted_default_values",
    "unredacted_value_ranges",
    "binary_hashes",
    "absolute_source_paths",
)

WINDOWS_ABSOLUTE_PATH_RE = re.compile(r"^[A-Za-z]:[\\/]")


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
        raise ValueError("redacted schema verifier report must have .json extension")
    VERIFIER_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, VERIFIER_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(VERIFIER_ROOT.resolve(strict=True)):
        raise ValueError(f"redacted schema verifier parent must stay under {VERIFIER_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_review_packet(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, REVIEW_ROOT, "parameter schema review packet")
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


def validate_review_packet(packet: dict[str, Any]) -> list[str]:
    errors = require_local_only(packet, "parameter schema review packet")
    if packet.get("packet_kind") != "aex_parameter_schema_review_packet":
        errors.append("parameter schema review packet kind must be aex_parameter_schema_review_packet")
    if packet.get("review_state") != "parameter_schema_review_ready_no_payload":
        errors.append("parameter schema review packet review_state must be parameter_schema_review_ready_no_payload")
    if packet.get("parser_design_state") != "payload_parser_design_review_ready_parser_disabled":
        errors.append("parameter schema review packet parser_design_state must keep parser disabled")
    if packet.get("redaction_policy_state") != "redaction_policy_ready_no_schema_output":
        errors.append("parameter schema review packet redaction_policy_state must be ready without output")
    if packet.get("payload_parser_enabled") is not False:
        errors.append("parameter schema review packet payload_parser_enabled must be false")
    if packet.get("redacted_schema_available") is not False:
        errors.append("parameter schema review packet redacted_schema_available must be false")
    if packet.get("ofx_describe_mapping_ready") is not False:
        errors.append("parameter schema review packet ofx_describe_mapping_ready must be false")
    policy = packet.get("redaction_policy")
    if not isinstance(policy, dict):
        errors.append("parameter schema review packet redaction_policy must be an object")
    else:
        allowed = policy.get("allowed_after_review")
        forbidden = policy.get("forbidden_without_additional_approval")
        if not isinstance(allowed, list):
            errors.append("redaction policy allowed_after_review must be a list")
        else:
            missing = [field for field in REQUIRED_ALLOWED_FIELDS if field not in allowed]
            if missing:
                errors.append(f"redaction policy missing allowed fields: {', '.join(missing)}")
        if not isinstance(forbidden, list):
            errors.append("redaction policy forbidden_without_additional_approval must be a list")
        else:
            missing = [field for field in REQUIRED_FORBIDDEN_FIELDS if field not in forbidden]
            if missing:
                errors.append(f"redaction policy missing forbidden fields: {', '.join(missing)}")
    errors.extend(safety_errors(packet, "parameter schema review packet"))
    return errors


def list_strings(value: Any) -> list[str]:
    if isinstance(value, str):
        return [value]
    if isinstance(value, dict):
        result: list[str] = []
        for key, item in value.items():
            result.extend(list_strings(key))
            result.extend(list_strings(item))
        return result
    if isinstance(value, list):
        result = []
        for item in value:
            result.extend(list_strings(item))
        return result
    return []


def list_keys(value: Any) -> list[str]:
    if isinstance(value, dict):
        result = list(value.keys())
        for item in value.values():
            result.extend(list_keys(item))
        return result
    if isinstance(value, list):
        result: list[str] = []
        for item in value:
            result.extend(list_keys(item))
        return result
    return []


def is_absolute_path_string(value: str) -> bool:
    return bool(WINDOWS_ABSOLUTE_PATH_RE.match(value) or value.startswith("/") or value.startswith("\\\\"))


def validate_schema_candidate(candidate: dict[str, Any], *, allowed: set[str], forbidden: set[str]) -> list[str]:
    errors: list[str] = []
    top_level_keys = set(candidate.keys())
    all_keys = set(list_keys(candidate))
    extra_keys = top_level_keys - allowed
    if extra_keys:
        errors.append(f"candidate contains non-allowlisted top-level keys: {', '.join(sorted(extra_keys))}")
    forbidden_hits = all_keys & forbidden
    if forbidden_hits:
        errors.append(f"candidate contains forbidden keys: {', '.join(sorted(forbidden_hits))}")
    absolute_strings = [value for value in list_strings(candidate) if is_absolute_path_string(value)]
    if absolute_strings:
        errors.append("candidate contains absolute path-like strings")
    return errors


def build_synthetic_schema_fixture(mapping_counts: dict[str, Any]) -> dict[str, Any]:
    return {
        "schema_version": 1,
        "compatibility_class": "synthetic_redaction_contract_only",
        "mapping_state_counts": mapping_counts,
        "parameter_count_if_non_identifying": 0,
        "parameter_type_categories_if_non_identifying": [],
        "range_presence_flags_if_non_identifying": {"has_ranges": False},
    }


def build_redaction_contract(policy: dict[str, Any]) -> dict[str, Any]:
    allowed = [field for field in policy.get("allowed_after_review", []) if isinstance(field, str)]
    forbidden = [field for field in policy.get("forbidden_without_additional_approval", []) if isinstance(field, str)]
    return {
        "state": "allowlist_and_forbidden_fields_ready",
        "allowed_top_level_fields": allowed,
        "forbidden_fields": forbidden,
        "absolute_path_strings_allowed": False,
        "raw_payload_values_allowed": False,
        "real_schema_values_allowed": False,
    }


def build_verifier_checks(
    *,
    synthetic_fixture: dict[str, Any],
    allowed: set[str],
    forbidden: set[str],
) -> list[dict[str, Any]]:
    candidate_errors = validate_schema_candidate(synthetic_fixture, allowed=allowed, forbidden=forbidden)
    return [
        {
            "check_id": "synthetic_fixture_uses_allowlisted_fields_only",
            "passed": not candidate_errors,
            "error_count": len(candidate_errors),
        },
        {
            "check_id": "forbidden_field_names_are_blocked",
            "passed": bool(forbidden),
            "blocked_field_count": len(forbidden),
        },
        {
            "check_id": "absolute_source_paths_are_blocked",
            "passed": "absolute_source_paths" in forbidden,
        },
        {
            "check_id": "raw_payload_and_unredacted_values_are_blocked",
            "passed": all(
                field in forbidden
                for field in (
                    "raw_pipl_payload_bytes",
                    "private_resource_payload",
                    "unredacted_parameter_names",
                    "unredacted_default_values",
                    "unredacted_value_ranges",
                )
            ),
        },
    ]


def build_verifier_report(*, review_packet: dict[str, Any], review_packet_path: Path) -> dict[str, Any]:
    errors = validate_review_packet(review_packet)
    if errors:
        raise ValueError("; ".join(errors))

    policy = review_packet["redaction_policy"]
    contract = build_redaction_contract(policy)
    allowed = set(contract["allowed_top_level_fields"])
    forbidden = set(contract["forbidden_fields"])
    summary = review_packet.get("summary", {}) if isinstance(review_packet.get("summary"), dict) else {}
    mapping_counts = summary.get("mapping_state_counts", {})
    if not isinstance(mapping_counts, dict):
        mapping_counts = {}
    synthetic_fixture = build_synthetic_schema_fixture(mapping_counts)
    fixture_errors = validate_schema_candidate(synthetic_fixture, allowed=allowed, forbidden=forbidden)
    checks = build_verifier_checks(synthetic_fixture=synthetic_fixture, allowed=allowed, forbidden=forbidden)
    checks_passed = not fixture_errors and all(check["passed"] for check in checks)
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_redacted_schema_verifier",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_parameter_schema_review": str(review_packet_path),
        "verifier_state": "redacted_schema_verifier_ready_no_real_schema",
        "verifier_ready": checks_passed,
        "real_redacted_schema_available": False,
        "real_parameter_schema_available": False,
        "payload_parser_enabled": False,
        "ofx_describe_mapping_ready": False,
        "synthetic_schema_fixture_used": True,
        "synthetic_fixture_serialized": False,
        "redaction_contract": contract,
        "verifier_checks": checks,
        "synthetic_fixture_summary": {
            "field_count": len(synthetic_fixture),
            "allowed_field_count": len(allowed),
            "forbidden_field_count": len(forbidden),
            "candidate_error_count": len(fixture_errors),
            "candidate_errors": fixture_errors,
        },
        "review_packet_link": {
            "source_review_state": review_packet.get("review_state"),
            "source_redaction_policy_state": review_packet.get("redaction_policy_state"),
            "this_verifier_enables_payload_parser": False,
            "this_verifier_emits_real_schema": False,
            "this_verifier_emits_redacted_schema": False,
            "this_verifier_enables_ofx_describe": False,
            "this_verifier_reduces_blocker_to": "needs_parser_output_and_explicit_schema_emission_approval",
        },
        "blockers": [
            "real_redacted_schema_not_emitted",
            "payload_parser_not_implemented",
            "ofx_describe_mapping_deferred",
            "fixture_not_approved",
            "load_gate_closed",
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
            "Verifier reads the parameter schema review packet JSON only.",
            "Synthetic schema fixture is used in memory and not serialized as a real schema artifact.",
            "No real or redacted parameter schema is emitted.",
            "No PiPL payload, AEX, AE, OFX, render, or project-write action is performed.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build no-real-schema redaction verifier report")
    parser.add_argument("--review-packet", required=True, help="Parameter schema review packet under target/parameter-schema-review")
    parser.add_argument("--out", required=True, help="Create-new verifier JSON under target/redacted-schema-verifier")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    review_packet, review_packet_path = load_review_packet(Path(args.review_packet))
    report = build_verifier_report(review_packet=review_packet, review_packet_path=review_packet_path)
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

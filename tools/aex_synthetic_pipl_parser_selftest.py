#!/usr/bin/env python3
"""Run a no-real-payload synthetic PiPL parser selftest.

This selftest reads the redacted schema verifier JSON only. It exercises a tiny
synthetic length-prefixed parser contract in memory to prove bounds checks and
no-raw-payload reporting before any real PiPL payload parser is enabled.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
VERIFIER_ROOT = TARGET_ROOT / "redacted-schema-verifier"
SELFTEST_ROOT = TARGET_ROOT / "synthetic-pipl-parser-selftest"

SYNTHETIC_MAGIC = b"SPIP"
MAX_SYNTHETIC_PAYLOAD_BYTES = 256
ALLOWED_TAGS = {
    1: "parameter_type_category",
    2: "range_presence_flag",
    3: "parameter_count_hint",
}

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
    "raw_payload_serialized",
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
        raise ValueError("synthetic PiPL parser selftest report must have .json extension")
    SELFTEST_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, SELFTEST_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(SELFTEST_ROOT.resolve(strict=True)):
        raise ValueError(f"synthetic PiPL parser selftest parent must stay under {SELFTEST_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_redacted_schema_verifier(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, VERIFIER_ROOT, "redacted schema verifier")
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


def validate_redacted_schema_verifier(report: dict[str, Any]) -> list[str]:
    errors = require_local_only(report, "redacted schema verifier")
    if report.get("report_kind") != "aex_redacted_schema_verifier":
        errors.append("redacted schema verifier report_kind must be aex_redacted_schema_verifier")
    if report.get("verifier_state") != "redacted_schema_verifier_ready_no_real_schema":
        errors.append("redacted schema verifier verifier_state must be ready without real schema")
    if report.get("verifier_ready") is not True:
        errors.append("redacted schema verifier verifier_ready must be true")
    if report.get("real_redacted_schema_available") is not False:
        errors.append("redacted schema verifier real_redacted_schema_available must be false")
    if report.get("payload_parser_enabled") is not False:
        errors.append("redacted schema verifier payload_parser_enabled must be false")
    if report.get("synthetic_schema_fixture_used") is not True:
        errors.append("redacted schema verifier synthetic_schema_fixture_used must be true")
    contract = report.get("redaction_contract")
    if not isinstance(contract, dict):
        errors.append("redacted schema verifier redaction_contract must be an object")
    else:
        if contract.get("raw_payload_values_allowed") is not False:
            errors.append("redacted schema verifier must forbid raw payload values")
        if contract.get("real_schema_values_allowed") is not False:
            errors.append("redacted schema verifier must forbid real schema values")
    errors.extend(safety_errors(report, "redacted schema verifier"))
    return errors


def synthetic_payload(records: list[tuple[int, bytes]]) -> bytes:
    body = bytearray(SYNTHETIC_MAGIC)
    for tag, value in records:
        if len(value) > 255:
            raise ValueError("synthetic record value too large")
        body.append(tag)
        body.append(len(value))
        body.extend(value)
    return bytes(body)


def parse_synthetic_payload(payload: bytes) -> dict[str, Any]:
    if len(payload) > MAX_SYNTHETIC_PAYLOAD_BYTES:
        return {
            "parse_state": "rejected_oversized_synthetic_payload",
            "metadata": {},
            "errors": ["synthetic payload exceeds size limit"],
        }
    if not payload.startswith(SYNTHETIC_MAGIC):
        return {"parse_state": "rejected_bad_magic", "metadata": {}, "errors": ["bad synthetic magic"]}
    offset = len(SYNTHETIC_MAGIC)
    tag_counts: dict[str, int] = {}
    unknown_tag_count = 0
    record_count = 0
    errors: list[str] = []
    while offset < len(payload):
        if offset + 2 > len(payload):
            errors.append("truncated record header")
            break
        tag = payload[offset]
        length = payload[offset + 1]
        offset += 2
        if offset + length > len(payload):
            errors.append("truncated record value")
            break
        tag_name = ALLOWED_TAGS.get(tag)
        if tag_name is None:
            unknown_tag_count += 1
        else:
            tag_counts[tag_name] = tag_counts.get(tag_name, 0) + 1
        record_count += 1
        offset += length
    parse_state = "parsed_synthetic_payload_metadata_only" if not errors else "rejected_malformed_synthetic_payload"
    return {
        "parse_state": parse_state,
        "metadata": {
            "record_count": record_count,
            "known_tag_counts": dict(sorted(tag_counts.items())),
            "unknown_tag_count": unknown_tag_count,
            "payload_size_bytes": len(payload),
        },
        "errors": errors,
    }


def contains_raw_payload_value(value: Any) -> bool:
    if isinstance(value, (bytes, bytearray)):
        return True
    if isinstance(value, dict):
        return any(contains_raw_payload_value(item) for item in value.values())
    if isinstance(value, list):
        return any(contains_raw_payload_value(item) for item in value)
    return False


def has_raw_payload_keys(value: Any) -> bool:
    forbidden_fragments = ("payload", "bytes", "raw")
    if isinstance(value, dict):
        for key, item in value.items():
            if any(fragment in str(key).lower() for fragment in forbidden_fragments):
                if key != "payload_size_bytes":
                    return True
            if has_raw_payload_keys(item):
                return True
    if isinstance(value, list):
        return any(has_raw_payload_keys(item) for item in value)
    return False


def run_selftest_cases() -> list[dict[str, Any]]:
    cases = [
        {
            "case_id": "valid_metadata_only_records",
            "payload": synthetic_payload([(1, b"type"), (2, b"range"), (3, b"count")]),
            "expected_state": "parsed_synthetic_payload_metadata_only",
        },
        {
            "case_id": "unknown_tag_is_counted_not_serialized",
            "payload": synthetic_payload([(99, b"opaque")]),
            "expected_state": "parsed_synthetic_payload_metadata_only",
        },
        {
            "case_id": "truncated_value_is_rejected",
            "payload": SYNTHETIC_MAGIC + bytes([1, 8]) + b"tiny",
            "expected_state": "rejected_malformed_synthetic_payload",
        },
        {
            "case_id": "oversized_payload_is_rejected",
            "payload": SYNTHETIC_MAGIC + b"\x01\x01x" * 90,
            "expected_state": "rejected_oversized_synthetic_payload",
        },
        {
            "case_id": "bad_magic_is_rejected",
            "payload": b"NOPE\x01\x01x",
            "expected_state": "rejected_bad_magic",
        },
    ]
    results: list[dict[str, Any]] = []
    for case in cases:
        parsed = parse_synthetic_payload(case["payload"])
        output_has_raw = contains_raw_payload_value(parsed) or has_raw_payload_keys(parsed)
        passed = parsed["parse_state"] == case["expected_state"] and not output_has_raw
        results.append(
            {
                "case_id": case["case_id"],
                "expected_state": case["expected_state"],
                "parse_state": parsed["parse_state"],
                "passed": passed,
                "metadata": parsed["metadata"],
                "error_count": len(parsed["errors"]),
                "raw_payload_serialized": output_has_raw,
            }
        )
    return results


def build_selftest_report(*, redacted_schema_verifier: dict[str, Any], verifier_path: Path) -> dict[str, Any]:
    errors = validate_redacted_schema_verifier(redacted_schema_verifier)
    if errors:
        raise ValueError("; ".join(errors))

    case_results = run_selftest_cases()
    passed_count = sum(1 for result in case_results if result["passed"])
    failed_count = len(case_results) - passed_count
    raw_payload_serialized = any(result["raw_payload_serialized"] for result in case_results)
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_synthetic_pipl_parser_selftest",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_redacted_schema_verifier": str(verifier_path),
        "selftest_state": (
            "synthetic_pipl_parser_selftest_passed_no_real_payload"
            if failed_count == 0
            else "synthetic_pipl_parser_selftest_failed"
        ),
        "synthetic_parser_ready": failed_count == 0,
        "real_pipl_payload_parser_enabled": False,
        "real_pipl_payload_parsed": False,
        "synthetic_payloads_used": True,
        "synthetic_payloads_serialized": False,
        "raw_payload_serialized": raw_payload_serialized,
        "parser_contract": {
            "state": "synthetic_bounds_contract_ready",
            "magic": "SPIP",
            "max_synthetic_payload_bytes": MAX_SYNTHETIC_PAYLOAD_BYTES,
            "allowed_tag_categories": list(ALLOWED_TAGS.values()),
            "output_policy": "metadata_counts_only_no_values",
            "raw_payload_output_allowed": False,
            "real_pipl_payload_input_allowed": False,
        },
        "case_results": case_results,
        "summary": {
            "case_count": len(case_results),
            "passed_count": passed_count,
            "failed_count": failed_count,
            "raw_payload_serialized_count": sum(1 for result in case_results if result["raw_payload_serialized"]),
            "synthetic_parser_ready": failed_count == 0,
        },
        "verifier_link": {
            "source_verifier_state": redacted_schema_verifier.get("verifier_state"),
            "source_verifier_ready": redacted_schema_verifier.get("verifier_ready"),
            "this_selftest_enables_real_payload_parser": False,
            "this_selftest_parses_real_pipl_payload": False,
            "this_selftest_emits_parameter_schema": False,
            "this_selftest_reduces_blocker_to": "needs_real_pipl_parser_implementation_review_and_fixture_approval",
        },
        "blockers": [
            "real_pipl_payload_parser_not_enabled",
            "real_pipl_payload_not_parsed",
            "fixture_not_approved",
            "load_gate_closed",
            "schema_emission_not_approved",
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
            "Selftest reads the redacted schema verifier JSON only.",
            "Synthetic byte payloads are generated in memory and are not serialized to disk.",
            "No real PiPL resource payload is parsed or copied.",
            "No AEX, AE, OFX, render, schema emission, or project-write action is performed.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run no-real-payload synthetic PiPL parser selftest")
    parser.add_argument("--redacted-schema-verifier", required=True, help="Verifier JSON under target/redacted-schema-verifier")
    parser.add_argument("--out", required=True, help="Create-new selftest report under target/synthetic-pipl-parser-selftest")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    verifier, verifier_path = load_redacted_schema_verifier(Path(args.redacted_schema_verifier))
    report = build_selftest_report(redacted_schema_verifier=verifier, verifier_path=verifier_path)
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

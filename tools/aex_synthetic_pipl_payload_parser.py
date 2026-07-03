#!/usr/bin/env python3
"""Run a reviewed synthetic PiPL payload parser without real payload input.

The parser implementation is intentionally limited to in-memory synthetic
payloads from the bounds harness. It reads gate/selftest JSON evidence only and
does not accept AEX paths, open resource payloads, parse real PiPL bytes, or
emit parameter schemas.
"""

from __future__ import annotations

import argparse
import json
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TOOLS_ROOT = LAB_ROOT / "tools"
if str(TOOLS_ROOT) not in sys.path:
    sys.path.insert(0, str(TOOLS_ROOT))

import aex_pipl_parser_gate
import aex_synthetic_pipl_parser_selftest


TARGET_ROOT = LAB_ROOT / "target"
PIPL_GATE_ROOT = TARGET_ROOT / "pipl-parser-gate"
SYNTHETIC_SELFTEST_ROOT = TARGET_ROOT / "synthetic-pipl-parser-selftest"
PARSER_ROOT = TARGET_ROOT / "synthetic-pipl-payload-parser"

SYNTHETIC_MAGIC = aex_synthetic_pipl_parser_selftest.SYNTHETIC_MAGIC
ALLOWED_TAGS = aex_synthetic_pipl_parser_selftest.ALLOWED_TAGS
MAX_SYNTHETIC_PAYLOAD_BYTES = aex_synthetic_pipl_parser_selftest.MAX_SYNTHETIC_PAYLOAD_BYTES

SAFETY_FLAGS = tuple(
    dict.fromkeys(
        (
            *aex_pipl_parser_gate.SAFETY_FLAGS,
            "resource_payload_opened",
            "pipl_payload_parsed",
            "parameter_schema_emitted",
            "redacted_schema_emitted",
        )
    )
)

FORBIDDEN_CLI_INPUTS = (
    "--aex",
    "--aex-path",
    "--pipl",
    "--pipl-payload",
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
        raise ValueError("synthetic PiPL payload parser report must have .json extension")
    PARSER_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, PARSER_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(PARSER_ROOT.resolve(strict=True)):
        raise ValueError(f"synthetic PiPL payload parser parent must stay under {PARSER_ROOT}")
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


def load_synthetic_selftest(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, SYNTHETIC_SELFTEST_ROOT, "synthetic PiPL parser selftest")
    return read_json_object(resolved), resolved


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def reject_forbidden_cli_inputs(argv: list[str]) -> None:
    forbidden = set(FORBIDDEN_CLI_INPUTS)
    for raw_arg in argv:
        token = raw_arg.split("=", 1)[0]
        if token in forbidden or raw_arg in forbidden:
            raise ValueError(f"forbidden CLI input for synthetic PiPL parser: {raw_arg}")


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
        errors.append("PiPL parser gate must not allow real payload input now")
    checks = report.get("parser_gate_checks")
    if not isinstance(checks, list) or not checks:
        errors.append("PiPL parser gate checks must be a non-empty list")
    elif not all(isinstance(check, dict) and check.get("passed") is True for check in checks):
        errors.append("PiPL parser gate checks must all pass")
    errors.extend(safety_errors(report, "PiPL parser gate"))
    return errors


def validate_synthetic_selftest(report: dict[str, Any]) -> list[str]:
    errors = aex_pipl_parser_gate.validate_synthetic_selftest(report)
    summary = report.get("summary")
    if isinstance(summary, dict) and summary.get("case_count", 0) <= 0:
        errors.append("synthetic parser selftest case_count must be positive")
    return errors


def synthetic_payload(records: list[tuple[int, bytes]]) -> bytes:
    return aex_synthetic_pipl_parser_selftest.synthetic_payload(records)


def parse_synthetic_pipl_payload(payload: bytes) -> dict[str, Any]:
    if len(payload) > MAX_SYNTHETIC_PAYLOAD_BYTES:
        return {
            "parse_state": "rejected_oversized_synthetic_payload",
            "metadata": {},
            "errors": ["synthetic payload exceeds size limit"],
        }
    if not payload.startswith(SYNTHETIC_MAGIC):
        return {"parse_state": "rejected_bad_magic", "metadata": {}, "errors": ["bad synthetic magic"]}

    offset = len(SYNTHETIC_MAGIC)
    record_count = 0
    unknown_tag_count = 0
    tag_counts: dict[str, int] = {}
    value_lengths: list[int] = []
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
        value_lengths.append(length)
        offset += length

    parse_state = "parsed_synthetic_payload_metadata_only" if not errors else "rejected_malformed_synthetic_payload"
    return {
        "parse_state": parse_state,
        "metadata": {
            "record_count": record_count,
            "known_tag_counts": dict(sorted(tag_counts.items())),
            "unknown_tag_count": unknown_tag_count,
            "payload_size_bytes": len(payload),
            "min_record_value_length": min(value_lengths) if value_lengths else 0,
            "max_record_value_length": max(value_lengths) if value_lengths else 0,
            "total_record_value_length": sum(value_lengths),
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


def has_forbidden_output_keys(value: Any) -> bool:
    allowed_payload_keys = {"payload_size_bytes"}
    forbidden_fragments = ("raw", "payload_bytes", "value_bytes", "bytes_hex", "byte_values")
    if isinstance(value, dict):
        for key, item in value.items():
            lowered = str(key).lower()
            if lowered not in allowed_payload_keys and any(fragment in lowered for fragment in forbidden_fragments):
                return True
            if has_forbidden_output_keys(item):
                return True
    if isinstance(value, list):
        return any(has_forbidden_output_keys(item) for item in value)
    return False


def parser_cases() -> list[dict[str, Any]]:
    return [
        {
            "case_id": "valid_known_tags_metadata_only",
            "payload": synthetic_payload([(1, b"type"), (2, b"range"), (3, b"count")]),
            "expected_state": "parsed_synthetic_payload_metadata_only",
        },
        {
            "case_id": "repeated_tag_counts_only",
            "payload": synthetic_payload([(1, b"a"), (1, b"b"), (2, b"c")]),
            "expected_state": "parsed_synthetic_payload_metadata_only",
        },
        {
            "case_id": "unknown_tag_counted_without_value",
            "payload": synthetic_payload([(99, b"private-value")]),
            "expected_state": "parsed_synthetic_payload_metadata_only",
        },
        {
            "case_id": "zero_length_record_metadata_only",
            "payload": synthetic_payload([(3, b"")]),
            "expected_state": "parsed_synthetic_payload_metadata_only",
        },
        {
            "case_id": "truncated_header_rejected",
            "payload": SYNTHETIC_MAGIC + b"\x01",
            "expected_state": "rejected_malformed_synthetic_payload",
        },
        {
            "case_id": "truncated_value_rejected",
            "payload": SYNTHETIC_MAGIC + bytes([1, 8]) + b"tiny",
            "expected_state": "rejected_malformed_synthetic_payload",
        },
        {
            "case_id": "oversized_payload_rejected",
            "payload": SYNTHETIC_MAGIC + b"\x01\x01x" * 90,
            "expected_state": "rejected_oversized_synthetic_payload",
        },
        {
            "case_id": "bad_magic_rejected",
            "payload": b"NOPE\x01\x01x",
            "expected_state": "rejected_bad_magic",
        },
    ]


def run_parser_cases() -> list[dict[str, Any]]:
    results: list[dict[str, Any]] = []
    for case in parser_cases():
        parsed = parse_synthetic_pipl_payload(case["payload"])
        raw_output = contains_raw_payload_value(parsed) or has_forbidden_output_keys(parsed)
        passed = parsed["parse_state"] == case["expected_state"] and not raw_output
        results.append(
            {
                "case_id": case["case_id"],
                "expected_state": case["expected_state"],
                "parse_state": parsed["parse_state"],
                "passed": passed,
                "metadata": parsed["metadata"],
                "error_count": len(parsed["errors"]),
                "raw_payload_serialized": raw_output,
            }
        )
    return results


def build_parser_report(
    *,
    pipl_parser_gate: dict[str, Any],
    pipl_parser_gate_path: Path,
    synthetic_selftest: dict[str, Any],
    synthetic_selftest_path: Path,
) -> dict[str, Any]:
    errors = validate_pipl_parser_gate(pipl_parser_gate) + validate_synthetic_selftest(synthetic_selftest)
    if errors:
        raise ValueError("; ".join(errors))

    case_results = run_parser_cases()
    passed_count = sum(1 for result in case_results if result["passed"])
    failed_count = len(case_results) - passed_count
    raw_payload_serialized = any(result["raw_payload_serialized"] for result in case_results)
    parser_ready = failed_count == 0 and not raw_payload_serialized
    contract = pipl_parser_gate.get("parser_input_contract")
    if not isinstance(contract, dict):
        contract = {}

    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_synthetic_pipl_payload_parser",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_pipl_parser_gate": str(pipl_parser_gate_path),
        "source_synthetic_pipl_parser_selftest": str(synthetic_selftest_path),
        "synthetic_payload_parser_state": (
            "synthetic_pipl_payload_parser_ready_real_payload_closed"
            if parser_ready
            else "synthetic_pipl_payload_parser_failed"
        ),
        "synthetic_payload_parser_ready": parser_ready,
        "synthetic_parser_implemented": True,
        "synthetic_bounds_harness_reused": True,
        "synthetic_payload_cases_passed": parser_ready,
        "synthetic_payloads_used": True,
        "synthetic_payloads_serialized": False,
        "real_payload_input_allowed_now": False,
        "output_metadata_only": True,
        "real_pipl_payload_parser_enabled": False,
        "real_pipl_payload_parsed": False,
        "resource_payload_opened": False,
        "raw_payload_serialized": raw_payload_serialized,
        "parser_case_count": len(case_results),
        "parser_case_passed_count": passed_count,
        "parser_case_failed_count": failed_count,
        "parser_api_contract": {
            "state": "synthetic_parser_implementation_ready_real_payload_closed",
            "accepted_input_kind": "synthetic_in_memory_spip_payload_only",
            "real_pipl_payload_input_allowed": False,
            "resource_payload_file_input_allowed": False,
            "aex_path_input_allowed": False,
            "max_synthetic_payload_bytes": MAX_SYNTHETIC_PAYLOAD_BYTES,
            "real_parser_limit_reference_bytes": contract.get("proposed_real_parser_limit_bytes"),
            "known_tag_categories": list(ALLOWED_TAGS.values()),
            "output_policy": "metadata_counts_and_lengths_only_no_values",
            "raw_payload_output_allowed": False,
            "parameter_schema_output_allowed": False,
        },
        "parser_case_results": case_results,
        "summary": {
            "parser_case_count": len(case_results),
            "parser_case_passed_count": passed_count,
            "parser_case_failed_count": failed_count,
            "raw_payload_serialized_count": sum(1 for result in case_results if result["raw_payload_serialized"]),
            "synthetic_payload_parser_ready": parser_ready,
        },
        "gate_link": {
            "source_gate_state": pipl_parser_gate.get("gate_state"),
            "source_gate_ready_for_review": pipl_parser_gate.get("gate_ready_for_review"),
            "source_metadata_budget_ready": pipl_parser_gate.get("metadata_budget_ready"),
            "this_parser_accepts_real_payload": False,
            "this_parser_enables_schema_emission": False,
            "this_parser_reduces_blocker_to": "review_real_payload_adapter_and_fixture_approval",
        },
        "blockers": [
            "real_payload_adapter_not_enabled",
            "real_pipl_payload_not_parsed",
            "fixture_not_approved",
            "load_gate_closed",
            "schema_emission_not_approved",
        ],
        "forbidden_cli_inputs": list(FORBIDDEN_CLI_INPUTS),
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
            "Parser reads gate and synthetic selftest JSON only.",
            "Synthetic payload bytes are generated in memory and never serialized to disk.",
            "Output contains metadata counts and lengths, not record values or raw bytes.",
            "No real PiPL resource payload, AEX path, AE, OFX, render, or schema action is performed.",
        ],
    }


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    reject_forbidden_cli_inputs(sys.argv[1:] if argv is None else argv)
    parser = argparse.ArgumentParser(description="Run synthetic PiPL payload parser without real payload input")
    parser.add_argument("--pipl-parser-gate", required=True, help="PiPL parser gate JSON under target/pipl-parser-gate")
    parser.add_argument(
        "--synthetic-selftest",
        required=True,
        help="Synthetic parser selftest JSON under target/synthetic-pipl-parser-selftest",
    )
    parser.add_argument("--out", required=True, help="Create-new parser report under target/synthetic-pipl-payload-parser")
    return parser.parse_args(argv)


def main() -> int:
    args = parse_args()
    pipl_parser_gate, pipl_parser_gate_path = load_pipl_parser_gate(Path(args.pipl_parser_gate))
    synthetic_selftest, synthetic_selftest_path = load_synthetic_selftest(Path(args.synthetic_selftest))
    report = build_parser_report(
        pipl_parser_gate=pipl_parser_gate,
        pipl_parser_gate_path=pipl_parser_gate_path,
        synthetic_selftest=synthetic_selftest,
        synthetic_selftest_path=synthetic_selftest_path,
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

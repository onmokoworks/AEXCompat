#!/usr/bin/env python3
"""Classify redacted AEPX text inventory rows without reading project files.

The classifier reads ready JSON evidence only: a redacted text inventory and
its round-trip validator. It never opens AEPX/AEP/AEX files, exports payload
values or hashes, writes project files, starts After Effects, invokes OFX, or
renders.
"""

from __future__ import annotations

import argparse
import json
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
ROUNDTRIP_ROOT = TARGET_ROOT / "aepx-roundtrip-validator"
TEXT_INVENTORY_ROOT = TARGET_ROOT / "aepx-redacted-text-inventory"
CLASSIFIER_ROOT = TARGET_ROOT / "aepx-redacted-text-classifier"

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

PAYLOAD_EXPORT_FLAGS = (
    "text_payload_exported",
    "text_payload_hash_exported",
    "bdata_payload_exported",
    "raw_text_fields_present",
    "value_hashes_emitted",
    "raw_payload_serialized",
)

FORBIDDEN_ROW_KEYS = {
    "raw_text",
    "trimmed_text",
    "text_value",
    "text_prefix",
    "text_suffix",
    "text_hash",
    "value_hash",
    "bdata_value",
    "attribute_values",
    "replacement_text",
    "replacement_value",
    "patch",
    "diff",
    "absolute_source_path",
    "absolute_source_path_in_rows",
}

ALLOWED_ROW_KEYS = {
    "attribute_key_count",
    "attribute_keys",
    "candidate_state",
    "character_class_flags",
    "child_count",
    "contains_bdata_attribute",
    "depth",
    "element_ordinal",
    "inventory_index",
    "leading_or_trailing_whitespace",
    "namespace_id",
    "namespace_present",
    "newline_count_bucket",
    "parent_tag",
    "payload_hash_exported",
    "payload_value_exported",
    "row_id",
    "sensitivity_flags",
    "sibling_index",
    "tag",
    "text_length_bucket",
    "text_length_chars",
    "text_presence",
    "trim_delta_bucket",
    "write_risk",
    "xml_path",
}


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


def validate_inventory_input(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("AEPX redacted text inventory input must have .json extension")
    return resolve_under_root(path, TEXT_INVENTORY_ROOT, must_exist=True)


def validate_roundtrip_input(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("AEPX round-trip validator input must have .json extension")
    return resolve_under_root(path, ROUNDTRIP_ROOT, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("AEPX redacted text classifier report must have .json extension")
    CLASSIFIER_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, CLASSIFIER_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(CLASSIFIER_ROOT.resolve(strict=True)):
        raise ValueError(f"AEPX redacted text classifier parent must stay under {CLASSIFIER_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_inventory(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_inventory_input(path)
    return read_json_object(resolved), resolved


def load_roundtrip_validator(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_roundtrip_input(path)
    return read_json_object(resolved), resolved


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def payload_export_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in PAYLOAD_EXPORT_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def validate_roundtrip(roundtrip: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if roundtrip.get("publication_status") != "local-only":
        errors.append("AEPX round-trip validator publication_status must be local-only")
    if roundtrip.get("report_kind") != "aepx_roundtrip_validator":
        errors.append("AEPX round-trip validator report_kind must be aepx_roundtrip_validator")
    if roundtrip.get("roundtrip_state") != "aepx_roundtrip_validator_ready_no_write":
        errors.append("AEPX round-trip validator roundtrip_state must be ready no-write")
    if roundtrip.get("validator_ready") is not True:
        errors.append("AEPX round-trip validator validator_ready must be true")
    if roundtrip.get("source_structure_match") is not True:
        errors.append("AEPX round-trip validator source_structure_match must be true")
    if roundtrip.get("roundtrip_structure_match") is not True:
        errors.append("AEPX round-trip validator roundtrip_structure_match must be true")
    if roundtrip.get("roundtrip_xml_serialized_to_disk") is not False:
        errors.append("AEPX round-trip validator must not serialize XML to disk")
    if roundtrip.get("text_payload_exported") is not False:
        errors.append("AEPX round-trip validator text_payload_exported must be false")
    if roundtrip.get("bdata_payload_exported") is not False:
        errors.append("AEPX round-trip validator bdata_payload_exported must be false")
    errors.extend(safety_errors(roundtrip, "AEPX round-trip validator"))
    return errors


def row_schema_errors(row: dict[str, Any], index: int) -> list[str]:
    errors: list[str] = []
    unknown_keys = sorted(set(row) - ALLOWED_ROW_KEYS)
    forbidden_keys = sorted(set(row) & FORBIDDEN_ROW_KEYS)
    if unknown_keys:
        errors.append(f"text_nodes[{index}] has unknown keys: {', '.join(unknown_keys)}")
    if forbidden_keys:
        errors.append(f"text_nodes[{index}] has forbidden payload keys: {', '.join(forbidden_keys)}")
    if not isinstance(row.get("row_id"), str):
        errors.append(f"text_nodes[{index}] row_id must be a string")
    if not isinstance(row.get("xml_path"), str):
        errors.append(f"text_nodes[{index}] xml_path must be a string")
    if row.get("payload_value_exported") is not False:
        errors.append(f"text_nodes[{index}] payload_value_exported must be false")
    if row.get("payload_hash_exported") is not False:
        errors.append(f"text_nodes[{index}] payload_hash_exported must be false")
    if not isinstance(row.get("sensitivity_flags"), dict):
        errors.append(f"text_nodes[{index}] sensitivity_flags must be an object")
    if not isinstance(row.get("character_class_flags"), dict):
        errors.append(f"text_nodes[{index}] character_class_flags must be an object")
    return errors


def validate_inventory(
    *,
    inventory: dict[str, Any],
    inventory_path: Path,
    roundtrip: dict[str, Any],
    roundtrip_path: Path,
) -> list[str]:
    errors: list[str] = []
    if inventory.get("publication_status") != "local-only":
        errors.append("AEPX redacted text inventory publication_status must be local-only")
    if inventory.get("report_kind") != "aepx_redacted_text_inventory":
        errors.append("AEPX redacted text inventory report_kind must be aepx_redacted_text_inventory")
    if inventory.get("inventory_state") != "aepx_redacted_text_inventory_ready_no_write":
        errors.append("AEPX redacted text inventory inventory_state must be ready no-write")
    if inventory.get("inventory_ready") is not True:
        errors.append("AEPX redacted text inventory inventory_ready must be true")
    if inventory.get("source_roundtrip_state") != "aepx_roundtrip_validator_ready_no_write":
        errors.append("AEPX redacted text inventory source_roundtrip_state must be ready no-write")
    if inventory.get("source_validator_ready") is not True:
        errors.append("AEPX redacted text inventory source_validator_ready must be true")
    if inventory.get("source_structure_match") is not True:
        errors.append("AEPX redacted text inventory source_structure_match must be true")
    if inventory.get("roundtrip_structure_match") is not True:
        errors.append("AEPX redacted text inventory roundtrip_structure_match must be true")
    if inventory.get("absolute_source_paths_in_inventory_rows") is not False:
        errors.append("AEPX redacted text inventory must not include absolute source paths in rows")
    if not isinstance(inventory.get("summary"), dict):
        errors.append("AEPX redacted text inventory summary must be an object")
    if not isinstance(inventory.get("redaction_policy"), dict):
        errors.append("AEPX redacted text inventory redaction_policy must be an object")
    if not isinstance(inventory.get("inventory_contract"), dict):
        errors.append("AEPX redacted text inventory inventory_contract must be an object")
    text_nodes = inventory.get("text_nodes")
    if not isinstance(text_nodes, list):
        errors.append("AEPX redacted text inventory text_nodes must be a list")
    else:
        for index, row in enumerate(text_nodes):
            if not isinstance(row, dict):
                errors.append(f"text_nodes[{index}] must be an object")
                continue
            errors.extend(row_schema_errors(row, index))

    source_roundtrip = inventory.get("source_aepx_roundtrip_validator")
    if not isinstance(source_roundtrip, str):
        errors.append("AEPX redacted text inventory source_aepx_roundtrip_validator must be a string")
    elif Path(source_roundtrip).resolve() != roundtrip_path.resolve():
        errors.append("AEPX redacted text inventory source_aepx_roundtrip_validator must match input")

    inventory_source_aepx = inventory.get("source_aepx")
    roundtrip_source_aepx = roundtrip.get("source_aepx")
    if isinstance(inventory_source_aepx, str) and isinstance(roundtrip_source_aepx, str):
        if Path(inventory_source_aepx).resolve() != Path(roundtrip_source_aepx).resolve():
            errors.append("AEPX redacted text inventory source_aepx must match round-trip validator")

    summary = inventory.get("summary", {}) if isinstance(inventory.get("summary"), dict) else {}
    if isinstance(text_nodes, list) and summary.get("text_node_count") != len(text_nodes):
        errors.append("AEPX redacted text inventory summary text_node_count must match text_nodes length")

    errors.extend(payload_export_errors(inventory, "AEPX redacted text inventory"))
    errors.extend(safety_errors(inventory, "AEPX redacted text inventory"))
    errors.extend(validate_roundtrip(roundtrip))
    return errors


def bool_flag(flags: Any, key: str) -> bool:
    return isinstance(flags, dict) and flags.get(key) is True


def classify_row(row: dict[str, Any]) -> dict[str, Any]:
    sensitivity = row.get("sensitivity_flags")
    characters = row.get("character_class_flags")
    tag = str(row.get("tag") or "")
    parent_tag = str(row.get("parent_tag") or "")
    length = int(row.get("text_length_chars") or 0)
    length_bucket = str(row.get("text_length_bucket") or "unknown")
    reasons: list[str] = []
    required_reviews = ["schema_review"]
    classification = "review_generic_text_candidate_no_write"
    risk_level = "medium"

    if row.get("contains_bdata_attribute") is True or "bdata" in [str(key).lower() for key in row.get("attribute_keys", [])]:
        classification = "hold_bdata_adjacent_text_no_write"
        risk_level = "critical"
        reasons.append("bdata_adjacent_or_attribute_present")
        required_reviews.append("binary_payload_schema_review")
    elif tag == "ProjectXMPMetadata" or length > 64 or length_bucket == "65_plus" or bool_flag(characters, "has_linebreak"):
        classification = "hold_structural_or_large_metadata_payload_no_write"
        risk_level = "critical"
        reasons.append("large_or_multiline_metadata_payload")
        required_reviews.append("metadata_payload_schema_review")
    elif any(
        bool_flag(sensitivity, key)
        for key in ("looks_like_path", "looks_like_url", "looks_like_email", "looks_like_guid", "looks_like_numeric_id")
    ):
        classification = "hold_sensitive_reference_or_identifier_no_write"
        risk_level = "high"
        reasons.append("sensitivity_flag_present")
        required_reviews.append("identifier_or_reference_mapping_review")
    elif tag == "string" and parent_tag in {"SLay", "CLay", "Layr", "Item", "Fold"} and length <= 64:
        classification = "review_label_like_string_candidate_no_write"
        risk_level = "medium"
        reasons.append("label_like_string_context_pending_schema")
    elif length <= 4:
        classification = "review_short_token_candidate_no_write"
        risk_level = "medium"
        reasons.append("short_token_pending_schema")
    else:
        reasons.append("generic_text_metadata_pending_schema")

    return {
        "row_id": row["row_id"],
        "inventory_index": row.get("inventory_index"),
        "element_ordinal": row.get("element_ordinal"),
        "xml_path": row["xml_path"],
        "parent_tag": row.get("parent_tag"),
        "tag": row.get("tag"),
        "depth": row.get("depth"),
        "text_length_chars": length,
        "text_length_bucket": length_bucket,
        "contains_bdata_attribute": row.get("contains_bdata_attribute") is True,
        "source_candidate_state": row.get("candidate_state"),
        "source_write_risk": row.get("write_risk"),
        "classification": classification,
        "risk_level": risk_level,
        "confidence_bucket": "metadata_only_medium_confidence",
        "write_recommendation": "do_not_write_project_files",
        "write_allowed_now": False,
        "payload_value_exported": False,
        "payload_hash_exported": False,
        "classification_reasons": reasons,
        "required_reviews": sorted(set(required_reviews)),
    }


def summarize_classifications(rows: list[dict[str, Any]], inventory_summary: dict[str, Any]) -> dict[str, Any]:
    classification_counts = Counter(row["classification"] for row in rows)
    risk_counts = Counter(row["risk_level"] for row in rows)
    return {
        "classification_row_count": len(rows),
        "source_text_node_count": inventory_summary.get("text_node_count"),
        "row_count_matches_inventory_summary": inventory_summary.get("text_node_count") == len(rows),
        "approved_write_candidate_count": sum(1 for row in rows if row["write_allowed_now"] is True),
        "no_write_row_count": sum(1 for row in rows if row["write_allowed_now"] is False),
        "unknown_row_count": 0,
        "classification_counts": dict(sorted(classification_counts.items())),
        "risk_counts": dict(sorted(risk_counts.items())),
        "max_text_length_chars": inventory_summary.get("max_text_length_chars"),
        "unique_text_tag_count": inventory_summary.get("unique_text_tag_count"),
    }


def build_redacted_text_classifier(
    *,
    inventory: dict[str, Any],
    inventory_path: Path,
    roundtrip_validator: dict[str, Any],
    roundtrip_validator_path: Path,
) -> dict[str, Any]:
    errors = validate_inventory(
        inventory=inventory,
        inventory_path=inventory_path,
        roundtrip=roundtrip_validator,
        roundtrip_path=roundtrip_validator_path,
    )
    if errors:
        raise ValueError("; ".join(errors))

    text_nodes = inventory["text_nodes"]
    classification_rows = [classify_row(row) for row in text_nodes]
    summary = summarize_classifications(classification_rows, inventory["summary"])
    classifier_ready = summary["row_count_matches_inventory_summary"] and summary["no_write_row_count"] == len(
        classification_rows
    )
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aepx_redacted_text_classifier",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_aepx_redacted_text_inventory": str(inventory_path),
        "source_aepx_roundtrip_validator": str(roundtrip_validator_path),
        "source_inventory_state": inventory.get("inventory_state"),
        "source_inventory_ready": inventory.get("inventory_ready"),
        "source_roundtrip_state": roundtrip_validator.get("roundtrip_state"),
        "source_validator_ready": roundtrip_validator.get("validator_ready"),
        "classifier_state": (
            "aepx_redacted_text_classifier_ready_no_write"
            if classifier_ready
            else "aepx_redacted_text_classifier_failed"
        ),
        "classifier_ready": classifier_ready,
        "source_chain_valid": True,
        "inventory_rows_classified": classifier_ready,
        "row_count_matches_inventory_summary": summary["row_count_matches_inventory_summary"],
        "redacted_text_classification_ready": classifier_ready,
        "project_write_recommendation": "do_not_write_project_files",
        "project_write_ready": False,
        "project_write_allowed_now": False,
        "classifier_approves_project_write": False,
        "schema_write_allowed_now": False,
        "approved_write_candidate_count": summary["approved_write_candidate_count"],
        "classification_row_count": summary["classification_row_count"],
        "no_write_row_count": summary["no_write_row_count"],
        "unknown_row_count": summary["unknown_row_count"],
        "text_payload_exported": False,
        "text_payload_hash_exported": False,
        "bdata_payload_exported": False,
        "raw_text_fields_present": False,
        "value_hashes_emitted": False,
        "absolute_source_paths_in_classifier_rows": False,
        "raw_payload_serialized": False,
        "summary": summary,
        "classification_rows": classification_rows,
        "classifier_contract": {
            "state": "redacted_text_classifier_ready_no_write",
            "input_contract": [
                "read_redacted_text_inventory_json",
                "read_roundtrip_validator_json",
                "validate_inventory_row_schema",
                "classify_rows_from_metadata_only",
            ],
            "allowed_output_fields": [
                "row_id",
                "xml_path",
                "tag",
                "parent_tag",
                "text_length_bucket",
                "classification",
                "risk_level",
                "confidence_bucket",
                "write_recommendation",
                "required_reviews",
            ],
            "blocked_actions": [
                "open_aepx",
                "open_aep",
                "modify_aepx",
                "write_aepx",
                "write_aep",
                "start_after_effects",
                "load_aex",
                "route_ofx",
                "render_project",
                "export_text_payload_values",
                "export_text_payload_hashes",
                "export_bdata_payload_values",
                "emit_project_edit_schema",
            ],
        },
        "blockers": [
            "project_write_not_approved",
            "ae_host_validation_closed",
            "schema_review_required_for_all_text_classes",
            "bdata_schema_unknown",
            "attribute_values_redacted",
            "text_values_redacted",
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
            "Classifier reads redacted JSON evidence only and never dereferences source_aepx.",
            "Every row remains no-write; classifications are review buckets, not edit approval.",
            "Raw text values, hashes, attribute values, bdata values, project writes, AE, AEX, OFX, and render paths remain blocked.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Classify redacted AEPX text inventory rows")
    parser.add_argument(
        "--text-inventory",
        required=True,
        help="AEPX redacted text inventory JSON under target/aepx-redacted-text-inventory",
    )
    parser.add_argument(
        "--roundtrip-validator",
        required=True,
        help="AEPX round-trip validator JSON under target/aepx-roundtrip-validator",
    )
    parser.add_argument("--out", required=True, help="Create-new report under target/aepx-redacted-text-classifier")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    inventory, inventory_path = load_inventory(Path(args.text_inventory))
    roundtrip_validator, roundtrip_validator_path = load_roundtrip_validator(Path(args.roundtrip_validator))
    report = build_redacted_text_classifier(
        inventory=inventory,
        inventory_path=inventory_path,
        roundtrip_validator=roundtrip_validator,
        roundtrip_validator_path=roundtrip_validator_path,
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Build a redacted AEPX text-node inventory without writing project files.

The inventory reads a ready AEPX round-trip validator report, opens the source
AEPX as read-only XML, and records structure plus text length metadata only.
It never exports text values, text hashes, bdata values, writes AEPX/AEP files,
starts After Effects, loads AEX, routes OFX, or renders.
"""

from __future__ import annotations

import argparse
import json
import re
import xml.etree.ElementTree as ET
from collections import Counter, defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TOOLS_ROOT = LAB_ROOT.parent
TARGET_ROOT = LAB_ROOT / "target"
ROUNDTRIP_ROOT = TARGET_ROOT / "aepx-roundtrip-validator"
TEXT_INVENTORY_ROOT = TARGET_ROOT / "aepx-redacted-text-inventory"

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


def validate_roundtrip_input(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("AEPX round-trip validator input must have .json extension")
    return resolve_under_root(path, ROUNDTRIP_ROOT, must_exist=True)


def validate_aepx_source(path: Path) -> Path:
    if path.suffix.lower() != ".aepx":
        raise ValueError("source AEPX must have .aepx extension")
    return resolve_under_root(path, TOOLS_ROOT, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("AEPX redacted text inventory report must have .json extension")
    TEXT_INVENTORY_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, TEXT_INVENTORY_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(TEXT_INVENTORY_ROOT.resolve(strict=True)):
        raise ValueError(f"AEPX redacted text inventory parent must stay under {TEXT_INVENTORY_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_roundtrip_validator(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_roundtrip_input(path)
    return read_json_object(resolved), resolved


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
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
    if not isinstance(roundtrip.get("source_aepx"), str):
        errors.append("AEPX round-trip validator source_aepx must be a string")
    errors.extend(safety_errors(roundtrip, "AEPX round-trip validator"))
    return errors


def split_tag(tag: str) -> tuple[str | None, str]:
    if tag.startswith("{") and "}" in tag:
        namespace, local = tag[1:].split("}", 1)
        return namespace, local
    return None, tag


def local_attribute_name(attribute_name: str) -> str:
    _namespace, local = split_tag(attribute_name)
    return local


def text_length_bucket(length: int) -> str:
    if length == 0:
        return "empty"
    if length <= 4:
        return "1_4"
    if length <= 16:
        return "5_16"
    if length <= 64:
        return "17_64"
    return "65_plus"


def small_count_bucket(count: int) -> str:
    if count == 0:
        return "0"
    if count == 1:
        return "1"
    if count <= 4:
        return "2_4"
    return "5_plus"


def character_class_flags(text: str) -> dict[str, bool]:
    return {
        "has_ascii": any(ord(char) < 128 for char in text),
        "has_non_ascii": any(ord(char) >= 128 for char in text),
        "has_digits": any(char.isdigit() for char in text),
        "has_symbol": any(not char.isalnum() and not char.isspace() for char in text),
        "has_linebreak": any(char in "\r\n" for char in text),
    }


def sensitivity_flags(text: str) -> dict[str, bool]:
    compact = text.strip()
    return {
        "looks_like_path": bool(re.search(r"(^|[A-Za-z]:|[/\\])[^ \t\r\n]*[/\\][^ \t\r\n]+", compact)),
        "looks_like_url": bool(re.search(r"\b[a-zA-Z][a-zA-Z0-9+.-]{1,20}://", compact)),
        "looks_like_email": bool(re.search(r"\b[^@\s]+@[^@\s]+\.[^@\s]+\b", compact)),
        "looks_like_guid": bool(
            re.search(
                r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-"
                r"[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b",
                compact,
            )
        ),
        "looks_like_numeric_id": bool(re.fullmatch(r"[0-9]{4,}", compact)),
    }


def child_tag_counts(element: ET.Element) -> Counter[str]:
    counts: Counter[str] = Counter()
    for child in list(element):
        _namespace, local = split_tag(child.tag)
        counts[local] += 1
    return counts


def walk_text_nodes(
    element: ET.Element,
    *,
    depth: int,
    path_parts: list[str],
    parent_tag: str | None = None,
    sibling_index: int = 1,
) -> list[dict[str, Any]]:
    namespace, local_tag = split_tag(element.tag)
    text = element.text or ""
    stripped = text.strip()
    entries: list[dict[str, Any]] = []
    if stripped:
        attribute_keys = sorted(local_attribute_name(key) for key in element.attrib)
        trim_delta = len(text) - len(stripped)
        newline_count = text.count("\n") + text.count("\r")
        entries.append(
            {
                "xml_path": "/" + "/".join(path_parts),
                "parent_tag": parent_tag,
                "tag": local_tag,
                "namespace_present": namespace is not None,
                "namespace_id": "ns0" if namespace is not None else "none",
                "sibling_index": sibling_index,
                "depth": depth,
                "child_count": len(list(element)),
                "attribute_key_count": len(attribute_keys),
                "attribute_keys": attribute_keys,
                "contains_bdata_attribute": "bdata" in attribute_keys,
                "text_presence": "nonempty",
                "text_length_chars": len(stripped),
                "text_length_bucket": text_length_bucket(len(stripped)),
                "trim_delta_bucket": small_count_bucket(trim_delta),
                "newline_count_bucket": small_count_bucket(newline_count),
                "character_class_flags": character_class_flags(stripped),
                "sensitivity_flags": sensitivity_flags(stripped),
                "candidate_state": "text_label_candidate_pending_schema",
                "leading_or_trailing_whitespace": text != stripped,
                "payload_value_exported": False,
                "payload_hash_exported": False,
                "write_risk": "high_pending_schema_review",
            }
        )

    sibling_totals = child_tag_counts(element)
    sibling_seen: Counter[str] = Counter()
    for child in list(element):
        _child_namespace, child_tag = split_tag(child.tag)
        sibling_seen[child_tag] += 1
        token = f"{child_tag}[{sibling_seen[child_tag]}]"
        if sibling_totals[child_tag] == 1:
            token = child_tag
        entries.extend(
            walk_text_nodes(
                child,
                depth=depth + 1,
                path_parts=[*path_parts, token],
                parent_tag=local_tag,
                sibling_index=sibling_seen[child_tag],
            )
        )
    return entries


def summarize_entries(entries: list[dict[str, Any]]) -> dict[str, Any]:
    by_tag: dict[str, dict[str, Any]] = {}
    grouped: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for entry in entries:
        grouped[entry["tag"]].append(entry)

    for tag, tag_entries in sorted(grouped.items()):
        lengths = [entry["text_length_chars"] for entry in tag_entries]
        buckets = Counter(entry["text_length_bucket"] for entry in tag_entries)
        depths = sorted({entry["depth"] for entry in tag_entries})
        by_tag[tag] = {
            "count": len(tag_entries),
            "min_text_length_chars": min(lengths),
            "max_text_length_chars": max(lengths),
            "total_text_length_chars": sum(lengths),
            "length_buckets": dict(sorted(buckets.items())),
            "depths": depths,
            "example_paths": [entry["xml_path"] for entry in tag_entries[:5]],
        }

    total_chars = sum(entry["text_length_chars"] for entry in entries)
    buckets = Counter(entry["text_length_bucket"] for entry in entries)
    return {
        "text_node_count": len(entries),
        "unique_text_tag_count": len(by_tag),
        "total_text_length_chars": total_chars,
        "length_buckets": dict(sorted(buckets.items())),
        "max_text_length_chars": max((entry["text_length_chars"] for entry in entries), default=0),
        "max_depth": max((entry["depth"] for entry in entries), default=0),
        "by_tag": by_tag,
    }


def build_redacted_text_inventory(
    *,
    roundtrip_validator: dict[str, Any],
    roundtrip_validator_path: Path,
) -> dict[str, Any]:
    errors = validate_roundtrip(roundtrip_validator)
    if errors:
        raise ValueError("; ".join(errors))

    source_aepx = validate_aepx_source(Path(roundtrip_validator["source_aepx"]))
    root = ET.parse(source_aepx).getroot()
    _root_namespace, root_tag = split_tag(root.tag)
    entries = walk_text_nodes(root, depth=0, path_parts=[root_tag])
    for index, entry in enumerate(entries):
        entry["inventory_index"] = index
        entry["row_id"] = f"text_{index:04d}"
        entry["element_ordinal"] = index
    summary = summarize_entries(entries)
    inventory_ready = True
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aepx_redacted_text_inventory",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_aepx_roundtrip_validator": str(roundtrip_validator_path),
        "source_aepx": str(source_aepx),
        "inventory_state": (
            "aepx_redacted_text_inventory_ready_no_write"
            if inventory_ready
            else "aepx_redacted_text_inventory_failed"
        ),
        "inventory_ready": inventory_ready,
        "source_roundtrip_state": roundtrip_validator.get("roundtrip_state"),
        "source_validator_ready": roundtrip_validator.get("validator_ready"),
        "source_structure_match": roundtrip_validator.get("source_structure_match"),
        "roundtrip_structure_match": roundtrip_validator.get("roundtrip_structure_match"),
        "aepx_xml_parsed": True,
        "text_payload_exported": False,
        "text_payload_hash_exported": False,
        "bdata_payload_exported": False,
        "raw_text_fields_present": False,
        "value_hashes_emitted": False,
        "absolute_source_paths_in_inventory_rows": False,
        "raw_payload_serialized": False,
        "roundtrip_xml_serialized_to_disk": False,
        "summary": summary,
        "text_nodes": entries,
        "redaction_policy": {
            "state": "text_inventory_redacted_metadata_only",
            "allowed_fields": [
                "xml_path",
                "row_id",
                "element_ordinal",
                "parent_tag",
                "tag",
                "namespace_present",
                "namespace_id",
                "sibling_index",
                "depth",
                "child_count",
                "attribute_key_count",
                "attribute_keys",
                "contains_bdata_attribute",
                "text_presence",
                "text_length_chars",
                "text_length_bucket",
                "trim_delta_bucket",
                "newline_count_bucket",
                "character_class_flags",
                "sensitivity_flags",
                "candidate_state",
                "leading_or_trailing_whitespace",
            ],
            "forbidden_fields": [
                "raw_text",
                "trimmed_text",
                "text_prefix",
                "text_suffix",
                "text_hash",
                "bdata_value",
                "attribute_values",
                "absolute_source_path_in_rows",
            ],
        },
        "inventory_contract": {
            "state": "redacted_text_inventory_ready_no_write",
            "allowed_current_actions": [
                "read_roundtrip_validator_json",
                "read_source_aepx_as_xml",
                "emit_text_node_structure_metadata",
                "emit_text_length_buckets",
            ],
            "blocked_actions": [
                "modify_aepx",
                "write_aepx",
                "write_aep",
                "start_after_effects",
                "load_aex",
                "render_project",
                "export_text_payload_values",
                "export_text_payload_hashes",
                "export_bdata_payload_values",
                "export_attribute_values",
            ],
        },
        "blockers": [
            "project_write_not_approved",
            "ae_host_validation_closed",
            "text_semantics_unknown",
            "bdata_schema_unknown",
            "attribute_values_redacted",
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
            "Inventory parses the source AEPX as read-only XML after round-trip validation.",
            "Raw text values, text hashes, bdata values, and attribute values are not exported.",
            "AEPX/AEP project writes and AE/AEX/OFX/render paths remain blocked.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build redacted AEPX text-node inventory report")
    parser.add_argument(
        "--roundtrip-validator",
        required=True,
        help="AEPX round-trip validator JSON under target/aepx-roundtrip-validator",
    )
    parser.add_argument("--out", required=True, help="Create-new report under target/aepx-redacted-text-inventory")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    roundtrip_validator, roundtrip_validator_path = load_roundtrip_validator(Path(args.roundtrip_validator))
    report = build_redacted_text_inventory(
        roundtrip_validator=roundtrip_validator,
        roundtrip_validator_path=roundtrip_validator_path,
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

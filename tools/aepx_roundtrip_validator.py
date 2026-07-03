#!/usr/bin/env python3
"""Validate AEPX XML round-trip structure without writing project files.

The validator reads the AEPX static probe and no-write edit plan JSON, then
parses the source AEPX path recorded by the probe. It serializes XML in memory,
reparses that in-memory bytes object, and compares structure/count metadata.
It never writes AEPX/AEP files, starts After Effects, loads AEX, or renders.
"""

from __future__ import annotations

import argparse
import json
import xml.etree.ElementTree as ET
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TOOLS_ROOT = LAB_ROOT.parent
TARGET_ROOT = LAB_ROOT / "target"
AEPX_PROBE_ROOT = TARGET_ROOT / "aepx-static-probe"
AEPX_EDIT_PLAN_ROOT = TARGET_ROOT / "aepx-edit-plan"
ROUNDTRIP_ROOT = TARGET_ROOT / "aepx-roundtrip-validator"

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


def validate_json_input(path: Path, root: Path, label: str) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError(f"{label} must have .json extension")
    return resolve_under_root(path, root, must_exist=True)


def validate_aepx_source(path: Path) -> Path:
    if path.suffix.lower() != ".aepx":
        raise ValueError("source AEPX must have .aepx extension")
    return resolve_under_root(path, TOOLS_ROOT, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("AEPX round-trip validator report must have .json extension")
    ROUNDTRIP_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, ROUNDTRIP_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(ROUNDTRIP_ROOT.resolve(strict=True)):
        raise ValueError(f"AEPX round-trip validator parent must stay under {ROUNDTRIP_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_aepx_probe(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, AEPX_PROBE_ROOT, "AEPX static probe")
    return read_json_object(resolved), resolved


def load_aepx_edit_plan(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, AEPX_EDIT_PLAN_ROOT, "AEPX edit plan")
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


def validate_probe(probe: dict[str, Any]) -> list[str]:
    errors = require_local_only(probe, "AEPX probe")
    if probe.get("report_kind") != "aepx_static_probe":
        errors.append("AEPX probe report_kind must be aepx_static_probe")
    if probe.get("probe_state") != "aepx_static_probe_ready_no_write":
        errors.append("AEPX probe probe_state must be aepx_static_probe_ready_no_write")
    if probe.get("xml_parse_state") != "parsed":
        errors.append("AEPX probe xml_parse_state must be parsed")
    if not isinstance(probe.get("summary"), dict):
        errors.append("AEPX probe summary must be an object")
    if not isinstance(probe.get("root"), dict):
        errors.append("AEPX probe root must be an object")
    if not isinstance(probe.get("source_aepx"), str):
        errors.append("AEPX probe source_aepx must be a string")
    errors.extend(safety_errors(probe, "AEPX probe"))
    return errors


def validate_edit_plan(plan: dict[str, Any], probe_path: Path) -> list[str]:
    errors = require_local_only(plan, "AEPX edit plan")
    if plan.get("packet_kind") != "aepx_edit_plan_packet":
        errors.append("AEPX edit plan packet_kind must be aepx_edit_plan_packet")
    if plan.get("edit_plan_state") != "aepx_edit_plan_ready_no_write":
        errors.append("AEPX edit plan edit_plan_state must be aepx_edit_plan_ready_no_write")
    if plan.get("write_recommendation") != "do_not_write_project_files":
        errors.append("AEPX edit plan write_recommendation must be do_not_write_project_files")
    if not isinstance(plan.get("review_blockers"), list):
        errors.append("AEPX edit plan review_blockers must be a list")
    else:
        blocker_ids = {item.get("blocker_id") for item in plan["review_blockers"] if isinstance(item, dict)}
        if "round_trip_validator_missing" not in blocker_ids:
            errors.append("AEPX edit plan must record round_trip_validator_missing blocker")
    source_probe = plan.get("source_aepx_probe")
    if isinstance(source_probe, str) and Path(source_probe).resolve() != probe_path.resolve():
        errors.append("AEPX edit plan source_aepx_probe must match probe input")
    errors.extend(safety_errors(plan, "AEPX edit plan"))
    return errors


def split_tag(tag: str) -> tuple[str | None, str]:
    if tag.startswith("{") and "}" in tag:
        namespace, local = tag[1:].split("}", 1)
        return namespace, local
    return None, tag


def parse_hex_bdata_size(value: str) -> int | None:
    text = value.strip()
    if not text:
        return 0
    if any(ch not in "0123456789abcdefABCDEF" for ch in text):
        return None
    if len(text) % 2:
        return None
    return len(text) // 2


def iter_elements_with_depth(root: ET.Element) -> list[tuple[ET.Element, int]]:
    result: list[tuple[ET.Element, int]] = []
    stack: list[tuple[ET.Element, int]] = [(root, 0)]
    while stack:
        element, depth = stack.pop()
        result.append((element, depth))
        for child in reversed(list(element)):
            stack.append((child, depth + 1))
    return result


def structure_signature(root: ET.Element) -> dict[str, Any]:
    elements = iter_elements_with_depth(root)
    namespace_counts: Counter[str] = Counter()
    tag_counts: Counter[str] = Counter()
    attribute_counts: Counter[str] = Counter()
    bdata_counts: Counter[str] = Counter()
    text_counts: Counter[str] = Counter()
    bdata_total_bytes = 0
    bdata_invalid_count = 0
    text_total_chars = 0
    max_depth = 0
    max_attribute_count = 0

    for element, depth in elements:
        namespace, local_tag = split_tag(element.tag)
        tag_counts[local_tag] += 1
        if namespace:
            namespace_counts[namespace] += 1
        max_depth = max(max_depth, depth)
        max_attribute_count = max(max_attribute_count, len(element.attrib))
        attribute_counts[local_tag] += len(element.attrib)
        bdata = element.attrib.get("bdata")
        if bdata is not None:
            bdata_counts[local_tag] += 1
            size = parse_hex_bdata_size(bdata)
            if size is None:
                bdata_invalid_count += 1
            else:
                bdata_total_bytes += size
        text = element.text or ""
        if text.strip():
            text_counts[local_tag] += 1
            text_total_chars += len(text.strip())

    root_namespace, root_tag = split_tag(root.tag)
    return {
        "root_tag": root_tag,
        "root_namespace": root_namespace,
        "element_count": len(elements),
        "unique_tag_count": len(tag_counts),
        "namespace_count": len(namespace_counts),
        "max_depth": max_depth,
        "max_attribute_count": max_attribute_count,
        "bdata_attribute_count": sum(bdata_counts.values()),
        "bdata_total_decoded_bytes_if_hex": bdata_total_bytes,
        "bdata_invalid_hex_count": bdata_invalid_count,
        "nonempty_text_node_count": sum(text_counts.values()),
        "nonempty_text_total_chars": text_total_chars,
        "tag_counts": dict(sorted(tag_counts.items())),
        "namespace_counts": dict(sorted(namespace_counts.items())),
    }


def probe_signature(probe: dict[str, Any]) -> dict[str, Any]:
    summary = probe.get("summary", {}) if isinstance(probe.get("summary"), dict) else {}
    root = probe.get("root", {}) if isinstance(probe.get("root"), dict) else {}
    return {
        "root_tag": root.get("tag"),
        "root_namespace": root.get("namespace"),
        "element_count": summary.get("element_count"),
        "unique_tag_count": summary.get("unique_tag_count"),
        "namespace_count": summary.get("namespace_count"),
        "max_depth": summary.get("max_depth"),
        "max_attribute_count": summary.get("max_attribute_count"),
        "bdata_attribute_count": summary.get("bdata_attribute_count"),
        "bdata_total_decoded_bytes_if_hex": summary.get("bdata_total_decoded_bytes_if_hex"),
        "bdata_invalid_hex_count": summary.get("bdata_invalid_hex_count"),
        "nonempty_text_node_count": summary.get("nonempty_text_node_count"),
        "nonempty_text_total_chars": summary.get("nonempty_text_total_chars"),
    }


def comparable_signature(signature: dict[str, Any]) -> dict[str, Any]:
    return {
        key: signature.get(key)
        for key in (
            "root_tag",
            "root_namespace",
            "element_count",
            "unique_tag_count",
            "namespace_count",
            "max_depth",
            "max_attribute_count",
            "bdata_attribute_count",
            "bdata_total_decoded_bytes_if_hex",
            "bdata_invalid_hex_count",
            "nonempty_text_node_count",
            "nonempty_text_total_chars",
        )
    }


def compare_signatures(left: dict[str, Any], right: dict[str, Any]) -> dict[str, Any]:
    mismatches = []
    for key in sorted(set(left) | set(right)):
        if left.get(key) != right.get(key):
            mismatches.append({"field": key, "left": left.get(key), "right": right.get(key)})
    return {"match": not mismatches, "mismatch_count": len(mismatches), "mismatches": mismatches[:20]}


def build_roundtrip_validator(
    *,
    aepx_probe: dict[str, Any],
    aepx_probe_path: Path,
    aepx_edit_plan: dict[str, Any],
    aepx_edit_plan_path: Path,
) -> dict[str, Any]:
    errors = validate_probe(aepx_probe) + validate_edit_plan(aepx_edit_plan, aepx_probe_path)
    if errors:
        raise ValueError("; ".join(errors))

    source_aepx = validate_aepx_source(Path(aepx_probe["source_aepx"]))
    tree = ET.parse(source_aepx)
    source_root = tree.getroot()
    source_signature = structure_signature(source_root)
    serialized = ET.tostring(source_root, encoding="utf-8")
    reparsed_root = ET.fromstring(serialized)
    roundtrip_signature = structure_signature(reparsed_root)
    probe_compare = compare_signatures(probe_signature(aepx_probe), comparable_signature(source_signature))
    roundtrip_compare = compare_signatures(
        comparable_signature(source_signature),
        comparable_signature(roundtrip_signature),
    )
    validator_ready = probe_compare["match"] and roundtrip_compare["match"]
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aepx_roundtrip_validator",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_aepx_probe": str(aepx_probe_path),
        "source_aepx_edit_plan": str(aepx_edit_plan_path),
        "source_aepx": str(source_aepx),
        "roundtrip_state": (
            "aepx_roundtrip_validator_ready_no_write" if validator_ready else "aepx_roundtrip_validator_failed"
        ),
        "validator_ready": validator_ready,
        "source_structure_match": probe_compare["match"],
        "roundtrip_structure_match": roundtrip_compare["match"],
        "roundtrip_xml_serialized_to_memory": True,
        "roundtrip_xml_serialized_to_disk": False,
        "text_payload_exported": False,
        "bdata_payload_exported": False,
        "source_probe_compare": probe_compare,
        "roundtrip_compare": roundtrip_compare,
        "structure_summary": comparable_signature(source_signature),
        "roundtrip_contract": {
            "state": "structure_roundtrip_ready_no_write" if validator_ready else "structure_roundtrip_failed",
            "allowed_current_actions": [
                "in_memory_xml_parse",
                "in_memory_xml_serialize",
                "in_memory_xml_reparse",
                "metadata_only_structure_compare",
            ],
            "blocked_actions": [
                "modify_aepx",
                "write_aepx",
                "write_aep",
                "start_after_effects",
                "load_aex",
                "render_project",
                "export_text_payload_values",
                "export_bdata_payload_values",
            ],
        },
        "blockers": [
            "project_write_not_approved",
            "ae_host_validation_closed",
            "binary_bdata_schema_unknown",
            "text_payload_redaction_not_approved",
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
            "Validator parses the source AEPX and serializes XML in memory only.",
            "No AEPX/AEP file is modified or written.",
            "Text and bdata payload values are not exported in the report.",
            "After Effects is not started and no AEX/OFX/render path is invoked.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build no-write AEPX round-trip validator report")
    parser.add_argument("--aepx-probe", required=True, help="AEPX static probe JSON under target/aepx-static-probe")
    parser.add_argument("--aepx-edit-plan", required=True, help="AEPX edit plan JSON under target/aepx-edit-plan")
    parser.add_argument("--out", required=True, help="Create-new validator report under target/aepx-roundtrip-validator")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    aepx_probe, aepx_probe_path = load_aepx_probe(Path(args.aepx_probe))
    aepx_edit_plan, aepx_edit_plan_path = load_aepx_edit_plan(Path(args.aepx_edit_plan))
    report = build_roundtrip_validator(
        aepx_probe=aepx_probe,
        aepx_probe_path=aepx_probe_path,
        aepx_edit_plan=aepx_edit_plan,
        aepx_edit_plan_path=aepx_edit_plan_path,
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

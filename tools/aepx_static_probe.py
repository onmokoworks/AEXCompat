#!/usr/bin/env python3
"""Static AEPX/XML project probe for AE project editability groundwork.

This reads `.aepx` XML files as metadata only. It never modifies project files,
starts After Effects, loads AEX plug-ins, renders, or routes OFX.
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

MAX_RECORDED_TAGS = 40


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


def validate_aepx_input_path(path: Path) -> Path:
    if path.suffix.lower() != ".aepx":
        raise ValueError("AEPX input must have .aepx extension")
    return resolve_under_root(path, TOOLS_ROOT, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("AEPX probe output must have .json extension")
    AEPX_PROBE_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, AEPX_PROBE_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(AEPX_PROBE_ROOT.resolve(strict=True)):
        raise ValueError(f"AEPX probe parent must stay under {AEPX_PROBE_ROOT}")
    return resolved


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
        children = list(element)
        for child in reversed(children):
            stack.append((child, depth + 1))
    return result


def tag_rows(
    tag_counts: Counter[str],
    attribute_counts: Counter[str],
    bdata_counts: Counter[str],
    text_counts: Counter[str],
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for tag, count in tag_counts.most_common(MAX_RECORDED_TAGS):
        rows.append(
            {
                "tag": tag,
                "count": count,
                "attribute_count": attribute_counts.get(tag, 0),
                "bdata_attribute_count": bdata_counts.get(tag, 0),
                "nonempty_text_count": text_counts.get(tag, 0),
            }
        )
    return rows


def build_aepx_probe(path: Path) -> dict[str, Any]:
    source_path = validate_aepx_input_path(path)
    try:
        tree = ET.parse(source_path)
    except ET.ParseError as exc:
        raise ValueError(f"AEPX XML parse failed: {exc}") from exc
    root = tree.getroot()
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
    source_size = source_path.stat().st_size
    root_attributes = {
        key: value
        for key, value in root.attrib.items()
        if key in {"majorVersion", "minorVersion"}
    }
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aepx_static_probe",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_aepx": str(source_path),
        "source_size_bytes": source_size,
        "probe_state": "aepx_static_probe_ready_no_write",
        "xml_parse_state": "parsed",
        "edit_readiness_state": "xml_static_edit_candidate_pending_schema_review",
        "root": {
            "tag": root_tag,
            "namespace": root_namespace,
            "attributes": root_attributes,
        },
        "summary": {
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
        },
        "namespace_counts": dict(sorted(namespace_counts.items())),
        "top_tags": tag_rows(tag_counts, attribute_counts, bdata_counts, text_counts),
        "edit_surface": {
            "aepx_xml_parseable": True,
            "aep_binary_editable_by_this_tool": False,
            "aepx_write_approved": False,
            "text_payload_exported": False,
            "safe_next_actions": [
                "schema-aware XML read-only mapping",
                "small redacted edit-plan packet",
                "round-trip validation design before any write tool",
            ],
            "blocked_actions": [
                "modify_aepx",
                "write_aep",
                "start_after_effects",
                "load_aex",
                "render_project",
            ],
        },
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
            "Probe parses AEPX XML as metadata only.",
            "String and CDATA payloads are counted by length only and are not exported.",
            "No project file is modified and After Effects is not started.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build local no-write AEPX static probe report")
    parser.add_argument("--input", required=True, help="AEPX file under D:\\Projects\\01_Project\\04_Tools")
    parser.add_argument("--out", required=True, help="Create-new probe JSON under target/aepx-static-probe")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    report = build_aepx_probe(Path(args.input))
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""No-load static metadata probe for After Effects .aex plug-ins.

The probe reads bytes as ordinary files and summarizes PE/resource metadata.
It never loads a DLL/AEX, calls entrypoints, starts After Effects, or renders.
"""

from __future__ import annotations

import argparse
import json
import struct
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable


LAB_ROOT = Path(__file__).resolve().parents[1]
REPORT_ROOT = LAB_ROOT / "target" / "aex-static-probe"
MAX_SCAN_BYTES = 256 * 1024 * 1024

MACHINE_LABELS = {
    0x014C: "x86",
    0x8664: "x64",
    0xAA64: "arm64",
}

SUBSYSTEM_LABELS = {
    2: "windows_gui",
    3: "windows_cui",
}

PE_CHARACTERISTICS = {
    "executable_image": 0x0002,
    "large_address_aware": 0x0020,
    "dll": 0x2000,
}

MARKERS = {
    "pipl_ascii_marker_count": b"PiPL",
    "effect_main_marker_count": b"EffectMain",
    "ae_effect_marker_count": b"AE_Effect",
    "pf_cmd_marker_count": b"PF_Cmd",
    "ae_plugin_marker_count": b"AEGP",
}


def read_u16(data: bytes, offset: int) -> int | None:
    if offset < 0 or offset + 2 > len(data):
        return None
    return struct.unpack_from("<H", data, offset)[0]


def read_u32(data: bytes, offset: int) -> int | None:
    if offset < 0 or offset + 4 > len(data):
        return None
    return struct.unpack_from("<I", data, offset)[0]


def read_c_string(data: bytes, offset: int, max_length: int = 512) -> str | None:
    if offset < 0 or offset >= len(data):
        return None
    end_limit = min(len(data), offset + max_length)
    end = data.find(b"\x00", offset, end_limit)
    if end < 0:
        return None
    raw = data[offset:end]
    try:
        return raw.decode("ascii", errors="strict")
    except UnicodeDecodeError:
        return raw.decode("ascii", errors="replace")


def read_utf16_resource_name(data: bytes, resource_base: int, name_offset: int) -> str | None:
    length = read_u16(data, resource_base + name_offset)
    if length is None:
        return None
    start = resource_base + name_offset + 2
    end = start + length * 2
    if end > len(data):
        return None
    try:
        return data[start:end].decode("utf-16le", errors="strict")
    except UnicodeDecodeError:
        return None


def rva_to_offset(rva: int, sections: list[dict[str, Any]]) -> int | None:
    for section in sections:
        va = section["virtual_address"]
        size = max(section["virtual_size"], section["raw_size"])
        if va <= rva < va + size:
            return section["raw_pointer"] + (rva - va)
    return None


def parse_sections(data: bytes, section_table: int, count: int) -> list[dict[str, Any]]:
    sections: list[dict[str, Any]] = []
    for index in range(count):
        offset = section_table + index * 40
        if offset + 40 > len(data):
            break
        raw_name = data[offset : offset + 8].split(b"\x00", 1)[0]
        name = raw_name.decode("ascii", errors="replace")
        virtual_size = read_u32(data, offset + 8) or 0
        virtual_address = read_u32(data, offset + 12) or 0
        raw_size = read_u32(data, offset + 16) or 0
        raw_pointer = read_u32(data, offset + 20) or 0
        sections.append(
            {
                "name": name,
                "virtual_size": virtual_size,
                "virtual_address": virtual_address,
                "raw_size": raw_size,
                "raw_pointer": raw_pointer,
            }
        )
    return sections


def parse_resource_tree(data: bytes, resource_base: int) -> dict[str, Any]:
    summary: dict[str, Any] = {
        "resource_dir_present": False,
        "type_count": 0,
        "type_names": [],
        "type_ids": [],
        "type_details": [],
        "resource_entries": [],
        "pipl_resource_type_present": False,
        "pipl_resource_data_entry_count": 0,
        "pipl_resource_entries": [],
        "pipl_resource_total_size": 0,
        "resource_data_entry_count": 0,
        "resource_parse_truncated": False,
    }
    if resource_base < 0 or resource_base + 16 > len(data):
        return summary

    summary["resource_dir_present"] = True
    seen_entries: set[int] = set()
    type_names: list[str] = []
    type_ids: list[int] = []
    type_entry_counts: dict[str, int] = {}
    resource_entries: list[dict[str, Any]] = []
    data_entry_count = 0
    truncated = False

    def type_label(name_value: str | int | None) -> str:
        if isinstance(name_value, str):
            return name_value
        if isinstance(name_value, int):
            return f"#{name_value}"
        return "#unknown"

    def value_for_report(name_value: str | int | None) -> str | int | None:
        if isinstance(name_value, (str, int)):
            return name_value
        return None

    def walk_directory(relative_offset: int, depth: int, path_values: list[str | int | None]) -> None:
        nonlocal data_entry_count, truncated
        if depth > 4:
            truncated = True
            return
        directory = resource_base + relative_offset
        if directory + 16 > len(data):
            truncated = True
            return
        named_count = read_u16(data, directory + 12) or 0
        id_count = read_u16(data, directory + 14) or 0
        total = named_count + id_count
        entries_start = directory + 16
        for i in range(total):
            entry = entries_start + i * 8
            if entry + 8 > len(data):
                truncated = True
                return
            name_raw = read_u32(data, entry) or 0
            offset_raw = read_u32(data, entry + 4) or 0
            is_named = bool(name_raw & 0x80000000)
            is_directory = bool(offset_raw & 0x80000000)
            name_value: str | int | None
            if is_named:
                name_value = read_utf16_resource_name(data, resource_base, name_raw & 0x7FFFFFFF)
            else:
                name_value = name_raw & 0xFFFF
            if depth == 0:
                if isinstance(name_value, str):
                    type_names.append(name_value)
                elif isinstance(name_value, int):
                    type_ids.append(name_value)
            next_path = path_values + [name_value]
            current_type = type_label(next_path[0]) if next_path else "#unknown"
            target = offset_raw & 0x7FFFFFFF
            absolute_target = resource_base + target
            if absolute_target in seen_entries:
                truncated = True
                continue
            seen_entries.add(absolute_target)
            if is_directory:
                walk_directory(target, depth + 1, next_path)
            else:
                if absolute_target + 16 <= len(data):
                    data_entry_count += 1
                    label = current_type
                    type_entry_counts[label] = type_entry_counts.get(label, 0) + 1
                    data_rva = read_u32(data, absolute_target)
                    data_size = read_u32(data, absolute_target + 4)
                    codepage = read_u32(data, absolute_target + 8)
                    reserved = read_u32(data, absolute_target + 12)
                    if data_rva is None or data_size is None or codepage is None or reserved is None:
                        truncated = True
                        continue
                    resource_entries.append(
                        {
                            "type": label,
                            "name": value_for_report(next_path[1]) if len(next_path) > 1 else None,
                            "language": value_for_report(next_path[2]) if len(next_path) > 2 else None,
                            "data_rva": data_rva,
                            "size_bytes": data_size,
                            "codepage": codepage,
                            "reserved": reserved,
                        }
                    )
                else:
                    truncated = True

    walk_directory(0, 0, [])
    type_names = sorted(set(type_names))
    type_ids = sorted(set(type_ids))
    type_details = [
        {"type": name, "entry_count": count}
        for name, count in sorted(type_entry_counts.items(), key=lambda item: item[0].lower())
    ]
    pipl_entries = [entry for entry in resource_entries if str(entry.get("type", "")).lower() == "pipl"]
    pipl_entry_count = len(pipl_entries)
    summary.update(
        {
            "type_count": len(type_names) + len(type_ids),
            "type_names": type_names[:32],
            "type_ids": type_ids[:64],
            "type_details": type_details[:64],
            "resource_entries": resource_entries[:256],
            "pipl_resource_type_present": any(name.lower() == "pipl" for name in type_names),
            "pipl_resource_data_entry_count": pipl_entry_count,
            "pipl_resource_entries": pipl_entries[:64],
            "pipl_resource_total_size": sum(int(entry.get("size_bytes") or 0) for entry in pipl_entries),
            "resource_data_entry_count": data_entry_count,
            "resource_parse_truncated": truncated or len(resource_entries) > 256 or len(pipl_entries) > 64,
        }
    )
    return summary


def parse_export_directory(data: bytes, export_rva: int, sections: list[dict[str, Any]]) -> dict[str, Any]:
    summary: dict[str, Any] = {
        "export_dir_present": False,
        "dll_name": None,
        "exported_function_count": 0,
        "exported_name_count": 0,
        "exported_names": [],
        "effect_main_export_present": False,
        "export_parse_truncated": False,
    }
    if not export_rva:
        return summary
    export_offset = rva_to_offset(export_rva, sections)
    if export_offset is None or export_offset + 40 > len(data):
        summary["export_parse_truncated"] = True
        return summary

    summary["export_dir_present"] = True
    name_rva = read_u32(data, export_offset + 12) or 0
    function_count = read_u32(data, export_offset + 20) or 0
    name_count = read_u32(data, export_offset + 24) or 0
    names_rva = read_u32(data, export_offset + 32) or 0
    dll_name_offset = rva_to_offset(name_rva, sections) if name_rva else None
    if dll_name_offset is not None:
        summary["dll_name"] = read_c_string(data, dll_name_offset)
    elif name_rva:
        summary["export_parse_truncated"] = True

    exported_names: list[str] = []
    names_offset = rva_to_offset(names_rva, sections) if names_rva else None
    if name_count and names_offset is None:
        summary["export_parse_truncated"] = True
    elif names_offset is not None:
        for index in range(min(name_count, 256)):
            name_ptr = names_offset + index * 4
            name_string_rva = read_u32(data, name_ptr)
            if name_string_rva is None:
                summary["export_parse_truncated"] = True
                break
            name_string_offset = rva_to_offset(name_string_rva, sections)
            if name_string_offset is None:
                summary["export_parse_truncated"] = True
                continue
            exported_name = read_c_string(data, name_string_offset)
            if exported_name is None:
                summary["export_parse_truncated"] = True
                continue
            exported_names.append(exported_name)
        if name_count > 256:
            summary["export_parse_truncated"] = True

    summary.update(
        {
            "exported_function_count": function_count,
            "exported_name_count": name_count,
            "exported_names": exported_names[:64],
            "effect_main_export_present": any(name == "EffectMain" for name in exported_names),
        }
    )
    return summary


def parse_import_directory(data: bytes, import_rva: int, sections: list[dict[str, Any]]) -> dict[str, Any]:
    summary: dict[str, Any] = {
        "import_dir_present": False,
        "import_descriptor_count": 0,
        "dll_count": 0,
        "dll_names": [],
        "import_parse_truncated": False,
    }
    if not import_rva:
        return summary
    import_offset = rva_to_offset(import_rva, sections)
    if import_offset is None or import_offset + 20 > len(data):
        summary["import_parse_truncated"] = True
        return summary

    summary["import_dir_present"] = True
    dll_names: list[str] = []
    descriptor_count = 0
    for index in range(512):
        descriptor = import_offset + index * 20
        if descriptor + 20 > len(data):
            summary["import_parse_truncated"] = True
            break
        fields = struct.unpack_from("<IIIII", data, descriptor)
        if fields == (0, 0, 0, 0, 0):
            break
        descriptor_count += 1
        name_rva = fields[3]
        name_offset = rva_to_offset(name_rva, sections) if name_rva else None
        if name_offset is None:
            summary["import_parse_truncated"] = True
            continue
        dll_name = read_c_string(data, name_offset, max_length=260)
        if dll_name is None:
            summary["import_parse_truncated"] = True
            continue
        dll_names.append(dll_name)
    else:
        summary["import_parse_truncated"] = True

    unique_names = sorted(set(dll_names), key=str.lower)
    summary.update(
        {
            "import_descriptor_count": descriptor_count,
            "dll_count": len(unique_names),
            "dll_names": unique_names[:64],
        }
    )
    return summary


def data_directory(data: bytes, optional: int, optional_header_size: int, magic: int | None, index: int) -> tuple[int, int]:
    data_directory_start = optional + (112 if magic == 0x20B else 96)
    offset = data_directory_start + 8 * index
    if offset + 8 > optional + optional_header_size or offset + 8 > len(data):
        return (0, 0)
    return (read_u32(data, offset) or 0, read_u32(data, offset + 4) or 0)


def pe_characteristic_flags(characteristics: int | None) -> dict[str, bool]:
    if characteristics is None:
        return {name: False for name in PE_CHARACTERISTICS}
    return {name: bool(characteristics & mask) for name, mask in PE_CHARACTERISTICS.items()}


def parse_pe(data: bytes) -> dict[str, Any]:
    pe: dict[str, Any] = {
        "mz_header_present": data.startswith(b"MZ"),
        "pe_valid": False,
        "machine": None,
        "machine_label": None,
        "section_count": 0,
        "section_names": [],
        "time_date_stamp": None,
        "characteristics_hex": None,
        "characteristics_flags": pe_characteristic_flags(None),
        "optional_header_magic": None,
        "subsystem": None,
        "subsystem_label": None,
        "entry_point_rva": None,
        "export_rva": 0,
        "export_size": 0,
        "import_rva": 0,
        "import_size": 0,
        "resource_rva": 0,
        "resource_size": 0,
        "export_summary": parse_export_directory(b"", 0, []),
        "import_summary": parse_import_directory(b"", 0, []),
        "resource_summary": parse_resource_tree(b"", 0),
    }
    if len(data) < 0x40 or not data.startswith(b"MZ"):
        return pe
    pe_offset = read_u32(data, 0x3C)
    if pe_offset is None or pe_offset + 24 > len(data):
        return pe
    if data[pe_offset : pe_offset + 4] != b"PE\x00\x00":
        return pe
    coff = pe_offset + 4
    machine = read_u16(data, coff)
    section_count = read_u16(data, coff + 2) or 0
    time_date_stamp = read_u32(data, coff + 4)
    optional_header_size = read_u16(data, coff + 16) or 0
    characteristics = read_u16(data, coff + 18)
    optional = coff + 20
    if optional + optional_header_size > len(data):
        return pe
    magic = read_u16(data, optional)
    entry_point = read_u32(data, optional + 16)
    subsystem = read_u16(data, optional + 68)
    export_rva, export_size = data_directory(data, optional, optional_header_size, magic, 0)
    import_rva, import_size = data_directory(data, optional, optional_header_size, magic, 1)
    resource_rva = 0
    resource_size = 0
    resource_rva, resource_size = data_directory(data, optional, optional_header_size, magic, 2)
    section_table = optional + optional_header_size
    sections = parse_sections(data, section_table, section_count)
    export_summary = parse_export_directory(data, export_rva, sections)
    import_summary = parse_import_directory(data, import_rva, sections)
    resource_summary = parse_resource_tree(b"", 0)
    if resource_rva:
        resource_offset = rva_to_offset(resource_rva, sections)
        if resource_offset is not None:
            resource_summary = parse_resource_tree(data, resource_offset)
    pe.update(
        {
            "pe_valid": True,
            "machine": machine,
            "machine_label": MACHINE_LABELS.get(machine, "unknown"),
            "section_count": len(sections),
            "section_names": [section["name"] for section in sections[:32]],
            "time_date_stamp": time_date_stamp,
            "characteristics_hex": f"0x{characteristics:04x}" if characteristics is not None else None,
            "characteristics_flags": pe_characteristic_flags(characteristics),
            "optional_header_magic": f"0x{magic:04x}" if magic is not None else None,
            "subsystem": subsystem,
            "subsystem_label": SUBSYSTEM_LABELS.get(subsystem, "unknown"),
            "entry_point_rva": entry_point,
            "export_rva": export_rva,
            "export_size": export_size,
            "import_rva": import_rva,
            "import_size": import_size,
            "resource_rva": resource_rva,
            "resource_size": resource_size,
            "export_summary": export_summary,
            "import_summary": import_summary,
            "resource_summary": resource_summary,
        }
    )
    return pe


def marker_summary(data: bytes) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for name, marker in MARKERS.items():
        result[name] = data.count(marker)
    result["effect_main_marker_present"] = result["effect_main_marker_count"] > 0
    result["pipl_ascii_marker_present"] = result["pipl_ascii_marker_count"] > 0
    return result


def classify_entry(entry: dict[str, Any]) -> dict[str, Any]:
    pe = entry.get("pe", {})
    markers = entry.get("markers", {})
    export_summary = pe.get("export_summary", {})
    has_pipl = bool(entry.get("pipl_signal_present"))
    has_effect_main_export = bool(export_summary.get("effect_main_export_present"))
    has_effect_main_marker = bool(markers.get("effect_main_marker_present"))
    aegp_marker_count = int(markers.get("ae_plugin_marker_count") or 0)
    size_bytes = int(entry.get("size_bytes") or 0)

    score = 0
    reasons: list[str] = []
    if pe.get("pe_valid"):
        score += 10
        reasons.append("pe-valid")
    if pe.get("machine_label") == "x64":
        score += 5
        reasons.append("x64")
    if pe.get("characteristics_flags", {}).get("dll"):
        score += 5
        reasons.append("dll-image")
    if has_pipl:
        score += 25
        reasons.append("pipl-signal")
    if has_effect_main_export:
        score += 35
        reasons.append("EffectMain-export")
    elif has_effect_main_marker:
        score += 20
        reasons.append("EffectMain-marker")
    if aegp_marker_count:
        score -= 20
        reasons.append("AEGP-marker")
    if 0 < size_bytes <= 256 * 1024:
        score += 15
        reasons.append("small-local-fixture-size")
    elif 0 < size_bytes <= 1024 * 1024:
        score += 10
        reasons.append("moderate-fixture-size")
    elif 0 < size_bytes <= 6 * 1024 * 1024:
        score += 3
        reasons.append("large-but-reviewable")

    if has_pipl and (has_effect_main_export or has_effect_main_marker) and aegp_marker_count:
        compatibility_class = "classic_pf_effect_with_aegp_markers"
    elif has_pipl and (has_effect_main_export or has_effect_main_marker):
        compatibility_class = "classic_pf_effect_candidate"
    elif aegp_marker_count and not has_effect_main_marker and not has_effect_main_export:
        compatibility_class = "aegp_or_helper_candidate"
    elif has_pipl:
        compatibility_class = "pipl_present_unclassified"
    else:
        compatibility_class = "not_aex_effect_candidate"

    return {
        "compatibility_class": compatibility_class,
        "fixture_candidate_score": score,
        "fixture_candidate_reasons": reasons,
    }


def analyze_aex_file(path: Path, root: Path | None = None) -> dict[str, Any]:
    stat = path.stat()
    entry: dict[str, Any] = {
        "relative_path": str(path.relative_to(root)) if root else path.name,
        "file_name": path.name,
        "size_bytes": stat.st_size,
        "mtime_utc": datetime.fromtimestamp(stat.st_mtime, timezone.utc).isoformat(),
        "read_truncated": stat.st_size > MAX_SCAN_BYTES,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
    }
    with path.open("rb") as handle:
        data = handle.read(MAX_SCAN_BYTES + 1)
    if len(data) > MAX_SCAN_BYTES:
        data = data[:MAX_SCAN_BYTES]
    entry["pe"] = parse_pe(data)
    entry["markers"] = marker_summary(data)
    entry["aex_static_metadata_ready"] = bool(entry["pe"]["mz_header_present"])
    entry["pipl_signal_present"] = bool(
        entry["markers"]["pipl_ascii_marker_present"]
        or entry["pe"]["resource_summary"]["pipl_resource_type_present"]
    )
    entry.update(classify_entry(entry))
    return entry


def iter_aex_files(input_path: Path) -> Iterable[Path]:
    if input_path.is_file():
        if input_path.suffix.lower() == ".aex":
            yield input_path
        return
    yield from sorted(path for path in input_path.rglob("*") if path.is_file() and path.suffix.lower() == ".aex")


def build_report(input_path: Path) -> dict[str, Any]:
    root = input_path if input_path.is_dir() else input_path.parent
    entries = [analyze_aex_file(path, root=root) for path in iter_aex_files(input_path)]
    return {
        "schema_version": 3,
        "publication_status": "local-only",
        "report_kind": "aex_static_probe",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "input_root": str(input_path),
        "aex_count": len(entries),
        "summary": summarize_entries(entries),
        "fixture_candidates": select_fixture_candidates(entries),
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "entries": entries,
        "notes": [
            "Reads AEX files as bytes only.",
            "Does not load DLLs, call EffectMain, start After Effects, render, or route through OFX.",
            "Reports metadata and marker counts only; no binary payloads or hashes are emitted.",
        ],
    }


def summarize_entries(entries: list[dict[str, Any]]) -> dict[str, Any]:
    class_counts: dict[str, int] = {}
    resource_type_counts: dict[str, int] = {}
    for entry in entries:
        compatibility_class = str(entry.get("compatibility_class", "unknown"))
        class_counts[compatibility_class] = class_counts.get(compatibility_class, 0) + 1
        for detail in entry.get("pe", {}).get("resource_summary", {}).get("type_details", []):
            if not isinstance(detail, dict):
                continue
            resource_type = str(detail.get("type", "#unknown"))
            resource_type_counts[resource_type] = resource_type_counts.get(resource_type, 0) + int(
                detail.get("entry_count") or 0
            )
    return {
        "aex_count": len(entries),
        "pe_valid_count": sum(1 for entry in entries if entry.get("pe", {}).get("pe_valid")),
        "pipl_signal_count": sum(1 for entry in entries if entry.get("pipl_signal_present")),
        "pipl_resource_entry_count": sum(
            int(entry.get("pe", {}).get("resource_summary", {}).get("pipl_resource_data_entry_count") or 0)
            for entry in entries
        ),
        "pipl_resource_total_size": sum(
            int(entry.get("pe", {}).get("resource_summary", {}).get("pipl_resource_total_size") or 0)
            for entry in entries
        ),
        "effect_main_marker_count": sum(
            1 for entry in entries if entry.get("markers", {}).get("effect_main_marker_present")
        ),
        "effect_main_export_count": sum(
            1
            for entry in entries
            if entry.get("pe", {}).get("export_summary", {}).get("effect_main_export_present")
        ),
        "aegp_marker_entry_count": sum(
            1 for entry in entries if int(entry.get("markers", {}).get("ae_plugin_marker_count") or 0) > 0
        ),
        "class_counts": dict(sorted(class_counts.items())),
        "resource_type_counts": dict(sorted(resource_type_counts.items(), key=lambda item: item[0].lower())),
    }


def select_fixture_candidates(entries: list[dict[str, Any]], limit: int = 12) -> list[dict[str, Any]]:
    candidates = [
        entry
        for entry in entries
        if entry.get("fixture_candidate_score", 0) > 0
        and entry.get("compatibility_class") == "classic_pf_effect_candidate"
    ]
    candidates.sort(key=lambda entry: (-int(entry["fixture_candidate_score"]), int(entry.get("size_bytes") or 0), entry["relative_path"]))
    return [
        {
            "relative_path": entry["relative_path"],
            "file_name": entry["file_name"],
            "size_bytes": entry["size_bytes"],
            "machine_label": entry.get("pe", {}).get("machine_label"),
            "compatibility_class": entry.get("compatibility_class"),
            "fixture_candidate_score": entry.get("fixture_candidate_score"),
            "fixture_candidate_reasons": entry.get("fixture_candidate_reasons"),
            "effect_main_export_present": entry.get("pe", {})
            .get("export_summary", {})
            .get("effect_main_export_present"),
            "effect_main_marker_present": entry.get("markers", {}).get("effect_main_marker_present"),
            "pipl_signal_present": entry.get("pipl_signal_present"),
            "pipl_resource_data_entry_count": entry.get("pe", {})
            .get("resource_summary", {})
            .get("pipl_resource_data_entry_count"),
            "pipl_resource_total_size": entry.get("pe", {})
            .get("resource_summary", {})
            .get("pipl_resource_total_size"),
        }
        for entry in candidates[:limit]
    ]


def path_has_traversal(path: Path) -> bool:
    return any(part in ("..", ".") for part in path.parts)


def validate_output_path(path: Path, root: Path) -> None:
    if path_has_traversal(path):
        raise ValueError("output path must not contain traversal components")
    if path.suffix.lower() != ".json":
        raise ValueError("output path must have .json extension")
    root.mkdir(parents=True, exist_ok=True)
    absolute = path if path.is_absolute() else LAB_ROOT / path
    if not absolute.resolve(strict=False).is_relative_to(root.resolve(strict=True)):
        raise ValueError(f"output path must stay under {root}")


def write_json_create_new(path: Path, payload: dict[str, Any]) -> None:
    validate_output_path(path, REPORT_ROOT)
    absolute = path if path.is_absolute() else LAB_ROOT / path
    absolute.parent.mkdir(parents=True, exist_ok=True)
    if not absolute.parent.resolve(strict=True).is_relative_to(REPORT_ROOT.resolve(strict=True)):
        raise ValueError(f"output parent must resolve under {REPORT_ROOT}")
    with absolute.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="No-load static AEX metadata probe")
    parser.add_argument("--input", required=True, help="AEX file or directory to scan")
    parser.add_argument("--out", help="Create-new JSON report under target/aex-static-probe")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    report = build_report(Path(args.input))
    if args.out:
        write_json_create_new(Path(args.out), report)
        print(args.out)
    else:
        print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

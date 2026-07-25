#!/usr/bin/env python3
"""Static (no-load) real PiPL parser: extract AE plug-in identity from .aex bytes.

This reads the Windows PiPL resource payload directly out of the PE resource
section and decodes the Adobe property list (Kind, CodeWin64X86 entrypoint,
Name, Category, Match Name, versions, out-flags). It never loads the DLL/AEX,
never calls an entrypoint, never starts After Effects, and never renders.

The parser is bounded and fail-closed: malformed, overflowing, or ambiguous
input yields an explicit diagnostic (never a crash and never a silent guess),
so an arbitrary AEX becomes a reproducible observation. It reuses the PE walking
helpers in ``aex_static_probe`` and never opens or loads the binary as a module.
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

import aex_static_probe as probe  # noqa: E402  (path shim above is required first)

REPORT_ROOT = LAB_ROOT / "target" / "aex-pipl-identity"
MAX_SCAN_BYTES = probe.MAX_SCAN_BYTES
# Match the worker parser bound (minihost parse_pipl_entrypoint) so the static
# view mirrors the worker's fail-closed dispatch decision as closely as a no-load
# parse can (a static approximation, not a byte-for-byte oracle of the runtime).
MAX_PIPL_PAYLOAD_BYTES = 1024 * 1024
MAX_PROPERTY_COUNT = 256
MAX_PIPL_RESOURCES = 64
MAX_STRING_BYTES = 512

# On disk the vendor OSType '8BIM' is stored little-endian, so the bytes read
# "MIB8". Match that on-disk orientation (as the worker parser does).
VENDOR_ADOBE = b"MIB8"

# canonical Kind OSType -> human label. Kind values are stored as a
# little-endian DWORD in the resource, so on-disk "TKFe" reverses to 'eFKT'.
KIND_LABELS = {
    "eFKT": "AEEffect",
    "AEgx": "AEGP",
    "AEgp": "AEGeneral",
    "eFST": "AEAccelerator",
    "FXIF": "AEImageFormat",
    "eFPF": "AEForeignProjectFormat",
    "8BFM": "PhotoshopFilter",
    "8BYM": "PhotoshopParser",
    "8BIF": "PhotoshopImageFormat",
}

STAGE_LABELS = {0: "develop", 1: "alpha", 2: "beta", 3: "release"}


def _reverse4(four: bytes) -> str:
    """Recover a canonical four-char OSType from its little-endian byte order."""
    return bytes(reversed(four)).decode("ascii", errors="replace")


def _u32(payload: bytes, offset: int) -> int:
    return int.from_bytes(payload[offset : offset + 4], "little")


def _u16(payload: bytes, offset: int) -> int:
    return int.from_bytes(payload[offset : offset + 2], "little")


def _decode_ae_version(value: int) -> dict[str, Any]:
    vers = (value >> 19) & 0x1FF
    subvers = (value >> 15) & 0xF
    bugvers = (value >> 11) & 0xF
    stage = (value >> 9) & 0x3
    build = value & 0x1FF
    return {
        "raw": value,
        "vers": vers,
        "subvers": subvers,
        "bugvers": bugvers,
        "stage": stage,
        "stage_label": STAGE_LABELS.get(stage, "unknown"),
        "build": build,
        "display": f"{vers}.{subvers}.{bugvers}",
    }


def _pstring(data: bytes) -> str | None:
    if not data:
        return None
    length = data[0]
    if 1 + length > len(data):
        return None
    return data[1 : 1 + length].decode("ascii", errors="replace")


def _cstring(data: bytes) -> str | None:
    end = data.find(b"\x00")
    raw = data if end < 0 else data[:end]
    if not raw:
        return None
    return raw.decode("ascii", errors="replace")


def _valid_export_symbol(data: bytes) -> str | None:
    """Mirror the worker's valid_export_symbol (minihost/src/l2_main.cpp).

    Returns the export identifier only when it is a NUL-terminated ASCII symbol
    (first char [_A-Za-z], rest [_A-Za-z0-9]), length 1..127, total size 1..256,
    with all bytes after the NUL zero. Otherwise the worker rejects the plug-in,
    so the static view must not treat it as a resolvable entrypoint.
    """
    if len(data) == 0 or len(data) > 256:
        return None
    nul = data.find(b"\x00")
    if nul <= 0 or nul > 127:
        return None
    symbol = data[:nul]
    first = symbol[0]
    if not (first == 0x5F or 0x41 <= first <= 0x5A or 0x61 <= first <= 0x7A):
        return None
    for value in symbol[1:]:
        if not (value == 0x5F or 0x41 <= value <= 0x5A or 0x61 <= value <= 0x7A or 0x30 <= value <= 0x39):
            return None
    if any(byte != 0 for byte in data[nul + 1 :]):
        return None
    return symbol.decode("ascii")


def _terminal_zero_ae_reserved_property(data: bytes) -> bool:
    """Recognize the exact public AE_Reserved zero tail used by shipping BCC PiPLs."""
    return (
        len(data) == 20
        and data[0:4] == VENDOR_ADOBE
        and data[4:8] == b"DRea"  # serialized little-endian ``aeRD``
        and _u32(data, 8) == 0
        and _u32(data, 12) == 4
        and _u32(data, 16) == 0
    )


def parse_pipl_payload(payload: bytes | None) -> dict[str, Any]:
    """Decode one PiPL resource payload into an identity record.

    Returns a dict with ``parse_state`` == ``"parsed"`` on success or
    ``"invalid"`` with a ``reason`` when the bytes violate the bounded PiPL
    contract. Never raises on malformed input.
    """
    record: dict[str, Any] = {
        "parse_state": "invalid",
        "reason": None,
        "kind_code": None,
        "kind_label": None,
        "kind_count": 0,
        "code_win64_count": 0,
        "entrypoint_win64": None,
        "entrypoint_win64_valid": False,
        "entrypoint_win32": None,
        "dispatchable_effect": False,
        "name": None,
        "category": None,
        "match_name": None,
        "version": None,
        "spec_version": None,
        "pipl_version": None,
        "global_out_flags": None,
        "global_out_flags_2": None,
        "info_flags": None,
        "support_url": None,
        "property_keys": [],
        "property_count_declared": None,
        "payload_size_bytes": len(payload) if payload else 0,
    }

    if payload is None:
        record["reason"] = "no_payload"
        return record
    size = len(payload)
    if size < 10:
        record["reason"] = "payload_shorter_than_header"
        return record
    if size > MAX_PIPL_PAYLOAD_BYTES:
        record["reason"] = "payload_exceeds_bound"
        return record

    version = _u32(payload, 0)
    count = _u16(payload, 6)
    record["property_count_declared"] = count
    if version > 1 or payload[4] != 0 or payload[5] != 0 or payload[8] != 0 or payload[9] != 0:
        record["reason"] = "bad_pipl_header"
        return record
    if count == 0 or count > MAX_PROPERTY_COUNT:
        record["reason"] = "bad_property_count"
        return record

    keys: list[str] = []
    kind_count = 0
    code_count = 0
    offset = 10
    for _ in range(count):
        if offset > size or size - offset < 16:
            record["reason"] = "truncated_property_header"
            return record
        vendor = payload[offset : offset + 4]
        key_raw = payload[offset + 4 : offset + 8]
        length = _u32(payload, offset + 12)
        offset += 16
        if length > size - offset:
            record["reason"] = "property_length_overflow"
            return record
        padded = (length + 3) & ~3
        if padded > size - offset:
            record["reason"] = "property_padding_overflow"
            return record
        data = payload[offset : offset + length]
        # trailing pad bytes must be zero (matches worker fail-closed parser)
        for pad_index in range(length, padded):
            if payload[offset + pad_index] != 0:
                record["reason"] = "nonzero_property_padding"
                return record

        adobe = vendor == VENDOR_ADOBE
        canonical_key = _reverse4(key_raw)
        keys.append(canonical_key if adobe else f"{_reverse4(vendor)}:{canonical_key}")
        if adobe:
            # The worker validates Kind and CodeWin64X86 independent of the Kind
            # value and fails the whole resource (and the whole module) on a
            # duplicate/malformed Kind or a duplicate/invalid CodeWin64X86. Mirror
            # that at parse time so a bad sibling resource is not silently ignored.
            if canonical_key == "kind":
                if kind_count >= 1 or len(data) != 4:
                    record["reason"] = "duplicate_or_malformed_kind"
                    return record
                kind_count += 1
                record["kind_code"] = _reverse4(data)
                record["kind_label"] = KIND_LABELS.get(record["kind_code"], "unknown")
            elif canonical_key == "8664":  # CodeWin64X86
                if code_count >= 1:
                    record["reason"] = "duplicate_code_win64"
                    return record
                record["entrypoint_win64"] = _cstring(data)  # best-effort display
                if _valid_export_symbol(data) is None:
                    record["reason"] = "invalid_code_win64_symbol"
                    return record
                record["entrypoint_win64_valid"] = True
                code_count += 1
            else:
                _apply_adobe_property(record, canonical_key, data)
        offset += padded

    if offset != size:
        if not _terminal_zero_ae_reserved_property(payload[offset:]):
            record["reason"] = "trailing_bytes_after_properties"
            return record
        keys.append("aeRD")
        offset = size
    # Worker requires exactly one Kind, and an Effect Kind requires its Win64
    # entrypoint; both are Invalid otherwise.
    if kind_count == 0:
        record["reason"] = "missing_kind"
        return record
    if record["kind_code"] == "eFKT" and code_count == 0:
        record["reason"] = "effect_missing_code_win64"
        return record

    record["property_keys"] = keys
    record["kind_count"] = kind_count
    record["code_win64_count"] = code_count
    record["parse_state"] = "parsed"
    record["reason"] = None
    # After the parse-time gates above, a parsed eFKT resource necessarily has a
    # single valid CodeWin64X86; keep the full predicate for clarity/safety.
    record["dispatchable_effect"] = (
        record["kind_code"] == "eFKT" and code_count == 1 and record["entrypoint_win64_valid"]
    )
    return record


def _apply_adobe_property(record: dict[str, Any], key: str, data: bytes) -> None:
    # Kind and CodeWin64X86 are validated inline in parse_pipl_payload; this
    # handles the display-only identity properties.
    if key == "wx86":  # CodeWin32X86
        record["entrypoint_win32"] = _cstring(data)
    elif key == "name":
        record["name"] = _pstring(data)
    elif key == "catg":
        record["category"] = _pstring(data)
    elif key == "eMNA":  # AE_Effect_Match_Name
        record["match_name"] = _pstring(data)
    elif key == "eURL":  # AE_Effect_Support_URL
        record["support_url"] = _pstring(data)
    elif key == "eVER" and len(data) == 4:  # AE_Effect_Version
        record["version"] = _decode_ae_version(_u32(data, 0))
    elif key == "eSVR" and len(data) == 4:  # AE_Effect_Spec_Version
        record["spec_version"] = {"major": _u16(data, 0), "minor": _u16(data, 2)}
    elif key == "ePVR" and len(data) == 4:  # AE_PiPL_Version
        record["pipl_version"] = {"major": _u16(data, 0), "minor": _u16(data, 2)}
    elif key == "eGLO" and len(data) == 4:  # AE_Effect_Global_OutFlags
        record["global_out_flags"] = f"0x{_u32(data, 0):08x}"
    elif key == "eGL2" and len(data) == 4:  # AE_Effect_Global_OutFlags_2
        record["global_out_flags_2"] = f"0x{_u32(data, 0):08x}"
    elif key == "eINF" and len(data) >= 2:  # AE_Effect_Info_Flags
        record["info_flags"] = f"0x{_u16(data, 0):04x}"


def _pe_sections(data: bytes) -> list[dict[str, Any]] | None:
    if len(data) < 0x40 or not data.startswith(b"MZ"):
        return None
    pe_offset = probe.read_u32(data, 0x3C)
    if pe_offset is None or pe_offset + 24 > len(data):
        return None
    if data[pe_offset : pe_offset + 4] != b"PE\x00\x00":
        return None
    coff = pe_offset + 4
    section_count = probe.read_u16(data, coff + 2) or 0
    optional_header_size = probe.read_u16(data, coff + 16) or 0
    section_table = coff + 20 + optional_header_size
    return probe.parse_sections(data, section_table, section_count)


def _read_pipl_payloads(data: bytes, pe: dict[str, Any]) -> list[bytes]:
    sections = _pe_sections(data)
    if not sections:
        return []
    entries = pe.get("resource_summary", {}).get("pipl_resource_entries", [])
    payloads: list[bytes] = []
    for entry in entries[:MAX_PIPL_RESOURCES]:
        rva = entry.get("data_rva")
        size = entry.get("size_bytes")
        if not isinstance(rva, int) or not isinstance(size, int) or size <= 0:
            payloads.append(b"")
            continue
        if size > MAX_PIPL_PAYLOAD_BYTES:
            payloads.append(b"")
            continue
        offset = probe.rva_to_offset(rva, sections)
        if offset is None or offset < 0 or offset + size > len(data):
            payloads.append(b"")
            continue
        payloads.append(data[offset : offset + size])
    return payloads


def _classify(records: list[dict[str, Any]]) -> str:
    """Static approximation of the worker's fail-closed dispatch decision.

    A record that the worker's parse_pipl_entrypoint would reject as Invalid maps
    to ``invalid_pipl`` here: a structural failure, a resource without exactly one
    Kind, or an Effect Kind without exactly one valid CodeWin64X86. The classes
    ``ambiguous_multiple_effect`` / ``ambiguous_effect_and_aegp`` are surfaced
    separately for the listing, but like ``invalid_pipl`` they are never
    dispatchable — only ``effect`` means the worker would resolve and run it.
    """
    if not records:
        return "no_pipl"
    if any(r.get("parse_state") != "parsed" for r in records):
        return "invalid_pipl"
    for record in records:
        # exactly one Kind is required; an Effect Kind must resolve a single
        # valid Win64 entrypoint. Anything else is Invalid at the worker.
        if record.get("kind_count") != 1:
            return "invalid_pipl"
        if record.get("kind_code") == "eFKT" and not record.get("dispatchable_effect"):
            return "invalid_pipl"
    effects = [r for r in records if r.get("dispatchable_effect")]
    aegp = [r for r in records if r.get("kind_code") == "AEgx"]
    if len(effects) > 1:
        return "ambiguous_multiple_effect"
    if effects and aegp:
        return "ambiguous_effect_and_aegp"
    if effects:
        return "effect"
    if aegp:
        return "aegp"
    return "unknown"


def analyze_aex_file(path: Path, root: Path | None = None) -> dict[str, Any]:
    stat = path.stat()
    entry: dict[str, Any] = {
        "relative_path": str(path.relative_to(root)) if root else path.name,
        "file_name": path.name,
        "size_bytes": stat.st_size,
        "read_truncated": stat.st_size > MAX_SCAN_BYTES,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
    }
    with path.open("rb") as handle:
        data = handle.read(MAX_SCAN_BYTES + 1)
    if len(data) > MAX_SCAN_BYTES:
        data = data[:MAX_SCAN_BYTES]

    pe = probe.parse_pe(data)
    entry["machine_label"] = pe.get("machine_label")
    resource_summary = pe.get("resource_summary", {})
    total_pipl = int(resource_summary.get("pipl_resource_data_entry_count") or 0)
    payloads = _read_pipl_payloads(data, pe)
    records = [parse_pipl_payload(payload) for payload in payloads]
    entry["pipl_resource_count"] = total_pipl
    entry["pipl_records"] = records
    # The worker's resource enumeration fails closed past 64 PiPL resources, so a
    # binary carrying more than that is Invalid regardless of the first 64.
    if total_pipl > MAX_PIPL_RESOURCES:
        entry["classification"] = "invalid_pipl"
    else:
        entry["classification"] = _classify(records)

    # Surface the selected identity: the single dispatchable Effect if
    # unambiguous, else the first parsed record, so the listing always shows what
    # it could read even when the plug-in is not dispatchable.
    selected = None
    effects = [r for r in records if r.get("dispatchable_effect")]
    if len(effects) == 1:
        selected = effects[0]
    elif records:
        parsed = [r for r in records if r["parse_state"] == "parsed"]
        selected = parsed[0] if parsed else records[0]
    entry["identity"] = _selected_identity(selected)
    return entry


def _selected_identity(record: dict[str, Any] | None) -> dict[str, Any]:
    if record is None:
        return {field: None for field in (
            "name", "match_name", "category", "kind_code", "kind_label",
            "entrypoint_win64", "version_display",
        )}
    version = record.get("version")
    return {
        "name": record.get("name"),
        "match_name": record.get("match_name"),
        "category": record.get("category"),
        "kind_code": record.get("kind_code"),
        "kind_label": record.get("kind_label"),
        "entrypoint_win64": record.get("entrypoint_win64"),
        "version_display": version.get("display") if isinstance(version, dict) else None,
    }


def iter_aex_files(input_path: Path):
    if input_path.is_file():
        if input_path.suffix.lower() == ".aex":
            yield input_path
        return
    yield from sorted(
        path for path in input_path.rglob("*") if path.is_file() and path.suffix.lower() == ".aex"
    )


def build_report(input_path: Path) -> dict[str, Any]:
    root = input_path if input_path.is_dir() else input_path.parent
    entries = [analyze_aex_file(path, root=root) for path in iter_aex_files(input_path)]
    class_counts: dict[str, int] = {}
    for entry in entries:
        cls = str(entry.get("classification"))
        class_counts[cls] = class_counts.get(cls, 0) + 1
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_pipl_identity",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        # Only the leaf name of the scanned root; never the caller's absolute
        # filesystem path (per-entry relative_path carries the useful identity).
        "input_root": input_path.name,
        "aex_count": len(entries),
        "classification_counts": dict(sorted(class_counts.items())),
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "entries": entries,
        "notes": [
            "Reads the PiPL resource payload as bytes; does not load the AEX as a module.",
            "Never calls an entrypoint, starts After Effects, or renders.",
            "Bounded, fail-closed PiPL property decode; malformed input is reported, not guessed.",
        ],
    }


def path_has_traversal(path: Path) -> bool:
    return any(part in ("..", ".") for part in path.parts)


def write_json_create_new(path: Path, payload: dict[str, Any]) -> None:
    if path_has_traversal(path):
        raise ValueError("output path must not contain traversal components")
    if path.suffix.lower() != ".json":
        raise ValueError("output path must have .json extension")
    REPORT_ROOT.mkdir(parents=True, exist_ok=True)
    absolute = path if path.is_absolute() else LAB_ROOT / path
    if not absolute.resolve(strict=False).is_relative_to(REPORT_ROOT.resolve(strict=True)):
        raise ValueError(f"output path must stay under {REPORT_ROOT}")
    absolute.parent.mkdir(parents=True, exist_ok=True)
    with absolute.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="No-load real PiPL identity parser")
    parser.add_argument("--input", required=True, help="AEX file or directory to scan")
    parser.add_argument("--out", help="Create-new JSON report under target/aex-pipl-identity")
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

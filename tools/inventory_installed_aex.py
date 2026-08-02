#!/usr/bin/env python3
"""Build a path- and SHA-addressed inventory of installed Windows AEX files.

The inventory is deliberately static: it never calls LoadLibrary and never
changes an installed file.  It discovers the explicit local corpus, real
Adobe After Effects/MediaCore roots under both Program Files trees, and
additional existing roots exposed by environment variables, the Windows
registry, or path literals in repository configuration.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import stat
import struct
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable, Iterator


SCHEMA_VERSION = 1
MAX_HASH_BYTES = 1024 * 1024 * 1024
MAX_REGISTRY_KEYS = 20000
MAX_CONFIG_FILE_BYTES = 4 * 1024 * 1024
MAX_EXPORT_NAMES = 4096
PATH_TOKEN_RE = re.compile(
    r"(?i)([A-Za-z]:[\\/][^\"\r\n]+?(?:Plug-ins|Plugins|MediaCore|Ae_Plugins)[^\"\r\n]*)"
)
TEXT_SUFFIXES = {".json", ".toml", ".yaml", ".yml", ".ini", ".env", ".ps1", ".py", ".rs", ".md"}
SYMBOL_HINTS = (
    "EffectMain",
    "PluginDataEntryFunction",
    "MainEntry",
    "FilterMain",
    "SmartPreRender",
    "SmartRender",
)


@dataclass
class RootSpec:
    path: Path
    category: str
    discovery: set[str] = field(default_factory=set)


def canonical(path: Path) -> Path:
    return Path(os.path.realpath(os.path.abspath(os.fspath(path))))


def existing_dir(path: Path) -> bool:
    try:
        return canonical(path).is_dir()
    except OSError:
        return False


def split_path_value(value: str) -> Iterator[Path]:
    # Registry values commonly use ';' even when os.pathsep is ':'.
    for item in re.split(r"[;\r\n]+", value):
        item = item.strip().strip('"')
        if item:
            yield Path(os.path.expandvars(item))


def add_root(roots: dict[str, RootSpec], path: Path, category: str, discovery: str) -> None:
    if not existing_dir(path):
        return
    key = os.fspath(canonical(path)).casefold()
    current = roots.get(key)
    if current is None:
        roots[key] = RootSpec(canonical(path), category, {discovery})
    else:
        current.discovery.add(discovery)
        # Prefer the most specific corpus label when a registry/config path
        # points at a root that was independently classified as Adobe/local.
        if current.category == "config_discovered" and category != "config_discovered":
            current.category = category


def registry_values() -> Iterator[tuple[str, str, str]]:
    if os.name != "nt":
        return
    try:
        import winreg
    except ImportError:
        return

    hives = ((winreg.HKEY_CURRENT_USER, "HKCU"), (winreg.HKEY_LOCAL_MACHINE, "HKLM"))
    queue = [
        (hive, hive_name, r"Software\Adobe")
        for hive, hive_name in hives
    ]
    seen: set[tuple[int, str]] = set()
    while queue:
        if len(seen) >= MAX_REGISTRY_KEYS:
            return
        hive, hive_name, subkey = queue.pop()
        marker = (int(hive), subkey.casefold())
        if marker in seen:
            continue
        seen.add(marker)
        try:
            with winreg.OpenKey(hive, subkey) as key:
                index = 0
                while True:
                    try:
                        name, value, _ = winreg.EnumValue(key, index)
                    except OSError:
                        break
                    if isinstance(value, str):
                        yield hive_name, subkey, f"{name}={value}"
                    index += 1
                index = 0
                while True:
                    try:
                        child = winreg.EnumKey(key, index)
                    except OSError:
                        break
                    queue.append((hive, hive_name, f"{subkey}\\{child}"))
                    index += 1
        except OSError:
            continue


def discover_registry_roots(roots: dict[str, RootSpec]) -> None:
    for hive, key, value in registry_values():
        lowered = f"{key} {value}".casefold()
        if not any(token in lowered for token in ("plugin", "mediacore", "aex", "after effects")):
            continue
        for candidate in split_path_value(value.split("=", 1)[-1]):
            if candidate.suffix.casefold() == ".aex":
                candidate = candidate.parent
            if existing_dir(candidate):
                add_root(roots, candidate, "config_discovered", f"registry:{hive}:{key}")


def discover_environment_roots(roots: dict[str, RootSpec]) -> None:
    for name, value in os.environ.items():
        lowered = name.casefold()
        if not any(token in lowered for token in ("aex", "plugin", "mediacore")):
            continue
        for candidate in split_path_value(value):
            if candidate.suffix.casefold() == ".aex":
                candidate = candidate.parent
            if existing_dir(candidate):
                add_root(roots, candidate, "config_discovered", f"environment:{name}")


def program_files_bases() -> Iterator[tuple[Path, str]]:
    candidates: dict[str, tuple[Path, set[str]]] = {}
    for name, category in (("ProgramFiles", "program_files_x64"), ("ProgramFiles(x86)", "program_files_x86")):
        value = os.environ.get(name)
        if value:
            path = canonical(Path(value))
            candidates.setdefault(os.fspath(path).casefold(), (path, set()))[1].add(f"environment:{name}")

    if os.name == "nt":
        try:
            import winreg

            with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows\CurrentVersion") as key:
                for value_name, category in (("ProgramFilesDir", "program_files_x64"), ("ProgramFilesDir (x86)", "program_files_x86")):
                    try:
                        value, _ = winreg.QueryValueEx(key, value_name)
                    except OSError:
                        continue
                    path = canonical(Path(str(value)))
                    candidates.setdefault(os.fspath(path).casefold(), (path, set()))[1].add(f"registry:{value_name}")
        except (ImportError, OSError):
            pass

    for path, discoveries in candidates.values():
        if path.is_dir():
            yield path, ";".join(sorted(discoveries))


def walk_dirs(root: Path) -> Iterator[Path]:
    """Walk without following junctions/symlinks and without revisiting a dir."""
    visited: set[str] = set()
    for dirpath, dirnames, _ in os.walk(root, topdown=True, followlinks=False):
        safe_dirs: list[str] = []
        for name in dirnames:
            path = Path(dirpath) / name
            try:
                info = path.lstat()
            except OSError:
                continue
            if stat.S_ISLNK(info.st_mode):
                continue
            # FILE_ATTRIBUTE_REPARSE_POINT is 0x400 on Windows.  Avoid
            # junction loops while remaining portable for ordinary folders.
            if getattr(info, "st_file_attributes", 0) & 0x400:
                continue
            safe_dirs.append(name)
        dirnames[:] = safe_dirs
        real = os.fspath(canonical(Path(dirpath))).casefold()
        if real in visited:
            dirnames[:] = []
            continue
        visited.add(real)
        yield canonical(Path(dirpath))


def discover_adobe_roots(roots: dict[str, RootSpec]) -> None:
    for base, discovery in program_files_bases():
        adobe = base / "Adobe"
        if not adobe.is_dir():
            continue
        for child in sorted(adobe.iterdir(), key=lambda p: p.name.casefold()):
            if child.is_dir() and child.name.casefold().startswith("adobe after effects"):
                add_root(roots, child / "Support Files" / "Plug-ins", "adobe_ae_plugins", f"{discovery}:after-effects")
        common = adobe / "Common"
        if common.is_dir():
            for directory in walk_dirs(common):
                if directory.name.casefold() == "mediacore":
                    add_root(roots, directory, "adobe_mediacore", f"{discovery}:common")


def discover_repo_config_roots(roots: dict[str, RootSpec], repo_root: Path) -> None:
    if not repo_root.is_dir():
        return
    ignored_dirs = {".git", "target", "node_modules", ".venv", "venv", "build", "dist", "__pycache__"}
    for directory, dirnames, files in os.walk(repo_root, topdown=True, followlinks=False):
        dirnames[:] = [name for name in dirnames if name.casefold() not in ignored_dirs]
        directory_path = Path(directory)
        for filename in files:
            path = directory_path / filename
            if path.suffix.casefold() not in TEXT_SUFFIXES or path.stat().st_size > MAX_CONFIG_FILE_BYTES:
                continue
            try:
                text = path.read_text(encoding="utf-8", errors="ignore")
            except OSError:
                continue
            for match in PATH_TOKEN_RE.finditer(text):
                candidate_text = match.group(1).rstrip(" ,)]}'")
                candidate = Path(os.path.expandvars(candidate_text))
                if candidate.suffix.casefold() == ".aex":
                    candidate = candidate.parent
                if existing_dir(candidate):
                    add_root(roots, candidate, "config_discovered", f"repo:{path.relative_to(repo_root)}")


def discover_roots(local_root: Path, repo_root: Path, include_registry: bool = True, include_repo_config: bool = True) -> list[RootSpec]:
    roots: dict[str, RootSpec] = {}
    add_root(roots, local_root, "local_development", "explicit:local-corpus")
    discover_adobe_roots(roots)
    discover_environment_roots(roots)
    if include_registry:
        discover_registry_roots(roots)
    if include_repo_config:
        discover_repo_config_roots(roots, repo_root)
    return sorted(roots.values(), key=lambda root: os.fspath(root.path).casefold())


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        total = 0
        while chunk := stream.read(1024 * 1024):
            total += len(chunk)
            if total > MAX_HASH_BYTES:
                raise ValueError(f"file exceeds bounded hash size: {path}")
            digest.update(chunk)
    return digest.hexdigest()


def rva_to_offset(rva: int, sections: list[tuple[int, int, int, int]], length: int) -> int | None:
    for virtual_size, virtual_address, raw_size, raw_pointer in sections:
        span = max(virtual_size, raw_size)
        if virtual_address <= rva < virtual_address + span:
            offset = raw_pointer + (rva - virtual_address)
            return offset if 0 <= offset < length else None
    return None


def pe_static_info(data: bytes) -> dict[str, object]:
    result: dict[str, object] = {
        "valid_pe": False,
        "architecture": "invalid",
        "optional_header": None,
        "export_names": [],
        "export_name_count": 0,
        "entrypoint_symbol_hints": [],
    }
    if len(data) < 0x40 or data[:2] != b"MZ":
        return result
    pe_offset = struct.unpack_from("<I", data, 0x3C)[0]
    if pe_offset < 0x40 or pe_offset + 24 > len(data) or data[pe_offset:pe_offset + 4] != b"PE\0\0":
        return result
    machine, section_count, _, _, _, optional_size, _ = struct.unpack_from("<HHIIIHH", data, pe_offset + 4)
    optional = pe_offset + 24
    if optional + optional_size > len(data) or optional_size < 96:
        return result
    magic = struct.unpack_from("<H", data, optional)[0]
    architecture = {0x8664: "x64", 0x14C: "x86", 0xAA64: "arm64"}.get(machine, f"machine_0x{machine:04x}")
    result.update({"valid_pe": True, "architecture": architecture, "optional_header": "PE32+" if magic == 0x20B else "PE32" if magic == 0x10B else f"magic_0x{magic:04x}"})
    section_table = optional + optional_size
    sections: list[tuple[int, int, int, int]] = []
    for index in range(section_count):
        offset = section_table + index * 40
        if offset + 40 > len(data):
            break
        virtual_size, virtual_address, raw_size, raw_pointer = struct.unpack_from("<IIII", data, offset + 8)
        sections.append((virtual_size, virtual_address, raw_size, raw_pointer))
    # The first data directory is the export directory for both formats, but
    # PE32+ has wider image-base and stack/heap fields before that table.
    data_directory_offset = {0x10B: 96, 0x20B: 112}.get(magic)
    if data_directory_offset is not None and optional_size >= data_directory_offset + 8:
        export_rva, export_size = struct.unpack_from("<II", data, optional + data_directory_offset)
    else:
        export_rva, export_size = 0, 0
    export_offset = rva_to_offset(export_rva, sections, len(data)) if export_rva else None
    names: list[str] = []
    if export_offset is not None and export_offset + 40 <= len(data):
        number_of_names = struct.unpack_from("<I", data, export_offset + 24)[0]
        address_of_names = struct.unpack_from("<I", data, export_offset + 32)[0]
        count = min(number_of_names, MAX_EXPORT_NAMES)
        names_offset = rva_to_offset(address_of_names, sections, len(data)) if address_of_names else None
        if names_offset is not None and names_offset + count * 4 <= len(data):
            for index in range(count):
                name_rva = struct.unpack_from("<I", data, names_offset + index * 4)[0]
                name_offset = rva_to_offset(name_rva, sections, len(data))
                if name_offset is None:
                    continue
                end = data.find(b"\0", name_offset, min(len(data), name_offset + 512))
                if end > name_offset:
                    try:
                        names.append(data[name_offset:end].decode("ascii"))
                    except UnicodeDecodeError:
                        continue
    result["export_names"] = names
    result["export_name_count"] = len(names)
    result["entrypoint_symbol_hints"] = sorted(name for name in names if any(hint.casefold() in name.casefold() for hint in SYMBOL_HINTS))
    return result


def classify_path(path: Path) -> dict[str, object]:
    lowered = "\\".join(part.casefold() for part in path.parts)
    fixture_tokens = ("fixture", "negative", "corrupt", "invalid", "test")
    return {
        "fixture_hint": any(token in lowered for token in fixture_tokens),
        "backup_hint": "backup" in lowered,
    }


def inspect_file(path: Path, root: RootSpec, relative: str) -> dict[str, object]:
    info: dict[str, object] = {
        "canonical_path": os.fspath(path),
        "root": os.fspath(root.path),
        "root_category": root.category,
        "root_discovery": sorted(root.discovery),
        "relative_to_root": relative,
        "size": None,
        "sha256": None,
        "read_error": None,
        "source_category": "adobe_installed" if root.category.startswith("adobe_") else "local_development" if root.category == "local_development" else "config_discovered",
    }
    info.update(classify_path(path))
    try:
        stat_result = path.stat()
        info["size"] = stat_result.st_size
        info["sha256"] = sha256_file(path)
        with path.open("rb") as stream:
            data = stream.read(MAX_HASH_BYTES + 1)
        if len(data) > MAX_HASH_BYTES:
            raise ValueError("bounded static read exceeded")
    except (OSError, ValueError) as exc:
        info["read_error"] = str(exc)
        info.update(pe_static_info(b""))
        return info
    info.update(pe_static_info(data))
    info["pipl_markers"] = sorted(marker.decode("ascii") for marker in (b"PiPL", b"8BIM", b"MIB8") if marker in data)
    info["pipl_present"] = bool(info["pipl_markers"])
    return info


def collect_file_records(roots: Iterable[RootSpec]) -> list[dict[str, object]]:
    category_priority = {"local_development": 0, "adobe_ae_plugins": 1, "adobe_mediacore": 1, "config_discovered": 2}
    by_path: dict[str, dict[str, object]] = {}
    for root in roots:
        for directory in walk_dirs(root.path):
            try:
                files = list(directory.iterdir())
            except OSError:
                continue
            for path in files:
                if not path.is_file() or path.suffix.casefold() != ".aex":
                    continue
                real = canonical(path)
                key = os.fspath(real).casefold()
                relative = os.path.relpath(os.fspath(real), os.fspath(root.path))
                item = {
                    "canonical_path": os.fspath(real),
                    "root": os.fspath(root.path),
                    "root_category": root.category,
                    "root_discovery": sorted(root.discovery),
                    "relative_to_root": relative,
                }
                existing = by_path.get(key)
                if existing is None:
                    by_path[key] = item
                else:
                    existing["root_discovery"] = sorted(set(existing.get("root_discovery", [])) | set(item.get("root_discovery", [])))
                    existing["root_categories"] = sorted(set(existing.get("root_categories", [existing["root_category"]])) | {item["root_category"]})
                    if category_priority.get(str(item["root_category"]), 9) < category_priority.get(str(existing["root_category"]), 9):
                        for key in ("root", "root_category", "relative_to_root"):
                            existing[key] = item[key]
    return sorted(by_path.values(), key=lambda item: str(item["canonical_path"]).casefold())


def inspect_records(records: Iterable[dict[str, object]]) -> list[dict[str, object]]:
    entries: list[dict[str, object]] = []
    for record in records:
        root = RootSpec(Path(str(record["root"])), str(record["root_category"]), set(record.get("root_discovery", [])))
        path = Path(str(record["canonical_path"]))
        item = inspect_file(path, root, str(record.get("relative_to_root", path.name)))
        item.update({key: value for key, value in record.items() if key in {"root_categories"}})
        entries.append(item)
    by_sha: defaultdict[str, list[dict[str, object]]] = defaultdict(list)
    for entry in entries:
        if entry.get("sha256"):
            by_sha[str(entry["sha256"])].append(entry)
    for entry in entries:
        sha = entry.get("sha256")
        entry["sha_duplicate_count"] = len(by_sha[str(sha)]) if sha else 0
        entry["dedupe_status"] = "unique_sha" if sha and len(by_sha[str(sha)]) == 1 else "duplicate_sha" if sha else "unhashed"
        entry["product_target_candidate"] = bool(entry.get("valid_pe") and entry.get("architecture") == "x64" and not entry.get("fixture_hint") and not entry.get("backup_hint"))
    return sorted(entries, key=lambda item: str(item["canonical_path"]).casefold())


def collect_entries(roots: Iterable[RootSpec]) -> list[dict[str, object]]:
    return inspect_records(collect_file_records(roots))


def summarize(entries: list[dict[str, object]], roots: list[RootSpec]) -> dict[str, object]:
    sha_groups: defaultdict[str, int] = defaultdict(int)
    for entry in entries:
        if entry.get("sha256"):
            sha_groups[str(entry["sha256"])] += 1
    root_counts = Counter(str(entry["root_category"]) for entry in entries)
    architecture = Counter(str(entry.get("architecture")) for entry in entries)
    return {
        "root_count": len(roots),
        "root_categories": Counter(root.category for root in roots),
        "canonical_aex_count": len(entries),
        "sha_unique_count": len(sha_groups),
        "sha_duplicate_group_count": sum(1 for count in sha_groups.values() if count > 1),
        "sha_duplicate_file_count": sum(count - 1 for count in sha_groups.values() if count > 1),
        "aex_count_by_root_category": root_counts,
        "architecture": architecture,
        "valid_pe_count": sum(bool(entry.get("valid_pe")) for entry in entries),
        "pipl_present_count": sum(bool(entry.get("pipl_present")) for entry in entries),
        "entrypoint_hint_count": sum(bool(entry.get("entrypoint_symbol_hints")) for entry in entries),
        "product_target_candidate_count": sum(bool(entry.get("product_target_candidate")) for entry in entries),
        "read_error_count": sum(bool(entry.get("read_error")) for entry in entries),
    }


def json_safe(value: object) -> object:
    if isinstance(value, Counter):
        return dict(sorted(value.items()))
    if isinstance(value, set):
        return sorted(value)
    if isinstance(value, dict):
        return {key: json_safe(item) for key, item in value.items()}
    if isinstance(value, list):
        return [json_safe(item) for item in value]
    return value


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--local-root",
        type=Path,
        default=Path(os.environ.get("AEXCOMPAT_LOCAL_AEX_ROOT", "target/local-aex")),
    )
    parser.add_argument("--repo-root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output", type=Path, default=Path("target/issue478-installed-aex/inventory.json"))
    parser.add_argument("--list-only", action="store_true", help="write a fast canonical file list without hashing or static reads")
    parser.add_argument("--roots-only", action="store_true", help="discover roots without walking any root")
    parser.add_argument("--file-list", type=Path, help="process records from a prior --list-only artifact")
    parser.add_argument("--start", type=int, default=0, help="zero-based record offset for --file-list")
    parser.add_argument("--limit", type=int, help="maximum records for --file-list")
    parser.add_argument("--skip-registry", action="store_true")
    parser.add_argument("--skip-repo-config", action="store_true")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv or sys.argv[1:])
    if args.file_list:
        source = json.loads(args.file_list.read_text(encoding="utf-8"))
        roots = [RootSpec(Path(str(item["path"])), str(item["category"]), set(item.get("discovery", []))) for item in source.get("roots", [])]
        records = source.get("entries", [])[args.start: args.start + args.limit if args.limit is not None else None]
        entries = inspect_records(records)
        mode = "read_only_static_metadata_batch"
    else:
        roots = discover_roots(args.local_root, args.repo_root, not args.skip_registry, not args.skip_repo_config)
        records = [] if args.roots_only else collect_file_records(roots)
        entries = [] if args.list_only else inspect_records(records)
        mode = "read_only_canonical_file_list" if args.list_only else "read_only_static_inventory"
    manifest = {
        "schema_version": SCHEMA_VERSION,
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "mode": mode,
        "roots": [
            {"path": os.fspath(root.path), "category": root.category, "discovery": sorted(root.discovery)}
            for root in roots
        ],
        "summary": json_safe(summarize(entries, roots)) if not args.list_only else {"canonical_aex_count": len(records), "root_count": len(roots)},
        "entries": records if args.list_only else entries,
    }
    output = args.output if args.output.is_absolute() else args.repo_root / args.output
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"output": os.fspath(output), "summary": manifest["summary"]}, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

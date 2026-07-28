#!/usr/bin/env python3
"""Select a deterministic, SHA-disjoint macOS x64 guest corpus tranche."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from collections import defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from sweep_macos_x64_aex import (  # noqa: E402
    SweepError,
    sha256_file,
    validate_source_pair,
)


MAX_PLUGIN_BYTES = 256 * 1024
REGISTRATION_EXPORTS = {
    "v1": "PluginDataEntryFunction",
    "v2": "PluginDataEntryFunction2",
}


def _reject_duplicate_pairs(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise SweepError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_bound_json(
    path: Path,
    expected_sha256: str,
    label: str,
) -> tuple[dict[str, object], str]:
    try:
        payload = path.read_bytes()
    except OSError as error:
        raise SweepError(f"read {label} {path}: {error}") from error
    actual_sha256 = hashlib.sha256(payload).hexdigest()
    if actual_sha256 != expected_sha256.lower():
        raise SweepError(f"{label} SHA-256 differs from expected identity")
    try:
        value = json.loads(
            payload.decode("utf-8"),
            object_pairs_hook=_reject_duplicate_pairs,
        )
    except (UnicodeError, json.JSONDecodeError) as error:
        raise SweepError(f"parse {label} {path}: {error}") from error
    if not isinstance(value, dict):
        raise SweepError(f"{label} root must be an object")
    return value, actual_sha256


def load_excluded_shas(path: Path, expected_sha256: str) -> tuple[set[str], str]:
    try:
        payload = path.read_bytes()
    except OSError as error:
        raise SweepError(f"read excluded SHA list {path}: {error}") from error
    actual_sha256 = hashlib.sha256(payload).hexdigest()
    if actual_sha256 != expected_sha256.lower():
        raise SweepError("excluded SHA file differs from expected identity")
    try:
        lines = payload.decode("ascii").splitlines()
    except UnicodeError as error:
        raise SweepError(f"decode excluded SHA list {path}: {error}") from error
    result: set[str] = set()
    for line_number, raw_value in enumerate(lines, 1):
        value = raw_value.strip().lower()
        if not value:
            continue
        if len(value) != 64 or any(char not in "0123456789abcdef" for char in value):
            raise SweepError(f"excluded SHA line {line_number} is invalid")
        if value in result:
            raise SweepError(f"excluded SHA line {line_number} is duplicated")
        result.add(value)
    if not result:
        raise SweepError("excluded SHA list is empty")
    return result, actual_sha256


def resolve_output_path(output: Path, provenance_inputs: tuple[Path, ...]) -> Path:
    resolved_output = output.resolve()
    for source in provenance_inputs:
        resolved_source = source.resolve(strict=True)
        if resolved_output == resolved_source:
            raise SweepError("output path aliases a provenance input")
        if resolved_output.exists() and resolved_output.samefile(resolved_source):
            raise SweepError("output path aliases a provenance input")
    return resolved_output


def registration_abi(entry: dict[str, object]) -> str | None:
    exports = entry.get("export_names")
    if not isinstance(exports, list) or not all(
        isinstance(value, str) for value in exports
    ):
        raise SweepError("inventory entry export_names must be a string array")
    has_v1 = REGISTRATION_EXPORTS["v1"] in exports
    has_v2 = REGISTRATION_EXPORTS["v2"] in exports
    if has_v1 and has_v2:
        raise SweepError("inventory entry ambiguously exports PluginData v1 and v2")
    if has_v2:
        return "v2"
    if has_v1:
        return "v1"
    return None


def is_eligible(entry: dict[str, object]) -> bool:
    canonical_path = entry.get("canonical_path")
    relative_path = entry.get("relative_to_root")
    size = entry.get("size")
    canonical_components = (
        canonical_path.replace("/", "\\").split("\\")
        if isinstance(canonical_path, str)
        else []
    )
    relative_components = (
        relative_path.replace("/", "\\").split("\\")
        if isinstance(relative_path, str)
        else []
    )
    expected_suffix = [
        "Adobe After Effects 2025",
        "Support Files",
        "Plug-ins",
        *relative_components,
    ]
    canonical_suffix_matches = len(canonical_components) >= len(
        expected_suffix
    ) and all(
        actual.casefold() == expected.casefold()
        for actual, expected in zip(
            canonical_components[-len(expected_suffix) :],
            expected_suffix,
            strict=True,
        )
    )
    if (
        entry.get("architecture") != "x64"
        or entry.get("valid_pe") is not True
        or entry.get("product_target_candidate") is not True
        or entry.get("fixture_hint") is not False
        or entry.get("backup_hint") is not False
        or entry.get("source_category") != "adobe_installed"
        or entry.get("root_category") != "adobe_ae_plugins"
        or not isinstance(canonical_path, str)
        or not isinstance(relative_path, str)
        or not relative_path.casefold().startswith("effects\\")
        or not canonical_suffix_matches
        or not isinstance(size, int)
        or isinstance(size, bool)
        or size <= 0
        or size > MAX_PLUGIN_BYTES
    ):
        return False
    return not any(
        component.casefold().startswith("aud_")
        for component in relative_path.replace("/", "\\").split("\\")
    )


def unique_eligible_by_sha(
    inventory: dict[str, object],
) -> dict[str, dict[str, object]]:
    entries = inventory.get("entries")
    if not isinstance(entries, list):
        raise SweepError("Windows inventory entries must be an array")
    grouped: dict[str, list[dict[str, object]]] = defaultdict(list)
    for index, value in enumerate(entries):
        if not isinstance(value, dict):
            raise SweepError(f"inventory entry {index} is not an object")
        if not is_eligible(value):
            continue
        sha = value.get("sha256")
        if (
            not isinstance(sha, str)
            or len(sha) != 64
            or any(char not in "0123456789abcdefABCDEF" for char in sha)
        ):
            raise SweepError(f"eligible inventory entry {index} has invalid SHA-256")
        grouped[sha.lower()].append(value)

    result: dict[str, dict[str, object]] = {}
    for sha, matches in grouped.items():
        identities = {
            (
                match.get("size"),
                registration_abi(match),
                match.get("architecture"),
                match.get("source_category"),
            )
            for match in matches
        }
        if len(identities) != 1:
            raise SweepError(f"duplicate SHA has inconsistent inventory identity: {sha}")
        result[sha] = min(
            matches,
            key=lambda match: str(match.get("canonical_path", "")).casefold(),
        )
    return result


def select_tranche(
    inventory: dict[str, object],
    excluded_shas: set[str],
    per_registration_abi: int,
) -> list[dict[str, object]]:
    if per_registration_abi <= 0 or per_registration_abi > 64:
        raise SweepError("per-registration-abi must be between 1 and 64")
    eligible = unique_eligible_by_sha(inventory)
    missing_exclusions = sorted(excluded_shas - set(eligible))
    if missing_exclusions:
        raise SweepError(
            "excluded SHA is absent from the eligible source inventory: "
            + ", ".join(missing_exclusions[:4])
        )

    selected: list[dict[str, object]] = []
    for abi in ("v1", "v2"):
        candidates = [
            entry
            for sha, entry in eligible.items()
            if sha not in excluded_shas and registration_abi(entry) == abi
        ]
        candidates.sort(
            key=lambda entry: (
                int(entry["size"]),
                str(entry["sha256"]).casefold(),
            )
        )
        if len(candidates) < per_registration_abi:
            raise SweepError(
                f"only {len(candidates)} eligible {abi} entries remain; "
                f"{per_registration_abi} required"
            )
        for entry in candidates[:per_registration_abi]:
            selected.append(
                {
                    "canonical_path": entry["canonical_path"],
                    "relative_to_root": entry["relative_to_root"],
                    "size": entry["size"],
                    "sha256": str(entry["sha256"]).lower(),
                    "export_names": entry["export_names"],
                    "root_category": entry["root_category"],
                    "source_category": entry["source_category"],
                    "architecture": entry["architecture"],
                    "registration_abi": abi,
                }
            )
    selected.sort(key=lambda entry: str(entry["sha256"]))
    if len({entry["sha256"] for entry in selected}) != len(selected):
        raise SweepError("selected tranche contains duplicate SHA identities")
    return selected


def build_manifest(args: argparse.Namespace) -> dict[str, object]:
    inventory_path = args.inventory.resolve(strict=True)
    summary_path = args.windows_summary.resolve(strict=True)
    excluded_path = args.exclude_sha_file.resolve(strict=True)
    inventory, inventory_sha = load_bound_json(
        inventory_path,
        args.expected_inventory_sha256,
        "Windows inventory",
    )
    windows_summary, summary_sha = load_bound_json(
        summary_path,
        args.expected_summary_sha256,
        "Windows summary",
    )
    if inventory.get("schema_version") != 1:
        raise SweepError("Windows inventory schema_version must be 1")
    validate_source_pair(inventory, windows_summary, inventory_sha)
    excluded, excluded_sha = load_excluded_shas(
        excluded_path,
        args.expected_excluded_sha256,
    )
    if len(excluded) != args.expected_excluded_count:
        raise SweepError(
            f"excluded SHA count differs: {len(excluded)} != "
            f"{args.expected_excluded_count}"
        )
    entries = select_tranche(inventory, excluded, args.per_registration_abi)
    return {
        "schema_version": 1,
        "source": {
            "windows_inventory_sha256": inventory_sha,
            "windows_summary_sha256": summary_sha,
            "windows_inventory_entries": len(inventory["entries"]),
            "excluded_sha256_file_sha256": excluded_sha,
            "excluded_sha256_count": len(excluded),
            "excluded_sha256s": sorted(excluded),
        },
        "rule": {
            "release": "After Effects 2025",
            "root_prefix": "Effects\\",
            "max_bytes": MAX_PLUGIN_BYTES,
            "exclude_audio_prefix": True,
            "registration_abis": ["v1", "v2"],
            "per_registration_abi": args.per_registration_abi,
            "selection_partition_order": ["v1", "v2"],
            "selection_rank_order": ["size", "sha256"],
            "output_order": ["sha256"],
        },
        "entries": entries,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--inventory", type=Path, required=True)
    parser.add_argument("--windows-summary", type=Path, required=True)
    parser.add_argument("--exclude-sha-file", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--expected-inventory-sha256", required=True)
    parser.add_argument("--expected-summary-sha256", required=True)
    parser.add_argument("--expected-excluded-sha256", required=True)
    parser.add_argument("--expected-excluded-count", type=int, required=True)
    parser.add_argument("--per-registration-abi", type=int, default=12)
    return parser.parse_args()


def main() -> int:
    try:
        args = parse_args()
        output = resolve_output_path(
            args.output,
            (
                args.inventory,
                args.windows_summary,
                args.exclude_sha_file,
            ),
        )
        manifest = build_manifest(args)
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(
            json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
    except (OSError, SweepError) as error:
        print(f"macos_aex_tranche_error: {error}", file=sys.stderr)
        return 1
    print(
        json.dumps(
            {
                "entry_count": len(manifest["entries"]),
                "output": str(output),
                "total_bytes": sum(
                    int(entry["size"]) for entry in manifest["entries"]
                ),
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

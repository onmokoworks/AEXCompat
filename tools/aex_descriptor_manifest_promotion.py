#!/usr/bin/env python3
"""Regenerate and compare a promoted AEX descriptor manifest from an L2 report."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import math
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
L2_ROOT = LAB_ROOT / "target" / "l2-results"
PROFILE_ROOT = LAB_ROOT / "profiles"
OUTPUT_ROOT = LAB_ROOT / "target" / "descriptor-manifest-promotion"
INPUT_LIMIT = 1024 * 1024
MANIFEST_LIMIT = 64 * 1024
NUMERIC_TYPES = {1: "integer", 4: "integer", 7: "integer", 10: "float"}


def resolve_json(path: Path, root: Path, *, must_exist: bool) -> Path:
    if path.suffix.lower() != ".json" or any(part in (".", "..") for part in path.parts):
        raise ValueError("JSON path is invalid")
    root.mkdir(parents=True, exist_ok=True)
    absolute = path if path.is_absolute() else LAB_ROOT / path
    resolved = absolute.resolve(strict=must_exist)
    if not resolved.is_relative_to(root.resolve(strict=True)):
        raise ValueError(f"path must stay under {root}")
    if not must_exist:
        resolved.parent.mkdir(parents=True, exist_ok=True)
        if not resolved.parent.resolve(strict=True).is_relative_to(root.resolve(strict=True)):
            raise ValueError("output parent escapes promotion root")
    return resolved


def read_object(path: Path, limit: int) -> dict[str, Any]:
    if path.stat().st_size <= 0 or path.stat().st_size > limit:
        raise ValueError("JSON input size invalid")
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError("JSON input must be an object")
    return value


def finite_number(value: Any, label: str) -> int | float:
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
        raise ValueError(f"{label} must be finite numeric")
    return value


def canonical_bytes(manifest: dict[str, Any]) -> bytes:
    normalized = copy.deepcopy(manifest)
    for descriptor in normalized.get("descriptors", []):
        for key in ("minimum", "maximum", "default"):
            if key in descriptor:
                descriptor[key] = float(descriptor[key])
    return json.dumps(normalized, ensure_ascii=False, separators=(",", ":")).encode("utf-8")


def canonical_sha256(manifest: dict[str, Any]) -> str:
    return hashlib.sha256(canonical_bytes(manifest)).hexdigest().upper()


def validate_l2(report: dict[str, Any]) -> tuple[str, str, str, list[dict[str, Any]]]:
    worker = report.get("worker_report")
    if (
        report.get("schema_version") != 1
        or report.get("stage") != "L2"
        or report.get("passed") is not True
        or report.get("worker_exit") != "ok"
        or not isinstance(report.get("plugin_id"), str)
        or not isinstance(report.get("expected_sha256"), str)
        or len(report["expected_sha256"]) != 64
        or not isinstance(report.get("receipt_id"), str)
        or not report["receipt_id"]
        or not isinstance(worker, dict)
        or worker.get("status") != "selectors_completed"
        or worker.get("params_setup_error") != 0
        or worker.get("render_performed") is not False
        or not isinstance(worker.get("parameters"), list)
        or worker.get("reported_num_params") != len(worker["parameters"]) + 1
    ):
        raise ValueError("L2 report is not a successful descriptor observation")
    if not all(character in "0123456789abcdefABCDEF" for character in report["expected_sha256"]):
        raise ValueError("L2 plugin digest is malformed")
    return (
        report["plugin_id"],
        report["expected_sha256"].upper(),
        report["receipt_id"],
        worker["parameters"],
    )


def regenerate(report: dict[str, Any], promoted: dict[str, Any]) -> tuple[dict[str, Any], list[str]]:
    plugin_id, plugin_sha256, receipt_id, observed = validate_l2(report)
    if promoted.get("schema_version") != 1 or promoted.get("plugin_id") != plugin_id:
        raise ValueError("promoted manifest identity mismatch")
    promoted_descriptors = promoted.get("descriptors")
    if not isinstance(promoted_descriptors, list) or not promoted_descriptors:
        raise ValueError("promoted manifest descriptors missing")
    curation: dict[str, dict[str, Any]] = {}
    for descriptor in promoted_descriptors:
        if not isinstance(descriptor, dict) or not isinstance(descriptor.get("display_name"), str):
            raise ValueError("promoted descriptor is malformed")
        if descriptor["display_name"] in curation:
            raise ValueError("promoted descriptor names must be unique")
        curation[descriptor["display_name"]] = descriptor

    candidate_descriptors: list[dict[str, Any]] = []
    review_required: list[str] = []
    observed_names: set[str] = set()
    for slot, parameter in enumerate(observed, start=1):
        if not isinstance(parameter, dict) or not isinstance(parameter.get("name"), str):
            raise ValueError("observed descriptor is malformed")
        name = parameter["name"]
        observed_type = parameter.get("type")
        if name in observed_names or isinstance(observed_type, bool) or not isinstance(observed_type, int):
            raise ValueError("observed descriptor identity is malformed")
        observed_names.add(name)
        reviewed = curation.get(name)
        assignable = bool(reviewed and reviewed.get("assignable") is True)
        descriptor: dict[str, Any] = {
            "slot": slot,
            "observed_type": observed_type,
            "display_name": name,
            "assignable": assignable,
        }
        if reviewed is None:
            review_required.append(f"new_descriptor:{name}")
        elif reviewed.get("slot") != slot:
            review_required.append(f"slot_changed:{name}")
        if assignable:
            kind = NUMERIC_TYPES.get(observed_type)
            if kind is None or reviewed.get("kind") != kind or not isinstance(reviewed.get("id"), str):
                raise ValueError("assignable descriptor curation conflicts with observed type")
            minimum = finite_number(parameter.get("valid_min"), f"{name} minimum")
            maximum = finite_number(parameter.get("valid_max"), f"{name} maximum")
            default = finite_number(parameter.get("default"), f"{name} default")
            if minimum > maximum or default < minimum or default > maximum:
                raise ValueError("observed descriptor range/default is invalid")
            descriptor.update(
                id=reviewed["id"],
                kind=kind,
                minimum=minimum,
                maximum=maximum,
                default=default,
            )
        candidate_descriptors.append(descriptor)
    for missing in sorted(curation.keys() - observed_names):
        review_required.append(f"missing_descriptor:{missing}")
    candidate = {
        "schema_version": 1,
        "plugin_id": plugin_id,
        "source": {
            "stage": "L2",
            "plugin_sha256": plugin_sha256,
            "receipt_id": receipt_id,
        },
        "descriptors": candidate_descriptors,
    }
    return candidate, review_required


def write_new(path: Path, value: dict[str, Any]) -> None:
    with path.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(value, handle, ensure_ascii=False, indent=2)
        handle.write("\n")


def run(source_path: Path, promoted_path: Path, candidate_path: Path, report_path: Path) -> bool:
    source = read_object(resolve_json(source_path, L2_ROOT, must_exist=True), INPUT_LIMIT)
    promoted = read_object(resolve_json(promoted_path, PROFILE_ROOT, must_exist=True), MANIFEST_LIMIT)
    candidate_output = resolve_json(candidate_path, OUTPUT_ROOT, must_exist=False)
    report_output = resolve_json(report_path, OUTPUT_ROOT, must_exist=False)
    if candidate_output == report_output or candidate_output.exists() or report_output.exists():
        raise FileExistsError("promotion outputs must be distinct create-new files")
    candidate, review_required = regenerate(source, promoted)
    matches = candidate == promoted and not review_required
    report = {
        "schema_version": 1,
        "report_kind": "aex_descriptor_manifest_promotion",
        "plugin_id": candidate["plugin_id"],
        "source_stage": "L2",
        "source_receipt_id": candidate["source"]["receipt_id"],
        "observed_descriptor_count": len(candidate["descriptors"]),
        "assignable_descriptor_count": sum(item["assignable"] for item in candidate["descriptors"]),
        "candidate_canonical_sha256": canonical_sha256(candidate),
        "promoted_canonical_sha256": canonical_sha256(promoted),
        "matches_promoted": matches,
        "review_required": review_required,
        "native_process_started": False,
    }
    write_new(candidate_output, candidate)
    write_new(report_output, report)
    return matches


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source_l2_report", type=Path)
    parser.add_argument("promoted_manifest", type=Path)
    parser.add_argument("candidate_output", type=Path)
    parser.add_argument("comparison_report", type=Path)
    args = parser.parse_args(argv)
    try:
        matches = run(
            args.source_l2_report,
            args.promoted_manifest,
            args.candidate_output,
            args.comparison_report,
        )
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"descriptor manifest promotion failed: {error}")
        return 2
    return 0 if matches else 3


if __name__ == "__main__":
    raise SystemExit(main())

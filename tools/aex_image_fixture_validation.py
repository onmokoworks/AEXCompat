#!/usr/bin/env python3
"""Validate generated PPM image fixtures without AEX/AE/OFX runtime access.

This reads an image fixture suite JSON and generated PPM fixtures only. It
verifies manifest/file consistency and records byte-level fixture invariants.
It never opens AEX files, invokes AE, renders, or routes OFX.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TOOLS_ROOT = LAB_ROOT / "tools"
if str(TOOLS_ROOT) not in sys.path:
    sys.path.insert(0, str(TOOLS_ROOT))

import ppm_fixture_tool

TARGET_ROOT = LAB_ROOT / "target"
IMAGE_SUITE_ROOT = TARGET_ROOT / "image-fixture-suite"
VALIDATION_ROOT = TARGET_ROOT / "image-fixture-validation"
PPM_FIXTURE_ROOT = TARGET_ROOT / "ppm-fixtures"

SAFETY_FLAGS = (
    "native_load_enabled",
    "native_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
    "aex_file_opened",
)

ALLOWED_PATTERNS = {"gradient", "checker", "solid"}


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


def validate_suite_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("image fixture suite must have .json extension")
    return resolve_under_root(path, IMAGE_SUITE_ROOT, must_exist=True)


def validate_ppm_path(path: Path) -> Path:
    if path.suffix.lower() != ".ppm":
        raise ValueError("fixture PPM must have .ppm extension")
    return resolve_under_root(path, PPM_FIXTURE_ROOT, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("image fixture validation output must have .json extension")
    VALIDATION_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, VALIDATION_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(VALIDATION_ROOT.resolve(strict=True)):
        raise ValueError(f"image fixture validation parent must stay under {VALIDATION_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_image_suite(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_suite_path(path)
    return read_json_object(resolved), resolved


def validate_suite(suite: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if suite.get("report_kind") != "aex_image_fixture_suite":
        errors.append("source report_kind must be aex_image_fixture_suite")
    if suite.get("suite_state") != "image_fixture_suite_ready":
        errors.append("source suite_state must be image_fixture_suite_ready")
    if suite.get("publication_status") != "local-only":
        errors.append("source publication_status must be local-only")
    fixtures = suite.get("fixtures")
    if not isinstance(fixtures, list) or not fixtures:
        errors.append("source fixtures must be a non-empty list")
    for flag in SAFETY_FLAGS:
        if suite.get(flag) is not False:
            errors.append(f"source {flag} must be false")
    if isinstance(fixtures, list):
        for index, fixture in enumerate(fixtures):
            if not isinstance(fixture, dict):
                errors.append(f"fixture {index} must be an object")
                continue
            if not isinstance(fixture.get("case_id"), str) or not fixture.get("case_id"):
                errors.append(f"fixture {index} case_id must be a non-empty string")
            if not isinstance(fixture.get("ppm_path"), str):
                errors.append(f"fixture {index} ppm_path must be a string")
    return errors


def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def expected_int(value: Any) -> int | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value
    return None


def validate_fixture_row(
    fixture: dict[str, Any],
    *,
    seen_case_ids: set[str],
    seen_paths: set[str],
) -> dict[str, Any]:
    errors: list[str] = []
    case_id = str(fixture.get("case_id", ""))
    pattern = str(fixture.get("pattern", ""))
    if case_id in seen_case_ids:
        errors.append("case_id is duplicated")
    seen_case_ids.add(case_id)
    if pattern not in ALLOWED_PATTERNS:
        errors.append("pattern is not recognized")

    resolved_path: Path | None = None
    image: ppm_fixture_tool.PpmImage | None = None
    raw_bytes = b""
    try:
        resolved_path = validate_ppm_path(Path(str(fixture.get("ppm_path"))))
        path_key = str(resolved_path).lower()
        if path_key in seen_paths:
            errors.append("ppm_path is duplicated")
        seen_paths.add(path_key)
        raw_bytes = resolved_path.read_bytes()
        image = ppm_fixture_tool.read_ppm(resolved_path)
    except (OSError, ValueError) as exc:
        errors.append(f"ppm validation failed: {exc}")

    manifest_width = expected_int(fixture.get("width"))
    manifest_height = expected_int(fixture.get("height"))
    manifest_pixel_bytes = expected_int(fixture.get("pixel_bytes"))
    actual_width = image.width if image else None
    actual_height = image.height if image else None
    actual_pixel_bytes = len(image.pixels) if image else None
    if image is not None:
        expected_bytes = image.width * image.height * 3
        if actual_pixel_bytes != expected_bytes:
            errors.append("actual pixel byte count does not match dimensions")
        if manifest_width is not None and manifest_width != image.width:
            errors.append("manifest width does not match PPM width")
        if manifest_height is not None and manifest_height != image.height:
            errors.append("manifest height does not match PPM height")
        if manifest_pixel_bytes is not None and manifest_pixel_bytes != actual_pixel_bytes:
            errors.append("manifest pixel_bytes does not match PPM pixel byte count")
    return {
        "case_id": case_id,
        "pattern": pattern,
        "ppm_path": str(resolved_path) if resolved_path else fixture.get("ppm_path"),
        "manifest_width": manifest_width,
        "manifest_height": manifest_height,
        "manifest_pixel_bytes": manifest_pixel_bytes,
        "actual_width": actual_width,
        "actual_height": actual_height,
        "actual_pixel_bytes": actual_pixel_bytes,
        "file_size_bytes": len(raw_bytes) if raw_bytes else None,
        "pixel_sha256": sha256_hex(image.pixels) if image else None,
        "file_sha256": sha256_hex(raw_bytes) if raw_bytes else None,
        "validation_status": "passed" if not errors else "failed",
        "errors": errors,
    }


def summarize(rows: list[dict[str, Any]]) -> dict[str, Any]:
    pattern_counts: dict[str, int] = {}
    for row in rows:
        pattern = str(row.get("pattern"))
        pattern_counts[pattern] = pattern_counts.get(pattern, 0) + 1
    passed_count = sum(1 for row in rows if row.get("validation_status") == "passed")
    total_pixel_bytes = sum(int(row.get("actual_pixel_bytes") or 0) for row in rows)
    return {
        "fixture_count": len(rows),
        "passed_count": passed_count,
        "failed_count": len(rows) - passed_count,
        "total_pixel_bytes": total_pixel_bytes,
        "pattern_counts": dict(sorted(pattern_counts.items())),
    }


def build_validation_report(suite: dict[str, Any], suite_path: Path) -> dict[str, Any]:
    suite_errors = validate_suite(suite)
    if suite_errors:
        raise ValueError("; ".join(suite_errors))
    seen_case_ids: set[str] = set()
    seen_paths: set[str] = set()
    fixture_results = [
        validate_fixture_row(fixture, seen_case_ids=seen_case_ids, seen_paths=seen_paths)
        for fixture in suite.get("fixtures", [])
    ]
    summary = summarize(fixture_results)
    all_errors = [
        f"{row.get('case_id')}: {error}"
        for row in fixture_results
        for error in row.get("errors", [])
    ]
    validation_passed = not all_errors
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_image_fixture_validation",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_image_fixture_suite": str(suite_path),
        "source_suite_state": suite.get("suite_state"),
        "target_candidate": suite.get("target_candidate"),
        "validation_state": "image_fixture_validation_passed_no_load"
        if validation_passed
        else "image_fixture_validation_failed",
        "validation_passed": validation_passed,
        "fixture_results": fixture_results,
        "summary": summary,
        "errors": all_errors,
        "native_load_enabled": False,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "notes": [
            "Validation reads image-suite JSON and generated PPM fixtures only.",
            "Hashes are computed only for generated PPM fixtures, not AEX binaries.",
            "No AEX file is opened, copied, hashed, loaded, or executed.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Validate generated AEX image fixture suite")
    parser.add_argument("--image-suite", required=True, help="Image fixture suite JSON under target/image-fixture-suite")
    parser.add_argument("--out", required=True, help="Create-new validation JSON under target/image-fixture-validation")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    suite, suite_path = load_image_suite(Path(args.image_suite))
    report = build_validation_report(suite, suite_path)
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0 if report["validation_passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())

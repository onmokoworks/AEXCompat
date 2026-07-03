#!/usr/bin/env python3
"""Build a no-load compatibility card for the selected AEX candidate.

The card combines static candidate metadata, PiPL/resource metadata, no-load
image/worker/OFX mock runner evidence, and provenance-answer validator state.
It reads JSON artifacts only and never opens, hashes, copies, loads, renders,
or routes the candidate AEX.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
CANDIDATE_MATRIX_ROOT = TARGET_ROOT / "candidate-matrix"
PIPL_CATALOG_ROOT = TARGET_ROOT / "pipl-resource-catalog"
CANDIDATE_RUNNER_ROOT = TARGET_ROOT / "candidate-test-runner"
ANSWER_VALIDATOR_ROOT = TARGET_ROOT / "fixture-provenance-answer-validator-selftest"
COMPAT_CARD_ROOT = TARGET_ROOT / "candidate-compat-card"

SAFETY_FLAGS = (
    "native_load_enabled",
    "native_load_performed",
    "dll_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
    "aex_file_opened",
    "aex_file_hashed",
    "aex_file_copied",
    "aepx_file_modified",
    "aep_binary_modified",
    "ae_project_write_performed",
    "ofx_plugin_built",
    "ofx_describe_performed",
    "ofx_render_performed",
    "aex_render_performed",
    "render_validation_performed",
    "pipl_payload_parsed",
    "parameter_schema_emitted",
    "redacted_schema_emitted",
    "real_pipl_payload_parser_enabled",
    "real_pipl_payload_parsed",
    "resource_payload_opened",
    "resource_payload_extracted",
    "raw_payload_serialized",
)
BLOCKED_ACTIONS = (
    "accept_aex_path",
    "open_aex_file",
    "hash_aex_file",
    "copy_selected_aex_fixture",
    "load_aex_dll",
    "load_dependency_dll",
    "call_EffectMain",
    "dispatch_PF_Cmd",
    "start_after_effects",
    "render_with_aex",
    "route_through_real_ofx",
    "build_ofx_binary",
    "ofx_describe_from_aex",
    "ofx_render_with_aex",
    "parse_real_pipl_payload",
    "emit_parameter_schema",
    "emit_redacted_schema",
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


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("candidate compatibility card must have .json extension")
    COMPAT_CARD_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, COMPAT_CARD_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(COMPAT_CARD_ROOT.resolve(strict=True)):
        raise ValueError(f"candidate compatibility card parent must stay under {COMPAT_CARD_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_candidate_matrix(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, CANDIDATE_MATRIX_ROOT, "candidate matrix")
    return read_json_object(resolved), resolved


def load_pipl_catalog(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, PIPL_CATALOG_ROOT, "PiPL/resource catalog")
    return read_json_object(resolved), resolved


def load_candidate_runner(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, CANDIDATE_RUNNER_ROOT, "candidate no-load runner")
    return read_json_object(resolved), resolved


def load_answer_validator(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, ANSWER_VALIDATOR_ROOT, "fixture provenance answer validator selftest")
    return read_json_object(resolved), resolved


def safety_errors(payload: dict[str, Any], label: str, *, require_false_if_present: bool = True) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in payload:
            if payload.get(flag) is not False:
                errors.append(f"{label} {flag} must be false")
        elif not require_false_if_present:
            continue
    return errors


def validate_candidate_matrix(matrix: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if matrix.get("publication_status") != "local-only":
        errors.append("candidate matrix publication_status must be local-only")
    if matrix.get("report_kind") != "aex_candidate_matrix":
        errors.append("candidate matrix report_kind must be aex_candidate_matrix")
    if matrix.get("matrix_state") != "candidate_matrix_ready":
        errors.append("candidate matrix matrix_state must be candidate_matrix_ready")
    rows = matrix.get("rows")
    if not isinstance(rows, list) or not rows:
        errors.append("candidate matrix rows must be a non-empty list")
    errors.extend(safety_errors(matrix, "candidate matrix"))
    return errors


def validate_pipl_catalog(catalog: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if catalog.get("publication_status") != "local-only":
        errors.append("PiPL catalog publication_status must be local-only")
    if catalog.get("report_kind") != "aex_pipl_resource_catalog":
        errors.append("PiPL catalog report_kind must be aex_pipl_resource_catalog")
    if catalog.get("catalog_state") != "pipl_resource_catalog_ready_no_payload":
        errors.append("PiPL catalog catalog_state must be pipl_resource_catalog_ready_no_payload")
    if catalog.get("payload_policy") != "metadata_only_no_resource_payload":
        errors.append("PiPL catalog payload_policy must be metadata-only")
    rows = catalog.get("rows")
    if not isinstance(rows, list) or not rows:
        errors.append("PiPL catalog rows must be a non-empty list")
    errors.extend(safety_errors(catalog, "PiPL catalog"))
    return errors


def validate_candidate_runner(runner: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if runner.get("publication_status") != "local-only":
        errors.append("candidate runner publication_status must be local-only")
    if runner.get("report_kind") != "aex_candidate_no_load_test_runner":
        errors.append("candidate runner report_kind must be aex_candidate_no_load_test_runner")
    if runner.get("runner_state") != "candidate_no_load_test_runner_passed_native_closed":
        errors.append("candidate runner must be passed native closed")
    true_fields = (
        "runner_ready",
        "no_load_execution_performed",
        "worker_invoked",
        "ofx_mock_invoked",
        "worker_identity_passed",
        "ofx_noop_identity_passed",
        "image_fixture_validation_passed",
        "image_smoke_identity_passed",
        "render_contract_review_ready",
        "ofx_route_contract_review_ready",
        "blocked_load_aex_verified",
    )
    for key in true_fields:
        if runner.get(key) is not True:
            errors.append(f"candidate runner {key} must be true")
    false_fields = (
        "native_execution_performed",
        "real_render_execution_performed",
        "real_ofx_route_execution_performed",
        "ofx_runtime_invoked",
        "approval_manifest_created",
        "fixture_approval_satisfied",
        "path_acceptance_ready",
        "aex_path_acceptance_enabled",
        "path_payload_supplied",
        "real_render_open",
        "real_route_open",
    )
    for key in false_fields:
        if runner.get(key) is not False:
            errors.append(f"candidate runner {key} must be false")
    if runner.get("native_load_gate") != "closed":
        errors.append("candidate runner native_load_gate must be closed")
    if runner.get("accepted_aex_path") is not None:
        errors.append("candidate runner accepted_aex_path must be null")
    if not isinstance(runner.get("candidate_relative_path"), str):
        errors.append("candidate runner candidate_relative_path must be a string")
    if (runner.get("executed_worker_case_count") or 0) <= 0:
        errors.append("candidate runner executed_worker_case_count must be positive")
    if (runner.get("executed_ofx_noop_case_count") or 0) <= 0:
        errors.append("candidate runner executed_ofx_noop_case_count must be positive")
    errors.extend(safety_errors(runner, "candidate runner"))
    return errors


def validate_answer_validator(validator: dict[str, Any], candidate_relative_path: str | None) -> list[str]:
    errors: list[str] = []
    if validator.get("publication_status") != "local-only":
        errors.append("answer validator publication_status must be local-only")
    if validator.get("report_kind") != "aex_fixture_provenance_answer_validator_selftest":
        errors.append("answer validator report_kind must be aex_fixture_provenance_answer_validator_selftest")
    if validator.get("validator_selftest_state") != "fixture_provenance_answer_validator_selftest_passed_no_user_answers":
        errors.append("answer validator selftest must be passed no-user-answers")
    if validator.get("validator_ready") is not True:
        errors.append("answer validator validator_ready must be true")
    if validator.get("real_user_answer_artifact_consumed") is not False:
        errors.append("answer validator must not consume real user answers")
    if validator.get("synthetic_payloads_serialized") is not False:
        errors.append("answer validator synthetic payloads must not be serialized")
    if validator.get("answer_schema_validated") is not True:
        errors.append("answer validator answer_schema_validated must be true")
    if validator.get("candidate_relative_path") != candidate_relative_path:
        errors.append("answer validator candidate_relative_path must match runner")
    if validator.get("answers_present") is not False:
        errors.append("answer validator answers_present must be false")
    if validator.get("answers_validated_for_manual_review") is not False:
        errors.append("answer validator must not validate real manual review answers yet")
    if validator.get("approval_can_be_issued_now") is not False:
        errors.append("answer validator approval_can_be_issued_now must be false")
    if validator.get("fixture_approval_satisfied") is not False:
        errors.append("answer validator fixture_approval_satisfied must be false")
    if validator.get("native_load_gate") != "closed":
        errors.append("answer validator native_load_gate must be closed")
    if validator.get("accepted_aex_path") is not None:
        errors.append("answer validator accepted_aex_path must be null")
    if (validator.get("synthetic_case_count") or 0) <= 0:
        errors.append("answer validator synthetic_case_count must be positive")
    if validator.get("synthetic_case_count") != validator.get("synthetic_case_passed_count"):
        errors.append("answer validator synthetic cases must all pass")
    if validator.get("synthetic_case_failed_count") != 0:
        errors.append("answer validator synthetic_case_failed_count must be zero")
    errors.extend(safety_errors(validator, "answer validator"))
    return errors


def find_row(rows: Any, candidate_relative_path: str, label: str) -> dict[str, Any]:
    if not isinstance(rows, list):
        raise ValueError(f"{label} rows must be a list")
    for row in rows:
        if isinstance(row, dict) and row.get("relative_path") == candidate_relative_path:
            return row
    raise ValueError(f"{label} row for selected candidate not found")


def no_path_fixture_results(report: Any) -> list[dict[str, Any]]:
    if not isinstance(report, dict):
        return []
    results = report.get("fixture_results")
    if not isinstance(results, list):
        return []
    compact: list[dict[str, Any]] = []
    for item in results:
        if not isinstance(item, dict):
            continue
        check = item.get("identity_check") if isinstance(item.get("identity_check"), dict) else {}
        inspect = item.get("inspect") if isinstance(item.get("inspect"), dict) else {}
        width = check.get("width", inspect.get("width"))
        height = check.get("height", inspect.get("height"))
        byte_count = check.get("bytes", inspect.get("bytes"))
        compact.append(
            {
                "case_id": item.get("case_id"),
                "pattern": item.get("pattern"),
                "width": width,
                "height": height,
                "bytes": byte_count,
                "pixel_match": check.get("pixel_match"),
                "dimension_match": check.get("dimension_match"),
                "mock_state": item.get("mock_state"),
                "ppm_paths_exported": False,
            }
        )
    return compact


def build_metadata_card(matrix_row: dict[str, Any], catalog_row: dict[str, Any]) -> dict[str, Any]:
    return {
        "relative_path": matrix_row.get("relative_path"),
        "file_name": matrix_row.get("file_name"),
        "size_bytes": matrix_row.get("size_bytes"),
        "mtime_utc": matrix_row.get("mtime_utc"),
        "compatibility_class": matrix_row.get("compatibility_class"),
        "review_bucket": matrix_row.get("review_bucket"),
        "fixture_candidate_score": matrix_row.get("fixture_candidate_score"),
        "fixture_candidate_reasons": matrix_row.get("fixture_candidate_reasons", []),
        "risk_flags": matrix_row.get("risk_flags", []),
        "machine_label": matrix_row.get("machine_label"),
        "dll_image": matrix_row.get("dll_image"),
        "effect_main_export_present": matrix_row.get("effect_main_export_present"),
        "effect_main_marker_present": matrix_row.get("effect_main_marker_present"),
        "aegp_marker_count": matrix_row.get("aegp_marker_count"),
        "imported_dll_count": catalog_row.get("imported_dll_count"),
        "import_dll_names_metadata_only": matrix_row.get("import_dll_names", []),
        "resource_types": catalog_row.get("resource_type_details", []),
        "pipl_metadata": {
            "metadata_state": catalog_row.get("metadata_state"),
            "payload_policy": catalog_row.get("payload_policy"),
            "pipl_signal_present": catalog_row.get("pipl_signal_present"),
            "pipl_resource_type_present": catalog_row.get("pipl_resource_type_present"),
            "pipl_resource_data_entry_count": catalog_row.get("pipl_resource_data_entry_count"),
            "pipl_resource_total_size": catalog_row.get("pipl_resource_total_size"),
            "pipl_resource_entries": catalog_row.get("pipl_resource_entries", []),
            "resource_payload_opened": False,
            "resource_payload_extracted": False,
            "raw_payload_serialized": False,
        },
    }


def build_no_load_test_card(runner: dict[str, Any]) -> dict[str, Any]:
    worker_results = no_path_fixture_results(runner.get("worker_report"))
    ofx_results = no_path_fixture_results(runner.get("ofx_noop_report"))
    return {
        "runner_state": runner.get("runner_state"),
        "runner_ready": runner.get("runner_ready"),
        "no_load_execution_performed": runner.get("no_load_execution_performed"),
        "worker_invoked": runner.get("worker_invoked"),
        "ofx_mock_invoked": runner.get("ofx_mock_invoked"),
        "ofx_runtime_invoked": runner.get("ofx_runtime_invoked"),
        "worker_identity_passed": runner.get("worker_identity_passed"),
        "ofx_noop_identity_passed": runner.get("ofx_noop_identity_passed"),
        "blocked_load_aex_verified": runner.get("blocked_load_aex_verified"),
        "image_fixture_case_count": runner.get("image_fixture_case_count"),
        "executed_worker_case_count": runner.get("executed_worker_case_count"),
        "executed_ofx_noop_case_count": runner.get("executed_ofx_noop_case_count"),
        "planned_no_load_case_count": runner.get("planned_no_load_case_count"),
        "blocked_case_count": runner.get("blocked_case_count"),
        "worker_fixture_results": worker_results,
        "ofx_noop_fixture_results": ofx_results,
        "ppm_paths_exported": False,
        "real_render_open": False,
        "real_route_open": False,
    }


def build_gate_card(validator: dict[str, Any], runner: dict[str, Any]) -> dict[str, Any]:
    return {
        "provenance_answer_validator_state": validator.get("validator_selftest_state"),
        "real_user_answer_artifact_consumed": validator.get("real_user_answer_artifact_consumed"),
        "answers_present": validator.get("answers_present"),
        "answers_validated_for_manual_review": validator.get("answers_validated_for_manual_review"),
        "approval_can_be_issued_now": False,
        "approval_manifest_created": False,
        "fixture_approval_satisfied": False,
        "native_load_gate": "closed",
        "path_acceptance_ready": runner.get("path_acceptance_ready"),
        "aex_path_acceptance_enabled": runner.get("aex_path_acceptance_enabled"),
        "accepted_aex_path": None,
        "native_load_gate_stays_closed": True,
    }


def build_candidate_compatibility_card(
    *,
    candidate_matrix: dict[str, Any],
    candidate_matrix_path: Path,
    pipl_catalog: dict[str, Any],
    pipl_catalog_path: Path,
    candidate_runner: dict[str, Any],
    candidate_runner_path: Path,
    answer_validator: dict[str, Any],
    answer_validator_path: Path,
) -> dict[str, Any]:
    candidate_relative_path = candidate_runner.get("candidate_relative_path")
    if not isinstance(candidate_relative_path, str):
        candidate_relative_path = None
    errors = (
        validate_candidate_matrix(candidate_matrix)
        + validate_pipl_catalog(pipl_catalog)
        + validate_candidate_runner(candidate_runner)
        + validate_answer_validator(answer_validator, candidate_relative_path)
    )
    if errors:
        raise ValueError("; ".join(errors))
    assert isinstance(candidate_relative_path, str)
    matrix_row = find_row(candidate_matrix.get("rows"), candidate_relative_path, "candidate matrix")
    catalog_row = find_row(pipl_catalog.get("rows"), candidate_relative_path, "PiPL catalog")
    if matrix_row.get("file_name") != catalog_row.get("file_name"):
        raise ValueError("candidate matrix and PiPL catalog file_name must match")

    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_compatibility_card",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_candidate_matrix": str(candidate_matrix_path),
        "source_pipl_resource_catalog": str(pipl_catalog_path),
        "source_candidate_runner": str(candidate_runner_path),
        "source_fixture_provenance_answer_validator_selftest": str(answer_validator_path),
        "compatibility_card_state": "candidate_compatibility_card_ready_no_load",
        "compatibility_card_ready": True,
        "card_target": "selected_candidate_no_load_image_ofx_mock_bridge",
        "candidate_relative_path": candidate_relative_path,
        "candidate_metadata": build_metadata_card(matrix_row, catalog_row),
        "no_load_test_card": build_no_load_test_card(candidate_runner),
        "gate_card": build_gate_card(answer_validator, candidate_runner),
        "safe_bridge_surfaces": [
            "static_metadata_summary",
            "pipl_resource_metadata_no_payload",
            "ppm_worker_identity_results_no_paths",
            "ofx_noop_identity_results_no_paths",
            "closed_gate_status",
        ],
        "blocked_actions": list(BLOCKED_ACTIONS),
        "next_safe_actions": [
            "display this card in a local no-load candidate browser",
            "feed metadata-only fields into future image-test planning",
            "author and validate separate user provenance answers before any fixture decision change",
        ],
        "unsafe_exports_present": False,
        "absolute_ppm_paths_exported": False,
        "absolute_aex_paths_exported": False,
        "resource_payload_opened": False,
        "resource_payload_extracted": False,
        "raw_payload_serialized": False,
        "pipl_payload_parsed": False,
        "parameter_schema_emitted": False,
        "redacted_schema_emitted": False,
        "approval_can_be_issued_now": False,
        "approval_manifest_created": False,
        "current_fixture_approval_valid": False,
        "fixture_approval_satisfied": False,
        "approval_gate_stays_closed": True,
        "native_load_gate": "closed",
        "native_load_gate_stays_closed": True,
        "accepted_aex_path": None,
        "path_payload_supplied": False,
        "path_acceptance_ready": False,
        "aex_path_acceptance_enabled": False,
        "real_render_open": False,
        "real_route_open": False,
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "aex_file_hashed": False,
        "aex_file_copied": False,
        "aepx_file_modified": False,
        "aep_binary_modified": False,
        "ae_project_write_performed": False,
        "ofx_plugin_built": False,
        "ofx_describe_performed": False,
        "ofx_render_performed": False,
        "aex_render_performed": False,
        "render_validation_performed": False,
        "real_pipl_payload_parser_enabled": False,
        "real_pipl_payload_parsed": False,
        "notes": [
            "Card reads JSON artifacts only and redacts PPM/AEX absolute paths from test summaries.",
            "PiPL/resource details are metadata-only and contain no resource payload bytes.",
            "The card is a no-load planning bridge, not fixture approval or native-load readiness.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build no-load selected candidate compatibility card")
    parser.add_argument("--candidate-matrix", required=True, help="Candidate matrix JSON under target/candidate-matrix")
    parser.add_argument("--pipl-catalog", required=True, help="PiPL/resource catalog JSON under target/pipl-resource-catalog")
    parser.add_argument("--candidate-runner", required=True, help="Candidate no-load runner JSON under target/candidate-test-runner")
    parser.add_argument(
        "--answer-validator",
        required=True,
        help="Fixture provenance answer validator selftest under target/fixture-provenance-answer-validator-selftest",
    )
    parser.add_argument("--out", required=True, help="Create-new card under target/candidate-compat-card")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    candidate_matrix, candidate_matrix_path = load_candidate_matrix(Path(args.candidate_matrix))
    pipl_catalog, pipl_catalog_path = load_pipl_catalog(Path(args.pipl_catalog))
    candidate_runner, candidate_runner_path = load_candidate_runner(Path(args.candidate_runner))
    answer_validator, answer_validator_path = load_answer_validator(Path(args.answer_validator))
    card = build_candidate_compatibility_card(
        candidate_matrix=candidate_matrix,
        candidate_matrix_path=candidate_matrix_path,
        pipl_catalog=pipl_catalog,
        pipl_catalog_path=pipl_catalog_path,
        candidate_runner=candidate_runner,
        candidate_runner_path=candidate_runner_path,
        answer_validator=answer_validator,
        answer_validator_path=answer_validator_path,
    )
    written = write_json_create_new(Path(args.out), card)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

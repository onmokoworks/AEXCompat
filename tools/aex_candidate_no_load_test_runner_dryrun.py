#!/usr/bin/env python3
"""Build a dry-run manifest for no-load candidate tests.

This tool turns the candidate handoff and existing image/worker/OFX no-op
evidence into a concrete test-runner manifest. It never executes a test, opens
an AEX, accepts an AEX path, invokes AE/OFX, or renders.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
CANDIDATE_HANDOFF_ROOT = TARGET_ROOT / "candidate-test-handoff"
IMAGE_SUITE_ROOT = TARGET_ROOT / "image-fixture-suite"
IMAGE_VALIDATION_ROOT = TARGET_ROOT / "image-fixture-validation"
IMAGE_SUITE_SELFTEST_ROOT = TARGET_ROOT / "image-suite-selftest"
OFX_SUITE_SELFTEST_ROOT = TARGET_ROOT / "ofx-suite-selftest"
IMAGE_INPUT_SMOKE_ROOT = TARGET_ROOT / "image-input-smoke"
RENDER_VALIDATION_ROOT = TARGET_ROOT / "render-validation-contract"
OFX_ROUTE_CONTRACT_ROOT = TARGET_ROOT / "ofx-route-contract"
RUNNER_DRYRUN_ROOT = TARGET_ROOT / "candidate-test-runner-dryrun"

SAFETY_FLAGS = (
    "native_load_enabled",
    "native_load_performed",
    "dll_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
    "aex_file_opened",
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
    "raw_payload_serialized",
)

FORBIDDEN_RUNNER_ACTIONS = (
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
    "modify_aepx_file",
    "modify_aep_binary",
    "write_ae_project",
    "parse_real_pipl_payload",
    "emit_parameter_schema",
    "emit_redacted_schema",
    "serialize_raw_payload",
)

FORBIDDEN_CLI_INPUTS = (
    "--aex",
    "--aex-path",
    "--dll",
    "--load",
    "--execute",
    "--render",
    "--ofx",
    "--approve",
    "--explicit-user-approval",
    "APPROVE_AEX_LOAD_GATE",
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
        raise ValueError("candidate no-load test runner dry-run report must have .json extension")
    RUNNER_DRYRUN_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, RUNNER_DRYRUN_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(RUNNER_DRYRUN_ROOT.resolve(strict=True)):
        raise ValueError(f"candidate test runner dry-run parent must stay under {RUNNER_DRYRUN_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_candidate_handoff(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, CANDIDATE_HANDOFF_ROOT, "candidate test handoff")
    return read_json_object(resolved), resolved


def load_image_suite(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, IMAGE_SUITE_ROOT, "image fixture suite")
    return read_json_object(resolved), resolved


def load_image_validation(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, IMAGE_VALIDATION_ROOT, "image fixture validation")
    return read_json_object(resolved), resolved


def load_image_suite_selftest(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, IMAGE_SUITE_SELFTEST_ROOT, "image suite selftest")
    return read_json_object(resolved), resolved


def load_ofx_suite_selftest(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, OFX_SUITE_SELFTEST_ROOT, "OFX suite selftest")
    return read_json_object(resolved), resolved


def load_image_input_smoke(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, IMAGE_INPUT_SMOKE_ROOT, "image input smoke")
    return read_json_object(resolved), resolved


def load_render_validation_contract(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, RENDER_VALIDATION_ROOT, "render validation contract")
    return read_json_object(resolved), resolved


def load_ofx_route_contract(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, OFX_ROUTE_CONTRACT_ROOT, "OFX route contract")
    return read_json_object(resolved), resolved


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def target_candidate_path(payload: dict[str, Any]) -> str | None:
    candidate = payload.get("target_candidate")
    if isinstance(candidate, dict) and isinstance(candidate.get("relative_path"), str):
        return candidate["relative_path"]
    return None


def fixture_ids(fixtures: Any) -> list[str]:
    if not isinstance(fixtures, list):
        return []
    ids: list[str] = []
    for item in fixtures:
        if isinstance(item, dict) and isinstance(item.get("case_id"), str):
            ids.append(item["case_id"])
    return ids


def result_ids(results: Any) -> list[str]:
    if not isinstance(results, list):
        return []
    ids: list[str] = []
    for item in results:
        if isinstance(item, dict) and isinstance(item.get("case_id"), str):
            ids.append(item["case_id"])
    return ids


def validate_candidate_handoff(handoff: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if handoff.get("publication_status") != "local-only":
        errors.append("candidate handoff publication_status must be local-only")
    if handoff.get("report_kind") != "aex_candidate_test_handoff_packet":
        errors.append("candidate handoff report_kind must be aex_candidate_test_handoff_packet")
    if handoff.get("handoff_state") != "candidate_test_handoff_ready_no_load_native_closed":
        errors.append("candidate handoff must be ready with native closed")
    required_true = (
        "handoff_packet_ready",
        "no_load_test_handoff_ready",
        "approval_request_ready",
        "candidate_dependencies_clear",
        "runtime_containment_selftest_passed",
        "synthetic_subprocess_only",
        "path_policy_selftest_passed",
        "no_load_image_test_ready",
        "image_fixture_validation_passed",
        "worker_identity_passed",
        "ofx_identity_passed",
        "no_load_render_contract_ready",
        "no_load_validation_ready",
        "no_load_ofx_mock_ready",
        "mock_route_ready",
    )
    for key in required_true:
        if handoff.get(key) is not True:
            errors.append(f"candidate handoff {key} must be true")
    required_false = (
        "native_test_handoff_ready",
        "approval_can_be_issued_now",
        "approval_manifest_created",
        "fixture_approval_satisfied",
        "global_dependency_blockers_apply_to_candidate",
        "path_acceptance_ready",
        "aex_path_acceptance_enabled",
        "candidate_path_string_accepted",
        "raw_input_paths_serialized",
        "real_render_open",
        "real_route_open",
    )
    for key in required_false:
        if handoff.get(key) is not False:
            errors.append(f"candidate handoff {key} must be false")
    if handoff.get("native_load_gate") != "closed":
        errors.append("candidate handoff native_load_gate must be closed")
    if handoff.get("accepted_aex_path") is not None:
        errors.append("candidate handoff accepted_aex_path must be null")
    if not isinstance(handoff.get("candidate_relative_path"), str):
        errors.append("candidate handoff candidate_relative_path must be a string")
    errors.extend(safety_errors(handoff, "candidate handoff"))
    return errors


def validate_image_suite(suite: dict[str, Any], candidate_relative_path: str | None) -> list[str]:
    errors: list[str] = []
    if suite.get("publication_status") != "local-only":
        errors.append("image suite publication_status must be local-only")
    if suite.get("report_kind") != "aex_image_fixture_suite":
        errors.append("image suite report_kind must be aex_image_fixture_suite")
    if suite.get("suite_state") != "image_fixture_suite_ready":
        errors.append("image suite must be ready")
    if target_candidate_path(suite) != candidate_relative_path:
        errors.append("image suite target candidate must match handoff")
    fixtures = suite.get("fixtures")
    if not isinstance(fixtures, list) or not fixtures:
        errors.append("image suite fixtures must be a non-empty list")
    elif len(fixture_ids(fixtures)) != len(fixtures):
        errors.append("image suite every fixture must have a case_id")
    if suite.get("fixture_count") != len(fixtures or []):
        errors.append("image suite fixture_count must match fixtures")
    errors.extend(safety_errors(suite, "image suite"))
    return errors


def validate_image_validation(
    validation: dict[str, Any],
    candidate_relative_path: str | None,
    expected_case_ids: list[str],
) -> list[str]:
    errors: list[str] = []
    if validation.get("publication_status") != "local-only":
        errors.append("image validation publication_status must be local-only")
    if validation.get("report_kind") != "aex_image_fixture_validation":
        errors.append("image validation report_kind must be aex_image_fixture_validation")
    if validation.get("validation_state") != "image_fixture_validation_passed_no_load":
        errors.append("image validation must pass no-load")
    if validation.get("validation_passed") is not True:
        errors.append("image validation validation_passed must be true")
    if target_candidate_path(validation) != candidate_relative_path:
        errors.append("image validation target candidate must match handoff")
    results = validation.get("fixture_results")
    if result_ids(results) != expected_case_ids:
        errors.append("image validation case IDs must match image suite order")
    summary = validation.get("summary")
    if isinstance(summary, dict) and summary.get("failed_count") not in (0, None):
        errors.append("image validation failed_count must be zero")
    if isinstance(results, list):
        for result in results:
            if isinstance(result, dict) and result.get("validation_status") != "passed":
                errors.append(f"image validation case {result.get('case_id')} must be passed")
    errors.extend(safety_errors(validation, "image validation"))
    return errors


def validate_worker_suite_selftest(
    selftest: dict[str, Any],
    candidate_relative_path: str | None,
    expected_case_ids: list[str],
) -> list[str]:
    errors: list[str] = []
    if selftest.get("publication_status") != "local-only":
        errors.append("image suite selftest publication_status must be local-only")
    if selftest.get("report_kind") != "aex_image_suite_worker_selftest":
        errors.append("image suite selftest report_kind must be aex_image_suite_worker_selftest")
    if selftest.get("suite_selftest_state") != "image_suite_worker_selftest_passed":
        errors.append("image suite selftest must pass")
    if target_candidate_path(selftest) != candidate_relative_path:
        errors.append("image suite selftest target candidate must match handoff")
    results = selftest.get("fixture_results")
    if result_ids(results) != expected_case_ids:
        errors.append("image suite selftest case IDs must match image suite order")
    if isinstance(results, list):
        for result in results:
            check = result.get("identity_check") if isinstance(result, dict) else None
            if not isinstance(check, dict) or check.get("pixel_match") is not True or check.get("dimension_match") is not True:
                errors.append(f"worker identity case {result.get('case_id') if isinstance(result, dict) else '?'} must match")
    errors.extend(safety_errors(selftest, "image suite selftest"))
    return errors


def validate_ofx_suite_selftest(
    selftest: dict[str, Any],
    candidate_relative_path: str | None,
    expected_case_ids: list[str],
) -> list[str]:
    errors: list[str] = []
    if selftest.get("publication_status") != "local-only":
        errors.append("OFX suite selftest publication_status must be local-only")
    if selftest.get("report_kind") != "aex_ofx_suite_noop_selftest":
        errors.append("OFX suite selftest report_kind must be aex_ofx_suite_noop_selftest")
    if selftest.get("ofx_suite_selftest_state") != "ofx_suite_noop_identity_passed_route_closed":
        errors.append("OFX suite selftest must pass route closed")
    if target_candidate_path(selftest) != candidate_relative_path:
        errors.append("OFX suite selftest target candidate must match handoff")
    results = selftest.get("fixture_results")
    if result_ids(results) != expected_case_ids:
        errors.append("OFX suite selftest case IDs must match image suite order")
    if isinstance(results, list):
        for result in results:
            check = result.get("identity_check") if isinstance(result, dict) else None
            if not isinstance(check, dict) or check.get("pixel_match") is not True or check.get("dimension_match") is not True:
                errors.append(f"OFX no-op identity case {result.get('case_id') if isinstance(result, dict) else '?'} must match")
            if isinstance(result, dict) and result.get("mock_state") != "mock_identity_completed_route_closed":
                errors.append(f"OFX no-op identity case {result.get('case_id')} must stay route closed")
    errors.extend(safety_errors(selftest, "OFX suite selftest"))
    return errors


def validate_image_input_smoke(smoke: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if smoke.get("publication_status") != "local-only":
        errors.append("image input smoke publication_status must be local-only")
    if smoke.get("report_kind") != "aex_image_input_smoke_tool":
        errors.append("image input smoke report_kind must be aex_image_input_smoke_tool")
    if smoke.get("smoke_state") != "image_input_smoke_passed_route_closed":
        errors.append("image input smoke must pass route closed")
    if smoke.get("worker_identity_passed") is not True:
        errors.append("image input smoke worker identity must pass")
    if smoke.get("ofx_identity_passed") is not True:
        errors.append("image input smoke OFX identity must pass")
    errors.extend(safety_errors(smoke, "image input smoke"))
    return errors


def validate_render_validation_contract(contract: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if contract.get("publication_status") != "local-only":
        errors.append("render validation publication_status must be local-only")
    if contract.get("report_kind") != "aex_render_validation_contract":
        errors.append("render validation report_kind must be aex_render_validation_contract")
    if contract.get("contract_state") != "render_validation_contract_ready_render_closed":
        errors.append("render validation contract must be ready render closed")
    if contract.get("real_render_open") is not False:
        errors.append("render validation real_render_open must be false")
    if contract.get("no_load_validation_ready") is not True:
        errors.append("render validation no_load_validation_ready must be true")
    errors.extend(safety_errors(contract, "render validation"))
    return errors


def validate_ofx_route_contract(contract: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if contract.get("publication_status") != "local-only":
        errors.append("OFX route contract publication_status must be local-only")
    if contract.get("report_kind") != "aex_ofx_route_contract_probe":
        errors.append("OFX route contract report_kind must be aex_ofx_route_contract_probe")
    if contract.get("contract_state") != "ofx_route_contract_ready_route_closed":
        errors.append("OFX route contract must be ready route closed")
    if contract.get("real_route_open") is not False:
        errors.append("OFX route contract real_route_open must be false")
    if contract.get("mock_route_ready") is not True:
        errors.append("OFX route contract mock_route_ready must be true")
    errors.extend(safety_errors(contract, "OFX route contract"))
    return errors


def planned_fixture_cases(fixtures: list[dict[str, Any]]) -> list[dict[str, Any]]:
    planned: list[dict[str, Any]] = []
    for fixture in fixtures:
        case_id = fixture.get("case_id")
        pattern = fixture.get("pattern")
        for runner, action in (
            ("image_validation", "reuse_validated_ppm_metadata"),
            ("worker_identity", "rerun_worker_identity_if_requested"),
            ("ofx_noop_identity", "rerun_ofx_noop_identity_if_requested"),
        ):
            planned.append(
                {
                    "case_id": case_id,
                    "pattern": pattern,
                    "runner": runner,
                    "action": action,
                    "allowed_in_dryrun": True,
                    "requires_aex_path": False,
                    "requires_native_load": False,
                    "requires_real_render": False,
                    "requires_real_ofx_route": False,
                }
            )
    return planned


def planned_global_cases() -> list[dict[str, Any]]:
    return [
        {
            "case_id": "worker_lifecycle_hello",
            "runner": "no_load_worker",
            "action": "plan_worker_hello_message_only",
            "allowed_in_dryrun": True,
            "requires_aex_path": False,
            "requires_native_load": False,
            "requires_real_render": False,
            "requires_real_ofx_route": False,
        },
        {
            "case_id": "worker_lifecycle_inspect_environment",
            "runner": "no_load_worker",
            "action": "plan_worker_environment_inspection_only",
            "allowed_in_dryrun": True,
            "requires_aex_path": False,
            "requires_native_load": False,
            "requires_real_render": False,
            "requires_real_ofx_route": False,
        },
        {
            "case_id": "worker_lifecycle_blocked_load_aex",
            "runner": "no_load_worker",
            "action": "plan_blocked_load_aex_assertion_only",
            "allowed_in_dryrun": True,
            "requires_aex_path": False,
            "requires_native_load": False,
            "requires_real_render": False,
            "requires_real_ofx_route": False,
        },
        {
            "case_id": "worker_lifecycle_quit",
            "runner": "no_load_worker",
            "action": "plan_worker_quit_message_only",
            "allowed_in_dryrun": True,
            "requires_aex_path": False,
            "requires_native_load": False,
            "requires_real_render": False,
            "requires_real_ofx_route": False,
        },
        {
            "case_id": "single_image_input_smoke",
            "runner": "image_input_smoke",
            "action": "rerun_single_image_worker_and_ofx_noop_identity_if_requested",
            "allowed_in_dryrun": True,
            "requires_aex_path": False,
            "requires_native_load": False,
            "requires_real_render": False,
            "requires_real_ofx_route": False,
        },
        {
            "case_id": "render_validation_contract_review",
            "runner": "render_contract",
            "action": "review_closed_render_contract",
            "allowed_in_dryrun": True,
            "requires_aex_path": False,
            "requires_native_load": False,
            "requires_real_render": False,
            "requires_real_ofx_route": False,
        },
        {
            "case_id": "ofx_route_contract_review",
            "runner": "ofx_route_contract",
            "action": "review_closed_ofx_route_contract",
            "allowed_in_dryrun": True,
            "requires_aex_path": False,
            "requires_native_load": False,
            "requires_real_render": False,
            "requires_real_ofx_route": False,
        },
        {
            "case_id": "candidate_handoff_gate_review",
            "runner": "candidate_handoff",
            "action": "review_no_load_handoff_blockers",
            "allowed_in_dryrun": True,
            "requires_aex_path": False,
            "requires_native_load": False,
            "requires_real_render": False,
            "requires_real_ofx_route": False,
        },
        {
            "case_id": "runtime_synthetic_subprocess_review",
            "runner": "runtime_contract",
            "action": "review_existing_synthetic_runtime_selftest_evidence",
            "allowed_in_dryrun": True,
            "requires_aex_path": False,
            "requires_native_load": False,
            "requires_real_render": False,
            "requires_real_ofx_route": False,
        },
        {
            "case_id": "closed_path_policy_review",
            "runner": "path_policy",
            "action": "review_closed_path_policy_evidence_without_paths",
            "allowed_in_dryrun": True,
            "requires_aex_path": False,
            "requires_native_load": False,
            "requires_real_render": False,
            "requires_real_ofx_route": False,
        },
    ]


def build_candidate_no_load_test_runner_dryrun(
    *,
    candidate_handoff: dict[str, Any],
    candidate_handoff_path: Path,
    image_suite: dict[str, Any],
    image_suite_path: Path,
    image_validation: dict[str, Any],
    image_validation_path: Path,
    image_suite_selftest: dict[str, Any],
    image_suite_selftest_path: Path,
    ofx_suite_selftest: dict[str, Any],
    ofx_suite_selftest_path: Path,
    image_input_smoke: dict[str, Any],
    image_input_smoke_path: Path,
    render_validation_contract: dict[str, Any],
    render_validation_contract_path: Path,
    ofx_route_contract: dict[str, Any],
    ofx_route_contract_path: Path,
) -> dict[str, Any]:
    candidate_relative_path = candidate_handoff.get("candidate_relative_path")
    if not isinstance(candidate_relative_path, str):
        candidate_relative_path = None
    fixtures = image_suite.get("fixtures") if isinstance(image_suite.get("fixtures"), list) else []
    expected_case_ids = fixture_ids(fixtures)
    errors = (
        validate_candidate_handoff(candidate_handoff)
        + validate_image_suite(image_suite, candidate_relative_path)
        + validate_image_validation(image_validation, candidate_relative_path, expected_case_ids)
        + validate_worker_suite_selftest(image_suite_selftest, candidate_relative_path, expected_case_ids)
        + validate_ofx_suite_selftest(ofx_suite_selftest, candidate_relative_path, expected_case_ids)
        + validate_image_input_smoke(image_input_smoke)
        + validate_render_validation_contract(render_validation_contract)
        + validate_ofx_route_contract(ofx_route_contract)
    )
    if errors:
        raise ValueError("; ".join(errors))

    fixture_cases = planned_fixture_cases(fixtures)
    global_cases = planned_global_cases()
    planned_tests = fixture_cases + global_cases
    blocked_cases = [
        {
            "case_id": action,
            "runner": "blocked_native_or_real_route",
            "allowed_in_dryrun": False,
            "blocked_reason": "requires approval/path acceptance/native execution or real route",
        }
        for action in FORBIDDEN_RUNNER_ACTIONS
    ]
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_no_load_test_runner_dryrun",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_candidate_handoff": str(candidate_handoff_path),
        "source_image_suite": str(image_suite_path),
        "source_image_validation": str(image_validation_path),
        "source_image_suite_selftest": str(image_suite_selftest_path),
        "source_ofx_suite_selftest": str(ofx_suite_selftest_path),
        "source_image_input_smoke": str(image_input_smoke_path),
        "source_render_validation_contract": str(render_validation_contract_path),
        "source_ofx_route_contract": str(ofx_route_contract_path),
        "runner_dryrun_state": "candidate_no_load_test_runner_dryrun_ready_native_closed",
        "runner_dryrun_ready": True,
        "dry_run_only": True,
        "would_execute": False,
        "execution_performed": False,
        "candidate_relative_path": candidate_relative_path,
        "no_load_test_plan_ready": True,
        "native_test_plan_ready": False,
        "real_render_plan_ready": False,
        "real_ofx_route_plan_ready": False,
        "image_fixture_case_count": len(fixtures),
        "planned_no_load_case_count": len(planned_tests),
        "planned_native_case_count": 0,
        "planned_real_render_case_count": 0,
        "planned_real_ofx_route_case_count": 0,
        "blocked_case_count": len(blocked_cases),
        "image_fixture_validation_passed": True,
        "worker_suite_identity_passed": True,
        "ofx_suite_identity_passed": True,
        "image_smoke_identity_passed": True,
        "render_contract_review_ready": True,
        "ofx_route_contract_review_ready": True,
        "approval_manifest_created": False,
        "fixture_approval_satisfied": False,
        "native_load_gate": "closed",
        "path_acceptance_ready": False,
        "aex_path_acceptance_enabled": False,
        "accepted_aex_path": None,
        "path_payload_supplied": False,
        "real_render_open": False,
        "real_route_open": False,
        "planned_tests": planned_tests,
        "blocked_cases": blocked_cases,
        "forbidden_cli_inputs": list(FORBIDDEN_CLI_INPUTS),
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "aepx_file_modified": False,
        "aep_binary_modified": False,
        "ae_project_write_performed": False,
        "ofx_plugin_built": False,
        "ofx_describe_performed": False,
        "ofx_render_performed": False,
        "aex_render_performed": False,
        "render_validation_performed": False,
        "pipl_payload_parsed": False,
        "parameter_schema_emitted": False,
        "redacted_schema_emitted": False,
        "real_pipl_payload_parser_enabled": False,
        "real_pipl_payload_parsed": False,
        "resource_payload_opened": False,
        "raw_payload_serialized": False,
        "notes": [
            "This dry-run manifest reads JSON evidence only.",
            "It enumerates no-load test cases that may be rerun later by explicit tooling.",
            "It does not execute worker, OFX mock, render, native load, or path acceptance.",
            "Every native, real render, real OFX, project write, and PiPL payload/schema action remains blocked.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build no-load test runner dry-run manifest")
    parser.add_argument("--candidate-handoff", required=True, help="Candidate handoff JSON under target/candidate-test-handoff")
    parser.add_argument("--image-suite", required=True, help="Image fixture suite JSON under target/image-fixture-suite")
    parser.add_argument("--image-validation", required=True, help="Image validation JSON under target/image-fixture-validation")
    parser.add_argument("--image-suite-selftest", required=True, help="Image suite selftest JSON under target/image-suite-selftest")
    parser.add_argument("--ofx-suite-selftest", required=True, help="OFX suite selftest JSON under target/ofx-suite-selftest")
    parser.add_argument("--image-input-smoke", required=True, help="Image input smoke JSON under target/image-input-smoke")
    parser.add_argument(
        "--render-validation-contract",
        required=True,
        help="Render validation contract JSON under target/render-validation-contract",
    )
    parser.add_argument("--ofx-route-contract", required=True, help="OFX route contract JSON under target/ofx-route-contract")
    parser.add_argument("--out", required=True, help="Create-new dry-run report under target/candidate-test-runner-dryrun")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    handoff, handoff_path = load_candidate_handoff(Path(args.candidate_handoff))
    suite, suite_path = load_image_suite(Path(args.image_suite))
    validation, validation_path = load_image_validation(Path(args.image_validation))
    suite_selftest, suite_selftest_path = load_image_suite_selftest(Path(args.image_suite_selftest))
    ofx_selftest, ofx_selftest_path = load_ofx_suite_selftest(Path(args.ofx_suite_selftest))
    smoke, smoke_path = load_image_input_smoke(Path(args.image_input_smoke))
    render_contract, render_contract_path = load_render_validation_contract(Path(args.render_validation_contract))
    ofx_contract, ofx_contract_path = load_ofx_route_contract(Path(args.ofx_route_contract))
    report = build_candidate_no_load_test_runner_dryrun(
        candidate_handoff=handoff,
        candidate_handoff_path=handoff_path,
        image_suite=suite,
        image_suite_path=suite_path,
        image_validation=validation,
        image_validation_path=validation_path,
        image_suite_selftest=suite_selftest,
        image_suite_selftest_path=suite_selftest_path,
        ofx_suite_selftest=ofx_selftest,
        ofx_suite_selftest_path=ofx_selftest_path,
        image_input_smoke=smoke,
        image_input_smoke_path=smoke_path,
        render_validation_contract=render_contract,
        render_validation_contract_path=render_contract_path,
        ofx_route_contract=ofx_contract,
        ofx_route_contract_path=ofx_contract_path,
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

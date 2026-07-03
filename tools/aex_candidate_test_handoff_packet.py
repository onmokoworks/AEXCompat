#!/usr/bin/env python3
"""Build a no-load candidate test handoff packet.

The handoff packet collects the evidence needed before a selected AEX candidate
can be discussed by future image/render/native-loader tests. It is deliberately
not an approval artifact: it reads JSON evidence only and keeps AEX path
acceptance, native loading, real rendering, and real OFX routing closed.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
APPROVAL_REQUEST_ROOT = TARGET_ROOT / "fixture-approval-request"
CANDIDATE_LOAD_GATE_ROOT = TARGET_ROOT / "candidate-load-gate"
NATIVE_LOADER_DESIGN_ROOT = TARGET_ROOT / "native-loader-design"
NATIVE_LOADER_RUNTIME_ROOT = TARGET_ROOT / "native-loader-runtime-contract"
NATIVE_LOADER_RUNTIME_SELFTEST_ROOT = TARGET_ROOT / "native-loader-runtime-selftest"
PATH_POLICY_SELFTEST_ROOT = TARGET_ROOT / "native-loader-path-policy-selftest"
IMAGE_FIXTURE_VALIDATION_ROOT = TARGET_ROOT / "image-fixture-validation"
IMAGE_INPUT_SMOKE_ROOT = TARGET_ROOT / "image-input-smoke"
RENDER_VALIDATION_ROOT = TARGET_ROOT / "render-validation-contract"
OFX_ROUTE_CONTRACT_ROOT = TARGET_ROOT / "ofx-route-contract"
CANDIDATE_TEST_HANDOFF_ROOT = TARGET_ROOT / "candidate-test-handoff"

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

FORBIDDEN_HANDOFF_ACTIONS = (
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
        raise ValueError("candidate test handoff packet must have .json extension")
    CANDIDATE_TEST_HANDOFF_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, CANDIDATE_TEST_HANDOFF_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(
        CANDIDATE_TEST_HANDOFF_ROOT.resolve(strict=True)
    ):
        raise ValueError(f"candidate test handoff parent must stay under {CANDIDATE_TEST_HANDOFF_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_approval_request(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, APPROVAL_REQUEST_ROOT, "fixture approval request")
    return read_json_object(resolved), resolved


def load_candidate_load_gate(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, CANDIDATE_LOAD_GATE_ROOT, "candidate load gate dry-run")
    return read_json_object(resolved), resolved


def load_native_loader_design(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, NATIVE_LOADER_DESIGN_ROOT, "native loader design contract")
    return read_json_object(resolved), resolved


def load_native_loader_runtime(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, NATIVE_LOADER_RUNTIME_ROOT, "native loader runtime contract")
    return read_json_object(resolved), resolved


def load_native_loader_runtime_selftest(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, NATIVE_LOADER_RUNTIME_SELFTEST_ROOT, "native loader runtime selftest")
    return read_json_object(resolved), resolved


def load_path_policy_selftest(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, PATH_POLICY_SELFTEST_ROOT, "native loader path policy selftest")
    return read_json_object(resolved), resolved


def load_image_fixture_validation(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, IMAGE_FIXTURE_VALIDATION_ROOT, "image fixture validation")
    return read_json_object(resolved), resolved


def load_image_input_smoke(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_json_input(path, IMAGE_INPUT_SMOKE_ROOT, "image input smoke report")
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


def validate_approval_request(packet: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if packet.get("publication_status") != "local-only":
        errors.append("approval request publication_status must be local-only")
    if packet.get("report_kind") != "aex_fixture_approval_request_packet":
        errors.append("approval request report_kind must be aex_fixture_approval_request_packet")
    if packet.get("approval_request_state") != "fixture_approval_request_ready_pending_manual_approval":
        errors.append("approval request must be ready pending manual approval")
    checks = {
        "approval_request_ready": True,
        "approval_can_be_issued_now": False,
        "approval_manifest_created": False,
        "requires_explicit_user_approval": True,
        "current_fixture_approval_valid": False,
        "fixture_approval_satisfied": False,
        "manual_review_approval_ready": False,
        "approval_gate_stays_closed": True,
        "candidate_dependencies_clear": True,
        "path_policy_closed": True,
        "candidate_load_gate_closed": True,
        "approval_token_not_stored_in_manifest": True,
        "approval_only_prepares_next_gate": True,
    }
    for key, expected in checks.items():
        if packet.get(key) is not expected:
            errors.append(f"approval request {key} must be {expected}")
    if packet.get("native_load_gate") != "closed":
        errors.append("approval request native_load_gate must be closed")
    if packet.get("required_approval_token_name") != "APPROVE_AEX_LOAD_GATE":
        errors.append("approval request token name mismatch")
    if not isinstance(packet.get("candidate_relative_path"), str):
        errors.append("approval request candidate_relative_path must be a string")
    errors.extend(safety_errors(packet, "approval request"))
    return errors


def validate_candidate_load_gate(gate: dict[str, Any], candidate_relative_path: str | None) -> list[str]:
    errors: list[str] = []
    if gate.get("publication_status") != "local-only":
        errors.append("candidate load gate publication_status must be local-only")
    if gate.get("report_kind") != "aex_candidate_load_gate_dryrun":
        errors.append("candidate load gate report_kind must be aex_candidate_load_gate_dryrun")
    if gate.get("candidate_load_gate_dryrun_state") != "candidate_load_gate_dryrun_ready_no_load":
        errors.append("candidate load gate must be ready no-load")
    if gate.get("candidate_relative_path") != candidate_relative_path:
        errors.append("candidate load gate candidate_relative_path must match approval request")
    if gate.get("native_load_gate") != "closed":
        errors.append("candidate load gate native_load_gate must be closed")
    if gate.get("fixture_approval_satisfied") is not False:
        errors.append("candidate load gate fixture approval must be unsatisfied")
    if gate.get("candidate_dependencies_clear") is not True:
        errors.append("candidate load gate candidate dependencies must be clear")
    if gate.get("candidate_dependency_blockers_present") is not False:
        errors.append("candidate load gate candidate dependency blockers must be false")
    if gate.get("global_dependency_blockers_apply_to_candidate") is not False:
        errors.append("global dependency blockers must not apply to candidate")
    errors.extend(safety_errors(gate, "candidate load gate"))
    return errors


def validate_native_loader_design(contract: dict[str, Any], candidate_relative_path: str | None) -> list[str]:
    errors: list[str] = []
    if contract.get("publication_status") != "local-only":
        errors.append("native loader design publication_status must be local-only")
    if contract.get("report_kind") != "aex_native_loader_design_contract":
        errors.append("native loader design report_kind must be aex_native_loader_design_contract")
    if contract.get("candidate_relative_path") != candidate_relative_path:
        errors.append("native loader design candidate_relative_path must match approval request")
    if contract.get("native_loader_design_state") != "native_loader_design_ready_loader_closed":
        errors.append("native loader design state must be ready loader closed")
    if contract.get("contract_state") != "native_loader_design_contract_ready_loader_closed_pending_fixture_approval":
        errors.append("native loader design contract_state must remain pending fixture approval")
    required_false = ("fixture_approval_satisfied", "accepts_aex_path", "controller_loads_aex")
    for key in required_false:
        if contract.get(key) is not False:
            errors.append(f"native loader design {key} must be false")
    if contract.get("accepted_aex_path") is not None:
        errors.append("native loader design accepted_aex_path must be null")
    if contract.get("loader_design_ready") is not True:
        errors.append("native loader design loader_design_ready must be true")
    if contract.get("candidate_dependencies_clear") is not True:
        errors.append("native loader design candidate_dependencies_clear must be true")
    if contract.get("approval_required_before_aex_path") is not True:
        errors.append("native loader design approval_required_before_aex_path must be true")
    if contract.get("runtime_approval_required_before_load") is not True:
        errors.append("native loader design runtime_approval_required_before_load must be true")
    if contract.get("separate_process_required") is not True:
        errors.append("native loader design separate_process_required must be true")
    if contract.get("native_load_gate") != "closed":
        errors.append("native loader design native_load_gate must be closed")
    errors.extend(safety_errors(contract, "native loader design"))
    return errors


def validate_native_loader_runtime(contract: dict[str, Any], candidate_relative_path: str | None) -> list[str]:
    errors: list[str] = []
    if contract.get("publication_status") != "local-only":
        errors.append("native loader runtime publication_status must be local-only")
    if contract.get("report_kind") != "aex_native_loader_runtime_contract":
        errors.append("native loader runtime report_kind must be aex_native_loader_runtime_contract")
    if contract.get("candidate_relative_path") != candidate_relative_path:
        errors.append("native loader runtime candidate_relative_path must match approval request")
    if contract.get("native_loader_runtime_contract_state") != "runtime_containment_contract_ready_no_load":
        errors.append("native loader runtime contract state must be ready no-load")
    if contract.get("contract_state") != "runtime_containment_contract_ready_path_acceptance_closed":
        errors.append("native loader runtime contract_state must keep path acceptance closed")
    required_true = ("runtime_containment_ready", "broker_selftest_passed", "process_isolation_required")
    for key in required_true:
        if contract.get(key) is not True:
            errors.append(f"native loader runtime {key} must be true")
    required_false = (
        "fixture_approval_satisfied",
        "path_acceptance_ready",
        "aex_path_acceptance_enabled",
        "path_payload_supplied",
        "controller_loads_aex",
    )
    for key in required_false:
        if contract.get(key) is not False:
            errors.append(f"native loader runtime {key} must be false")
    if contract.get("accepted_aex_path") is not None:
        errors.append("native loader runtime accepted_aex_path must be null")
    if contract.get("path_allowlist_state") != "closed_no_aex_paths_accepted":
        errors.append("native loader runtime path allowlist must be closed")
    if contract.get("native_load_gate") != "closed":
        errors.append("native loader runtime native_load_gate must be closed")
    errors.extend(safety_errors(contract, "native loader runtime"))
    return errors


def validate_native_loader_runtime_selftest(selftest: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if selftest.get("publication_status") != "local-only":
        errors.append("native loader runtime selftest publication_status must be local-only")
    if selftest.get("report_kind") != "aex_native_loader_runtime_selftest":
        errors.append("native loader runtime selftest report_kind must be aex_native_loader_runtime_selftest")
    if selftest.get("runtime_selftest_state") != "runtime_containment_selftest_passed_no_load":
        errors.append("native loader runtime selftest state must pass no-load")
    for key in (
        "runtime_containment_selftest_passed",
        "synthetic_subprocess_only",
        "normal_exit_case_passed",
        "stderr_capture_passed",
        "timeout_case_passed",
        "child_cleanup_passed",
    ):
        if selftest.get(key) is not True:
            errors.append(f"native loader runtime selftest {key} must be true")
    for key in ("path_acceptance_ready", "aex_path_acceptance_enabled", "path_payload_supplied"):
        if selftest.get(key) is not False:
            errors.append(f"native loader runtime selftest {key} must be false")
    if selftest.get("accepted_aex_path") is not None:
        errors.append("native loader runtime selftest accepted_aex_path must be null")
    errors.extend(safety_errors(selftest, "native loader runtime selftest"))
    return errors


def validate_path_policy_selftest(selftest: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if selftest.get("publication_status") != "local-only":
        errors.append("path policy selftest publication_status must be local-only")
    if selftest.get("report_kind") != "aex_native_loader_path_policy_selftest":
        errors.append("path policy selftest report_kind must be aex_native_loader_path_policy_selftest")
    if selftest.get("path_policy_selftest_state") != "closed_path_policy_selftest_passed_no_aex_path":
        errors.append("path policy selftest state must be closed and passed")
    if selftest.get("path_policy_selftest_passed") is not True:
        errors.append("path policy selftest must pass")
    required_false = (
        "path_acceptance_ready",
        "aex_path_acceptance_enabled",
        "path_payload_supplied",
        "candidate_path_string_accepted",
        "raw_input_paths_serialized",
    )
    for key in required_false:
        if selftest.get(key) is not False:
            errors.append(f"path policy selftest {key} must be false")
    if selftest.get("accepted_aex_path") is not None:
        errors.append("path policy selftest accepted_aex_path must be null")
    for key in ("absolute_path_rejected", "traversal_rejected", "non_aex_suffix_rejected", "redaction_passed"):
        if selftest.get(key) is not True:
            errors.append(f"path policy selftest {key} must be true")
    errors.extend(safety_errors(selftest, "path policy selftest"))
    return errors


def validate_image_fixture_validation(report: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if report.get("publication_status") != "local-only":
        errors.append("image fixture validation publication_status must be local-only")
    if report.get("report_kind") != "aex_image_fixture_validation":
        errors.append("image fixture validation report_kind must be aex_image_fixture_validation")
    if report.get("validation_state") != "image_fixture_validation_passed_no_load":
        errors.append("image fixture validation must pass no-load")
    if report.get("validation_passed") is not True:
        errors.append("image fixture validation validation_passed must be true")
    errors.extend(safety_errors(report, "image fixture validation"))
    return errors


def validate_image_input_smoke(report: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if report.get("publication_status") != "local-only":
        errors.append("image input smoke publication_status must be local-only")
    if report.get("report_kind") != "aex_image_input_smoke_tool":
        errors.append("image input smoke report_kind must be aex_image_input_smoke_tool")
    if report.get("smoke_state") != "image_input_smoke_passed_route_closed":
        errors.append("image input smoke must pass with route closed")
    if report.get("worker_identity_passed") is not True:
        errors.append("image input smoke worker identity must pass")
    if report.get("ofx_identity_passed") is not True:
        errors.append("image input smoke OFX identity must pass")
    errors.extend(safety_errors(report, "image input smoke"))
    return errors


def validate_render_validation_contract(contract: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if contract.get("publication_status") != "local-only":
        errors.append("render validation publication_status must be local-only")
    if contract.get("report_kind") != "aex_render_validation_contract":
        errors.append("render validation report_kind must be aex_render_validation_contract")
    if contract.get("contract_state") != "render_validation_contract_ready_render_closed":
        errors.append("render validation contract must be ready with render closed")
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


def handoff_check(check_id: str, status: str, evidence: dict[str, Any], next_action: str | None = None) -> dict[str, Any]:
    item: dict[str, Any] = {"id": check_id, "status": status, "evidence": evidence}
    if next_action:
        item["next_action"] = next_action
    return item


def build_candidate_test_handoff_packet(
    *,
    approval_request: dict[str, Any],
    approval_request_path: Path,
    candidate_load_gate: dict[str, Any],
    candidate_load_gate_path: Path,
    native_loader_design: dict[str, Any],
    native_loader_design_path: Path,
    native_loader_runtime: dict[str, Any],
    native_loader_runtime_path: Path,
    native_loader_runtime_selftest: dict[str, Any],
    native_loader_runtime_selftest_path: Path,
    path_policy_selftest: dict[str, Any],
    path_policy_selftest_path: Path,
    image_fixture_validation: dict[str, Any],
    image_fixture_validation_path: Path,
    image_input_smoke: dict[str, Any],
    image_input_smoke_path: Path,
    render_validation_contract: dict[str, Any],
    render_validation_contract_path: Path,
    ofx_route_contract: dict[str, Any],
    ofx_route_contract_path: Path,
) -> dict[str, Any]:
    candidate_relative_path = approval_request.get("candidate_relative_path")
    if not isinstance(candidate_relative_path, str):
        candidate_relative_path = None
    errors = (
        validate_approval_request(approval_request)
        + validate_candidate_load_gate(candidate_load_gate, candidate_relative_path)
        + validate_native_loader_design(native_loader_design, candidate_relative_path)
        + validate_native_loader_runtime(native_loader_runtime, candidate_relative_path)
        + validate_native_loader_runtime_selftest(native_loader_runtime_selftest)
        + validate_path_policy_selftest(path_policy_selftest)
        + validate_image_fixture_validation(image_fixture_validation)
        + validate_image_input_smoke(image_input_smoke)
        + validate_render_validation_contract(render_validation_contract)
        + validate_ofx_route_contract(ofx_route_contract)
    )
    if errors:
        raise ValueError("; ".join(errors))

    handoff_blockers = [
        {
            "id": "manual_fixture_approval_pending",
            "state": approval_request.get("approval_request_state"),
            "next_action": "Complete manual fixture review before creating approval.",
        },
        {
            "id": "approval_manifest_missing_by_design",
            "state": approval_request.get("approval_manifest_created"),
            "next_action": "Create no approval manifest until explicit user approval exists.",
        },
        {
            "id": "path_acceptance_closed",
            "state": native_loader_runtime.get("path_allowlist_state"),
            "next_action": "Keep AEX path acceptance closed until a separate path-acceptance approval exists.",
        },
        {
            "id": "real_render_closed",
            "state": render_validation_contract.get("contract_state"),
            "next_action": "Use only no-load image identity/smoke tests before real render validation.",
        },
        {
            "id": "real_ofx_route_closed",
            "state": ofx_route_contract.get("contract_state"),
            "next_action": "Use only the OFX no-op mock route before a reviewed native bridge exists.",
        },
    ]
    if candidate_load_gate.get("global_dependency_blockers_present") is True:
        handoff_blockers.append(
            {
                "id": "global_dependency_blockers_present",
                "state": candidate_load_gate.get("source_load_gate_dependency_recommendation"),
                "next_action": "Keep global/default-deny dependency blockers out of the native route until reviewed.",
            }
        )

    handoff_checks = [
        handoff_check(
            "approval_request",
            "ready_no_approval",
            {
                "approval_request_ready": approval_request.get("approval_request_ready"),
                "approval_can_be_issued_now": approval_request.get("approval_can_be_issued_now"),
                "approval_manifest_created": approval_request.get("approval_manifest_created"),
            },
            "Use this only as review evidence, not approval.",
        ),
        handoff_check(
            "candidate_load_gate_dryrun",
            "closed_candidate_dependencies_clear",
            {
                "candidate_dependencies_clear": candidate_load_gate.get("candidate_dependencies_clear"),
                "fixture_approval_satisfied": candidate_load_gate.get("fixture_approval_satisfied"),
                "native_load_gate": candidate_load_gate.get("native_load_gate"),
            },
        ),
        handoff_check(
            "pathless_native_loader_design",
            "ready_but_pathless",
            {
                "loader_design_ready": native_loader_design.get("loader_design_ready"),
                "accepts_aex_path": native_loader_design.get("accepts_aex_path"),
                "accepted_aex_path": native_loader_design.get("accepted_aex_path"),
            },
        ),
        handoff_check(
            "runtime_containment",
            "ready_path_acceptance_closed",
            {
            "runtime_containment_ready": native_loader_runtime.get("runtime_containment_ready"),
            "runtime_containment_selftest_passed": native_loader_runtime_selftest.get(
                "runtime_containment_selftest_passed"
            ),
            "synthetic_subprocess_only": native_loader_runtime_selftest.get("synthetic_subprocess_only"),
            "path_acceptance_ready": native_loader_runtime.get("path_acceptance_ready"),
            "aex_path_acceptance_enabled": native_loader_runtime.get("aex_path_acceptance_enabled"),
        },
        ),
        handoff_check(
            "closed_path_policy",
            "passed_no_aex_path",
            {
                "path_policy_selftest_passed": path_policy_selftest.get("path_policy_selftest_passed"),
                "candidate_path_string_accepted": path_policy_selftest.get("candidate_path_string_accepted"),
                "raw_input_paths_serialized": path_policy_selftest.get("raw_input_paths_serialized"),
            },
        ),
        handoff_check(
            "image_fixture_validation",
            "passed_no_load",
            {
                "validation_state": image_fixture_validation.get("validation_state"),
                "validation_passed": image_fixture_validation.get("validation_passed"),
            },
        ),
        handoff_check(
            "image_input_smoke",
            "passed_route_closed",
            {
                "worker_identity_passed": image_input_smoke.get("worker_identity_passed"),
                "ofx_identity_passed": image_input_smoke.get("ofx_identity_passed"),
            },
        ),
        handoff_check(
            "render_validation_contract",
            "ready_render_closed",
            {
                "real_render_open": render_validation_contract.get("real_render_open"),
                "no_load_validation_ready": render_validation_contract.get("no_load_validation_ready"),
            },
        ),
        handoff_check(
            "ofx_route_contract",
            "mock_ready_real_route_closed",
            {
                "real_route_open": ofx_route_contract.get("real_route_open"),
                "mock_route_ready": ofx_route_contract.get("mock_route_ready"),
            },
        ),
    ]

    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_test_handoff_packet",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_approval_request": str(approval_request_path),
        "source_candidate_load_gate": str(candidate_load_gate_path),
        "source_native_loader_design": str(native_loader_design_path),
        "source_native_loader_runtime": str(native_loader_runtime_path),
        "source_native_loader_runtime_selftest": str(native_loader_runtime_selftest_path),
        "source_path_policy_selftest": str(path_policy_selftest_path),
        "source_image_fixture_validation": str(image_fixture_validation_path),
        "source_image_input_smoke": str(image_input_smoke_path),
        "source_render_validation_contract": str(render_validation_contract_path),
        "source_ofx_route_contract": str(ofx_route_contract_path),
        "handoff_state": "candidate_test_handoff_ready_no_load_native_closed",
        "handoff_packet_ready": True,
        "candidate_relative_path": candidate_relative_path,
        "no_load_test_handoff_ready": True,
        "native_test_handoff_ready": False,
        "approval_request_state": approval_request.get("approval_request_state"),
        "approval_request_ready": True,
        "approval_can_be_issued_now": False,
        "approval_manifest_created": False,
        "requires_explicit_user_approval": True,
        "fixture_approval_satisfied": False,
        "native_load_gate": "closed",
        "candidate_dependencies_clear": True,
        "global_dependency_blockers_present": candidate_load_gate.get("global_dependency_blockers_present"),
        "global_dependency_blockers_apply_to_candidate": False,
        "native_loader_design_ready": True,
        "runtime_containment_ready": True,
        "runtime_containment_selftest_passed": True,
        "synthetic_subprocess_only": True,
        "normal_exit_case_passed": True,
        "stderr_capture_passed": True,
        "timeout_case_passed": True,
        "child_cleanup_passed": True,
        "path_allowlist_state": "closed_no_aex_paths_accepted",
        "path_acceptance_ready": False,
        "aex_path_acceptance_enabled": False,
        "accepted_aex_path": None,
        "path_payload_supplied": False,
        "path_policy_selftest_passed": True,
        "candidate_path_string_accepted": False,
        "raw_input_paths_serialized": False,
        "no_load_image_test_ready": True,
        "image_fixture_validation_state": image_fixture_validation.get("validation_state"),
        "image_fixture_validation_passed": True,
        "image_input_smoke_state": image_input_smoke.get("smoke_state"),
        "worker_identity_passed": True,
        "ofx_identity_passed": True,
        "no_load_render_contract_ready": True,
        "real_render_open": False,
        "no_load_validation_ready": True,
        "no_load_ofx_mock_ready": True,
        "real_route_open": False,
        "mock_route_ready": True,
        "handoff_blocker_count": len(handoff_blockers),
        "handoff_blockers": handoff_blockers,
        "handoff_checks": handoff_checks,
        "allowed_no_load_handoff_actions": [
            "rerun_image_input_smoke_identity",
            "rerun_ofx_noop_identity",
            "rerun_candidate_load_gate_dryrun",
            "review_runtime_containment_contract",
            "review_path_policy_without_accepting_real_paths",
        ],
        "forbidden_handoff_actions": list(FORBIDDEN_HANDOFF_ACTIONS),
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
            "This handoff packet reads JSON evidence only.",
            "It is safe for no-load planning and image/OFX no-op test coordination.",
            "It is not approval to accept an AEX path or perform native load.",
            "No AEX file, DLL, AE process, real render, real OFX route, or project write is used.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build no-load AEX candidate test handoff packet")
    parser.add_argument("--approval-request", required=True, help="Approval request JSON under target/fixture-approval-request")
    parser.add_argument("--candidate-load-gate", required=True, help="Candidate load gate JSON under target/candidate-load-gate")
    parser.add_argument("--native-loader-design", required=True, help="Native loader design JSON under target/native-loader-design")
    parser.add_argument(
        "--native-loader-runtime",
        required=True,
        help="Native loader runtime contract JSON under target/native-loader-runtime-contract",
    )
    parser.add_argument(
        "--native-loader-runtime-selftest",
        required=True,
        help="Native loader runtime selftest JSON under target/native-loader-runtime-selftest",
    )
    parser.add_argument(
        "--path-policy-selftest",
        required=True,
        help="Path policy selftest JSON under target/native-loader-path-policy-selftest",
    )
    parser.add_argument(
        "--image-fixture-validation",
        required=True,
        help="Image fixture validation JSON under target/image-fixture-validation",
    )
    parser.add_argument("--image-input-smoke", required=True, help="Image input smoke JSON under target/image-input-smoke")
    parser.add_argument(
        "--render-validation-contract",
        required=True,
        help="Render validation contract JSON under target/render-validation-contract",
    )
    parser.add_argument("--ofx-route-contract", required=True, help="OFX route contract JSON under target/ofx-route-contract")
    parser.add_argument("--out", required=True, help="Create-new packet under target/candidate-test-handoff")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    approval_request, approval_request_path = load_approval_request(Path(args.approval_request))
    candidate_load_gate, candidate_load_gate_path = load_candidate_load_gate(Path(args.candidate_load_gate))
    native_loader_design, native_loader_design_path = load_native_loader_design(Path(args.native_loader_design))
    native_loader_runtime, native_loader_runtime_path = load_native_loader_runtime(Path(args.native_loader_runtime))
    native_loader_runtime_selftest, native_loader_runtime_selftest_path = load_native_loader_runtime_selftest(
        Path(args.native_loader_runtime_selftest)
    )
    path_policy_selftest, path_policy_selftest_path = load_path_policy_selftest(Path(args.path_policy_selftest))
    image_fixture_validation, image_fixture_validation_path = load_image_fixture_validation(
        Path(args.image_fixture_validation)
    )
    image_input_smoke, image_input_smoke_path = load_image_input_smoke(Path(args.image_input_smoke))
    render_validation_contract, render_validation_contract_path = load_render_validation_contract(
        Path(args.render_validation_contract)
    )
    ofx_route_contract, ofx_route_contract_path = load_ofx_route_contract(Path(args.ofx_route_contract))
    packet = build_candidate_test_handoff_packet(
        approval_request=approval_request,
        approval_request_path=approval_request_path,
        candidate_load_gate=candidate_load_gate,
        candidate_load_gate_path=candidate_load_gate_path,
        native_loader_design=native_loader_design,
        native_loader_design_path=native_loader_design_path,
        native_loader_runtime=native_loader_runtime,
        native_loader_runtime_path=native_loader_runtime_path,
        native_loader_runtime_selftest=native_loader_runtime_selftest,
        native_loader_runtime_selftest_path=native_loader_runtime_selftest_path,
        path_policy_selftest=path_policy_selftest,
        path_policy_selftest_path=path_policy_selftest_path,
        image_fixture_validation=image_fixture_validation,
        image_fixture_validation_path=image_fixture_validation_path,
        image_input_smoke=image_input_smoke,
        image_input_smoke_path=image_input_smoke_path,
        render_validation_contract=render_validation_contract,
        render_validation_contract_path=render_validation_contract_path,
        ofx_route_contract=ofx_route_contract,
        ofx_route_contract_path=ofx_route_contract_path,
    )
    written = write_json_create_new(Path(args.out), packet)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

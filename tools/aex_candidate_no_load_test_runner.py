#!/usr/bin/env python3
"""Execute only the safe no-load candidate runner cases.

This runner consumes the dry-run manifest plus the same source JSON artifacts,
then reruns the PPM worker identity and OFX no-op identity suites. It never
accepts an AEX path, loads native code, invokes After Effects, performs a real
render, opens a real OFX route, or creates approval.
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

import aex_candidate_no_load_test_runner_dryrun as dryrun
import aex_image_suite_selftest
import aex_ofx_suite_noop_selftest


TARGET_ROOT = LAB_ROOT / "target"
RUNNER_ROOT = TARGET_ROOT / "candidate-test-runner"
RUNNER_DRYRUN_ROOT = TARGET_ROOT / "candidate-test-runner-dryrun"

SAFETY_FLAGS = dryrun.SAFETY_FLAGS
FORBIDDEN_RUNNER_ACTIONS = dryrun.FORBIDDEN_RUNNER_ACTIONS
FORBIDDEN_CLI_INPUTS = dryrun.FORBIDDEN_CLI_INPUTS

SOURCE_PATH_KEYS = {
    "source_candidate_handoff": "candidate handoff",
    "source_image_suite": "image suite",
    "source_image_validation": "image validation",
    "source_image_suite_selftest": "image suite selftest",
    "source_ofx_suite_selftest": "OFX suite selftest",
    "source_image_input_smoke": "image input smoke",
    "source_render_validation_contract": "render validation contract",
    "source_ofx_route_contract": "OFX route contract",
}


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


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("candidate no-load test runner report must have .json extension")
    RUNNER_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, RUNNER_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(RUNNER_ROOT.resolve(strict=True)):
        raise ValueError(f"candidate test runner parent must stay under {RUNNER_ROOT}")
    return resolved


def validate_runner_dryrun_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("candidate no-load test runner dry-run must have .json extension")
    return resolve_under_root(path, RUNNER_DRYRUN_ROOT, must_exist=True)


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_runner_dryrun(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_runner_dryrun_path(path)
    return read_json_object(resolved), resolved


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def reject_forbidden_cli_inputs(argv: list[str]) -> None:
    forbidden = set(FORBIDDEN_CLI_INPUTS)
    for raw_arg in argv:
        token = raw_arg.split("=", 1)[0]
        if token in forbidden or raw_arg in forbidden:
            raise ValueError(f"forbidden CLI input for no-load runner: {raw_arg}")


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def resolve_existing_source_string(value: Any, label: str) -> Path:
    if not isinstance(value, str):
        raise ValueError(f"{label} source path must be a string")
    return Path(value).resolve(strict=True)


def validate_dryrun_source_paths(
    runner_dryrun: dict[str, Any],
    expected_sources: dict[str, Path],
) -> list[str]:
    errors: list[str] = []
    for source_key, label in SOURCE_PATH_KEYS.items():
        try:
            actual = resolve_existing_source_string(runner_dryrun.get(source_key), label)
        except ValueError as exc:
            errors.append(str(exc))
            continue
        expected = expected_sources[source_key].resolve(strict=True)
        if actual != expected:
            errors.append(f"dry-run {source_key} must match explicit {label} input")
    return errors


def validate_runner_dryrun(
    runner_dryrun: dict[str, Any],
    *,
    expected_report: dict[str, Any],
    expected_sources: dict[str, Path],
) -> list[str]:
    errors: list[str] = []
    if runner_dryrun.get("publication_status") != "local-only":
        errors.append("runner dry-run publication_status must be local-only")
    if runner_dryrun.get("report_kind") != "aex_candidate_no_load_test_runner_dryrun":
        errors.append("runner dry-run report_kind must be aex_candidate_no_load_test_runner_dryrun")
    if runner_dryrun.get("runner_dryrun_state") != "candidate_no_load_test_runner_dryrun_ready_native_closed":
        errors.append("runner dry-run must be ready with native closed")

    required_true = (
        "runner_dryrun_ready",
        "dry_run_only",
        "no_load_test_plan_ready",
        "image_fixture_validation_passed",
        "worker_suite_identity_passed",
        "ofx_suite_identity_passed",
        "image_smoke_identity_passed",
        "render_contract_review_ready",
        "ofx_route_contract_review_ready",
    )
    for key in required_true:
        if runner_dryrun.get(key) is not True:
            errors.append(f"runner dry-run {key} must be true")

    required_false = (
        "would_execute",
        "execution_performed",
        "native_test_plan_ready",
        "real_render_plan_ready",
        "real_ofx_route_plan_ready",
        "approval_manifest_created",
        "fixture_approval_satisfied",
        "path_acceptance_ready",
        "aex_path_acceptance_enabled",
        "path_payload_supplied",
        "real_render_open",
        "real_route_open",
    )
    for key in required_false:
        if runner_dryrun.get(key) is not False:
            errors.append(f"runner dry-run {key} must be false")

    if runner_dryrun.get("native_load_gate") != "closed":
        errors.append("runner dry-run native_load_gate must be closed")
    if runner_dryrun.get("accepted_aex_path") is not None:
        errors.append("runner dry-run accepted_aex_path must be null")
    if (runner_dryrun.get("image_fixture_case_count") or 0) <= 0:
        errors.append("runner dry-run image_fixture_case_count must be positive")
    if (runner_dryrun.get("planned_no_load_case_count") or 0) <= 0:
        errors.append("runner dry-run planned_no_load_case_count must be positive")
    for key in ("planned_native_case_count", "planned_real_render_case_count", "planned_real_ofx_route_case_count"):
        if runner_dryrun.get(key) != 0:
            errors.append(f"runner dry-run {key} must be zero")
    if (runner_dryrun.get("blocked_case_count") or 0) <= 0:
        errors.append("runner dry-run blocked_case_count must be positive")

    planned_tests = runner_dryrun.get("planned_tests")
    if planned_tests != expected_report.get("planned_tests"):
        errors.append("runner dry-run planned_tests must match explicit source artifacts")
    blocked_cases = runner_dryrun.get("blocked_cases")
    if blocked_cases != expected_report.get("blocked_cases"):
        errors.append("runner dry-run blocked_cases must match explicit source artifacts")
    forbidden_inputs = runner_dryrun.get("forbidden_cli_inputs")
    if not isinstance(forbidden_inputs, list) or not set(FORBIDDEN_CLI_INPUTS).issubset(set(forbidden_inputs)):
        errors.append("runner dry-run forbidden_cli_inputs must include no-load runner forbidden inputs")
    blocked_case_ids = {
        item.get("case_id")
        for item in blocked_cases
        if isinstance(item, dict) and isinstance(item.get("case_id"), str)
    }
    missing_blocked = sorted(set(FORBIDDEN_RUNNER_ACTIONS) - blocked_case_ids)
    if missing_blocked:
        errors.append(f"runner dry-run blocked_cases missing actions: {', '.join(missing_blocked)}")

    errors.extend(validate_dryrun_source_paths(runner_dryrun, expected_sources))
    errors.extend(safety_errors(runner_dryrun, "runner dry-run"))
    return errors


def identity_passed(results: Any) -> bool:
    if not isinstance(results, list) or not results:
        return False
    for result in results:
        if not isinstance(result, dict):
            return False
        check = result.get("identity_check")
        if not isinstance(check, dict):
            return False
        if check.get("pixel_match") is not True or check.get("dimension_match") is not True:
            return False
    return True


def worker_blocked_load_verified(report: dict[str, Any]) -> bool:
    steps = report.get("steps")
    if not isinstance(steps, list):
        return False
    for step in steps:
        if (
            isinstance(step, dict)
            and step.get("step") == "blocked_load_aex"
            and step.get("response_type") == "error"
            and step.get("code") == "blocked_action"
        ):
            return True
    return False


def load_and_validate_sources(
    *,
    candidate_handoff_path: Path,
    image_suite_path: Path,
    image_validation_path: Path,
    image_suite_selftest_path: Path,
    ofx_suite_selftest_path: Path,
    image_input_smoke_path: Path,
    render_validation_contract_path: Path,
    ofx_route_contract_path: Path,
) -> tuple[dict[str, Any], dict[str, Path], dict[str, Any]]:
    candidate_handoff, resolved_handoff = dryrun.load_candidate_handoff(candidate_handoff_path)
    image_suite, resolved_suite = dryrun.load_image_suite(image_suite_path)
    image_validation, resolved_validation = dryrun.load_image_validation(image_validation_path)
    image_suite_selftest, resolved_suite_selftest = dryrun.load_image_suite_selftest(image_suite_selftest_path)
    ofx_suite_selftest, resolved_ofx_selftest = dryrun.load_ofx_suite_selftest(ofx_suite_selftest_path)
    image_input_smoke, resolved_smoke = dryrun.load_image_input_smoke(image_input_smoke_path)
    render_contract, resolved_render = dryrun.load_render_validation_contract(render_validation_contract_path)
    ofx_contract, resolved_route = dryrun.load_ofx_route_contract(ofx_route_contract_path)

    expected_report = dryrun.build_candidate_no_load_test_runner_dryrun(
        candidate_handoff=candidate_handoff,
        candidate_handoff_path=resolved_handoff,
        image_suite=image_suite,
        image_suite_path=resolved_suite,
        image_validation=image_validation,
        image_validation_path=resolved_validation,
        image_suite_selftest=image_suite_selftest,
        image_suite_selftest_path=resolved_suite_selftest,
        ofx_suite_selftest=ofx_suite_selftest,
        ofx_suite_selftest_path=resolved_ofx_selftest,
        image_input_smoke=image_input_smoke,
        image_input_smoke_path=resolved_smoke,
        render_validation_contract=render_contract,
        render_validation_contract_path=resolved_render,
        ofx_route_contract=ofx_contract,
        ofx_route_contract_path=resolved_route,
    )
    sources = {
        "source_candidate_handoff": resolved_handoff,
        "source_image_suite": resolved_suite,
        "source_image_validation": resolved_validation,
        "source_image_suite_selftest": resolved_suite_selftest,
        "source_ofx_suite_selftest": resolved_ofx_selftest,
        "source_image_input_smoke": resolved_smoke,
        "source_render_validation_contract": resolved_render,
        "source_ofx_route_contract": resolved_route,
    }
    payloads = {
        "candidate_handoff": candidate_handoff,
        "image_suite": image_suite,
        "image_validation": image_validation,
        "image_suite_selftest": image_suite_selftest,
        "ofx_suite_selftest": ofx_suite_selftest,
        "image_input_smoke": image_input_smoke,
        "render_validation_contract": render_contract,
        "ofx_route_contract": ofx_contract,
    }
    return payloads, sources, expected_report


def build_candidate_no_load_test_runner(
    *,
    runner_dryrun: dict[str, Any],
    runner_dryrun_path: Path,
    source_payloads: dict[str, Any],
    source_paths: dict[str, Path],
    expected_dryrun_report: dict[str, Any],
    ofx_packet: dict[str, Any],
    ofx_packet_path: Path,
    worker_path: Path,
    output_prefix: str,
) -> dict[str, Any]:
    dryrun_errors = validate_runner_dryrun(
        runner_dryrun,
        expected_report=expected_dryrun_report,
        expected_sources=source_paths,
    )
    if dryrun_errors:
        raise ValueError("; ".join(dryrun_errors))

    worker_report = aex_image_suite_selftest.run_suite_selftest(
        suite=source_payloads["image_suite"],
        suite_path=source_paths["source_image_suite"],
        worker_path=worker_path,
        output_prefix=f"{output_prefix}-worker",
    )
    ofx_report = aex_ofx_suite_noop_selftest.build_suite_report(
        packet=ofx_packet,
        packet_path=ofx_packet_path,
        suite=source_payloads["image_suite"],
        suite_path=source_paths["source_image_suite"],
        output_prefix=f"{output_prefix}-ofx",
    )

    worker_identity_ok = identity_passed(worker_report.get("fixture_results"))
    ofx_identity_ok = identity_passed(ofx_report.get("fixture_results"))
    blocked_load_ok = worker_blocked_load_verified(worker_report)
    fixture_count = len(source_payloads["image_suite"].get("fixtures", []))
    runner_ready = worker_identity_ok and ofx_identity_ok and blocked_load_ok
    if not runner_ready:
        raise AssertionError("no-load runner identity or blocked-load verification failed")

    safety = {flag: False for flag in SAFETY_FLAGS}
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_no_load_test_runner",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_runner_dryrun": str(runner_dryrun_path),
        "source_ofx_facade_packet": str(ofx_packet_path),
        **{key: str(path) for key, path in source_paths.items()},
        "runner_state": "candidate_no_load_test_runner_passed_native_closed",
        "runner_ready": True,
        "dry_run_only": False,
        "would_execute": True,
        "execution_performed": True,
        "candidate_relative_path": runner_dryrun.get("candidate_relative_path"),
        "no_load_test_plan_ready": True,
        "no_load_execution_performed": True,
        "native_test_plan_ready": False,
        "native_execution_performed": False,
        "real_render_plan_ready": False,
        "real_render_execution_performed": False,
        "real_ofx_route_plan_ready": False,
        "real_ofx_route_execution_performed": False,
        "worker_invoked": True,
        "ofx_mock_invoked": True,
        "ofx_runtime_invoked": False,
        "worker_identity_passed": True,
        "ofx_noop_identity_passed": True,
        "blocked_load_aex_verified": True,
        "image_fixture_validation_passed": True,
        "image_smoke_identity_passed": True,
        "render_contract_review_ready": True,
        "ofx_route_contract_review_ready": True,
        "image_fixture_case_count": fixture_count,
        "planned_no_load_case_count": runner_dryrun.get("planned_no_load_case_count"),
        "planned_native_case_count": 0,
        "planned_real_render_case_count": 0,
        "planned_real_ofx_route_case_count": 0,
        "executed_worker_case_count": worker_report.get("fixture_count"),
        "executed_ofx_noop_case_count": ofx_report.get("fixture_count"),
        "executed_worker_lifecycle_case_count": len(worker_report.get("steps", [])),
        "executed_native_case_count": 0,
        "executed_real_render_case_count": 0,
        "executed_real_ofx_route_case_count": 0,
        "blocked_case_count": runner_dryrun.get("blocked_case_count"),
        "approval_manifest_created": False,
        "fixture_approval_satisfied": False,
        "native_load_gate": "closed",
        "path_acceptance_ready": False,
        "aex_path_acceptance_enabled": False,
        "accepted_aex_path": None,
        "path_payload_supplied": False,
        "real_render_open": False,
        "real_route_open": False,
        "planned_tests": runner_dryrun.get("planned_tests"),
        "blocked_cases": runner_dryrun.get("blocked_cases"),
        "forbidden_cli_inputs": list(FORBIDDEN_CLI_INPUTS),
        "worker_report": {
            "suite_selftest_state": worker_report.get("suite_selftest_state"),
            "fixture_count": worker_report.get("fixture_count"),
            "steps": worker_report.get("steps"),
            "fixture_results": worker_report.get("fixture_results"),
        },
        "ofx_noop_report": {
            "ofx_suite_selftest_state": ofx_report.get("ofx_suite_selftest_state"),
            "fixture_count": ofx_report.get("fixture_count"),
            "fixture_results": ofx_report.get("fixture_results"),
            "mock_identity_transform_performed": ofx_report.get("mock_identity_transform_performed"),
        },
        "no_load_worker_invoked": True,
        "mock_identity_transform_performed": True,
        **safety,
        "notes": [
            "This runner executes only no-load PPM worker identity and OFX no-op identity cases.",
            "The source dry-run manifest and explicit source artifacts must agree before execution.",
            "The worker blocked load_aex path is verified with a synthetic string only.",
            "No AEX path is accepted, opened, copied, hashed, loaded, rendered, or routed through real OFX.",
        ],
    }


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    reject_forbidden_cli_inputs(sys.argv[1:] if argv is None else argv)
    parser = argparse.ArgumentParser(description="Run safe no-load candidate test cases")
    parser.add_argument("--runner-dryrun", required=True, help="Runner dry-run JSON under target/candidate-test-runner-dryrun")
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
    parser.add_argument("--ofx-packet", required=True, help="Deferred OFX facade packet under target/ofx-facade")
    parser.add_argument("--worker", default=str(TOOLS_ROOT / "aex_no_load_worker.py"))
    parser.add_argument("--output-prefix", required=True, help="Prefix for create-new no-load output PPMs")
    parser.add_argument("--out", required=True, help="Create-new runner JSON under target/candidate-test-runner")
    return parser.parse_args(argv)


def main() -> int:
    args = parse_args()
    runner_dryrun, runner_dryrun_path = load_runner_dryrun(Path(args.runner_dryrun))
    source_payloads, source_paths, expected_report = load_and_validate_sources(
        candidate_handoff_path=Path(args.candidate_handoff),
        image_suite_path=Path(args.image_suite),
        image_validation_path=Path(args.image_validation),
        image_suite_selftest_path=Path(args.image_suite_selftest),
        ofx_suite_selftest_path=Path(args.ofx_suite_selftest),
        image_input_smoke_path=Path(args.image_input_smoke),
        render_validation_contract_path=Path(args.render_validation_contract),
        ofx_route_contract_path=Path(args.ofx_route_contract),
    )
    ofx_packet, ofx_packet_path = aex_ofx_suite_noop_selftest.load_packet(Path(args.ofx_packet))
    report = build_candidate_no_load_test_runner(
        runner_dryrun=runner_dryrun,
        runner_dryrun_path=runner_dryrun_path,
        source_payloads=source_payloads,
        source_paths=source_paths,
        expected_dryrun_report=expected_report,
        ofx_packet=ofx_packet,
        ofx_packet_path=ofx_packet_path,
        worker_path=Path(args.worker),
        output_prefix=args.output_prefix,
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

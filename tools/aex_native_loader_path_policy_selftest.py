#!/usr/bin/env python3
"""Run a no-load closed path-policy selftest for future AEX path acceptance.

The selftest reads native-loader runtime selftest JSON only. It evaluates
synthetic path strings in memory, accepts no AEX path, opens no files, and
serializes no raw input paths into the report.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path, PureWindowsPath
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
RUNTIME_SELFTEST_ROOT = TARGET_ROOT / "native-loader-runtime-selftest"
PATH_POLICY_SELFTEST_ROOT = TARGET_ROOT / "native-loader-path-policy-selftest"

SAFETY_FLAGS = (
    "native_load_enabled",
    "native_load_performed",
    "dll_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
    "aex_file_opened",
)

SYNTHETIC_CASES = (
    {
        "case": "candidate_like_aex_path_stays_closed",
        "input": r"APPROVED_ROOT\Plugins\Effects\Candidate.aex",
        "expected_reason": "path_acceptance_gate_closed",
    },
    {
        "case": "absolute_private_path_rejected",
        "input": r"C:\Private\Fixture\Candidate.aex",
        "expected_reason": "absolute_path_not_allowed_while_gate_closed",
    },
    {
        "case": "traversal_path_rejected",
        "input": r"APPROVED_ROOT\..\Outside\Candidate.aex",
        "expected_reason": "traversal_component_rejected",
    },
    {
        "case": "non_aex_suffix_rejected",
        "input": r"APPROVED_ROOT\Plugins\Effects\Candidate.txt",
        "expected_reason": "non_aex_suffix_rejected",
    },
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


def validate_runtime_selftest_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("native-loader runtime selftest must have .json extension")
    return resolve_under_root(path, RUNTIME_SELFTEST_ROOT, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("native-loader path policy selftest report must have .json extension")
    PATH_POLICY_SELFTEST_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, PATH_POLICY_SELFTEST_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(PATH_POLICY_SELFTEST_ROOT.resolve(strict=True)):
        raise ValueError(f"native-loader path policy selftest parent must stay under {PATH_POLICY_SELFTEST_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def safety_errors(payload: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    for flag in SAFETY_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def validate_runtime_selftest(selftest: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if selftest.get("publication_status") != "local-only":
        errors.append("runtime selftest publication_status must be local-only")
    if selftest.get("report_kind") != "aex_native_loader_runtime_selftest":
        errors.append("runtime selftest report_kind must be aex_native_loader_runtime_selftest")
    if selftest.get("runtime_selftest_state") != "runtime_containment_selftest_passed_no_load":
        errors.append("runtime selftest state must be runtime_containment_selftest_passed_no_load")
    if selftest.get("runtime_containment_selftest_passed") is not True:
        errors.append("runtime_containment_selftest_passed must be true")
    if selftest.get("synthetic_subprocess_only") is not True:
        errors.append("synthetic_subprocess_only must be true")
    if selftest.get("path_allowlist_state") != "closed_no_aex_paths_accepted":
        errors.append("path_allowlist_state must be closed_no_aex_paths_accepted")
    if selftest.get("path_acceptance_ready") is not False:
        errors.append("path_acceptance_ready must be false")
    if selftest.get("aex_path_acceptance_enabled") is not False:
        errors.append("aex_path_acceptance_enabled must be false")
    if selftest.get("accepted_aex_path") is not None:
        errors.append("accepted_aex_path must be null")
    if selftest.get("path_payload_supplied") is not False:
        errors.append("path_payload_supplied must be false")
    for check in (
        "normal_exit_case_passed",
        "stderr_capture_passed",
        "timeout_case_passed",
        "child_cleanup_passed",
    ):
        if selftest.get(check) is not True:
            errors.append(f"{check} must be true")
    errors.extend(safety_errors(selftest, "runtime selftest"))
    return errors


def load_runtime_selftest(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_runtime_selftest_path(path)
    selftest = read_json_object(resolved)
    errors = validate_runtime_selftest(selftest)
    if errors:
        raise ValueError("; ".join(errors))
    return selftest, resolved


def classify_path_input(path_text: str) -> dict[str, Any]:
    pure = PureWindowsPath(path_text)
    parts = pure.parts
    suffix = pure.suffix.lower()
    if any(part in ("..", ".") for part in parts):
        reason = "traversal_component_rejected"
    elif pure.is_absolute():
        reason = "absolute_path_not_allowed_while_gate_closed"
    elif suffix != ".aex":
        reason = "non_aex_suffix_rejected"
    else:
        reason = "path_acceptance_gate_closed"
    return {
        "accepted": False,
        "reason": reason,
        "suffix": suffix,
        "path_tail": pure.name,
        "component_count": len(parts),
    }


def contains_raw_path(serialized: str, path_text: str) -> bool:
    return path_text in serialized or path_text.replace("\\", "\\\\") in serialized


def build_path_policy_selftest(
    *,
    runtime_selftest: dict[str, Any],
    runtime_selftest_path: Path,
) -> dict[str, Any]:
    errors = validate_runtime_selftest(runtime_selftest)
    if errors:
        raise ValueError("; ".join(errors))

    path_cases: list[dict[str, Any]] = []
    raw_inputs: list[str] = []
    for case in SYNTHETIC_CASES:
        path_text = case["input"]
        raw_inputs.append(path_text)
        classification = classify_path_input(path_text)
        if classification["accepted"] is not False:
            raise RuntimeError(f"{case['case']} unexpectedly accepted")
        if classification["reason"] != case["expected_reason"]:
            raise RuntimeError(f"{case['case']} reason mismatch")
        path_cases.append(
            {
                "case": case["case"],
                "accepted": False,
                "reason": classification["reason"],
                "suffix": classification["suffix"],
                "path_tail": classification["path_tail"],
                "component_count": classification["component_count"],
                "raw_path_serialized": False,
            }
        )

    report = {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_native_loader_path_policy_selftest",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_native_loader_runtime_selftest": str(runtime_selftest_path),
        "source_runtime_selftest_state": runtime_selftest.get("runtime_selftest_state"),
        "source_native_loader_runtime_contract_state": runtime_selftest.get(
            "source_native_loader_runtime_contract_state"
        ),
        "path_policy_selftest_state": "closed_path_policy_selftest_passed_no_aex_path",
        "path_policy_selftest_passed": True,
        "source_runtime_selftest_passed": runtime_selftest.get("runtime_containment_selftest_passed"),
        "path_allowlist_state": "closed_no_aex_paths_accepted",
        "path_acceptance_ready": False,
        "aex_path_acceptance_enabled": False,
        "accepted_aex_path": None,
        "path_payload_supplied": False,
        "synthetic_path_inputs_only": True,
        "candidate_path_string_accepted": False,
        "absolute_path_rejected": True,
        "traversal_rejected": True,
        "non_aex_suffix_rejected": True,
        "redaction_passed": True,
        "raw_input_paths_serialized": False,
        "path_case_count": len(path_cases),
        "path_cases": path_cases,
        "blocked_actions": runtime_selftest.get("blocked_actions", []),
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "notes": [
            "The selftest evaluates synthetic path strings in memory only.",
            "No path is supplied to the native-loader broker or accepted for runtime use.",
            "Raw input paths are not serialized into this report.",
            "No AEX file or DLL is opened, copied, hashed, loaded, or executed.",
        ],
    }
    serialized = json.dumps(report, ensure_ascii=False)
    if any(contains_raw_path(serialized, raw_input) for raw_input in raw_inputs):
        raise RuntimeError("raw synthetic path input leaked into report")
    return report


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run no-load native-loader closed path-policy selftest")
    parser.add_argument(
        "--runtime-selftest",
        required=True,
        help="Native-loader runtime selftest JSON under target/native-loader-runtime-selftest",
    )
    parser.add_argument("--out", required=True, help="Create-new report under target/native-loader-path-policy-selftest")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    runtime_selftest, runtime_selftest_path = load_runtime_selftest(Path(args.runtime_selftest))
    report = build_path_policy_selftest(
        runtime_selftest=runtime_selftest,
        runtime_selftest_path=runtime_selftest_path,
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Run a no-load runtime containment selftest with synthetic subprocesses.

The selftest reads the runtime contract JSON only. It exercises subprocess
normal exit, stderr capture, timeout termination, and child cleanup with small
synthetic Python processes. It accepts no AEX path and performs no native load.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import textwrap
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TARGET_ROOT = LAB_ROOT / "target"
RUNTIME_CONTRACT_ROOT = TARGET_ROOT / "native-loader-runtime-contract"
RUNTIME_SELFTEST_ROOT = TARGET_ROOT / "native-loader-runtime-selftest"

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


def validate_runtime_contract_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("native-loader runtime contract must have .json extension")
    return resolve_under_root(path, RUNTIME_CONTRACT_ROOT, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("native-loader runtime selftest report must have .json extension")
    RUNTIME_SELFTEST_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, RUNTIME_SELFTEST_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(RUNTIME_SELFTEST_ROOT.resolve(strict=True)):
        raise ValueError(f"native-loader runtime selftest parent must stay under {RUNTIME_SELFTEST_ROOT}")
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


def validate_runtime_contract(contract: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if contract.get("publication_status") != "local-only":
        errors.append("runtime contract publication_status must be local-only")
    if contract.get("report_kind") != "aex_native_loader_runtime_contract":
        errors.append("runtime contract report_kind must be aex_native_loader_runtime_contract")
    if contract.get("native_loader_runtime_contract_state") != "runtime_containment_contract_ready_no_load":
        errors.append("runtime contract state must be runtime_containment_contract_ready_no_load")
    if contract.get("contract_state") != "runtime_containment_contract_ready_path_acceptance_closed":
        errors.append("runtime contract contract_state must be path acceptance closed")
    if contract.get("runtime_containment_ready") is not True:
        errors.append("runtime_containment_ready must be true")
    if contract.get("path_allowlist_state") != "closed_no_aex_paths_accepted":
        errors.append("path_allowlist_state must be closed_no_aex_paths_accepted")
    if contract.get("path_acceptance_ready") is not False:
        errors.append("path_acceptance_ready must be false")
    if contract.get("aex_path_acceptance_enabled") is not False:
        errors.append("aex_path_acceptance_enabled must be false")
    if contract.get("accepted_aex_path") is not None:
        errors.append("accepted_aex_path must be null")
    if contract.get("path_payload_supplied") is not False:
        errors.append("path_payload_supplied must be false")
    if contract.get("broker_selftest_passed") is not True:
        errors.append("broker_selftest_passed must be true")
    if contract.get("process_isolation_required") is not True:
        errors.append("process_isolation_required must be true")
    if contract.get("native_load_gate") != "closed":
        errors.append("native_load_gate must be closed")
    if contract.get("fixture_approval_satisfied") is not False:
        errors.append("fixture approval must not be satisfied")
    if contract.get("candidate_dependencies_clear") is not True:
        errors.append("candidate_dependencies_clear must be true")
    errors.extend(safety_errors(contract, "runtime contract"))
    return errors


def load_runtime_contract(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_runtime_contract_path(path)
    contract = read_json_object(resolved)
    errors = validate_runtime_contract(contract)
    if errors:
        raise ValueError("; ".join(errors))
    return contract, resolved


def run_child_case(name: str, code: str, timeout_ms: int, cleanup_timeout_ms: int) -> dict[str, Any]:
    process = subprocess.Popen(
        [sys.executable, "-c", code],
        cwd=str(LAB_ROOT),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
    )
    timed_out = False
    terminated = False
    killed = False
    try:
        stdout, stderr = process.communicate(timeout=timeout_ms / 1000)
    except subprocess.TimeoutExpired:
        timed_out = True
        process.terminate()
        terminated = True
        try:
            stdout, stderr = process.communicate(timeout=cleanup_timeout_ms / 1000)
        except subprocess.TimeoutExpired:
            process.kill()
            killed = True
            stdout, stderr = process.communicate(timeout=cleanup_timeout_ms / 1000)
    return {
        "case": name,
        "return_code": process.returncode,
        "timed_out": timed_out,
        "terminated": terminated,
        "killed": killed,
        "cleanup_success": process.poll() is not None,
        "stdout": stdout,
        "stderr": stderr,
        "stdout_line_count": len(stdout.splitlines()),
        "stderr_line_count": len(stderr.splitlines()),
    }


def normal_child_code() -> str:
    return textwrap.dedent(
        """
        import json
        import sys
        sys.stdout.write(json.dumps({"case": "normal_exit", "synthetic": True}) + "\\n")
        sys.stdout.flush()
        sys.stderr.write("synthetic stderr capture\\n")
        sys.stderr.flush()
        """
    ).strip()


def timeout_child_code() -> str:
    return textwrap.dedent(
        """
        import sys
        import time
        sys.stderr.write("synthetic timeout stderr before sleep\\n")
        sys.stderr.flush()
        time.sleep(5)
        """
    ).strip()


def build_runtime_selftest(
    *,
    runtime_contract: dict[str, Any],
    runtime_contract_path: Path,
    timeout_ms: int = 1000,
    cleanup_timeout_ms: int = 2000,
) -> dict[str, Any]:
    errors = validate_runtime_contract(runtime_contract)
    if errors:
        raise ValueError("; ".join(errors))
    if timeout_ms <= 0:
        raise ValueError("timeout_ms must be positive")
    if cleanup_timeout_ms <= 0:
        raise ValueError("cleanup_timeout_ms must be positive")

    normal = run_child_case("normal_exit", normal_child_code(), timeout_ms, cleanup_timeout_ms)
    timeout = run_child_case("timeout_termination", timeout_child_code(), timeout_ms, cleanup_timeout_ms)

    normal_stdout_ok = False
    try:
        first_line = normal["stdout"].splitlines()[0]
        normal_stdout_ok = json.loads(first_line) == {"case": "normal_exit", "synthetic": True}
    except (IndexError, json.JSONDecodeError):
        normal_stdout_ok = False
    normal_exit_case_passed = (
        normal["return_code"] == 0
        and normal["timed_out"] is False
        and normal["cleanup_success"] is True
        and normal_stdout_ok
    )
    stderr_capture_passed = "synthetic stderr capture" in normal["stderr"]
    timeout_case_passed = timeout["timed_out"] is True and timeout["terminated"] is True
    child_cleanup_passed = timeout["cleanup_success"] is True and timeout["return_code"] is not None

    if not (normal_exit_case_passed and stderr_capture_passed and timeout_case_passed and child_cleanup_passed):
        raise RuntimeError("synthetic runtime containment selftest failed")

    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_native_loader_runtime_selftest",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_native_loader_runtime_contract": str(runtime_contract_path),
        "source_native_loader_runtime_contract_state": runtime_contract.get(
            "native_loader_runtime_contract_state"
        ),
        "source_contract_state": runtime_contract.get("contract_state"),
        "runtime_selftest_state": "runtime_containment_selftest_passed_no_load",
        "runtime_containment_selftest_passed": True,
        "runtime_containment_ready": runtime_contract.get("runtime_containment_ready"),
        "path_allowlist_state": runtime_contract.get("path_allowlist_state"),
        "path_acceptance_ready": False,
        "aex_path_acceptance_enabled": False,
        "accepted_aex_path": None,
        "path_payload_supplied": False,
        "synthetic_subprocess_only": True,
        "normal_exit_case_passed": normal_exit_case_passed,
        "stderr_capture_passed": stderr_capture_passed,
        "timeout_case_passed": timeout_case_passed,
        "child_cleanup_passed": child_cleanup_passed,
        "timeout_ms": timeout_ms,
        "cleanup_timeout_ms": cleanup_timeout_ms,
        "child_cases": [
            {
                "case": normal["case"],
                "return_code": normal["return_code"],
                "timed_out": normal["timed_out"],
                "cleanup_success": normal["cleanup_success"],
                "stdout_line_count": normal["stdout_line_count"],
                "stderr_line_count": normal["stderr_line_count"],
            },
            {
                "case": timeout["case"],
                "return_code": timeout["return_code"],
                "timed_out": timeout["timed_out"],
                "terminated": timeout["terminated"],
                "killed": timeout["killed"],
                "cleanup_success": timeout["cleanup_success"],
                "stdout_line_count": timeout["stdout_line_count"],
                "stderr_line_count": timeout["stderr_line_count"],
            },
        ],
        "captured_stderr_summary": {
            "normal_stderr_line_count": normal["stderr_line_count"],
            "timeout_stderr_line_count": timeout["stderr_line_count"],
            "raw_stderr_serialized": False,
        },
        "blocked_actions": runtime_contract.get("blocked_actions", []),
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "notes": [
            "The selftest uses synthetic Python subprocesses only.",
            "No AEX path is supplied to any process.",
            "No AEX file or DLL is opened, copied, hashed, loaded, or executed.",
            "No EffectMain, AE, render, or OFX route is invoked.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run no-load native-loader runtime containment selftest")
    parser.add_argument(
        "--runtime-contract",
        required=True,
        help="Native-loader runtime contract JSON under target/native-loader-runtime-contract",
    )
    parser.add_argument("--timeout-ms", type=int, default=1000, help="Synthetic child timeout in milliseconds")
    parser.add_argument(
        "--cleanup-timeout-ms",
        type=int,
        default=2000,
        help="Synthetic child cleanup timeout in milliseconds",
    )
    parser.add_argument("--out", required=True, help="Create-new report under target/native-loader-runtime-selftest")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    contract, contract_path = load_runtime_contract(Path(args.runtime_contract))
    report = build_runtime_selftest(
        runtime_contract=contract,
        runtime_contract_path=contract_path,
        timeout_ms=args.timeout_ms,
        cleanup_timeout_ms=args.cleanup_timeout_ms,
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

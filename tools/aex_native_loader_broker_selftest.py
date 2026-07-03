#!/usr/bin/env python3
"""Controller-side selftest for the pathless native-loader broker."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TOOLS_ROOT = LAB_ROOT / "tools"
NATIVE_LOADER_DESIGN_ROOT = LAB_ROOT / "target" / "native-loader-design"
BROKER_SELFTEST_ROOT = LAB_ROOT / "target" / "native-loader-broker-selftest"

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

BLOCKED_CHECK_MESSAGES = (
    "accept_aex_path",
    "open_aex_file",
    "load_aex_dll",
    "call_effect_main",
    "render_frame",
    "route_through_ofx",
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


def validate_broker_path(path: Path) -> Path:
    if path.suffix.lower() != ".py":
        raise ValueError("broker path must have .py extension")
    return resolve_under_root(path, TOOLS_ROOT, must_exist=True)


def validate_design_contract_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("native-loader design contract must have .json extension")
    return resolve_under_root(path, NATIVE_LOADER_DESIGN_ROOT, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("native-loader broker selftest report must have .json extension")
    BROKER_SELFTEST_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, BROKER_SELFTEST_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(BROKER_SELFTEST_ROOT.resolve(strict=True)):
        raise ValueError(f"native-loader broker selftest parent must stay under {BROKER_SELFTEST_ROOT}")
    return resolved


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("source JSON must be an object")
    return payload


def load_design_contract(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_design_contract_path(path)
    contract = read_json_object(resolved)
    errors = validate_design_contract(contract)
    if errors:
        raise ValueError("; ".join(errors))
    return contract, resolved


def validate_design_contract(contract: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if contract.get("publication_status") != "local-only":
        errors.append("native-loader design contract publication_status must be local-only")
    if contract.get("report_kind") != "aex_native_loader_design_contract":
        errors.append("native-loader design contract report_kind must be aex_native_loader_design_contract")
    if contract.get("native_loader_design_state") != "native_loader_design_ready_loader_closed":
        errors.append("native-loader design state must be native_loader_design_ready_loader_closed")
    if contract.get("contract_state") != "native_loader_design_contract_ready_loader_closed_pending_fixture_approval":
        errors.append("native-loader design contract_state must be ready loader closed pending fixture approval")
    if contract.get("loader_design_ready") is not True:
        errors.append("loader_design_ready must be true")
    if contract.get("accepts_aex_path") is not False:
        errors.append("design contract accepts_aex_path must be false")
    if contract.get("accepted_aex_path") is not None:
        errors.append("design contract accepted_aex_path must be null")
    if contract.get("controller_loads_aex") is not False:
        errors.append("design contract controller_loads_aex must be false")
    if contract.get("separate_process_required") is not True:
        errors.append("design contract separate_process_required must be true")
    if contract.get("fixture_approval_satisfied") is not False:
        errors.append("design contract fixture approval must not be satisfied")
    for flag in SAFETY_FLAGS:
        if contract.get(flag) is not False:
            errors.append(f"native-loader design contract {flag} must be false")
    return errors


def ensure_safety_state(response: dict[str, Any]) -> None:
    safety = response.get("safety_state")
    if not isinstance(safety, dict):
        raise AssertionError(f"response missing safety_state: {response}")
    for flag in SAFETY_FLAGS:
        if safety.get(flag) is not False:
            raise AssertionError(f"safety flag {flag} was not false in {response}")
    if safety.get("accepts_aex_path") is not False:
        raise AssertionError(f"accepts_aex_path was not false in {response}")
    if safety.get("accepted_aex_path") is not None:
        raise AssertionError(f"accepted_aex_path was not null in {response}")


class BrokerClient:
    def __init__(self, broker_path: Path):
        self.broker_path = broker_path
        self.process: subprocess.Popen[str] | None = None

    def __enter__(self) -> "BrokerClient":
        self.process = subprocess.Popen(
            [sys.executable, str(self.broker_path)],
            cwd=str(LAB_ROOT),
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
        )
        return self

    def __exit__(self, exc_type: Any, exc: Any, tb: Any) -> None:
        if self.process and self.process.poll() is None:
            self.process.terminate()
            self.process.wait(timeout=5)
        self.close_streams()

    def close_streams(self) -> None:
        if self.process is None:
            return
        for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
            if stream is not None and not stream.closed:
                stream.close()

    def send(self, message: dict[str, Any]) -> dict[str, Any]:
        if self.process is None or self.process.stdin is None or self.process.stdout is None:
            raise RuntimeError("broker process is not running")
        self.process.stdin.write(json.dumps(message, ensure_ascii=False) + "\n")
        self.process.stdin.flush()
        line = self.process.stdout.readline()
        if not line:
            stderr = self.process.stderr.read() if self.process.stderr else ""
            raise RuntimeError(f"broker produced no response; stderr={stderr}")
        response = json.loads(line)
        if not isinstance(response, dict):
            raise RuntimeError("broker response must be an object")
        return response

    def wait(self) -> tuple[int, str]:
        if self.process is None:
            return 0, ""
        return_code = self.process.wait(timeout=5)
        stderr = self.process.stderr.read() if self.process.stderr else ""
        self.close_streams()
        return return_code, stderr


def run_selftest(*, broker_path: Path, design_contract_path: Path) -> dict[str, Any]:
    contract, resolved_contract = load_design_contract(design_contract_path)
    resolved_broker = validate_broker_path(broker_path)
    steps: list[dict[str, Any]] = []
    blocked_action_checks: list[dict[str, Any]] = []

    with BrokerClient(resolved_broker) as client:
        hello = client.send({"id": "hello", "type": "hello"})
        ensure_safety_state(hello)
        if hello.get("type") != "hello_ack":
            raise AssertionError(f"unexpected hello response: {hello}")
        steps.append(
            {
                "step": "hello",
                "response_type": hello.get("type"),
                "broker_kind": hello.get("broker_kind"),
                "allowed_messages": hello.get("allowed_messages", []),
            }
        )

        environment = client.send({"id": "env", "type": "inspect_environment"})
        ensure_safety_state(environment)
        if environment.get("native_load_enabled") is not False:
            raise AssertionError("broker reported native_load_enabled != false")
        if environment.get("accepts_aex_path") is not False:
            raise AssertionError("broker reported accepts_aex_path != false")
        if environment.get("accepted_aex_path") is not None:
            raise AssertionError("broker reported accepted_aex_path != null")
        steps.append(
            {
                "step": "inspect_environment",
                "response_type": environment.get("type"),
                "process_bitness": environment.get("process_bitness"),
                "native_load_enabled": environment.get("native_load_enabled"),
                "accepts_aex_path": environment.get("accepts_aex_path"),
                "accepted_aex_path": environment.get("accepted_aex_path"),
            }
        )

        for message_type in BLOCKED_CHECK_MESSAGES:
            blocked = client.send({"id": f"blocked-{message_type}", "type": message_type})
            ensure_safety_state(blocked)
            if blocked.get("type") != "error" or blocked.get("code") != "blocked_action":
                raise AssertionError(f"{message_type} was not rejected: {blocked}")
            blocked_action_checks.append(
                {
                    "message_type": message_type,
                    "response_type": blocked.get("type"),
                    "code": blocked.get("code"),
                    "path_payload_supplied": False,
                }
            )

        steps.append(
            {
                "step": "blocked_pathless_native_actions",
                "response_type": "error",
                "blocked_action_count": len(blocked_action_checks),
                "path_payload_supplied": False,
            }
        )

        quit_response = client.send({"id": "quit", "type": "quit"})
        ensure_safety_state(quit_response)
        if quit_response.get("type") != "quit_ack":
            raise AssertionError(f"unexpected quit response: {quit_response}")
        return_code, stderr = client.wait()
        if return_code != 0:
            raise AssertionError(f"broker exited with {return_code}: {stderr}")
        steps.append({"step": "quit", "response_type": quit_response.get("type"), "return_code": return_code})

    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_native_loader_broker_selftest",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "broker": str(resolved_broker),
        "source_native_loader_design_contract": str(resolved_contract),
        "source_native_loader_design_state": contract.get("native_loader_design_state"),
        "source_contract_state": contract.get("contract_state"),
        "broker_selftest_state": "pathless_native_loader_broker_selftest_passed",
        "pathless_broker_ready": True,
        "native_loader_design_ready": contract.get("loader_design_ready"),
        "candidate_relative_path": contract.get("candidate_relative_path"),
        "candidate_dependencies_clear": contract.get("candidate_dependencies_clear"),
        "fixture_approval_satisfied": contract.get("fixture_approval_satisfied"),
        "accepts_aex_path": False,
        "accepted_aex_path": None,
        "path_payload_supplied": False,
        "blocked_action_count": len(blocked_action_checks),
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "blocked_action_checks": blocked_action_checks,
        "steps": steps,
        "allowed_next_actions": [
            "keep broker pathless until explicit fixture approval exists",
            "repeat candidate-scoped load gate after approval artifact changes",
            "design path allowlist and runtime containment before accepting any AEX path",
        ],
        "notes": [
            "The selftest starts a pathless broker subprocess and exchanges JSONL messages.",
            "No AEX path is supplied to the broker during this selftest.",
            "AEX path acceptance, file open, DLL load, EffectMain, render, and OFX route messages are blocked.",
            "No AEX file or DLL is opened, copied, hashed, loaded, or executed.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run pathless native-loader broker selftest")
    parser.add_argument("--broker", default=str(TOOLS_ROOT / "aex_native_loader_broker.py"))
    parser.add_argument(
        "--design-contract",
        required=True,
        help="Native-loader design contract JSON under target/native-loader-design",
    )
    parser.add_argument("--out", required=True, help="Create-new selftest JSON under target/native-loader-broker-selftest")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    report = run_selftest(
        broker_path=Path(args.broker),
        design_contract_path=Path(args.design_contract),
    )
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

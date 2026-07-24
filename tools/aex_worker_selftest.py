#!/usr/bin/env python3
"""Controller-side selftest for the no-load AEX worker harness."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
TOOLS_ROOT = LAB_ROOT / "tools"
if str(TOOLS_ROOT) not in sys.path:
    sys.path.insert(0, str(TOOLS_ROOT))

import ppm_fixture_tool

PPM_FIXTURE_ROOT = LAB_ROOT / "target" / "ppm-fixtures"
WORKER_SELFTEST_ROOT = LAB_ROOT / "target" / "worker-selftest"
WORKER_DESIGN_ROOT = LAB_ROOT / "target" / "worker-design"

SAFETY_FLAGS = (
    "native_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
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


def validate_output_json(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("selftest report must have .json extension")
    WORKER_SELFTEST_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, WORKER_SELFTEST_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(WORKER_SELFTEST_ROOT.resolve(strict=True)):
        raise ValueError(f"selftest report parent must stay under {WORKER_SELFTEST_ROOT}")
    return resolved


def validate_input_ppm(path: Path) -> Path:
    if path.suffix.lower() != ".ppm":
        raise ValueError("input PPM must have .ppm extension")
    PPM_FIXTURE_ROOT.mkdir(parents=True, exist_ok=True)
    return resolve_under_root(path, PPM_FIXTURE_ROOT, must_exist=True)


def validate_output_ppm(path: Path) -> Path:
    if path.suffix.lower() != ".ppm":
        raise ValueError("output PPM must have .ppm extension")
    WORKER_SELFTEST_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, WORKER_SELFTEST_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    return resolved


def validate_worker_path(path: Path) -> Path:
    if path.suffix.lower() != ".py":
        raise ValueError("worker path must have .py extension")
    return resolve_under_root(path, TOOLS_ROOT, must_exist=True)


def validate_design_packet_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("design packet must have .json extension")
    return resolve_under_root(path, WORKER_DESIGN_ROOT, must_exist=True)


def load_design_packet(path: Path | None) -> tuple[dict[str, Any] | None, Path | None]:
    if path is None:
        return None, None
    resolved = validate_design_packet_path(path)
    with resolved.open("r", encoding="utf-8") as handle:
        packet = json.load(handle)
    if not isinstance(packet, dict):
        raise ValueError("design packet must be a JSON object")
    if packet.get("packet_kind") != "aex_worker_sandbox_design_packet":
        raise ValueError("design packet kind mismatch")
    for flag in SAFETY_FLAGS:
        if packet.get(flag) is not False:
            raise ValueError(f"design packet {flag} must be false")
    return packet, resolved


def create_default_input_ppm() -> Path:
    PPM_FIXTURE_ROOT.mkdir(parents=True, exist_ok=True)
    output = PPM_FIXTURE_ROOT / f"worker-selftest-input-{time.time_ns()}.ppm"
    image = ppm_fixture_tool.generate_image(8, 8, "checker")
    ppm_fixture_tool.write_ppm_create_new(output, image)
    return output


def ensure_safety_state(response: dict[str, Any]) -> None:
    safety = response.get("safety_state")
    if not isinstance(safety, dict):
        raise AssertionError(f"response missing safety_state: {response}")
    expected_false = {
        "native_load_enabled",
        "native_load_performed",
        "render_performed",
        "ae_invoked",
        "ofx_route_invoked",
        "private_payload_copied",
        "aex_file_opened",
    }
    for flag in expected_false:
        if safety.get(flag) is not False:
            raise AssertionError(f"safety flag {flag} was not false in {response}")


class WorkerClient:
    def __init__(self, worker_path: Path):
        self.worker_path = worker_path
        self.process: subprocess.Popen[str] | None = None

    def __enter__(self) -> "WorkerClient":
        self.process = subprocess.Popen(
            [sys.executable, str(self.worker_path)],
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
            raise RuntimeError("worker process is not running")
        self.process.stdin.write(json.dumps(message, ensure_ascii=False) + "\n")
        self.process.stdin.flush()
        line = self.process.stdout.readline()
        if not line:
            stderr = self.process.stderr.read() if self.process.stderr else ""
            raise RuntimeError(f"worker produced no response; stderr={stderr}")
        response = json.loads(line)
        if not isinstance(response, dict):
            raise RuntimeError("worker response must be an object")
        return response

    def wait(self) -> tuple[int, str]:
        if self.process is None:
            return 0, ""
        return_code = self.process.wait(timeout=5)
        stderr = self.process.stderr.read() if self.process.stderr else ""
        self.close_streams()
        return return_code, stderr


def compare_ppm_identity(input_path: Path, output_path: Path) -> dict[str, Any]:
    input_image = ppm_fixture_tool.read_ppm(input_path)
    output_image = ppm_fixture_tool.read_ppm(output_path)
    pixel_match = input_image.pixels == output_image.pixels
    dimension_match = input_image.width == output_image.width and input_image.height == output_image.height
    if not pixel_match or not dimension_match:
        raise AssertionError("identity worker output did not match input pixels/dimensions")
    return {
        "width": input_image.width,
        "height": input_image.height,
        "bytes": len(input_image.pixels),
        "pixel_match": pixel_match,
        "dimension_match": dimension_match,
    }


def run_selftest(
    *,
    worker_path: Path,
    input_ppm: Path | None,
    output_ppm: Path,
    design_packet_path: Path | None = None,
) -> dict[str, Any]:
    design_packet, resolved_design_packet = load_design_packet(design_packet_path)
    resolved_worker = validate_worker_path(worker_path)
    resolved_input = validate_input_ppm(input_ppm) if input_ppm else create_default_input_ppm()
    resolved_output = validate_output_ppm(output_ppm)

    steps: list[dict[str, Any]] = []
    with WorkerClient(resolved_worker) as client:
        hello = client.send({"id": "hello", "type": "hello"})
        ensure_safety_state(hello)
        if hello.get("type") != "hello_ack":
            raise AssertionError(f"unexpected hello response: {hello}")
        steps.append({"step": "hello", "response_type": hello.get("type")})

        environment = client.send({"id": "env", "type": "inspect_environment"})
        ensure_safety_state(environment)
        if environment.get("native_load_enabled") is not False:
            raise AssertionError("worker reported native_load_enabled != false")
        steps.append(
            {
                "step": "inspect_environment",
                "response_type": environment.get("type"),
                "process_bitness": environment.get("process_bitness"),
                "native_load_enabled": environment.get("native_load_enabled"),
            }
        )

        inspect = client.send({"id": "inspect", "type": "inspect_ppm", "input": str(resolved_input)})
        ensure_safety_state(inspect)
        if inspect.get("type") != "ppm_summary":
            raise AssertionError(f"unexpected inspect response: {inspect}")
        steps.append(
            {
                "step": "inspect_ppm",
                "response_type": inspect.get("type"),
                "width": inspect.get("width"),
                "height": inspect.get("height"),
                "bytes": inspect.get("bytes"),
            }
        )

        transform = client.send(
            {
                "id": "identity",
                "type": "transform_ppm_identity",
                "input": str(resolved_input),
                "out": str(resolved_output),
            }
        )
        ensure_safety_state(transform)
        if transform.get("type") != "created_output":
            raise AssertionError(f"unexpected transform response: {transform}")
        identity_check = compare_ppm_identity(resolved_input, resolved_output)
        steps.append(
            {
                "step": "transform_ppm_identity",
                "response_type": transform.get("type"),
                "output": transform.get("path"),
                "identity_check": identity_check,
            }
        )

        blocked = client.send({"id": "blocked", "type": "load_aex", "path": "not-used.aex"})
        ensure_safety_state(blocked)
        if blocked.get("type") != "error" or blocked.get("code") != "blocked_action":
            raise AssertionError(f"blocked action was not rejected: {blocked}")
        steps.append({"step": "blocked_load_aex", "response_type": blocked.get("type"), "code": blocked.get("code")})

        quit_response = client.send({"id": "quit", "type": "quit"})
        ensure_safety_state(quit_response)
        if quit_response.get("type") != "quit_ack":
            raise AssertionError(f"unexpected quit response: {quit_response}")
        return_code, stderr = client.wait()
        if return_code != 0:
            raise AssertionError(f"worker exited with {return_code}: {stderr}")
        steps.append({"step": "quit", "response_type": quit_response.get("type"), "return_code": return_code})

    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_no_load_worker_selftest",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "worker": str(resolved_worker),
        "source_design_packet": str(resolved_design_packet) if resolved_design_packet else None,
        "source_design_packet_kind": design_packet.get("packet_kind") if design_packet else None,
        "input_ppm": str(resolved_input),
        "output_ppm": str(resolved_output),
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "worker_selftest_passed": True,
        "steps": steps,
        "notes": [
            "Controller starts a no-load worker subprocess and exchanges JSONL messages.",
            "Only PPM fixture inspection and identity transform are exercised.",
            "The blocked load_aex message is verified to fail closed.",
            "No AEX file is opened, copied, hashed, loaded, or executed.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_json(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run no-load AEX worker selftest")
    parser.add_argument("--worker", default=str(TOOLS_ROOT / "aex_no_load_worker.py"))
    parser.add_argument("--design-packet", help="Optional worker design packet under target/worker-design")
    parser.add_argument("--input-ppm", help="Optional input PPM under target/ppm-fixtures")
    parser.add_argument("--output-ppm", required=True, help="Create-new output PPM under target/worker-selftest")
    parser.add_argument("--out", required=True, help="Create-new selftest JSON under target/worker-selftest")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    output_ppm = validate_output_ppm(Path(args.output_ppm))
    report = run_selftest(
        worker_path=Path(args.worker),
        input_ppm=Path(args.input_ppm) if args.input_ppm else None,
        output_ppm=output_ppm,
        design_packet_path=Path(args.design_packet) if args.design_packet else None,
    )
    reported_output_ppm = Path(str(report.get("output_ppm", ""))).resolve(strict=False)
    if reported_output_ppm != output_ppm:
        raise AssertionError("selftest report output path does not match requested output")

    try:
        written = write_json_create_new(Path(args.out), report)
    except OSError:
        output_ppm.unlink(missing_ok=True)
        raise
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

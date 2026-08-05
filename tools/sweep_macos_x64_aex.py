#!/usr/bin/env python3
"""Run a SHA-addressed Windows inventory subset through selected macOS x64 guests."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import select
import signal
import struct
import subprocess
import tempfile
import time
from collections import Counter
from pathlib import Path
from typing import BinaryIO

from PIL import Image


SCHEMA_VERSION = 2
MAX_MESSAGE_BYTES = 64 * 1024
MAX_ERROR_BYTES = 4096
MAX_DURABLE_ERROR_BYTES = 1024
START_TIMEOUT_SECONDS = 10.0
RENDER_TIMEOUT_SECONDS = 30.0
CLOSE_TIMEOUT_SECONDS = 2.0


class SweepError(RuntimeError):
    pass


class AdmissionFailure(SweepError):
    def __init__(self, diagnostic: dict[str, object], render_error: int):
        self.diagnostic = diagnostic
        self.render_error = render_error
        self.termination_evidence = ""
        super().__init__(
            "resident admission probe failed: "
            f"{diagnostic['category']} {diagnostic['message']}"
        )


class BackendFailure(SweepError):
    def __init__(
        self,
        cause: Exception,
        failure_stage: str,
        admission_success: bool,
        render_success: bool,
    ):
        self.cause = cause
        self.failure_stage = failure_stage
        self.admission_success = admission_success
        self.render_success = render_success
        super().__init__(str(cause))


def bounded_utf8(value: str, limit: int) -> str:
    encoded = value.encode("utf-8")
    if len(encoded) <= limit:
        return value
    return encoded[:limit].decode("utf-8", errors="ignore")


def sanitize_error_text(
    value: str, redactions: dict[str, str] | None = None
) -> str:
    replacements = {os.fspath(Path.home()): "<home>"}
    replacements.update(redactions or {})
    for private_path, token in sorted(
        replacements.items(), key=lambda item: len(item[0]), reverse=True
    ):
        if private_path and private_path != os.path.sep:
            value = value.replace(private_path, token)
    if "crash_snapshot=" in value:
        value = value.split("crash_snapshot=", 1)[0] + "crash_snapshot=<omitted>"
    return bounded_utf8(value, MAX_DURABLE_ERROR_BYTES)


def sanitize_diagnostic(
    value: dict[str, object], redactions: dict[str, str] | None = None
) -> dict[str, object]:
    result = dict(value)
    result["message"] = sanitize_error_text(value["message"], redactions)
    if value["crash_reason"] is not None:
        result["crash_reason"] = sanitize_error_text(
            value["crash_reason"], redactions
        )
    return result


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _reject_duplicate_pairs(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise SweepError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json_strict(path: Path) -> dict[str, object]:
    try:
        value = json.loads(
            path.read_text(encoding="utf-8"),
            object_pairs_hook=_reject_duplicate_pairs,
        )
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise SweepError(f"read strict JSON {path}: {error}") from error
    if not isinstance(value, dict):
        raise SweepError(f"JSON root must be an object: {path}")
    return value


def inventory_by_sha(inventory: dict[str, object]) -> dict[str, list[dict[str, object]]]:
    if inventory.get("schema_version") != 1:
        raise SweepError("Windows inventory schema_version must be 1")
    entries = inventory.get("entries")
    if not isinstance(entries, list) or not entries:
        raise SweepError("Windows inventory entries must be a nonempty array")
    result: dict[str, list[dict[str, object]]] = {}
    for index, value in enumerate(entries):
        if not isinstance(value, dict):
            raise SweepError(f"inventory entry {index} is not an object")
        sha = value.get("sha256")
        if sha is None:
            continue
        if not isinstance(sha, str) or len(sha) != 64:
            raise SweepError(f"inventory entry {index} has an invalid SHA-256")
        result.setdefault(sha.lower(), []).append(value)
    return result


def map_corpus(
    inventory: dict[str, object], corpus_roots: list[Path]
) -> list[dict[str, object]]:
    by_sha = inventory_by_sha(inventory)
    files: list[Path] = []
    for root in corpus_roots:
        resolved = root.resolve(strict=True)
        if not resolved.is_dir():
            raise SweepError(f"corpus root is not a directory: {resolved}")
        files.extend(path for path in resolved.rglob("*") if path.is_file() and path.suffix.lower() == ".aex")
    files = sorted(set(files), key=lambda path: os.fspath(path).casefold())
    if not files:
        raise SweepError("Mac corpus contains no AEX files")
    seen_sha: dict[str, Path] = {}
    mapped = []
    for path in files:
        sha = sha256_file(path)
        previous = seen_sha.get(sha)
        if previous is not None:
            raise SweepError(f"duplicate Mac corpus SHA: {previous.name} and {path.name}")
        seen_sha[sha] = path
        matches = by_sha.get(sha, [])
        if not matches:
            raise SweepError(f"Mac AEX is absent from Windows inventory: {path.name} ({sha})")
        architectures = sorted({str(match.get("architecture")) for match in matches})
        if architectures != ["x64"]:
            raise SweepError(f"Mac AEX inventory architecture is not exactly x64: {path.name} {architectures}")
        mapped.append(
            {
                "path": path,
                "name": path.name,
                "sha256": sha,
                "windows_match_count": len(matches),
                "windows_root_categories": sorted(
                    {str(match.get("root_category")) for match in matches}
                ),
                "windows_source_categories": sorted(
                    {str(match.get("source_category")) for match in matches}
                ),
            }
        )
    return sorted(mapped, key=lambda item: item["sha256"])


def png_to_argb8(path: Path) -> tuple[int, int, bytes]:
    with Image.open(path) as image:
        rgba = image.convert("RGBA")
        width, height = rgba.size
        if width <= 0 or height <= 0 or width > 1920 or height > 1080:
            raise SweepError(f"input PNG dimensions are unsupported: {width}x{height}")
        source = rgba.tobytes()
    argb = bytearray(len(source))
    for offset in range(0, len(source), 4):
        red, green, blue, alpha = source[offset : offset + 4]
        argb[offset : offset + 4] = bytes((alpha, red, green, blue))
    return width, height, bytes(argb)


def write_message(stream: BinaryIO, value: dict[str, object]) -> None:
    payload = json.dumps(value, separators=(",", ":"), ensure_ascii=True).encode("ascii")
    if not payload or len(payload) > MAX_MESSAGE_BYTES:
        raise SweepError("control request exceeds the protocol bound")
    stream.write(struct.pack("<I", len(payload)))
    stream.write(payload)
    stream.flush()


def _read_exact_timeout(stream: BinaryIO, size: int, deadline: float) -> bytes:
    chunks = bytearray()
    descriptor = stream.fileno()
    while len(chunks) < size:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise SweepError("control response timed out")
        readable, _, _ = select.select([descriptor], [], [], remaining)
        if not readable:
            raise SweepError("control response timed out")
        chunk = os.read(descriptor, size - len(chunks))
        if not chunk:
            raise SweepError("worker closed its control stream")
        chunks.extend(chunk)
    return bytes(chunks)


def read_message(stream: BinaryIO, timeout: float) -> dict[str, object]:
    deadline = time.monotonic() + timeout
    length = struct.unpack("<I", _read_exact_timeout(stream, 4, deadline))[0]
    if length == 0 or length > MAX_MESSAGE_BYTES:
        raise SweepError(f"invalid control response length: {length}")
    payload = _read_exact_timeout(stream, length, deadline)
    try:
        value = json.loads(payload, object_pairs_hook=_reject_duplicate_pairs)
    except (UnicodeError, json.JSONDecodeError) as error:
        raise SweepError(f"invalid control response JSON: {error}") from error
    if not isinstance(value, dict):
        raise SweepError("control response root is not an object")
    return value


def require_exact_keys(value: dict[str, object], expected: set[str], label: str) -> None:
    if set(value) != expected:
        raise SweepError(f"{label} keys differ: {sorted(value)}")


def validate_setup(value: object) -> None:
    if not isinstance(value, dict):
        raise SweepError("session setup is not an object")
    require_exact_keys(
        value,
        {
            "schema_version",
            "execution_backend",
            "global_setup_error",
            "params_setup_error",
            "advertised_num_params",
            "out_flags",
            "out_flags2",
            "parameters",
            "suite_requests",
            "unsupported_suite_calls",
            "dropped_unsupported_suite_calls",
        },
        "session setup",
    )
    if (
        value.get("schema_version") != 1
        or not isinstance(value.get("execution_backend"), str)
        or value.get("global_setup_error") != 0
        or value.get("params_setup_error") != 0
        or not isinstance(value.get("parameters"), list)
        or not isinstance(value.get("suite_requests"), list)
        or not isinstance(value.get("unsupported_suite_calls"), list)
        or not isinstance(value.get("dropped_unsupported_suite_calls"), int)
    ):
        raise SweepError(f"invalid session setup: {value}")


def validate_ready(value: dict[str, object], pid: int) -> None:
    require_exact_keys(value, {"v", "type", "worker_pid", "setup"}, "session_ready")
    if (
        value.get("v") != 1
        or value.get("type") != "session_ready"
        or value.get("worker_pid") != pid
        or not isinstance(value.get("setup"), dict)
    ):
        raise SweepError(f"invalid session_ready: {value}")
    validate_setup(value["setup"])


def require_backend(
    ready: dict[str, object], expected_backend: str, label: str
) -> None:
    setup = ready["setup"]
    assert isinstance(setup, dict)
    actual = setup["execution_backend"]
    if actual != expected_backend:
        raise SweepError(
            f"{label} worker backend differs: {actual} != {expected_backend}"
        )


def validate_unsupported_suite_calls(value: object) -> None:
    if not isinstance(value, list):
        raise SweepError("unsupported_suite_calls is not an array")
    if len(value) > 64:
        raise SweepError("unsupported_suite_calls exceeds the diagnostic bound")
    for call in value:
        if not isinstance(call, dict):
            raise SweepError("unsupported suite call is not an object")
        require_exact_keys(
            call, {"name", "version", "slot", "call_count"}, "unsupported suite call"
        )
        if (
            not isinstance(call.get("name"), str)
            or type(call.get("version")) is not int
            or type(call.get("slot")) is not int
            or type(call.get("call_count")) is not int
            or call["version"] < 0
            or call["slot"] < 0
            or call["call_count"] < 1
        ):
            raise SweepError(f"invalid unsupported suite call: {call}")


def validate_failure_diagnostic(value: object) -> dict[str, object]:
    if not isinstance(value, dict):
        raise SweepError("admission failure diagnostic is not an object")
    require_exact_keys(
        value,
        {
            "schema_version",
            "stage",
            "execution_backend",
            "category",
            "selector",
            "error_code",
            "message",
            "crash_reason",
            "suite_requests",
            "dropped_suite_requests",
            "unsupported_suite_calls",
            "dropped_unsupported_suite_calls",
        },
        "admission failure diagnostic",
    )
    categories = {
        "callback",
        "capability",
        "crash",
        "dllmain",
        "emulation",
        "image",
        "import",
        "input",
        "mapping",
        "memory",
        "selector",
    }
    message = value.get("message")
    crash_reason = value.get("crash_reason")
    suite_requests = value.get("suite_requests")
    dropped_suite_requests = value.get("dropped_suite_requests")
    dropped_unsupported_suite_calls = value.get("dropped_unsupported_suite_calls")
    if (
        type(value.get("schema_version")) is not int
        or value["schema_version"] != 1
        or value.get("stage") != "admission_probe"
        or not isinstance(value.get("execution_backend"), str)
        or value.get("category") not in categories
        or (
            value.get("selector") is not None
            and not isinstance(value.get("selector"), str)
        )
        or (
            value.get("error_code") is not None
            and type(value.get("error_code")) is not int
        )
        or not isinstance(message, str)
        or len(message.encode("utf-8")) > 1024
        or (
            crash_reason is not None
            and (
                not isinstance(crash_reason, str)
                or len(crash_reason.encode("utf-8")) > 1024
            )
        )
        or not isinstance(suite_requests, list)
        or len(suite_requests) > 64
        or not all(
            isinstance(item, str) and len(item.encode("utf-8")) <= 256
            for item in suite_requests
        )
        or type(dropped_suite_requests) is not int
        or dropped_suite_requests < 0
        or type(dropped_unsupported_suite_calls) is not int
        or dropped_unsupported_suite_calls < 0
    ):
        raise SweepError(f"invalid admission failure diagnostic: {value}")
    validate_unsupported_suite_calls(value["unsupported_suite_calls"])
    return value


def validate_probe(value: dict[str, object], pid: int) -> None:
    common = {"v", "type", "worker_pid", "status", "guards_intact", "render_error"}
    if value.get("status") == "ok":
        require_exact_keys(value, common, "session_probed success")
    elif value.get("status") == "error":
        require_exact_keys(value, common | {"failure"}, "session_probed failure")
    else:
        raise SweepError(f"invalid session_probed status: {value}")
    if (
        value.get("v") != 1
        or value.get("type") != "session_probed"
        or value.get("worker_pid") != pid
    ):
        raise SweepError(f"invalid session_probed envelope: {value}")
    if value["status"] == "error":
        if value.get("guards_intact") is not False or not isinstance(
            value.get("render_error"), int
        ):
            raise SweepError(f"invalid failed session_probed: {value}")
        raise AdmissionFailure(
            validate_failure_diagnostic(value["failure"]), value["render_error"]
        )
    if value.get("guards_intact") is not True or value.get("render_error") != 0:
        raise SweepError(f"invalid successful session_probed: {value}")


def validate_close(value: dict[str, object], pid: int, expected_frames: int) -> None:
    require_exact_keys(value, {"v", "type", "worker_pid", "setup", "close"}, "session_closed")
    close = value.get("close")
    if not isinstance(close, dict):
        raise SweepError("session_closed has no close object")
    require_exact_keys(
        close,
        {
            "schema_version",
            "execution_backend",
            "frames_rendered",
            "frame_setdown_error",
            "sequence_setdown_error",
            "global_setdown_error",
            "suite_requests",
            "unsupported_suite_calls",
            "dropped_unsupported_suite_calls",
            "session_clean",
        },
        "session close report",
    )
    if (
        value.get("v") != 1
        or value.get("type") != "session_closed"
        or value.get("worker_pid") != pid
        or close.get("schema_version") != 1
        or close.get("frames_rendered") != expected_frames
        or close.get("frame_setdown_error") != 0
        or close.get("sequence_setdown_error") != 0
        or close.get("global_setdown_error") != 0
        or close.get("session_clean") is not True
    ):
        raise SweepError(f"resident cleanup was not clean: {value}")


def validate_frame(
    value: dict[str, object], width: int, height: int, expected_checksum: str
) -> None:
    require_exact_keys(
        value,
        {"v", "type", "frame_index", "status", "output", "render_error", "generation"},
        "frame_done",
    )
    output = value.get("output")
    if not isinstance(output, dict):
        raise SweepError(f"frame_done has no output: {value}")
    require_exact_keys(
        output,
        {"width", "height", "rowbytes", "pixel_format", "checksum", "guards_intact"},
        "frame output",
    )
    if (
        value.get("v") != 1
        or value.get("type") != "frame_done"
        or value.get("frame_index") != 0
        or value.get("status") != "ok"
        or value.get("render_error") != 0
        or value.get("generation") != 1
        or output.get("width") != width
        or output.get("height") != height
        or output.get("rowbytes") != width * 4
        or output.get("pixel_format") != "argb8"
        or output.get("checksum") != expected_checksum
        or output.get("guards_intact") is not True
    ):
        raise SweepError(f"resident frame invariants failed: {value}")


def spawn_worker(
    worker: Path,
    plugin: Path,
    input_slot: Path,
    output_slot: Path,
    width: int,
    height: int,
    extra_environment: dict[str, str] | None = None,
) -> subprocess.Popen[bytes]:
    environment = os.environ.copy()
    environment.pop("AEXCOMPAT_NATIVE_RUN_DLLMAIN", None)
    environment.update(extra_environment or {})
    return subprocess.Popen(
        [
            os.fspath(worker),
            "session",
            os.fspath(plugin),
            os.fspath(input_slot),
            os.fspath(output_slot),
            str(width),
            str(height),
            "30",
        ],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
        env=environment,
    )


def signal_worker_group(process: subprocess.Popen[bytes], signal_number: int) -> None:
    try:
        os.killpg(process.pid, signal_number)
    except ProcessLookupError:
        pass


def read_stderr_bounded(process: subprocess.Popen[bytes]) -> str:
    if process.stderr is None:
        return ""
    descriptor = process.stderr.fileno()
    chunks = bytearray()
    while len(chunks) <= MAX_ERROR_BYTES:
        readable, _, _ = select.select([descriptor], [], [], 0)
        if not readable:
            break
        chunk = os.read(descriptor, min(4096, MAX_ERROR_BYTES + 1 - len(chunks)))
        if not chunk:
            break
        chunks.extend(chunk)
    return bytes(chunks[:MAX_ERROR_BYTES]).decode("utf-8", errors="replace")


def process_exit_evidence(
    process: subprocess.Popen[bytes], stderr: str, runner_termination: str | None
) -> str:
    returncode = process.returncode
    if runner_termination is not None:
        status = (
            f"terminated_by_runner={runner_termination}; "
            f"final_returncode={returncode}"
        )
    elif returncode is None:
        status = "exit_status=unknown"
    elif returncode < 0:
        number = -returncode
        try:
            name = signal.Signals(number).name
        except ValueError:
            name = "UNKNOWN"
        status = f"signal={name}({number})"
    else:
        status = f"exit_status={returncode}"
    return f"{status}; stderr={stderr}" if stderr else status


def terminate_worker(process: subprocess.Popen[bytes]) -> str:
    if process.stdin:
        try:
            process.stdin.close()
        except (BrokenPipeError, OSError, ValueError):
            pass
    runner_termination = None
    if process.poll() is None:
        runner_termination = "SIGTERM"
        signal_worker_group(process, signal.SIGTERM)
        try:
            process.wait(timeout=CLOSE_TIMEOUT_SECONDS)
        except subprocess.TimeoutExpired:
            runner_termination = "SIGKILL"
            signal_worker_group(process, signal.SIGKILL)
            try:
                process.wait(timeout=CLOSE_TIMEOUT_SECONDS)
            except subprocess.TimeoutExpired:
                return "worker process group did not exit after SIGKILL"
    return process_exit_evidence(
        process, read_stderr_bounded(process), runner_termination
    )


def close_worker(process: subprocess.Popen[bytes], expected_frames: int) -> dict[str, object]:
    if process.stdin is None or process.stdout is None:
        raise SweepError("worker control pipes are unavailable")
    write_message(process.stdin, {"v": 1, "type": "close"})
    process.stdin.close()
    response = read_message(process.stdout, CLOSE_TIMEOUT_SECONDS)
    validate_close(response, process.pid, expected_frames)
    try:
        returncode = process.wait(timeout=CLOSE_TIMEOUT_SECONDS)
    except subprocess.TimeoutExpired as error:
        cleanup = terminate_worker(process)
        detail = f"; {cleanup}" if cleanup else ""
        raise SweepError(f"worker exceeded close deadline{detail}") from error
    stderr = read_stderr_bounded(process)
    if returncode != 0 or stderr:
        raise SweepError(f"worker close exit={returncode} stderr={stderr}")
    return response


def launch_ready(
    worker: Path,
    plugin: Path,
    input_slot: Path,
    output_slot: Path,
    width: int,
    height: int,
    extra_environment: dict[str, str],
) -> tuple[subprocess.Popen[bytes], dict[str, object]]:
    process = spawn_worker(
        worker,
        plugin,
        input_slot,
        output_slot,
        width,
        height,
        extra_environment,
    )
    try:
        if process.stdout is None:
            raise SweepError("worker stdout is unavailable")
        ready = read_message(process.stdout, START_TIMEOUT_SECONDS)
        validate_ready(ready, process.pid)
        return process, ready
    except Exception as error:
        stderr = terminate_worker(process)
        if stderr:
            raise SweepError(f"{error}; worker stderr: {stderr}") from error
        raise


def run_backend(
    worker: Path,
    expected_backend: str,
    extra_environment: dict[str, str],
    plugin: Path,
    argb8: bytes,
    width: int,
    height: int,
    directory: Path,
) -> dict[str, object]:
    input_slot = directory / "input.argb8"
    output_slot = directory / "output.argb8"
    input_slot.write_bytes(argb8)
    output_slot.write_bytes(bytes(len(argb8)))
    try:
        probe, probe_ready = launch_ready(
            worker,
            plugin,
            input_slot,
            output_slot,
            width,
            height,
            extra_environment,
        )
    except Exception as error:
        raise BackendFailure(error, "admission_setup", False, False) from error
    admission_success = False
    try:
        require_backend(probe_ready, expected_backend, "probe")
        if probe.stdin is None or probe.stdout is None:
            raise SweepError("probe control pipes are unavailable")
        write_message(probe.stdin, {"v": 1, "type": "probe"})
        validate_probe(read_message(probe.stdout, RENDER_TIMEOUT_SECONDS), probe.pid)
        admission_success = True
        close_worker(probe, 0)
    except AdmissionFailure as error:
        error.termination_evidence = terminate_worker(probe)
        raise
    except Exception as error:
        stderr = terminate_worker(probe)
        if stderr:
            error = SweepError(f"{error}; worker stderr: {stderr}")
        stage = "admission_cleanup" if admission_success else "admission_probe"
        raise BackendFailure(error, stage, admission_success, False) from error

    try:
        process, ready = launch_ready(
            worker,
            plugin,
            input_slot,
            output_slot,
            width,
            height,
            extra_environment,
        )
    except Exception as error:
        raise BackendFailure(error, "render_setup", True, False) from error
    render_success = False
    try:
        require_backend(ready, expected_backend, "render")
        if process.stdin is None or process.stdout is None:
            raise SweepError("render control pipes are unavailable")
        write_message(
            process.stdin,
            {
                "v": 2,
                "type": "render_frame",
                "frame_index": 0,
                "current_time": {"value": 0, "scale": 30},
                "parameters": "v2|",
            },
        )
        frame = read_message(process.stdout, RENDER_TIMEOUT_SECONDS)
        output = output_slot.read_bytes()
        if len(output) != len(argb8):
            raise SweepError(f"output slot size differs: {len(output)} != {len(argb8)}")
        checksum = hashlib.sha256(output).hexdigest()
        validate_frame(frame, width, height, checksum)
        render_success = True
        close = close_worker(process, 1)
    except Exception as error:
        stderr = terminate_worker(process)
        if stderr:
            error = SweepError(f"{error}; worker stderr: {stderr}")
        stage = "render_cleanup" if render_success else "render"
        raise BackendFailure(error, stage, True, render_success) from error
    return {
        "status": "rendered",
        "fresh_after_probe": probe_ready["worker_pid"] != ready["worker_pid"],
        "execution_backend": ready["setup"].get("execution_backend"),
        "output_sha256": checksum,
        "suite_requests": close["close"].get("suite_requests", []),
        "unsupported_suite_calls": close["close"].get("unsupported_suite_calls", []),
        "session_clean": True,
        "milestones": {
            "admission_success": True,
            "render_success": True,
            "cleanup_success": True,
        },
    }


def classify_failure(message: str) -> str:
    lowered = message.casefold()
    for needle, bucket in (
        ("timed out", "timeout"),
        ("cleanup", "cleanup"),
        ("signal=", "crash"),
        ("crash_snapshot", "crash"),
        ("cpu exception", "crash"),
        ("missing import", "import"),
        ("unsupported import", "import"),
        ("unsupported win64 import", "import"),
        ("native avx", "emulation"),
        ("dllmain", "DllMain"),
        ("tls", "TLS"),
        ("seh", "SEH"),
        ("thread", "thread"),
        ("suite", "Suite"),
        ("callback", "callback"),
        ("entry", "entrypoint"),
        ("pixel", "pixel-format"),
        ("metal", "platform-service"),
        ("vulkan", "platform-service"),
    ):
        if needle in lowered:
            return bucket
    return "guest-runtime"


def classify_diagnostic(diagnostic: dict[str, object]) -> str:
    if diagnostic["unsupported_suite_calls"]:
        return "Suite"
    return {
        "callback": "callback",
        "capability": "capability",
        "crash": "crash",
        "dllmain": "DllMain",
        "emulation": "emulation",
        "image": "image",
        "import": "import",
        "input": "input",
        "mapping": "mapping",
        "memory": "memory",
        "selector": "selector",
    }[diagnostic["category"]]


def validate_source_pair(
    inventory: dict[str, object],
    windows_summary: dict[str, object],
    inventory_sha: str,
) -> None:
    if windows_summary.get("schema_version") != 1:
        raise SweepError("Windows summary schema_version must be 1")
    corpus = windows_summary.get("corpus")
    entries = inventory.get("entries")
    if not isinstance(corpus, dict) or not isinstance(entries, list):
        raise SweepError("Windows summary/inventory has no corpus entries")
    if (
        corpus.get("inventory_sha256") != inventory_sha
        or corpus.get("canonical_count") != len(entries)
        or corpus.get("processed") != len(entries)
        or corpus.get("remaining") != 0
        or corpus.get("ordered_path_sha_identity_exact") is not True
    ):
        raise SweepError("Windows summary does not bind the supplied inventory")


def requested_backends(args: argparse.Namespace) -> list[str]:
    return args.backend or ["unicorn"]


def resolve_workers(
    args: argparse.Namespace,
) -> dict[str, tuple[Path, str, dict[str, str]]]:
    selected = requested_backends(args)
    worker_arguments = {
        "native": args.native_worker,
        "unicorn": args.unicorn_worker,
    }
    missing = [name for name in selected if worker_arguments[name] is None]
    if missing:
        raise SweepError(
            "worker path is required for selected backend(s): "
            + ", ".join(missing)
        )
    definitions = {
        "native": (
            "native-x86_64-carrier",
            (
                {"AEXCOMPAT_NATIVE_RUN_DLLMAIN": "1"}
                if args.native_run_dllmain
                else {}
            ),
        ),
        "unicorn": ("unicorn-x86_64", {}),
    }
    workers = {}
    for name in selected:
        worker_argument = worker_arguments[name]
        assert worker_argument is not None
        worker = worker_argument.resolve(strict=True)
        if not worker.is_file():
            raise SweepError(f"{name} worker is not a file: {worker}")
        expected_backend, extra_environment = definitions[name]
        workers[name] = (worker, expected_backend, extra_environment)
    return workers


def source_worker_identity(
    workers: dict[str, tuple[Path, str, dict[str, str]]],
    native_run_dllmain: bool,
) -> dict[str, object]:
    identity: dict[str, object] = {}
    if "native" in workers:
        identity["native_worker_sha256"] = sha256_file(workers["native"][0])
        identity["native_run_dllmain"] = native_run_dllmain
    if "unicorn" in workers:
        identity["unicorn_worker_sha256"] = sha256_file(workers["unicorn"][0])
    return identity


def _rate(numerator: int, denominator: int) -> dict[str, int]:
    return {"numerator": numerator, "denominator": denominator}


def summarize_compatibility(
    entries: list[dict[str, object]], backends: list[str]
) -> dict[str, object]:
    denominator = len(entries)
    by_backend: dict[str, object] = {}
    blocker_counts: Counter[tuple[str, str, str | None]] = Counter()
    for backend in backends:
        admission = render = cleanup = 0
        for entry in entries:
            result = entry["backends"][backend]
            milestones = result["milestones"]
            admission += int(milestones["admission_success"] is True)
            render += int(milestones["render_success"] is True)
            cleanup += int(milestones["cleanup_success"] is True)
            if result["status"] == "failed":
                selector = None
                diagnostic = result.get("diagnostic")
                if isinstance(diagnostic, dict):
                    selector = diagnostic.get("selector")
                blocker_counts[(backend, str(result["failure_class"]), selector)] += 1
        by_backend[backend] = {
            "denominator": denominator,
            "admission_success": admission,
            "admission_rate": _rate(admission, denominator),
            "render_success": render,
            "render_rate": _rate(render, denominator),
            "cleanup_success": cleanup,
            "cleanup_rate": _rate(cleanup, denominator),
        }
    blockers = [
        {
            "backend": backend,
            "failure_class": failure_class,
            "selector": selector,
            "count": count,
        }
        for (backend, failure_class, selector), count in sorted(
            blocker_counts.items(),
            key=lambda item: (-item[1], item[0][0], item[0][1], item[0][2] or ""),
        )
    ]
    return {"denominator": denominator, "by_backend": by_backend, "blockers": blockers}


def _entry_identity(report: dict[str, object]) -> list[str]:
    entries = report.get("entries")
    if not isinstance(entries, list):
        raise SweepError("baseline report entries must be an array")
    identities = []
    for entry in entries:
        if not isinstance(entry, dict) or not isinstance(entry.get("sha256"), str):
            raise SweepError("baseline report entry identity is invalid")
        identity = entry["sha256"]
        if len(identity) != 64 or any(
            character not in "0123456789abcdef" for character in identity
        ):
            raise SweepError("baseline report entry identity is invalid")
        identities.append(identity)
    if len(identities) != len(set(identities)):
        raise SweepError("baseline report contains duplicate identities")
    return identities


def _validated_milestones(
    entry: object, backend: str, label: str
) -> tuple[dict[str, object], dict[str, bool]]:
    if not isinstance(entry, dict) or not isinstance(entry.get("backends"), dict):
        raise SweepError(f"{label} report backend result is invalid")
    result = entry["backends"].get(backend)
    if not isinstance(result, dict) or result.get("status") not in {"rendered", "failed"}:
        raise SweepError(f"{label} report {backend} result is invalid")
    milestones = result.get("milestones")
    expected = {"admission_success", "render_success", "cleanup_success"}
    if (
        not isinstance(milestones, dict)
        or set(milestones) != expected
        or any(type(milestones[key]) is not bool for key in expected)
        or (milestones["render_success"] and not milestones["admission_success"])
        or (milestones["cleanup_success"] and not milestones["render_success"])
        or (
            result["status"] == "rendered"
            and not all(milestones[key] for key in expected)
        )
    ):
        raise SweepError(f"{label} report {backend} milestones are invalid")
    checksum = result.get("output_sha256")
    if result["status"] == "rendered" and (
        not isinstance(checksum, str)
        or len(checksum) != 64
        or any(character not in "0123456789abcdef" for character in checksum)
    ):
        raise SweepError(f"{label} report {backend} output SHA-256 is invalid")
    return result, milestones


def compare_baseline(
    report: dict[str, object], baseline: dict[str, object]
) -> dict[str, object]:
    if baseline.get("schema_version") != SCHEMA_VERSION:
        raise SweepError(f"baseline report schema_version must be {SCHEMA_VERSION}")
    source = report["source"]
    baseline_source = baseline.get("source")
    if not isinstance(baseline_source, dict):
        raise SweepError("baseline report source is invalid")
    condition_keys = {
        "windows_inventory_sha256",
        "windows_summary_sha256",
        "input_png_sha256",
        "input_dimensions",
        "backends",
        "native_run_dllmain",
    }
    conditions = {key: source.get(key) for key in condition_keys if key in source}
    baseline_conditions = {
        key: baseline_source.get(key)
        for key in condition_keys
        if key in baseline_source
    }
    if conditions != baseline_conditions:
        raise SweepError("baseline report execution conditions differ")
    backends = source["backends"]
    if (
        not isinstance(backends, list)
        or not backends
        or len(backends) != len(set(backends))
        or not all(backend in {"unicorn", "native"} for backend in backends)
    ):
        raise SweepError("current report backend list is invalid")
    for backend in backends:
        worker_key = f"{backend}_worker_sha256"
        for label, candidate in (
            ("current", source.get(worker_key)),
            ("baseline", baseline_source.get(worker_key)),
        ):
            if (
                not isinstance(candidate, str)
                or len(candidate) != 64
                or any(character not in "0123456789abcdef" for character in candidate)
            ):
                raise SweepError(f"{label} report {worker_key} is invalid")
    identities = _entry_identity(report)
    if identities != _entry_identity(baseline):
        raise SweepError("baseline report mapped identity order differs")

    comparison: dict[str, object] = {
        "regression": False,
        "baseline_workers": {
            key: baseline_source[key]
            for key in ("unicorn_worker_sha256", "native_worker_sha256")
            if key in baseline_source
        },
        "current_workers": {
            key: source[key]
            for key in ("unicorn_worker_sha256", "native_worker_sha256")
            if key in source
        },
        "by_backend": {},
    }
    baseline_entries = baseline["entries"]
    for backend in backends:
        gained = []
        lost = []
        cleanup_lost = []
        output_changed = []
        for identity, current_entry, baseline_entry in zip(
            identities, report["entries"], baseline_entries, strict=True
        ):
            current, current_milestones = _validated_milestones(
                current_entry, backend, "current"
            )
            previous, previous_milestones = _validated_milestones(
                baseline_entry, backend, "baseline"
            )
            if current_milestones["render_success"] and not previous_milestones["render_success"]:
                gained.append(identity)
            if previous_milestones["render_success"] and not current_milestones["render_success"]:
                lost.append(identity)
            if previous_milestones["cleanup_success"] and not current_milestones["cleanup_success"]:
                cleanup_lost.append(identity)
            if (
                current_milestones["render_success"]
                and previous_milestones["render_success"]
                and current.get("output_sha256") != previous.get("output_sha256")
            ):
                output_changed.append(identity)
        backend_regression = bool(lost or cleanup_lost or output_changed)
        comparison["regression"] = comparison["regression"] or backend_regression
        comparison["by_backend"][backend] = {
            "gained_render": gained,
            "lost_render": lost,
            "lost_cleanup": cleanup_lost,
            "output_changed": output_changed,
            "regression": backend_regression,
        }
    return comparison


def run_sweep(args: argparse.Namespace) -> dict[str, object]:
    inventory_path = args.inventory.resolve(strict=True)
    summary_path = args.windows_summary.resolve(strict=True)
    inventory_sha = sha256_file(inventory_path)
    summary_sha = sha256_file(summary_path)
    if inventory_sha != args.expected_inventory_sha256:
        raise SweepError("Windows inventory SHA-256 differs from the expected identity")
    if summary_sha != args.expected_summary_sha256:
        raise SweepError("Windows summary SHA-256 differs from the expected identity")
    inventory = load_json_strict(inventory_path)
    windows_summary = load_json_strict(summary_path)
    validate_source_pair(inventory, windows_summary, inventory_sha)
    corpus_roots = [root.resolve(strict=True) for root in args.corpus_root]
    input_png = args.input_png.resolve(strict=True)
    mapped = map_corpus(inventory, corpus_roots)
    width, height, argb8 = png_to_argb8(input_png)
    workers = resolve_workers(args)
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    run_root = output.parent / f"{output.stem}-runs"
    run_root.mkdir(parents=True, exist_ok=True)
    redactions = {
        os.fspath(inventory_path): "<inventory>",
        os.fspath(summary_path): "<windows-summary>",
        os.fspath(input_png): "<input-png>",
        os.fspath(output): "<output>",
        os.fspath(run_root): "<run-root>",
        **{
            os.fspath(root): f"<corpus-root:{index}>"
            for index, root in enumerate(corpus_roots)
        },
        **{
            os.fspath(worker): f"<{name}-worker>"
            for name, (worker, _, _) in workers.items()
        },
    }
    entries = []
    counts: Counter[str] = Counter()
    for index, item in enumerate(mapped):
        backend_results = {}
        for backend, (worker, expected_backend, extra_environment) in workers.items():
            directory = run_root / f"{index:04d}-{item['sha256'][:12]}-{backend}"
            directory.mkdir(parents=True, exist_ok=True)
            started = time.monotonic()
            try:
                result = run_backend(
                    worker,
                    expected_backend,
                    extra_environment,
                    item["path"],
                    argb8,
                    width,
                    height,
                    directory,
                )
                result["elapsed_ms"] = round((time.monotonic() - started) * 1000, 3)
                counts[f"{backend}:rendered"] += 1
            except AdmissionFailure as error:
                diagnostic = sanitize_diagnostic(error.diagnostic, redactions)
                result = {
                    "status": "failed",
                    "failure_class": classify_diagnostic(diagnostic),
                    "failure_stage": "admission_probe",
                    "render_error": error.render_error,
                    "diagnostic": diagnostic,
                    "termination_evidence": sanitize_error_text(
                        error.termination_evidence, redactions
                    ),
                    "elapsed_ms": round((time.monotonic() - started) * 1000, 3),
                    "milestones": {
                        "admission_success": False,
                        "render_success": False,
                        "cleanup_success": False,
                    },
                }
                counts[f"{backend}:{result['failure_class']}"] += 1
            except BackendFailure as error:
                message = sanitize_error_text(str(error), redactions)
                bucket = classify_failure(message)
                result = {
                    "status": "failed",
                    "failure_class": bucket,
                    "failure_stage": error.failure_stage,
                    "error": message,
                    "elapsed_ms": round((time.monotonic() - started) * 1000, 3),
                    "milestones": {
                        "admission_success": error.admission_success,
                        "render_success": error.render_success,
                        "cleanup_success": False,
                    },
                }
                counts[f"{backend}:{bucket}"] += 1
            except Exception as error:
                message = sanitize_error_text(str(error), redactions)
                bucket = classify_failure(message)
                result = {
                    "status": "failed",
                    "failure_class": bucket,
                    "failure_stage": "runner",
                    "error": message,
                    "elapsed_ms": round((time.monotonic() - started) * 1000, 3),
                    "milestones": {
                        "admission_success": False,
                        "render_success": False,
                        "cleanup_success": False,
                    },
                }
                counts[f"{backend}:{bucket}"] += 1
            backend_results[backend] = result
        entries.append(
            {
                key: value
                for key, value in item.items()
                if key != "path"
            }
            | {"backends": backend_results}
        )
    report = {
        "schema_version": SCHEMA_VERSION,
        "mode": "macos_x64_guest_inventory_sweep",
        "source": {
            "windows_inventory_sha256": inventory_sha,
            "windows_summary_sha256": summary_sha,
            "windows_inventory_entries": len(inventory["entries"]),
            "mapped_entries": len(mapped),
            "input_png_sha256": sha256_file(input_png),
            "input_dimensions": [width, height],
            "backends": list(workers),
        }
        | source_worker_identity(workers, args.native_run_dllmain),
        "summary": {
            "entry_count": len(entries),
            "backend_attempts": len(entries) * len(workers),
            "counts": dict(sorted(counts.items())),
            "compatibility": summarize_compatibility(entries, list(workers)),
        },
        "entries": entries,
    }
    if args.baseline_report is not None:
        report["baseline_comparison"] = compare_baseline(
            report, load_json_strict(args.baseline_report.resolve(strict=True))
        )
    output.write_text(
        json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    if report.get("baseline_comparison", {}).get("regression") is True:
        raise SweepError("baseline regression detected; report was retained")
    return report


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--inventory", type=Path, required=True)
    parser.add_argument("--windows-summary", type=Path, required=True)
    parser.add_argument("--corpus-root", type=Path, action="append", required=True)
    parser.add_argument("--input-png", type=Path, required=True)
    parser.add_argument("--native-worker", type=Path)
    parser.add_argument("--unicorn-worker", type=Path)
    parser.add_argument(
        "--backend",
        action="append",
        choices=("native", "unicorn"),
        help="backend to run; repeat for both (default: unicorn)",
    )
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--baseline-report",
        type=Path,
        help="schema-v2 report from an identical corpus and execution condition",
    )
    parser.add_argument("--expected-inventory-sha256", required=True)
    parser.add_argument("--expected-summary-sha256", required=True)
    parser.add_argument(
        "--native-run-dllmain",
        action="store_true",
        help="explicitly opt the isolated native carrier into DLL_PROCESS_ATTACH",
    )
    args = parser.parse_args(argv)
    selected = requested_backends(args)
    if len(selected) != len(set(selected)):
        parser.error("--backend values must not be duplicated")
    for backend in selected:
        if getattr(args, f"{backend}_worker") is None:
            parser.error(f"--{backend}-worker is required for {backend} backend")
    return args


def main() -> int:
    try:
        report = run_sweep(parse_args())
    except SweepError as error:
        print(f"macos_aex_sweep_error: {error}", file=os.sys.stderr)
        return 1
    print(json.dumps(report["summary"], ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

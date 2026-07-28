#!/usr/bin/env python3
"""Run a small, SHA-pinned AEX set through render-trace-png.

The input manifest is intentionally local: it names files to execute.  The
emitted report is portable and contains identities rather than those paths.
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import re
import signal
import stat
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path
from typing import Any

from PIL import Image, UnidentifiedImageError


SCHEMA_VERSION = 1
MAX_CASES = 8
MAX_CAPTURE_BYTES = 8 * 1024 * 1024
MAX_COMBINED_CAPTURE_BYTES = 8 * 1024 * 1024
MAX_ERROR_BYTES = 4096
MAX_OUTPUT_PNG_BYTES = 64 * 1024 * 1024
CASE_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.-]{0,63}$")
SHA256 = re.compile(r"^[0-9a-fA-F]{64}$")
PIXEL_FORMATS = {"argb8", "argb16", "argb32f"}


class TraceRunnerError(RuntimeError):
    pass


def _terminate_process_group(process: subprocess.Popen[bytes]) -> None:
    """Best-effort termination of the worker and descendants it spawned."""
    if os.name == "posix":
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            return
        except OSError:
            if process.poll() is None:
                process.terminate()
        try:
            process.wait(timeout=0.5)
        except subprocess.TimeoutExpired:
            pass
        # The leader may have exited while a descendant ignored SIGTERM.
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        except OSError:
            if process.poll() is None:
                process.kill()
        if process.poll() is None:
            process.wait()
        return
    elif os.name == "nt":
        if process.poll() is not None:
            return
        # CREATE_NEW_PROCESS_GROUP makes CTRL_BREAK address the group.  taskkill
        # is the reliable fallback for descendants which do not handle it.
        try:
            process.send_signal(signal.CTRL_BREAK_EVENT)
        except (OSError, ValueError):
            pass
    else:
        if process.poll() is not None:
            return
        process.terminate()
    try:
        process.wait(timeout=0.5)
        return
    except subprocess.TimeoutExpired:
        pass
    if os.name == "posix":
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            return
        except OSError:
            process.kill()
    elif os.name == "nt":
        try:
            subprocess.Popen(
                ["taskkill", "/PID", str(process.pid), "/T", "/F"],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            ).wait(timeout=2)
        except (OSError, subprocess.TimeoutExpired):
            process.kill()
    else:
        process.kill()
    try:
        process.wait(timeout=2)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()


def _run_bounded_process(
    command: list[str], timeout_seconds: float
) -> tuple[subprocess.CompletedProcess[bytes], bool, str | None]:
    """Drain both pipes concurrently while retaining at most the declared bounds."""
    popen_options: dict[str, Any] = {
        "stdin": subprocess.DEVNULL,
        "stdout": subprocess.PIPE,
        "stderr": subprocess.PIPE,
    }
    if os.name == "posix":
        popen_options["start_new_session"] = True
    elif os.name == "nt":
        popen_options["creationflags"] = subprocess.CREATE_NEW_PROCESS_GROUP
    process = subprocess.Popen(command, **popen_options)
    assert process.stdout is not None and process.stderr is not None

    captures = {"stdout": bytearray(), "stderr": bytearray()}
    total = 0
    lock = threading.Lock()
    overflow = threading.Event()
    stream_errors: list[BaseException] = []

    def drain(name: str, stream: Any) -> None:
        nonlocal total
        try:
            while True:
                chunk = stream.read(64 * 1024)
                if not chunk:
                    return
                with lock:
                    per_stream_remaining = MAX_CAPTURE_BYTES - len(captures[name])
                    combined_remaining = MAX_COMBINED_CAPTURE_BYTES - total
                    accepted = min(len(chunk), per_stream_remaining, combined_remaining)
                    if accepted > 0:
                        captures[name].extend(chunk[:accepted])
                        total += accepted
                    if accepted != len(chunk):
                        overflow.set()
                        return
        except (OSError, ValueError) as error:
            # Closing pipes during forced cleanup is expected.
            if process.poll() is None:
                stream_errors.append(error)

    threads = [
        threading.Thread(target=drain, args=("stdout", process.stdout), daemon=True),
        threading.Thread(target=drain, args=("stderr", process.stderr), daemon=True),
    ]
    for thread in threads:
        thread.start()

    deadline = time.monotonic() + timeout_seconds
    timed_out = False
    while process.poll() is None:
        if overflow.is_set():
            _terminate_process_group(process)
            break
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            timed_out = True
            _terminate_process_group(process)
            break
        try:
            process.wait(timeout=min(remaining, 0.05))
        except subprocess.TimeoutExpired:
            pass

    for stream in (process.stdout, process.stderr):
        if overflow.is_set() or timed_out:
            try:
                stream.close()
            except OSError:
                pass
    for thread in threads:
        thread.join(timeout=2)
    if any(thread.is_alive() for thread in threads):
        # A descendant may have inherited a pipe after the direct worker exited.
        _terminate_process_group(process)
        for stream in (process.stdout, process.stderr):
            try:
                stream.close()
            except OSError:
                pass
        for thread in threads:
            thread.join(timeout=2)
        if any(thread.is_alive() for thread in threads):
            raise TraceRunnerError("worker capture threads did not stop")
    if stream_errors:
        raise TraceRunnerError(f"worker pipe capture failed: {stream_errors[0]}")
    completed = subprocess.CompletedProcess(
        command,
        process.returncode,
        bytes(captures["stdout"]),
        bytes(captures["stderr"]),
    )
    reason = (
        "timeout"
        if timed_out
        else ("capture_limit" if overflow.is_set() else None)
    )
    return completed, timed_out, reason


def _object_without_duplicate_keys(
    pairs: list[tuple[str, Any]],
) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise TraceRunnerError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def strict_json_bytes(payload: bytes, label: str) -> dict[str, Any]:
    if len(payload) > MAX_CAPTURE_BYTES:
        raise TraceRunnerError(f"{label} exceeds {MAX_CAPTURE_BYTES} bytes")
    try:
        value = json.loads(
            payload.decode("utf-8"),
            object_pairs_hook=_object_without_duplicate_keys,
        )
    except (UnicodeError, json.JSONDecodeError) as error:
        raise TraceRunnerError(f"{label} is not strict UTF-8 JSON: {error}") from error
    if not isinstance(value, dict):
        raise TraceRunnerError(f"{label} JSON root must be an object")
    return value


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def pin_output_png(path: Path, case_id: str) -> str:
    """Validate and hash the worker artifact without accepting links/non-files."""
    try:
        metadata = path.lstat()
    except OSError as error:
        raise TraceRunnerError(
            f"worker output for {case_id} is missing or unreadable"
        ) from error
    if (
        not stat.S_ISREG(metadata.st_mode)
        or metadata.st_size <= 0
        or metadata.st_size > MAX_OUTPUT_PNG_BYTES
    ):
        raise TraceRunnerError(
            f"worker output for {case_id} is not a bounded regular PNG file"
        )
    try:
        payload = path.read_bytes()
        with Image.open(io.BytesIO(payload)) as image:
            if image.format != "PNG" or image.width <= 0 or image.height <= 0:
                raise TraceRunnerError(
                    f"worker output for {case_id} is not a valid PNG"
                )
            image.verify()
    except TraceRunnerError:
        raise
    except (OSError, UnidentifiedImageError, ValueError) as error:
        raise TraceRunnerError(
            f"worker output for {case_id} is not a valid readable PNG"
        ) from error
    return hashlib.sha256(payload).hexdigest()


def _require_file(value: Any, label: str) -> Path:
    if not isinstance(value, str) or not value:
        raise TraceRunnerError(f"{label} must be a nonempty path string")
    try:
        path = Path(value).expanduser().resolve(strict=True)
    except OSError as error:
        raise TraceRunnerError(f"{label} cannot be resolved") from error
    if not path.is_file():
        raise TraceRunnerError(f"{label} must be a file")
    return path


def _require_sha(value: Any, label: str) -> str:
    if not isinstance(value, str) or SHA256.fullmatch(value) is None:
        raise TraceRunnerError(f"{label} must be a SHA-256")
    return value.lower()


def _pin_file(value: Any, label: str) -> tuple[Path, str]:
    if not isinstance(value, dict) or set(value) != {"path", "sha256"}:
        raise TraceRunnerError(f"{label} must contain exactly path and sha256")
    path = _require_file(value["path"], f"{label}.path")
    expected = _require_sha(value["sha256"], f"{label}.sha256")
    actual = sha256_file(path)
    if actual != expected:
        raise TraceRunnerError(f"{label} SHA-256 mismatch")
    return path, actual


def load_manifest(path: Path) -> list[dict[str, Any]]:
    manifest = strict_json_bytes(path.read_bytes(), "manifest")
    if set(manifest) != {"schema_version", "cases"}:
        raise TraceRunnerError("manifest keys must be schema_version and cases")
    if manifest["schema_version"] != SCHEMA_VERSION:
        raise TraceRunnerError("manifest schema_version must be 1")
    cases = manifest["cases"]
    if not isinstance(cases, list) or not 1 <= len(cases) <= MAX_CASES:
        raise TraceRunnerError(f"manifest cases must contain 1..{MAX_CASES} entries")
    normalized = []
    seen_ids: set[str] = set()
    for index, case in enumerate(cases):
        label = f"cases[{index}]"
        if not isinstance(case, dict) or set(case) != {
            "id",
            "plugin",
            "input_png",
            "pixel_format",
            "parameters",
        }:
            raise TraceRunnerError(f"{label} has unexpected keys")
        case_id = case["id"]
        if not isinstance(case_id, str) or CASE_ID.fullmatch(case_id) is None:
            raise TraceRunnerError(f"{label}.id is invalid")
        if case_id in seen_ids:
            raise TraceRunnerError(f"duplicate case id: {case_id}")
        seen_ids.add(case_id)
        pixel_format = case["pixel_format"]
        if pixel_format not in PIXEL_FORMATS:
            raise TraceRunnerError(f"{label}.pixel_format is unsupported")
        parameters = case["parameters"]
        if (
            not isinstance(parameters, list)
            or len(parameters) > 64
            or any(
                not isinstance(value, str)
                or not value
                or len(value.encode("utf-8")) > 256
                or "=" not in value
                or value.startswith("-")
                or "/" in value
                or "\\" in value
                for value in parameters
            )
        ):
            raise TraceRunnerError(f"{label}.parameters is invalid")
        plugin, plugin_sha = _pin_file(case["plugin"], f"{label}.plugin")
        input_png, input_sha = _pin_file(case["input_png"], f"{label}.input_png")
        normalized.append(
            {
                "id": case_id,
                "plugin": plugin,
                "plugin_sha256": plugin_sha,
                "input_png": input_png,
                "input_png_sha256": input_sha,
                "pixel_format": pixel_format,
                "parameters": list(parameters),
            }
        )
    return normalized


def reject_output_alias(
    output: Path,
    protected: list[tuple[str, Path]],
) -> None:
    """Reject path and inode aliases before any worker or report write."""
    protected_identities: dict[tuple[int, int], str] = {}
    for label, path in protected:
        metadata = path.stat()
        protected_identities[(metadata.st_dev, metadata.st_ino)] = label
        if output == path:
            raise TraceRunnerError(f"output aliases protected {label}")
    try:
        output_metadata = output.stat()
    except FileNotFoundError:
        return
    except OSError as error:
        raise TraceRunnerError("output identity cannot be inspected safely") from error
    label = protected_identities.get(
        (output_metadata.st_dev, output_metadata.st_ino)
    )
    if label is not None:
        raise TraceRunnerError(f"output aliases protected {label}")


def write_report_atomic(output: Path, report: dict[str, Any]) -> None:
    """Replace the output directory entry without following a late symlink."""
    payload = (
        json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    )
    temporary: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            dir=output.parent,
            prefix=f".{output.name}.",
            suffix=".tmp",
            delete=False,
        ) as stream:
            temporary = Path(stream.name)
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, output)
        temporary = None
    finally:
        if temporary is not None:
            try:
                temporary.unlink()
            except FileNotFoundError:
                pass


def _redact_text(value: str, replacements: dict[str, str]) -> str:
    for source, token in sorted(
        replacements.items(), key=lambda item: len(item[0]), reverse=True
    ):
        if source:
            value = value.replace(source, token)
    # Do not let an unanticipated absolute POSIX path become public evidence.
    value = re.sub(r"(?<![\w.-])/(?:[^/\s:;,]+/)*[^/\s:;,]+", "<absolute-path>", value)
    encoded = value.encode("utf-8")
    if len(encoded) > MAX_ERROR_BYTES:
        value = encoded[:MAX_ERROR_BYTES].decode("utf-8", errors="ignore")
    return value


def _sanitize(value: Any, replacements: dict[str, str]) -> Any:
    if isinstance(value, str):
        return _redact_text(value, replacements)
    if isinstance(value, list):
        return [_sanitize(item, replacements) for item in value]
    if isinstance(value, dict):
        return {
            key: _sanitize(item, replacements)
            for key, item in value.items()
        }
    return value


def _parse_failure(
    completed: subprocess.CompletedProcess[bytes],
    replacements: dict[str, str],
) -> dict[str, Any]:
    stderr = completed.stderr[:MAX_CAPTURE_BYTES].decode("utf-8", errors="replace")
    marker = "crash_snapshot="
    snapshot = None
    message = stderr
    if marker in stderr:
        prefix, encoded = stderr.split(marker, 1)
        message = prefix.rstrip()
        try:
            snapshot, _ = json.JSONDecoder(
                object_pairs_hook=_object_without_duplicate_keys
            ).raw_decode(encoded.lstrip())
        except (json.JSONDecodeError, TraceRunnerError):
            snapshot = None
    result: dict[str, Any] = {
        "kind": "worker_error",
        "exit_code": completed.returncode,
        "message": _redact_text(message, replacements),
    }
    if completed.stdout:
        try:
            partial_report = strict_json_bytes(
                completed.stdout, "worker partial failure report"
            )
        except TraceRunnerError:
            partial_report = None
        if partial_report is not None:
            result["partial_report"] = _sanitize(partial_report, replacements)
    if isinstance(snapshot, dict):
        result["crash_snapshot"] = _sanitize(snapshot, replacements)
    return result


def run_case(
    worker: Path,
    case: dict[str, Any],
    run_root: Path,
    timeout_seconds: float,
) -> dict[str, Any]:
    output = run_root / f"{case['id']}.png"
    command = [
        os.fspath(worker),
        "render-trace-png",
        os.fspath(case["plugin"]),
        os.fspath(case["input_png"]),
        os.fspath(output),
        "--pixel-format",
        case["pixel_format"],
        *case["parameters"],
    ]
    replacements = {
        os.fspath(worker): "<worker>",
        os.fspath(case["plugin"]): "<plugin>",
        os.fspath(case["input_png"]): "<input-png>",
        os.fspath(output): "<output-png>",
        os.fspath(run_root): "<run-root>",
        os.fspath(Path.home()): "<home>",
    }
    completed, timed_out, stop_reason = _run_bounded_process(
        command, timeout_seconds
    )
    if timed_out:
        return {"kind": "timeout"}
    if stop_reason == "capture_limit":
        raise TraceRunnerError(
            f"worker output for {case['id']} exceeds bounded capture limit"
        )
    if completed.returncode != 0:
        return _parse_failure(completed, replacements)
    report = strict_json_bytes(completed.stdout, f"worker output for {case['id']}")
    traces = report.get("execution_traces")
    if not isinstance(traces, list) or not traces:
        raise TraceRunnerError(
            f"worker output for {case['id']} has no execution_traces"
        )
    return {
        "kind": "trace",
        "report": _sanitize(report, replacements),
        "output_png_sha256": pin_output_png(output, case["id"]),
    }


def run(args: argparse.Namespace) -> dict[str, Any]:
    manifest_path = args.manifest.resolve(strict=True)
    worker, worker_sha = _pin_file(
        {"path": os.fspath(args.worker), "sha256": args.expected_worker_sha256},
        "worker",
    )
    cases = load_manifest(manifest_path)
    output = args.output.resolve()
    protected = [("manifest", manifest_path), ("worker", worker)]
    for case in cases:
        protected.extend(
            [
                (f"plugin for case {case['id']}", case["plugin"]),
                (f"input PNG for case {case['id']}", case["input_png"]),
            ]
        )
    reject_output_alias(output, protected)
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(
        prefix="aex-targeted-trace-", dir=args.run_parent
    ) as temporary:
        run_root = Path(temporary)
        results = [
            {
                "id": case["id"],
                "plugin_sha256": case["plugin_sha256"],
                "input_png_sha256": case["input_png_sha256"],
                "pixel_format": case["pixel_format"],
                "parameters": case["parameters"],
                "result": run_case(worker, case, run_root, args.timeout),
            }
            for case in cases
        ]
    report = {
        "schema_version": SCHEMA_VERSION,
        "mode": "targeted_macos_x64_runtime_provenance",
        "worker_sha256": worker_sha,
        "case_count": len(results),
        "cases": results,
    }
    write_report_atomic(output, report)
    return report


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--worker", type=Path, required=True)
    parser.add_argument("--expected-worker-sha256", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timeout", type=float, default=60.0)
    parser.add_argument("--run-parent", type=Path)
    args = parser.parse_args(argv)
    if not 0 < args.timeout <= 300:
        parser.error("--timeout must be in (0, 300]")
    return args


def main() -> int:
    try:
        report = run(parse_args())
    except (OSError, TraceRunnerError) as error:
        print(f"targeted_aex_trace_error: {error}", file=sys.stderr)
        return 1
    print(
        json.dumps(
            {
                "case_count": report["case_count"],
                "result_kinds": [
                    case["result"]["kind"] for case in report["cases"]
                ],
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

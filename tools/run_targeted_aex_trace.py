#!/usr/bin/env python3
"""Run a small, SHA-pinned AEX set through render-trace-png.

The input manifest is intentionally local: it names files to execute.  The
emitted report is portable and contains identities rather than those paths.
Both successful traces and partial failure reports replace known paths with
semantic tokens; other absolute path values (Windows/POSIX, even outside home)
use <absolute-path>, including filename punctuation. Free-form message/reason
text consumes unknown paths through spaces and ordinary punctuation until a
newline, ': ' diagnostic suffix, or '; reason=' suffix; enclosing quotes delimit
quoted paths. This is a conservative diagnostic grammar, not universal natural-
language path parsing. The diagnostic byte bound applies after redaction.
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import re
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path, PurePosixPath, PureWindowsPath
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


class _WindowsCaptureJob:
    """Own a non-inheritable Job before allowing the worker to execute.

    Popen closes CreateProcess's primary-thread handle. Enumerating the threads
    of our still-suspended process recovers that one thread; this snapshot is
    never used to discover or terminate descendants. Job inheritance owns those.
    """

    def __init__(self) -> None:
        import ctypes
        from ctypes import wintypes

        self._ctypes = ctypes
        self._wintypes = wintypes
        self._kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        signatures = {
            "CreateJobObjectW": ([wintypes.LPVOID, wintypes.LPCWSTR], wintypes.HANDLE),
            "SetInformationJobObject": (
                [wintypes.HANDLE, ctypes.c_int, wintypes.LPVOID, wintypes.DWORD], wintypes.BOOL
            ),
            "AssignProcessToJobObject": ([wintypes.HANDLE, wintypes.HANDLE], wintypes.BOOL),
            "CloseHandle": ([wintypes.HANDLE], wintypes.BOOL),
            "CreateToolhelp32Snapshot": ([wintypes.DWORD, wintypes.DWORD], wintypes.HANDLE),
            "OpenThread": ([wintypes.DWORD, wintypes.BOOL, wintypes.DWORD], wintypes.HANDLE),
            "GetProcessIdOfThread": ([wintypes.HANDLE], wintypes.DWORD),
            "ResumeThread": ([wintypes.HANDLE], wintypes.DWORD),
        }
        for name, (arguments, result) in signatures.items():
            function = getattr(self._kernel32, name)
            function.argtypes = arguments
            function.restype = result

        class BasicLimits(ctypes.Structure):
            _fields_ = [
                ("PerProcessUserTimeLimit", ctypes.c_int64),
                ("PerJobUserTimeLimit", ctypes.c_int64),
                ("LimitFlags", wintypes.DWORD),
                ("MinimumWorkingSetSize", ctypes.c_size_t),
                ("MaximumWorkingSetSize", ctypes.c_size_t),
                ("ActiveProcessLimit", wintypes.DWORD),
                ("Affinity", ctypes.c_size_t),
                ("PriorityClass", wintypes.DWORD),
                ("SchedulingClass", wintypes.DWORD),
            ]

        class ExtendedLimits(ctypes.Structure):
            _fields_ = [
                ("BasicLimitInformation", BasicLimits),
                ("IoInfo", ctypes.c_uint64 * 6),
                ("ProcessMemoryLimit", ctypes.c_size_t),
                ("JobMemoryLimit", ctypes.c_size_t),
                ("PeakProcessMemoryUsed", ctypes.c_size_t),
                ("PeakJobMemoryUsed", ctypes.c_size_t),
            ]

        # NULL security attributes produce a non-inheritable unnamed handle.
        self._job = self._kernel32.CreateJobObjectW(None, None)
        if not self._job:
            raise ctypes.WinError(ctypes.get_last_error())
        limits = ExtendedLimits()
        limits.BasicLimitInformation.LimitFlags = 0x2000  # KILL_ON_JOB_CLOSE
        if not self._kernel32.SetInformationJobObject(
            self._job, 9, ctypes.byref(limits), ctypes.sizeof(limits)
        ):
            error = ctypes.get_last_error()
            self.close()
            raise ctypes.WinError(error)

    def close(self) -> None:
        if self._job:
            if not self._kernel32.CloseHandle(self._job):
                raise self._ctypes.WinError(self._ctypes.get_last_error())
            self._job = None

    def assign_and_resume(self, process: subprocess.Popen[bytes]) -> None:
        # Popen retains the process handle, so the PID cannot be reused here.
        if not self._kernel32.AssignProcessToJobObject(self._job, int(process._handle)):
            raise self._ctypes.WinError(self._ctypes.get_last_error())
        self._resume_primary_thread(process)

    def _resume_primary_thread(self, process: subprocess.Popen[bytes]) -> None:
        ctypes, wintypes, kernel32 = self._ctypes, self._wintypes, self._kernel32

        class ThreadEntry(ctypes.Structure):
            _fields_ = [
                ("dwSize", wintypes.DWORD),
                ("cntUsage", wintypes.DWORD),
                ("th32ThreadID", wintypes.DWORD),
                ("th32OwnerProcessID", wintypes.DWORD),
                ("tpBasePri", wintypes.LONG),
                ("tpDeltaPri", wintypes.LONG),
                ("dwFlags", wintypes.DWORD),
            ]

        for name in ("Thread32First", "Thread32Next"):
            function = getattr(kernel32, name)
            function.argtypes = [wintypes.HANDLE, ctypes.POINTER(ThreadEntry)]
            function.restype = wintypes.BOOL
        snapshot = kernel32.CreateToolhelp32Snapshot(0x4, 0)  # TH32CS_SNAPTHREAD only
        if snapshot == ctypes.c_void_p(-1).value:
            raise ctypes.WinError(ctypes.get_last_error())
        try:
            entry = ThreadEntry()
            entry.dwSize = ctypes.sizeof(entry)
            found = kernel32.Thread32First(snapshot, ctypes.byref(entry))
            thread_ids = []
            while found:
                if entry.th32OwnerProcessID == process.pid:
                    thread_ids.append(entry.th32ThreadID)
                entry.dwSize = ctypes.sizeof(entry)
                found = kernel32.Thread32Next(snapshot, ctypes.byref(entry))
            if ctypes.get_last_error() != 18:  # ERROR_NO_MORE_FILES
                raise ctypes.WinError(ctypes.get_last_error())
        finally:
            kernel32.CloseHandle(snapshot)
        if len(thread_ids) != 1:
            raise TraceRunnerError("suspended worker did not have one primary thread")
        # QUERY_LIMITED_INFORMATION lets us verify ownership after opening the TID.
        thread = kernel32.OpenThread(0x2 | 0x800, False, thread_ids[0])
        if not thread:
            raise ctypes.WinError(ctypes.get_last_error())
        try:
            if kernel32.GetProcessIdOfThread(thread) != process.pid or process.poll() is not None:
                raise TraceRunnerError("suspended worker thread ownership changed")
            previous_count = kernel32.ResumeThread(thread)
            if previous_count == 0xFFFFFFFF:
                raise ctypes.WinError(ctypes.get_last_error())
            if previous_count != 1:
                raise TraceRunnerError("worker primary thread was not suspended exactly once")
        finally:
            kernel32.CloseHandle(thread)


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
        popen_options["creationflags"] = subprocess.CREATE_NEW_PROCESS_GROUP | 0x4  # CREATE_SUSPENDED
    job = _WindowsCaptureJob() if os.name == "nt" else None
    process = None
    threads = []
    finished = False

    def stop_worker() -> None:
        assert process is not None
        if job is not None:
            job.close()
            process.wait(timeout=2)
        else:
            _terminate_process_group(process)

    try:
        process = subprocess.Popen(command, **popen_options)
        if job is not None:
            job.assign_and_resume(process)
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
                stop_worker()
                break
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                timed_out = True
                stop_worker()
                break
            try:
                process.wait(timeout=min(remaining, 0.05))
            except subprocess.TimeoutExpired:
                pass

        # Close the Job even after a normal leader exit, before any blocking pipe
        # close or join. Descendants can otherwise outlive the leader and hold pipes.
        if job is not None:
            job.close()

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
            stop_worker()
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
        finished = True
        return completed, timed_out, reason
    finally:
        if job is not None:
            job.close()
        if process is not None:
            # Covers Popen/Job assignment, thread startup, capture and interruption
            # failures too. An unassigned suspended worker must never be resumed.
            if job is None and not finished:
                # POSIX descendants may still own pipes after the leader exits.
                _terminate_process_group(process)
            elif process.poll() is None:
                process.kill()
                process.wait(timeout=2)
            for thread in threads:
                if thread.is_alive():
                    thread.join(timeout=2)
            for stream in (process.stdout, process.stderr):
                if stream is not None:
                    stream.close()


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


def _stage_verified_file(
    source: Path,
    destination: Path,
    expected_sha256: str,
    mode: int,
    label: str,
) -> Path:
    try:
        shutil.copyfile(source, destination)
        os.chmod(destination, mode)
        actual_sha256 = sha256_file(destination)
    except OSError as error:
        raise TraceRunnerError(f"stage {label} failed") from error
    if actual_sha256 != expected_sha256:
        try:
            os.chmod(destination, 0o700)
            destination.unlink()
        except FileNotFoundError:
            pass
        raise TraceRunnerError(f"staged {label} SHA-256 mismatch")
    return destination


def stage_sources(
    run_root: Path,
    worker: Path,
    worker_sha256: str,
    cases: list[dict[str, Any]],
) -> tuple[Path, list[dict[str, Any]]]:
    source_root = run_root / "sources"
    source_root.mkdir(mode=0o700)
    staged_worker = _stage_verified_file(
        worker,
        source_root / "worker",
        worker_sha256,
        0o500,
        "worker",
    )
    staged_cases = []
    for index, case in enumerate(cases):
        case_root = source_root / f"case-{index:02d}"
        case_root.mkdir(mode=0o700)
        staged_case = dict(case)
        staged_case["plugin"] = _stage_verified_file(
            case["plugin"],
            case_root / "plugin.aex",
            case["plugin_sha256"],
            0o400,
            f"plugin for case {case['id']}",
        )
        staged_case["input_png"] = _stage_verified_file(
            case["input_png"],
            case_root / "input.png",
            case["input_png_sha256"],
            0o400,
            f"input PNG for case {case['id']}",
        )
        staged_cases.append(staged_case)
    return staged_worker, staged_cases


def _replace_known_paths(value: str, replacements: dict[str, str]) -> str:
    for source, token in sorted(
        replacements.items(), key=lambda item: len(item[0]), reverse=True
    ):
        if source:
            value = value.replace(source, token)
    return value


def _redact_text(value: str, replacements: dict[str, str]) -> str:
    value = _replace_known_paths(value, replacements)
    # Recognize foreign-OS paths without filesystem access. Do not collapse an
    # entire path-prefixed error: its explicit diagnostic suffix must survive.
    value = re.sub(
        r"([\"'])((?:[A-Za-z]:[\\/]|\\\\|/)[^\r\n]*?)\1",
        lambda match: f"{match[1]}<absolute-path>{match[1]}",
        value,
    )
    value = re.sub(
        r"(?<![\w.-])(?:\\\\[?.]\\(?:UNC\\|[A-Za-z]:\\)?|[A-Za-z]:[\\/]|\\\\|/)"
        r"[^\r\n]*?(?=:\s|;\s*reason=|[\r\n]|$)",
        "<absolute-path>",
        value,
    )
    encoded = value.encode("utf-8")
    if len(encoded) > MAX_ERROR_BYTES:
        value = encoded[:MAX_ERROR_BYTES].decode("utf-8", errors="ignore")
    return value


def _sanitize(
    value: Any, replacements: dict[str, str], *, diagnostic_text: bool = False
) -> Any:
    if isinstance(value, str):
        if not diagnostic_text:
            value = _replace_known_paths(value, replacements)
            if PureWindowsPath(value).is_absolute() or PurePosixPath(value).is_absolute():
                return "<absolute-path>"
        return _redact_text(value, replacements)
    if isinstance(value, list):
        return [
            _sanitize(item, replacements, diagnostic_text=diagnostic_text)
            for item in value
        ]
    if isinstance(value, dict):
        return {
            key: _sanitize(item, replacements, diagnostic_text=key in {"message", "reason"})
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
    # Canonicalize the parent once but keep the final component lexical.  This
    # pins temp creation/replacement to one directory even if a parent symlink
    # is swapped later, without following a pre-existing final symlink victim.
    output_candidate = Path(
        os.path.abspath(os.fspath(args.output.expanduser()))
    )
    output_candidate.parent.mkdir(parents=True, exist_ok=True)
    output_parent = output_candidate.parent.resolve(strict=True)
    if not output_parent.is_dir():
        raise TraceRunnerError("output parent must be a directory")
    output = output_parent / output_candidate.name
    protected = [("manifest", manifest_path), ("worker", worker)]
    for case in cases:
        protected.extend(
            [
                (f"plugin for case {case['id']}", case["plugin"]),
                (f"input PNG for case {case['id']}", case["input_png"]),
            ]
        )
    reject_output_alias(output, protected)
    with tempfile.TemporaryDirectory(
        prefix="aex-targeted-trace-", dir=args.run_parent
    ) as temporary:
        run_root = Path(temporary)
        staged_worker, staged_cases = stage_sources(
            run_root, worker, worker_sha, cases
        )
        results = [
            {
                "id": case["id"],
                "plugin_sha256": case["plugin_sha256"],
                "input_png_sha256": case["input_png_sha256"],
                "pixel_format": case["pixel_format"],
                "parameters": case["parameters"],
                "result": run_case(
                    staged_worker, case, run_root, args.timeout
                ),
            }
            for case in staged_cases
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
    result_kinds = [
        case["result"]["kind"] for case in report["cases"]
    ]
    print(
        json.dumps(
            {
                "case_count": report["case_count"],
                "result_kinds": result_kinds,
            },
            sort_keys=True,
        )
    )
    return 0 if all(kind == "trace" for kind in result_kinds) else 1


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Drive a session-harness render under Frida and record a native_observation trace.

PID-resolution decision (see docs/KNOWN_FUNCTION_OBSERVATION_2026-07-19.md for the
full trade-off write-up): this launcher *spawns* the supported
``aexcompat-harness.exe --render-experimental-session`` command with Frida, injects
the resolved read plan while the process is suspended, then resumes. Frida owns the
PID, so hooks are in place before any render code runs and every invocation is
captured. This does not go through the broker's evidence-tier restricted token or
sealed load tree, but it still enforces the crash-containment floor: the spawned
harness is assigned to a Windows Job Object with kill-on-close and a process-memory
cap, so the whole process tree (including any descendant the plug-in spawns) is
terminated on exit/timeout. The session harness performs the plug-in admission and
hash checks. Deleted one-shot worker verbs are rejected before Frida is imported and
reported as structured blockers.

The message-to-event pipeline and spawn-argv assembly here are importable and unit
tested without Frida; only :func:`run_observation` imports ``frida`` (lazily), so the
machine-portable test suite never needs the observation runtime installed.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import tempfile
import threading
import uuid
from pathlib import Path
from typing import Any

try:
    from tools.known_function_observation import build_event, resolve_spec, session_boundary_event
except ModuleNotFoundError:  # invoked as a script from tools/
    from known_function_observation import build_event, resolve_spec, session_boundary_event


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_HARNESS = "broker/target/release/aexcompat-harness.exe"
# Observation traces may only be written under this root, canonicalised, with no
# reparse point on the path - so a -Out race cannot redirect the write elsewhere.
OUTPUT_ROOT = REPO_ROOT / "target" / "known-function-observation"
DEFAULT_TIMEOUT_SECONDS = 30
# Process-memory cap for the isolated worker tree (bytes).
WORKER_MEMORY_CAP = 2 * 1024 * 1024 * 1024


class ObservationError(RuntimeError):
    pass


class ObservationBlocker(ObservationError):
    """A reproducible prerequisite or transport blocker, never a success result."""

    def __init__(self, *, blocker_id: str, command: str, replacement: str, reason: str):
        self.blocker = {
            "status": "blocked",
            "blocker_id": blocker_id,
            "requested_command": command,
            "replacement": replacement,
            "reason": reason,
        }
        super().__init__(json.dumps(self.blocker, ensure_ascii=False, sort_keys=True))


def _is_reparse_point(path: Path) -> bool:
    """True for a symlink OR a Windows junction/mount point.

    ``Path.is_symlink()`` does not report NTFS junctions, which are reparse points
    that ``resolve()`` still follows; check the reparse attribute on Windows too.
    """

    try:
        if path.is_symlink():
            return True
    except OSError:
        return False
    if sys.platform == "win32":
        import stat as _stat

        try:
            attrs = os.lstat(path).st_file_attributes
        except (OSError, AttributeError):
            return False
        return bool(attrs & getattr(_stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0x400))
    return False


def safe_output_path(out_path: Path) -> Path:
    """Resolve ``out_path`` under OUTPUT_ROOT, rejecting escapes and reparse points.

    Guards against a path race / symlink redirect: the destination must resolve
    inside the allowed root and no existing component of the path may be a
    symlink or reparse point.
    """

    # Reject a reparse point on the FIXED root components (target/, the subdir)
    # before resolving: otherwise OUTPUT_ROOT.resolve() would follow a symlinked
    # 'target' and adopt the redirected destination as the allowed root, so a
    # normal --out would pass containment yet write outside the repo. REPO_ROOT is
    # already canonical (Path.resolve() at import).
    for component in (OUTPUT_ROOT, OUTPUT_ROOT.parent):
        if _is_reparse_point(component):
            raise ObservationError(f"output root component is a reparse point: {component.name}")
    OUTPUT_ROOT.mkdir(parents=True, exist_ok=True)
    root = OUTPUT_ROOT.resolve()
    candidate = out_path if out_path.is_absolute() else (REPO_ROOT / out_path)
    resolved = candidate.resolve()
    if not (resolved == root or root in resolved.parents):
        raise ObservationError(f"output must stay under {OUTPUT_ROOT}")
    # Must be a file path, not the root or an existing directory, so an invalid
    # target fails here (before spawn) rather than at the final os.replace.
    if resolved == root or resolved.is_dir():
        raise ObservationError("output must be a file path, not a directory")
    # Reject a reparse point / symlink anywhere on the existing prefix below root.
    probe = resolved
    while probe != root and probe != probe.parent:
        if probe.exists() and _is_reparse_point(probe):
            raise ObservationError(f"output path component is a reparse point: {probe.name}")
        probe = probe.parent
    resolved.parent.mkdir(parents=True, exist_ok=True)
    return resolved


def _atomic_write_jsonl(session: dict[str, Any], destination: Path) -> Path:
    """Atomically write the trace to an already-validated destination."""

    # allow_nan=False keeps the JSONL strict/portable: NaN/Infinity are already
    # rejected upstream by the validator, but fail closed at serialization too.
    lines = [json.dumps(event, ensure_ascii=False, allow_nan=False) for event in session["events"]]
    body = "\n".join(lines) + "\n"
    fd, tmp_name = tempfile.mkstemp(dir=str(destination.parent), suffix=".jsonl.tmp")
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            handle.write(body)
        os.replace(tmp_name, destination)  # atomic; does not follow a target symlink
    except BaseException:
        try:
            os.unlink(tmp_name)
        except OSError:
            pass
        raise
    return destination


def write_session_jsonl(session: dict[str, Any], out_path: Path) -> Path:
    """Validate the destination and atomically write the trace under the allowed root."""

    return _atomic_write_jsonl(session, safe_output_path(out_path))


def build_harness_argv(harness_program: str, session_args: list[str]) -> list[str]:
    """Assemble and validate the current session-harness command.

    ``session_args`` excludes the executable and must contain either the
    ``--render-experimental-session`` command or its parameterized variant. The
    old ``aex_render_worker.exe --render-image*`` transport was removed in #365;
    accepting it here would make an observation look runnable while spawning a
    command that cannot exist anymore.
    """

    if not session_args:
        raise ObservationError("session_args must start with a session-harness command")
    command = session_args[0]
    valid_commands = {
        "--render-experimental-session",
        "--render-experimental-session-param",
    }
    if command not in valid_commands:
        if command.startswith("--render"):
            raise ObservationBlocker(
                blocker_id="deleted_one_shot_worker_argv",
                command=command,
                replacement="--render-experimental-session <aex> <input-image> <output-image> <pixel-format> <classic|smart> <current-time> <total-time> <time-scale>",
                reason="one-shot worker render verbs were removed in #365; Frida observation must spawn the session harness",
            )
        raise ObservationError(
            "session_args must start with --render-experimental-session or "
            "--render-experimental-session-param"
        )
    expected_count = 11 if command.endswith("-param") else 9
    if len(session_args) != expected_count:
        raise ObservationError(
            f"{command} expects {expected_count - 1} arguments after the command; "
            f"received {len(session_args) - 1}"
        )
    return [harness_program, *session_args]


class MessageCollector:
    """Turn Frida ``send`` payloads into a validated native_observation session.

    Seeds a ``session_start`` boundary, appends one ``known_function_invoke`` per
    forwarded message (fail-closed: a redaction or shape violation raises), and
    finalises with ``session_end``. Frida-agnostic so it is unit tested directly.
    """

    def __init__(self, *, plugin_label: str, host_version_label: str, module_label: str, session_id: str):
        self.plugin_label = plugin_label
        self.host_version_label = host_version_label
        self.module_label = module_label
        self.session_id = session_id
        self.install_error: str | None = None
        self.installed_hook_count: int | None = None
        self.ready: bool = False
        self.read_error_count: int = 0
        self.format_error: str | None = None
        self._events: list[dict[str, Any]] = [
            session_boundary_event(
                "session_start",
                plugin_label=plugin_label,
                host_version_label=host_version_label,
                event_index=0,
            )
        ]

    def handle(self, payload: dict[str, Any]) -> None:
        kind = payload.get("type")
        if kind == "known_function":
            event = build_event(
                payload,
                module_label=self.module_label,
                plugin_label=self.plugin_label,
                host_version_label=self.host_version_label,
                event_index=len(self._events),
            )
            self._events.append(event)
        elif kind == "ready":
            # Loader watch armed (or hooks already attached). Safe to resume.
            self.ready = True
            if payload.get("installed"):
                self.installed_hook_count = payload.get("hook_count")
        elif kind == "installed":
            self.installed_hook_count = payload.get("hook_count")
        elif kind == "install_error":
            self.install_error = str(payload.get("message"))
        elif kind == "read_error":
            # A field read failed (null/out-of-range pointer); the field is
            # omitted from the trace and counted, never fabricated.
            self.read_error_count += 1
        # Only known_function reaches the trace; control messages never do.

    def finalize(self, *, completed: bool = True) -> dict[str, Any]:
        """Close the session.

        ``completed`` must reflect whether the worker actually exited. On a
        timeout/hang it is ``False``: no ``session_end`` boundary is appended and
        ``trace_complete`` is ``False``, so downstream validation sees a truncated
        observation as incomplete instead of trusting a fabricated end marker.
        """

        events = list(self._events)
        if completed:
            events.append(
                session_boundary_event(
                    "session_end",
                    plugin_label=self.plugin_label,
                    host_version_label=self.host_version_label,
                    event_index=len(events),
                )
            )
        return {
            "schema_version": 1,
            "session_id": self.session_id,
            "event_count": len(events),
            "trace_complete": bool(completed),
            "events": events,
        }


def run_observation(
    *,
    spec_path: Path,
    offset_map_path: Path,
    module_path: str,
    session_args: list[str],
    out_path: Path,
    harness_program: str = DEFAULT_HARNESS,
    plugin_label: str,
    host_version_label: str = "native-observation frida",
    timeout_seconds: int = DEFAULT_TIMEOUT_SECONDS,
) -> dict[str, Any]:
    """Spawn the session harness under Frida, inject the plan, and write the trace.

    ``module_path`` is the expected canonical path of the plug-in the harness
    loads; the JS binds hook installation to that exact module (not just its
    basename). Imports ``frida`` lazily so the module stays importable (and unit
    testable) without the observation runtime.
    """

    # Validate everything frida-independent first (spec, plan, output path) so an
    # invalid --out or hook set fails before any spawn/render, and without needing
    # the observation runtime installed.
    spec = json.loads(spec_path.read_text(encoding="utf-8"))
    plan = resolve_spec(spec, offset_map_path)
    expected_hook_count = len(plan["hooks"])
    module_path = str(Path(module_path).resolve())
    destination = safe_output_path(out_path)
    argv = build_harness_argv(harness_program, session_args)
    script_source = (Path(__file__).parent / "frida" / "known_function_probe.js").read_text(encoding="utf-8")

    try:
        import frida  # noqa: PLC0415  (intentional lazy, observation-only import)
    except ImportError as exc:  # pragma: no cover - runtime-only path
        raise ObservationError(
            "frida is required for live observation; install it in the observation "
            "environment (it is intentionally not in the uv-managed dev dependencies)"
        ) from exc

    collector = MessageCollector(
        plugin_label=plugin_label,
        host_version_label=host_version_label,
        module_label=plan["module_label"],
        session_id=str(uuid.uuid4()),
    )
    # Set once the JS acknowledges the loader watch is armed (or reports a
    # failure), so we never resume before it is safe.
    ready = threading.Event()

    def on_message(message, _data):  # pragma: no cover - requires frida runtime
        if message.get("type") == "send":
            payload = message.get("payload") or {}
            try:
                collector.handle(payload)
            except Exception as exc:  # noqa: BLE001 - Frida swallows callback errors
                # Record the failure so the run fails closed instead of silently
                # dropping a rejected invocation while still reporting complete.
                collector.format_error = str(exc)
            if payload.get("type") in ("ready", "install_error"):
                ready.set()
        elif message.get("type") == "error":
            collector.install_error = message.get("stack") or message.get("description")
            ready.set()

    device = frida.get_local_device()
    pid = device.spawn(argv)
    completed = False
    try:  # pragma: no cover - requires frida runtime + worker
        # The kill wraps the Job Object context so that even a failure inside
        # _JobIsolation.__enter__ (e.g. job assignment denied) still terminates the
        # suspended spawned worker rather than leaking it.
        with _JobIsolation(pid) as job:
            session = device.attach(pid)
            script = session.create_script(script_source)
            script.on("message", on_message)
            script.load()
            # The JS installs hooks from an async recv('plan') handler, so wait for
            # the 'ready' acknowledgement (loader watch armed) before resuming -
            # otherwise the worker could reach the observed function before any hook
            # exists. send() is asynchronous and never blocks the render.
            script.post({"type": "plan", "plan": plan, "module_path": module_path})
            if not ready.wait(timeout=min(timeout_seconds, DEFAULT_TIMEOUT_SECONDS)):
                raise ObservationError("Frida loader watch was not acknowledged before resume")
            if collector.install_error:
                raise ObservationError(f"Frida hook install failed: {collector.install_error}")
            device.resume(pid)
            exited = _await_exit(frida, device, pid, timeout_seconds)
            # Read the worker's own exit code before __exit__ kills the tree; a
            # non-zero exit (e.g. minihost render_failed=20) is a failed/partial
            # run and must not be recorded as complete.
            exit_code = job.worker_exit_code() if exited else None
            # A trace is complete only if the worker exited cleanly (code 0), the
            # expected hooks attached, no invocation was rejected by the formatter,
            # and every requested field read succeeded. A read_error means a
            # requested field was dropped (null/out-of-extent/unsafe 64-bit), so the
            # observation is missing data and must not be reported complete on the
            # side-channel count alone.
            completed = (
                exited
                and exit_code == 0
                and collector.installed_hook_count == expected_hook_count
                and collector.format_error is None
                and collector.read_error_count == 0
            )
    finally:
        try:
            device.kill(pid)
        except frida.ProcessNotFoundError:
            pass

    session = collector.finalize(completed=completed)
    _atomic_write_jsonl(session, destination)  # destination preflighted before spawn
    # The returned result carries run metadata alongside the schema-clean session
    # (the session dict itself stays validatable; metadata lives on the result).
    result = dict(session)
    result["output_path"] = str(destination)
    result["read_error_count"] = collector.read_error_count
    result["format_error"] = collector.format_error
    return result


class _JobIsolation:  # pragma: no cover - Windows runtime path
    """Assign a spawned PID to a Job Object with kill-on-close and a memory cap.

    This gives the observation path a crash-containment floor equivalent to the
    broker default tier: the whole process tree (including any descendant the
    plug-in spawns) is terminated when the job handle closes, and the tree is
    bounded by a process-memory cap. It is not a confidentiality sandbox and does
    not apply a restricted token (Frida spawn cannot); use secure_launch for the
    evidence tier. A no-op off Windows.
    """

    def __init__(self, pid: int):
        self.pid = pid
        self._job = None
        self._kernel32 = None
        self._proc_handle = None
        self._ctypes = None

    def __enter__(self):
        if sys.platform != "win32":
            return self
        import ctypes
        from ctypes import wintypes

        k32 = ctypes.WinDLL("kernel32", use_last_error=True)
        self._kernel32 = k32
        # Declare HANDLE-sized signatures so a 64-bit HANDLE is not truncated to
        # ctypes' default 32-bit c_int return/argument type.
        HANDLE = wintypes.HANDLE
        k32.CreateJobObjectW.restype = HANDLE
        k32.CreateJobObjectW.argtypes = [wintypes.LPVOID, wintypes.LPCWSTR]
        k32.OpenProcess.restype = HANDLE
        k32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        k32.SetInformationJobObject.restype = wintypes.BOOL
        k32.SetInformationJobObject.argtypes = [HANDLE, ctypes.c_int, wintypes.LPVOID, wintypes.DWORD]
        k32.AssignProcessToJobObject.restype = wintypes.BOOL
        k32.AssignProcessToJobObject.argtypes = [HANDLE, HANDLE]
        k32.CloseHandle.restype = wintypes.BOOL
        k32.CloseHandle.argtypes = [HANDLE]
        k32.GetExitCodeProcess.restype = wintypes.BOOL
        k32.GetExitCodeProcess.argtypes = [HANDLE, ctypes.POINTER(wintypes.DWORD)]
        self._ctypes = ctypes
        self._dword = wintypes.DWORD
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x2000
        JOB_OBJECT_LIMIT_PROCESS_MEMORY = 0x0100
        JOB_OBJECT_LIMIT_JOB_MEMORY = 0x0200
        JobObjectExtendedLimitInformation = 9
        PROCESS_SET_QUOTA = 0x0100
        PROCESS_TERMINATE = 0x0001
        PROCESS_QUERY_LIMITED_INFORMATION = 0x1000

        job = k32.CreateJobObjectW(None, None)
        if not job:
            raise ObservationError(f"CreateJobObject failed: {ctypes.get_last_error()}")

        class JOBOBJECT_BASIC_LIMIT_INFORMATION(ctypes.Structure):
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

        class IO_COUNTERS(ctypes.Structure):
            _fields_ = [(n, ctypes.c_uint64) for n in
                        ("ReadOperationCount", "WriteOperationCount", "OtherOperationCount",
                         "ReadTransferCount", "WriteTransferCount", "OtherTransferCount")]

        class JOBOBJECT_EXTENDED_LIMIT_INFORMATION(ctypes.Structure):
            _fields_ = [
                ("BasicLimitInformation", JOBOBJECT_BASIC_LIMIT_INFORMATION),
                ("IoInfo", IO_COUNTERS),
                ("ProcessMemoryLimit", ctypes.c_size_t),
                ("JobMemoryLimit", ctypes.c_size_t),
                ("PeakProcessMemoryUsed", ctypes.c_size_t),
                ("PeakJobMemoryUsed", ctypes.c_size_t),
            ]

        info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION()
        info.BasicLimitInformation.LimitFlags = (
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            | JOB_OBJECT_LIMIT_PROCESS_MEMORY
            | JOB_OBJECT_LIMIT_JOB_MEMORY
        )
        # Per-process cap AND a tree-wide (job-total) cap, so a plug-in that spawns
        # many children each under the per-process limit cannot exhaust memory in
        # aggregate - the crash-containment floor covers the whole tree.
        info.ProcessMemoryLimit = WORKER_MEMORY_CAP
        info.JobMemoryLimit = WORKER_MEMORY_CAP
        if not k32.SetInformationJobObject(
            job, JobObjectExtendedLimitInformation, ctypes.byref(info), ctypes.sizeof(info)
        ):
            err = ctypes.get_last_error()
            k32.CloseHandle(job)
            raise ObservationError(f"SetInformationJobObject failed: {err}")

        handle = k32.OpenProcess(
            PROCESS_SET_QUOTA | PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
            False,
            self.pid,
        )
        if not handle:
            err = ctypes.get_last_error()
            k32.CloseHandle(job)
            raise ObservationError(f"OpenProcess({self.pid}) failed: {err}")
        if not k32.AssignProcessToJobObject(job, handle):
            err = ctypes.get_last_error()
            k32.CloseHandle(handle)
            k32.CloseHandle(job)
            raise ObservationError(f"AssignProcessToJobObject failed: {err}")
        # Keep the handle open (with query rights) so worker_exit_code() can read
        # the real exit code after a natural exit, before __exit__ kills the tree.
        self._proc_handle = handle
        self._job = job
        return self

    def worker_exit_code(self):
        """Return the worker's exit code, or None if unavailable.

        Call this after the worker has exited naturally and before __exit__ (which
        kills the tree), so the code is the worker's own, not our termination code.
        """

        if not self._proc_handle or not self._kernel32 or not self._ctypes:
            return None
        STILL_ACTIVE = 259
        code = self._dword()
        if not self._kernel32.GetExitCodeProcess(self._proc_handle, self._ctypes.byref(code)):
            return None
        return None if code.value == STILL_ACTIVE else int(code.value)

    def __exit__(self, *exc):
        # Closing the job handle triggers kill-on-close, terminating the tree.
        if self._proc_handle and self._kernel32:
            self._kernel32.CloseHandle(self._proc_handle)
            self._proc_handle = None
        if self._job and self._kernel32:
            self._kernel32.CloseHandle(self._job)
            self._job = None
        return False


def _process_alive(device, pid) -> bool:  # pragma: no cover - runtime path
    """PID-aware liveness check.

    ``Device.get_process`` takes a process *name* (it lowercases the argument),
    so passing an int raises ``AttributeError``. Enumerate by PID instead.
    """

    try:
        procs = device.enumerate_processes(pids=[pid])
    except TypeError:  # older frida without the pids filter
        procs = [p for p in device.enumerate_processes() if p.pid == pid]
    return any(p.pid == pid for p in procs)


def _await_exit(frida_module, device, pid, timeout_seconds) -> bool:  # pragma: no cover - runtime path
    """Return True if the worker exited within the timeout, False on timeout/hang."""

    import time  # local import keeps the module import side-effect free

    deadline = time.monotonic() + timeout_seconds
    while time.monotonic() < deadline:
        if not _process_alive(device, pid):
            return True
        time.sleep(0.05)
    return False


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--spec", required=True, type=Path)
    parser.add_argument("--offset-map", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path, help="trace destination (must resolve under target/known-function-observation)")
    parser.add_argument("--module-path", required=True, help="canonical path of the plug-in module the harness loads; hooks bind to this exact module")
    parser.add_argument("--plugin-label", required=True, help="redacted filename stem for the trace")
    parser.add_argument("--harness", default=DEFAULT_HARNESS)
    parser.add_argument("--timeout-seconds", type=int, default=DEFAULT_TIMEOUT_SECONDS)
    parser.add_argument("session_args", nargs=argparse.REMAINDER, help="-- <session-harness command and args>")
    args = parser.parse_args(argv)

    session_args = args.session_args
    if session_args and session_args[0] == "--":
        session_args = session_args[1:]
    try:
        result = run_observation(
            spec_path=args.spec,
            offset_map_path=args.offset_map,
            module_path=args.module_path,
            session_args=session_args,
            out_path=args.out,
            harness_program=args.harness,
            plugin_label=args.plugin_label,
            timeout_seconds=args.timeout_seconds,
        )
    except ObservationBlocker as exc:
        print(json.dumps({"observed": False, "blocker": exc.blocker}, ensure_ascii=False, sort_keys=True))
        print(f"observe_known_functions: blocked: {exc}", file=sys.stderr)
        return 4
    except (OSError, ValueError, ObservationError) as exc:
        print(f"observe_known_functions: {type(exc).__name__}: {exc}", file=sys.stderr)
        return 2
    complete = result["trace_complete"]
    print(json.dumps({
        "observed": True,
        "event_count": result["event_count"],
        "complete": complete,
        "read_errors": result.get("read_error_count", 0),
        "format_error": result.get("format_error"),
    }))
    if not complete:
        reason = result.get("format_error") or "timeout or hooks not installed"
        print(f"observe_known_functions: incomplete observation ({reason})", file=sys.stderr)
        return 3
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

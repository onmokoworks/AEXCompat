#!/usr/bin/env python3
"""Drive a receipt-free worker render under Frida and record a native_observation trace.

PID-resolution decision (see docs/KNOWN_FUNCTION_OBSERVATION_2026-07-19.md for the
full trade-off write-up): this launcher *spawns* ``aex_render_worker.exe`` through
its own documented render CLI (``--render-image`` and friends) with Frida, injects
the resolved read plan while the process is suspended, then resumes. Frida owns the
PID, so hooks are in place before any render code runs and every invocation is
captured. This does not go through the broker's evidence-tier restricted token or
sealed load tree, but it still enforces the crash-containment floor: the spawned
worker is assigned to a Windows Job Object with kill-on-close and a process-memory
cap, so the whole process tree (including any descendant the plug-in spawns) is
terminated on exit/timeout. The worker also performs its own plug-in hash and
admission checks. The rejected alternatives (process-name enumeration; a broker-core
PID handoff with a resume gate) are documented in the same file.

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
DEFAULT_WORKER = "target/minihost-build/aex_render_worker.exe"
# Observation traces may only be written under this root, canonicalised, with no
# reparse point on the path - so a -Out race cannot redirect the write elsewhere.
OUTPUT_ROOT = REPO_ROOT / "target" / "known-function-observation"
DEFAULT_TIMEOUT_SECONDS = 30
# Process-memory cap for the isolated worker tree (bytes).
WORKER_MEMORY_CAP = 2 * 1024 * 1024 * 1024


class ObservationError(RuntimeError):
    pass


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
        if component.is_symlink():
            raise ObservationError(f"output root component is a reparse point: {component.name}")
    OUTPUT_ROOT.mkdir(parents=True, exist_ok=True)
    root = OUTPUT_ROOT.resolve()
    candidate = out_path if out_path.is_absolute() else (REPO_ROOT / out_path)
    resolved = candidate.resolve()
    if not (resolved == root or root in resolved.parents):
        raise ObservationError(f"output must stay under {OUTPUT_ROOT}")
    # Reject a reparse point / symlink anywhere on the existing prefix below root.
    probe = resolved
    while probe != root and probe != probe.parent:
        if probe.exists() and probe.is_symlink():
            raise ObservationError(f"output path component is a symlink: {probe.name}")
        probe = probe.parent
    resolved.parent.mkdir(parents=True, exist_ok=True)
    return resolved


def write_session_jsonl(session: dict[str, Any], out_path: Path) -> Path:
    """Atomically write the trace under the allowed root (temp + os.replace)."""

    destination = safe_output_path(out_path)
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


def build_worker_argv(worker_program: str, render_args: list[str]) -> list[str]:
    """Assemble the spawn argv for the worker's standalone render CLI.

    ``render_args`` is the worker command line as an existing render gate would
    pass it, e.g. ``["--render-image", aex, aex_sha256, "v5|", input, output,
    "16", "12", "0", "1", "1", "1"]`` (see tools/run-aex-render-gate.ps1).
    """

    if not render_args or not render_args[0].startswith("--render"):
        raise ObservationError("render_args must start with a --render* worker verb")
    return [worker_program, *render_args]


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
    render_args: list[str],
    out_path: Path,
    worker_program: str = DEFAULT_WORKER,
    plugin_label: str,
    host_version_label: str = "native-observation frida",
    timeout_seconds: int = DEFAULT_TIMEOUT_SECONDS,
) -> dict[str, Any]:
    """Spawn the worker under Frida, inject the plan, and write the trace.

    ``module_path`` is the expected canonical path of the plug-in the worker
    loads; the JS binds hook installation to that exact module (not just its
    basename). Imports ``frida`` lazily so the module stays importable (and unit
    testable) without the observation runtime.
    """

    try:
        import frida  # noqa: PLC0415  (intentional lazy, observation-only import)
    except ImportError as exc:  # pragma: no cover - runtime-only path
        raise ObservationError(
            "frida is required for live observation; install it in the observation "
            "environment (it is intentionally not in requirements-dev.txt)"
        ) from exc

    spec = json.loads(spec_path.read_text(encoding="utf-8"))
    plan = resolve_spec(spec, offset_map_path)
    expected_hook_count = len(plan["hooks"])
    module_path = str(Path(module_path).resolve())
    script_source = (Path(__file__).parent / "frida" / "known_function_probe.js").read_text(encoding="utf-8")

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
            collector.handle(payload)
            if payload.get("type") in ("ready", "install_error"):
                ready.set()
        elif message.get("type") == "error":
            collector.install_error = message.get("stack") or message.get("description")
            ready.set()

    device = frida.get_local_device()
    argv = build_worker_argv(worker_program, render_args)
    pid = device.spawn(argv)
    completed = False
    try:  # pragma: no cover - requires frida runtime + worker
        # The kill wraps the Job Object context so that even a failure inside
        # _JobIsolation.__enter__ (e.g. job assignment denied) still terminates the
        # suspended spawned worker rather than leaking it.
        with _JobIsolation(pid):
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
            # A trace is complete only if the worker exited AND the expected hooks
            # actually attached. If the module never loaded (module_path wrong, or a
            # loader path we do not watch), hooks never install and an empty trace
            # must not be reported as complete.
            completed = exited and collector.installed_hook_count == expected_hook_count
    finally:
        try:
            device.kill(pid)
        except frida.ProcessNotFoundError:
            pass

    session = collector.finalize(completed=completed)
    destination = write_session_jsonl(session, out_path)
    # The returned result carries run metadata alongside the schema-clean session
    # (the session dict itself stays validatable; metadata lives on the result).
    result = dict(session)
    result["output_path"] = str(destination)
    result["read_error_count"] = collector.read_error_count
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

    def __enter__(self):
        if sys.platform != "win32":
            return self
        import ctypes
        from ctypes import wintypes

        k32 = ctypes.WinDLL("kernel32", use_last_error=True)
        self._kernel32 = k32
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x2000
        JOB_OBJECT_LIMIT_PROCESS_MEMORY = 0x0100
        JobObjectExtendedLimitInformation = 9
        PROCESS_SET_QUOTA = 0x0100
        PROCESS_TERMINATE = 0x0001

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
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_PROCESS_MEMORY
        )
        info.ProcessMemoryLimit = WORKER_MEMORY_CAP
        if not k32.SetInformationJobObject(
            job, JobObjectExtendedLimitInformation, ctypes.byref(info), ctypes.sizeof(info)
        ):
            err = ctypes.get_last_error()
            k32.CloseHandle(job)
            raise ObservationError(f"SetInformationJobObject failed: {err}")

        handle = k32.OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, False, self.pid)
        if not handle:
            err = ctypes.get_last_error()
            k32.CloseHandle(job)
            raise ObservationError(f"OpenProcess({self.pid}) failed: {err}")
        try:
            if not k32.AssignProcessToJobObject(job, handle):
                err = ctypes.get_last_error()
                k32.CloseHandle(job)
                raise ObservationError(f"AssignProcessToJobObject failed: {err}")
        finally:
            k32.CloseHandle(handle)
        self._job = job
        return self

    def __exit__(self, *exc):
        # Closing the job handle triggers kill-on-close, terminating the tree.
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
    parser.add_argument("--module-path", required=True, help="canonical path of the plug-in module the worker loads; hooks bind to this exact module")
    parser.add_argument("--plugin-label", required=True, help="redacted filename stem for the trace")
    parser.add_argument("--worker", default=DEFAULT_WORKER)
    parser.add_argument("--timeout-seconds", type=int, default=DEFAULT_TIMEOUT_SECONDS)
    parser.add_argument("render_args", nargs=argparse.REMAINDER, help="-- <worker render verb and args>")
    args = parser.parse_args(argv)

    render_args = args.render_args
    if render_args and render_args[0] == "--":
        render_args = render_args[1:]
    try:
        result = run_observation(
            spec_path=args.spec,
            offset_map_path=args.offset_map,
            module_path=args.module_path,
            render_args=render_args,
            out_path=args.out,
            worker_program=args.worker,
            plugin_label=args.plugin_label,
            timeout_seconds=args.timeout_seconds,
        )
    except (OSError, ValueError, ObservationError) as exc:
        print(f"observe_known_functions: {type(exc).__name__}: {exc}", file=sys.stderr)
        return 2
    complete = result["trace_complete"]
    print(json.dumps({
        "observed": True,
        "event_count": result["event_count"],
        "complete": complete,
        "read_errors": result.get("read_error_count", 0),
    }))
    if not complete:
        # The worker timed out or the hooks never attached; the trace is truncated.
        print("observe_known_functions: incomplete observation (timeout or hooks not installed)", file=sys.stderr)
        return 3
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

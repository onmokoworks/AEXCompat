#!/usr/bin/env python3
"""Drive a receipt-free worker render under Frida and record a native_observation trace.

PID-resolution decision (see docs/KNOWN_FUNCTION_OBSERVATION_2026-07-19.md for the
full trade-off write-up): this launcher *spawns* ``aex_render_worker.exe`` through
its own documented render CLI (``--render-image`` and friends) with Frida, injects
the resolved read plan while the process is suspended, then resumes. Frida owns the
PID, so hooks are in place before any render code runs and every invocation is
captured. This deliberately does not go through the broker's Job Object / restricted
token / sealed load tree: that hardened launch path is for evidence-tier dispatch,
and known-function observation is an explicitly non-evidence reverse-engineering
path. The worker still performs its own plug-in hash and admission checks. The two
rejected alternatives (process-name enumeration; a broker-core PID handoff with a
resume gate) are documented in the same file.

The message-to-event pipeline and spawn-argv assembly here are importable and unit
tested without Frida; only :func:`run_observation` imports ``frida`` (lazily), so the
machine-portable test suite never needs the observation runtime installed.
"""

from __future__ import annotations

import argparse
import json
import sys
import threading
import uuid
from pathlib import Path
from typing import Any

try:
    from tools.known_function_observation import build_event, resolve_spec, session_boundary_event
except ModuleNotFoundError:  # invoked as a script from tools/
    from known_function_observation import build_event, resolve_spec, session_boundary_event


DEFAULT_WORKER = "target/minihost-build/aex_render_worker.exe"
DEFAULT_TIMEOUT_SECONDS = 30


class ObservationError(RuntimeError):
    pass


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


def write_session_jsonl(session: dict[str, Any], out_path: Path) -> None:
    lines = [json.dumps(event, ensure_ascii=False) for event in session["events"]]
    out_path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def run_observation(
    *,
    spec_path: Path,
    offset_map_path: Path,
    module_file: str,
    render_args: list[str],
    out_path: Path,
    worker_program: str = DEFAULT_WORKER,
    plugin_label: str,
    host_version_label: str = "native-observation frida",
    timeout_seconds: int = DEFAULT_TIMEOUT_SECONDS,
) -> dict[str, Any]:
    """Spawn the worker under Frida, inject the plan, and write the trace.

    Imports ``frida`` lazily so the module stays importable (and unit testable)
    without the observation runtime.
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
    script_source = (Path(__file__).parent / "frida" / "known_function_probe.js").read_text(encoding="utf-8")

    collector = MessageCollector(
        plugin_label=plugin_label,
        host_version_label=host_version_label,
        module_label=plan["module_label"],
        session_id=str(uuid.uuid4()),
    )
    # Set once the JS acknowledges hook installation (or reports a failure), so we
    # never resume the worker before the hooks are actually in place.
    installed = threading.Event()

    def on_message(message, _data):  # pragma: no cover - requires frida runtime
        if message.get("type") == "send":
            payload = message.get("payload") or {}
            collector.handle(payload)
            # 'ready' means the loader watch is armed (or hooks already attached),
            # so it is safe to resume; 'install_error' means it never will be.
            if payload.get("type") in ("ready", "install_error"):
                installed.set()
        elif message.get("type") == "error":
            collector.install_error = message.get("stack") or message.get("description")
            installed.set()

    device = frida.get_local_device()
    argv = build_worker_argv(worker_program, render_args)
    pid = device.spawn(argv)
    completed = False
    try:  # pragma: no cover - requires frida runtime + worker build
        session = device.attach(pid)
        script = session.create_script(script_source)
        script.on("message", on_message)
        script.load()
        # The JS installs hooks from an async recv('plan') handler, so wait for the
        # install acknowledgement before resuming — otherwise the worker could reach
        # the observed function before any hook exists and we would miss the calls
        # this path is meant to capture. send() itself is asynchronous and never
        # blocks the render, so the 30s render window is unaffected.
        script.post({"type": "plan", "plan": plan, "module_file": module_file})
        if not installed.wait(timeout=min(timeout_seconds, DEFAULT_TIMEOUT_SECONDS)):
            raise ObservationError("Frida hooks were not acknowledged installed before resume")
        if collector.install_error:
            raise ObservationError(f"Frida hook install failed: {collector.install_error}")
        device.resume(pid)
        completed = _await_exit(frida, device, pid, timeout_seconds)
    finally:  # pragma: no cover - requires frida runtime
        try:
            device.kill(pid)
        except frida.ProcessNotFoundError:
            pass

    result = collector.finalize(completed=completed)
    write_session_jsonl(result, out_path)
    return result


def _await_exit(frida_module, device, pid, timeout_seconds) -> bool:  # pragma: no cover - runtime path
    """Return True if the worker exited within the timeout, False on timeout/hang."""

    import time  # local import keeps the module import side-effect free

    deadline = time.monotonic() + timeout_seconds
    while time.monotonic() < deadline:
        try:
            device.get_process(pid)
        except frida_module.ProcessNotFoundError:
            return True
        time.sleep(0.05)
    return False


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--spec", required=True, type=Path)
    parser.add_argument("--offset-map", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--module-file", required=True, help="on-disk basename of the plug-in module to locate")
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
            module_file=args.module_file,
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
    print(json.dumps({"observed": True, "event_count": result["event_count"], "complete": complete}))
    if not complete:
        # The worker did not exit within the timeout; the trace is truncated.
        print("observe_known_functions: worker timed out; trace is incomplete", file=sys.stderr)
        return 3
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

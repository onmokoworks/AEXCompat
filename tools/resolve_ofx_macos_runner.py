"""Run one Resolve OFX frame through the verified macOS OpenFX adapter.

The native OFX module uses private temporary files to avoid a large JSON
payload in Resolve's process. This command never writes an output frame when
the adapter rejects the request or its worker evidence.
"""

from __future__ import annotations

import base64
import binascii
import json
import os
import signal
import sys
import uuid
from pathlib import Path

import openfx_macos_session as macos_session
from openfx_render_session_contract import MAX_DIMENSION, MAX_TRANSPORT_BYTES


def _terminate(_signum: int, _frame: object) -> None:
    # The shared backend's CLI installs this handler only from its own main().
    # This runner calls the backend API directly, so it owns the same cleanup.
    pid = macos_session.macos_backend._ACTIVE_HARNESS_PID
    if pid is not None:
        try:
            os.killpg(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        try:
            os.waitpid(pid, 0)
        except ChildProcessError:
            pass
    raise SystemExit(124)


def run(args: list[str]) -> int:
    if len(args) != 6:
        return 2
    source_name = os.environ.get("AEXCOMPAT_RESOLVE_AEX_PATH")
    if not source_name:
        return 2
    input_path, output_path, evidence_path = (Path(value) for value in args[:3])
    try:
        width, height, current_time = (int(value) for value in args[3:])
        if not (1 <= width <= MAX_DIMENSION and 1 <= height <= MAX_DIMENSION):
            return 2
        frame_bytes = width * height * 4
        if frame_bytes > MAX_TRANSPORT_BYTES:
            return 2
        pixels = input_path.read_bytes()
        if len(pixels) != frame_bytes:
            return 2
        packet = macos_session.render_macos_frame(
            plugin_relative_path=source_name,
            width=width,
            height=height,
            rowbytes=width * 4,
            pixels=pixels,
            current_time=current_time,
            total_time=max(1, current_time),
        )
        response = packet["frame_exchange"]["response"]
        evidence_path.write_text(json.dumps(packet, separators=(",", ":")), encoding="utf-8")
        if response["status"] != "rendered":
            return 1
        output = response["output"]
        rendered = base64.b64decode(output["data_base64"], validate=True)
        if len(rendered) != frame_bytes:
            return 1
        output_path.write_bytes(rendered)
        evidence_dir = os.environ.get("AEXCOMPAT_RESOLVE_EVIDENCE_DIR")
        if evidence_dir:
            summary = {
                "schema_version": 1,
                "status": "worker_rendered",
                "plugin_relative_path": source_name,
                "plugin_sha256": response["identity"]["plugin_sha256"],
                "worker_sha256": response["identity"]["worker_sha256"],
                "width": width,
                "height": height,
                "time_ms": current_time,
                "input_sha256": packet["frame_exchange"]["request"]["input"]["sha256"],
                "output_sha256": output["sha256"],
            }
            record = Path(evidence_dir) / f"resolve-aex-{uuid.uuid4().hex}.json"
            descriptor = os.open(record, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
                json.dump(summary, stream, separators=(",", ":"))
        return 0
    except (OSError, ValueError, KeyError, TypeError, binascii.Error):
        return 1


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, _terminate)
    raise SystemExit(run(sys.argv[1:]))

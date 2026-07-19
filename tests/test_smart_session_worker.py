"""Behavioral self-tests for the SmartFX resident session frame loop (v1.1).

Drives ``aex_smart_worker.exe --smart-session-v1`` over the same transport as
the classic session tests (docs/RENDER_SESSION_PROTOCOL_2026-07-19.md), using
the reproducible ``pf_smart_geometry_probe.aex`` fixture. The probe selects a
geometry scenario per render time (current_time % 4), which doubles as the
fail-closed coverage: scenario 0 renders full-frame, scenario 1 publishes a
larger-than-request result that a v1 session must reject.
"""
import hashlib
import json
import os
import subprocess
from pathlib import Path

import pytest

from test_render_session_worker import (
    EXIT_INVARIANT_FAILURE,
    HEIGHT,
    OUTPUT_GENERATION_OFFSET,
    SessionTransport,
    TIME_SCALE,
    WIDTH,
    render_frame_message,
)

ROOT = Path(__file__).resolve().parents[1]
WORKER = ROOT / "target" / "minihost-build" / "aex_smart_worker.exe"
RENDER_WORKER = ROOT / "target" / "minihost-build" / "aex_render_worker.exe"
PROBE = (
    ROOT / "target" / "pf-smart-geometry-probe-build" / "Release"
    / "pf_smart_geometry_probe.aex"
)

pytestmark = pytest.mark.skipif(os.name != "nt", reason="session transport is Windows-only")

# The probe reads current_time % 4; keeping every session frame on multiples
# of 4 stays in the full-frame scenario, while time 1 reaches the
# extra-pixels scenario whose result is larger than the session dimensions.
FULL_FRAME_TIMES = (0, 4)
EXTRA_PIXELS_TIME = 1
TOTAL_TIME = 300


def _require_artifacts():
    if not WORKER.is_file():
        pytest.skip("aex_smart_worker.exe is not built; run the minihost build")
    if not PROBE.is_file():
        pytest.skip("pf_smart_geometry_probe.aex is not built")


def _spawn(transport, command="--smart-session-v1", worker=None):
    binary = worker or WORKER
    aex_sha = hashlib.sha256(PROBE.read_bytes()).hexdigest()
    process = subprocess.Popen(
        [str(binary), command, str(PROBE), aex_sha, "v2|",
         str(WIDTH), str(HEIGHT), "1", str(TOTAL_TIME), str(TIME_SCALE)],
        cwd=ROOT, env=transport.environment(), close_fds=False,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    transport.close_child_ends()
    return process


def _finish(process, timeout=60):
    stdout, stderr = process.communicate(timeout=timeout)
    return process.returncode, stdout, stderr


def _logical_argb_hash(pattern):
    rgba = bytes((pattern + index) % 256 for index in range(WIDTH * HEIGHT * 4))
    argb = bytearray()
    for pixel in range(WIDTH * HEIGHT):
        red, green, blue, alpha = rgba[pixel * 4:pixel * 4 + 4]
        argb += bytes((alpha, red, green, blue))
    return hashlib.sha256(bytes(argb)).hexdigest()


def test_smart_session_renders_frames_with_hoisted_sequence():
    _require_artifacts()
    transport = SessionTransport()
    process = _spawn(transport)
    try:
        patterns = (11, 173)
        for frame_index, (time_value, pattern) in enumerate(
                zip(FULL_FRAME_TIMES, patterns)):
            transport.write_input(pattern, frame_index + 1)
            transport.send(render_frame_message(frame_index, time_value))
            done = transport.receive()
            assert done is not None, "worker closed the response pipe early"
            assert done["type"] == "frame_done"
            assert done["frame_index"] == frame_index
            assert done["status"] == "ok", done
            assert done["render_error"] == 0
            assert done["generation"] == frame_index + 1
            output = done["output"]
            assert output["width"] == WIDTH
            assert output["height"] == HEIGHT
            assert output["pixel_format"] == "argb8"
            assert output["guards_intact"] is True
            assert transport.read_header(OUTPUT_GENERATION_OFFSET) == frame_index + 1
            slot = transport.output_bytes(WIDTH * HEIGHT * 4)
            assert hashlib.sha256(slot).hexdigest() == output["checksum"]
        transport.send({"v": 1, "type": "close"})
        code, stdout, stderr = _finish(process)
        assert code == 0, (code, stderr[-500:])
        report = json.loads(stdout.strip())
        assert report["stage"] == "smartfx_render"
        assert report["status"] == "render_completed"
        assert report["session_mode"] is True
        assert report["session_frames_attempted"] == len(FULL_FRAME_TIMES)
        assert report["session_sequence_setup_error"] == 0
        assert report["session_sequence_setdown_error"] == 0
        assert report["session_render_error"] == 0
        assert report["session_protocol_violation"] is False
        assert report["session_invariant_failure"] is False
        # The final report's input hash is the last frame's logical ARGB
        # input: proof that each frame's slot bytes (not a stale copy)
        # reached the smart render pipeline. The probe's output does not
        # depend on the input pixels, so the input side carries the
        # liveness evidence.
        assert report["input_sha256"] == _logical_argb_hash(patterns[-1])
        # The SEQUENCE pair is hoisted out of the per-frame lifecycle: the
        # frame stages fire once per frame while the render_lifecycle sequence
        # stages never do (the hoisted setup goes through the selector
        # invoker, which does not emit stage traces).
        assert stderr.count("stage:frame_setup_begin") == len(FULL_FRAME_TIMES)
        assert stderr.count("stage:sequence_setup_begin") == 0
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)


def test_smart_session_deep32_cpu_command_renders_a_float_frame():
    _require_artifacts()
    transport = SessionTransport(depth_code=32, output_pixel_bytes=16)
    process = _spawn(transport, command="--smart-session32-cpu-v1")
    try:
        transport.write_input(57, 1)
        transport.send(render_frame_message(0, 0))
        done = transport.receive()
        assert done is not None, "worker closed the response pipe early"
        assert done["status"] == "ok", done
        output = done["output"]
        assert output["pixel_format"] == "argb32f"
        assert output["width"] == WIDTH
        assert output["height"] == HEIGHT
        slot = transport.output_bytes(WIDTH * HEIGHT * 16)
        assert hashlib.sha256(slot).hexdigest() == output["checksum"]
        transport.send({"v": 1, "type": "close"})
        code, stdout, stderr = _finish(process)
        assert code == 0, (code, stderr[-500:])
        report = json.loads(stdout.strip())
        assert report["status"] == "render_completed"
        assert report["pixel_format"] == "argb32f"
        assert report["case_id"] == "request_cpu"
        assert report["session_frames_attempted"] == 1
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)


def test_smart_session_survives_a_frame_local_time_scale_mismatch():
    _require_artifacts()
    transport = SessionTransport()
    process = _spawn(transport)
    try:
        transport.write_input(31, 1)
        transport.send(render_frame_message(0, 0, scale=TIME_SCALE + 1))
        rejected = transport.receive()
        assert rejected["status"] == "error"
        assert rejected["render_error"] == -40
        assert "output" not in rejected
        transport.send(render_frame_message(0, 0))
        done = transport.receive()
        assert done["status"] == "ok", done
        transport.send({"v": 1, "type": "close"})
        code, stdout, _ = _finish(process)
        assert code == 0
        report = json.loads(stdout.strip())
        assert report["session_render_error"] == 0
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)


def test_smart_session_fails_closed_on_a_result_larger_than_the_session():
    _require_artifacts()
    transport = SessionTransport()
    process = _spawn(transport)
    try:
        transport.write_input(93, 1)
        transport.send(render_frame_message(0, EXTRA_PIXELS_TIME))
        rejected = transport.receive()
        assert rejected is not None, "worker closed the response pipe early"
        assert rejected["status"] == "error"
        # kSessionDimensionMismatch: v1 requires every frame at the launch
        # dimensions, and the probe's extra-pixels scenario publishes a
        # larger result the slot layout cannot admit.
        assert rejected["render_error"] == -44
        code, stdout, stderr = _finish(process)
        assert code == EXIT_INVARIANT_FAILURE, (code, stderr[-500:])
        report = json.loads(stdout.strip())
        assert report["session_invariant_failure"] is True
        assert report["status"] == "render_failed"
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)


def test_session_commands_are_bound_to_their_worker_kind():
    _require_artifacts()
    if not RENDER_WORKER.is_file():
        pytest.skip("aex_render_worker.exe is not built; run the minihost build")
    # The render worker must not accept the smart session command and the
    # smart worker must not accept the classic one; both exit with the
    # command-rejection code before any session transport is touched.
    for binary, command in ((RENDER_WORKER, "--smart-session-v1"),
                            (WORKER, "--render-session-v1")):
        transport = SessionTransport()
        process = _spawn(transport, command=command, worker=binary)
        code, _, _ = _finish(process)
        assert code == 2, (binary.name, command, code)

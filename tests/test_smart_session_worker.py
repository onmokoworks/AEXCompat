"""Behavioral self-tests for the SmartFX resident session frame loop (v1.1).

Drives ``aex_worker.exe --kind smart --smart-session-v1`` over the same transport as
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
    probe_variant,
    EXIT_INVARIANT_FAILURE,
    HEIGHT,
    OUTPUT_GENERATION_OFFSET,
    SessionTransport,
    TIME_SCALE,
    WIDTH,
    render_frame_message,
)

ROOT = Path(__file__).resolve().parents[1]
WORKER = ROOT / "target" / "minihost-build" / "aex_worker.exe"
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
        pytest.skip("aex_worker.exe is not built; run the minihost build")
    if not PROBE.is_file():
        pytest.skip("pf_smart_geometry_probe.aex is not built")


def _spawn(transport, command="--smart-session-v1", kind="smart", variant=None):
    # `variant` names a depth-advertisement variant of the same probe; the
    # probe selects it from a marker in its own file name, so the variant
    # travels with the plug-in the run loaded instead of sitting in an ambient
    # environment variable every other test would inherit.
    probe = PROBE if variant is None else probe_variant(variant, PROBE)
    aex_sha = hashlib.sha256(probe.read_bytes()).hexdigest()
    process = subprocess.Popen(
        [str(WORKER), "--kind", kind, command, str(probe), aex_sha, "v2|",
         str(WIDTH), str(HEIGHT), "1", str(TOTAL_TIME), str(TIME_SCALE)],
        cwd=ROOT, env=transport.environment(), close_fds=False,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    transport.attach_process(process)
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
            assert output["packed_bytes"] == len(slot)
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
        assert output["packed_bytes"] == len(slot)
        transport.send({"v": 1, "type": "close"})
        code, stdout, stderr = _finish(process)
        assert code == 0, (code, stderr[-500:])
        report = json.loads(stdout.strip())
        assert report["status"] == "render_completed"
        assert report["pixel_format"] == "argb32f"
        assert report["case_id"] == "request_cpu"
        assert report["session_frames_attempted"] == 1
        # The advertised path: the plug-in itself saw float32 worlds. Asserted
        # so this test cannot quietly become the narrowed one - the slot is
        # argb32f either way.
        assert report["advertised_depth_supported"] is True
        assert report["dispatch_pixel_bytes"] == 16
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)


def test_smart_session_narrows_a_plug_in_that_does_not_advertise_the_depth():
    """After Effects does not refuse an effect that lacks DEEP_COLOR_AWARE in a
    deep project. A plug-in advertising neither deep depth has only 8 bits to
    be handed, so it renders at 8-bit precision inside a 32-bpc session rather
    than not at all (this host's rule; AE is measured only on the float-above-
    16 case, docs/DEPTH_FALLBACK_OBSERVATION_2026-09-17.md). The session slot,
    the frame message and the final report all describe the frame at the
    session's depth - the narrowing is between the host and the plug-in, and a
    caller reading the slot must not have to know it happened.
    """
    _require_artifacts()
    transport = SessionTransport(depth_code=32, output_pixel_bytes=16)
    process = _spawn(transport, command="--smart-session32-cpu-v1",
                     variant="shallow")
    try:
        transport.write_input(57, 1)
        transport.send(render_frame_message(0, 0))
        done = transport.receive()
        assert done is not None, "worker closed the response pipe early"
        assert done["status"] == "ok", json.dumps(done)
        output = done["output"]
        # The slot is float32 even though the plug-in rendered 8-bit.
        assert output["pixel_format"] == "argb32f"
        assert output["rowbytes"] == WIDTH * 16
        slot = transport.output_bytes(WIDTH * HEIGHT * 16)
        assert output["packed_bytes"] == len(slot)
        transport.send({"v": 1, "type": "close"})
        code, stdout, stderr = _finish(process)
        assert code == 0, (code, stderr[-500:])
        report = json.loads(stdout.strip())
        assert report["status"] == "render_completed"
        # The final report describes the same frame as the message above.
        assert report["pixel_format"] == "argb32f"
        assert report["rowbytes"] == WIDTH * 16
        assert report["bytes_written_per_row"] == WIDTH * 16
        assert report["undefined_tail_bytes_per_row"] == 0
        # ... and records that the plug-in itself never saw that depth.
        assert report["advertised_depth_supported"] is False
        assert report["depth_supported"] is True
        assert report["dispatch_pixel_bytes"] == 4
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)


def test_smart_session_dispatches_a_float_only_plug_in_at_float32():
    """The smart twin of the classic float-only case, and the configuration
    measured against After Effects: a SmartFX effect with FLOAT_COLOR_AWARE and
    without DEEP_COLOR_AWARE in a 16-bpc project. It is handed float32 worlds
    and the frame is narrowed into the 16-bit slot.
    """
    _require_artifacts()
    transport = SessionTransport(depth_code=16, output_pixel_bytes=8)
    process = _spawn(transport, command="--smart-session16-v1",
                     variant="floatonly")
    try:
        transport.write_input(57, 1)
        transport.send(render_frame_message(0, 0))
        done = transport.receive()
        assert done is not None, "worker closed the response pipe early"
        assert done["status"] == "ok", json.dumps(done)
        output = done["output"]
        assert output["pixel_format"] == "argb16"
        assert output["rowbytes"] == WIDTH * 8
        slot = transport.output_bytes(WIDTH * HEIGHT * 8)
        assert output["packed_bytes"] == len(slot)
        transport.send({"v": 1, "type": "close"})
        code, stdout, stderr = _finish(process)
        assert code == 0, (code, stderr[-500:])
        report = json.loads(stdout.strip())
        assert report["status"] == "render_completed"
        assert report["pixel_format"] == "argb16"
        assert report["rowbytes"] == WIDTH * 8
        assert report["undefined_tail_bytes_per_row"] == 0
        assert report["advertised_depth_supported"] is False
        assert report["depth_supported"] is True
        assert report["dispatch_pixel_bytes"] == 16
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)


def test_smart_session_depth_follows_global_setup_not_a_later_rewrite():
    """`out_data` is one buffer that every selector writes into and nothing
    restores between frames, so a plug-in that assigns rather than ORs its
    out-flags in PARAMS_SETUP erases what GLOBAL_SETUP advertised. The depth a
    session hands the plug-in its worlds in is decided by the advertisement,
    not by whatever is in that buffer when a frame starts: a host that reads it
    live would dispatch this probe at 8 bits while its report still says it
    advertised float32.
    """
    _require_artifacts()
    transport = SessionTransport(depth_code=32, output_pixel_bytes=16)
    process = _spawn(transport, command="--smart-session32-cpu-v1",
                     variant="rewrite")
    try:
        transport.write_input(57, 1)
        transport.send(render_frame_message(0, 0))
        done = transport.receive()
        assert done is not None, "worker closed the response pipe early"
        assert done["status"] == "ok", json.dumps(done)
        assert done["output"]["pixel_format"] == "argb32f"
        transport.send({"v": 1, "type": "close"})
        code, stdout, stderr = _finish(process)
        assert code == 0, (code, stderr[-500:])
        report = json.loads(stdout.strip())
        assert report["status"] == "render_completed"
        assert report["advertised_depth_supported"] is True
        # 16, not 4: the rewrite in PARAMS_SETUP did not move the dispatch.
        assert report["dispatch_pixel_bytes"] == 16
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)


def test_smart_session_frames_observe_their_own_current_time():
    _require_artifacts()
    transport = SessionTransport()
    process = _spawn(transport)
    try:
        # Frame 0 renders the full-frame scenario at time 0. Frame 1 moves to
        # time 1, whose probe scenario publishes a larger-than-session result:
        # reaching -44 on frame 1 requires the probe to have read the NEW
        # current_time from in_data (the smart runtime reseeds in_data's
        # timing fields on every frame, before FRAME_SETUP). A stale first
        # frame's time would keep the full-frame scenario and answer ok.
        transport.write_input(11, 1)
        transport.send(render_frame_message(0, 0))
        first = transport.receive()
        assert first["status"] == "ok", first
        transport.write_input(50, 2)
        transport.send(render_frame_message(1, EXTRA_PIXELS_TIME))
        second = transport.receive()
        assert second["status"] == "ok", second
        assert second["output"]["width"] == WIDTH + 4
        assert second["output"]["height"] == HEIGHT + 4
        transport.send({"v": 1, "type": "close"})
        code, _, stderr = _finish(process)
        assert code == 0, (code, stderr[-500:])
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


def test_smart_session_grows_for_a_result_larger_than_the_session():
    _require_artifacts()
    transport = SessionTransport()
    process = _spawn(transport)
    try:
        transport.write_input(93, 1)
        transport.send(render_frame_message(0, EXTRA_PIXELS_TIME))
        done = transport.receive()
        assert done is not None, "worker closed the response pipe early"
        assert done["status"] == "ok", done
        assert done["output"]["width"] == WIDTH + 4
        assert done["output"]["height"] == HEIGHT + 4
        transport.send({"v": 1, "type": "close"})
        code, stdout, stderr = _finish(process)
        assert code == 0, (code, stderr[-500:])
        report = json.loads(stdout.strip())
        assert report["session_invariant_failure"] is False
        assert report["status"] == "render_completed"
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)


def test_session_commands_are_bound_to_their_worker_kind():
    _require_artifacts()
    # The classic kind must not accept the smart session command and the
    # smart kind must not accept the classic one; both exit with the
    # command-rejection code before any session transport is touched.
    for kind, command in (("classic", "--smart-session-v1"),
                          ("smart", "--render-session-v1")):
        transport = SessionTransport()
        process = _spawn(transport, command=command, kind=kind)
        code, _, _ = _finish(process)
        assert code == 2, (kind, command, code)

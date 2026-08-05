"""A SmartFX plug-in must be able to read its parameters on any frame (#828).

The smart path answers parameter checkouts from a hosted ledger whose frame
time nothing configured, so the ledger admitted t=0 and refused every other
time with PF_Err_OUT_OF_MEMORY. Adobe's Displacement checks a parameter out
from SMART_RENDER, so it rendered at the timeline origin and nowhere else.

``pf_smart_param_time_probe.aex`` checks one slider out at
``in_data->current_time`` from QUERY_DYNAMIC_FLAGS, SMART_PRE_RENDER and
SMART_RENDER, and returns the host's own error when a checkout fails. Rendering
it away from t=0 is what pins the fix: the native gate self-test
(``tests/test_param_checkout_time.py``) can only show what the ledger answers
once configured, not that anything configures it.
"""
import hashlib
import json
import os
import subprocess
from pathlib import Path

import pytest

from test_render_session_worker import (
    HEIGHT,
    SessionTransport,
    TIME_SCALE,
    WIDTH,
    render_frame_message,
)

ROOT = Path(__file__).resolve().parents[1]
WORKER = ROOT / "target" / "minihost-build" / "aex_smart_worker.exe"
PROBE = (
    ROOT / "target" / "pf-smart-param-time-probe-build" / "Release"
    / "pf_smart_param_time_probe.aex"
)

pytestmark = pytest.mark.skipif(os.name != "nt", reason="session transport is Windows-only")

# Frame 0 is the one time the unconfigured ledger already answered, so it is
# kept as the control: the regression is only visible against the others.
FRAME_TIMES = (0, 1, 34, 297)
TOTAL_TIME = 300


def _require_artifacts():
    if not WORKER.is_file():
        pytest.skip("aex_smart_worker.exe is not built; run the minihost build")
    if not PROBE.is_file():
        pytest.skip("pf_smart_param_time_probe.aex is not built; "
                    "run tools/build-pf-smart-param-time-probe.ps1")


def _spawn(transport):
    aex_sha = hashlib.sha256(PROBE.read_bytes()).hexdigest()
    process = subprocess.Popen(
        [str(WORKER), "--smart-session-v1", str(PROBE), aex_sha, "v2|",
         str(WIDTH), str(HEIGHT), "1", str(TOTAL_TIME), str(TIME_SCALE)],
        cwd=ROOT, env=transport.environment(), close_fds=False,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    transport.attach_process(process)
    transport.close_child_ends()
    return process


def test_smart_frames_can_check_parameters_out_at_their_own_time():
    _require_artifacts()
    transport = SessionTransport()
    process = _spawn(transport)
    try:
        for frame_index, time_value in enumerate(FRAME_TIMES):
            transport.write_input(frame_index * 7 + 11, frame_index + 1)
            transport.send(render_frame_message(frame_index, time_value))
            done = transport.receive()
            assert done is not None, "worker closed the response pipe early"
            assert done["type"] == "frame_done"
            # Before #828 every frame past the first came back as an error. The
            # session collapses the QUERY_DYNAMIC_FLAGS refusal (-5) and the
            # SMART_PRE_RENDER / SMART_RENDER refusal (4, the host's own error
            # returned by the plug-in) into this one render_error, so the frame
            # reports "error" whichever of the three checkouts was reached first.
            assert done["status"] == "ok", (time_value, done)
            assert done["render_error"] == 0, (time_value, done)
        transport.send({"v": 1, "type": "close"})
        stdout, stderr = process.communicate(timeout=60)
        assert process.returncode == 0, (process.returncode, stderr[-500:])
        report = json.loads(stdout.strip())
        assert report["session_render_error"] == 0
        assert report["session_frames_attempted"] == len(FRAME_TIMES)
        assert report["session_invariant_failure"] is False
        # The last frame's three checkouts were all checked back in: the fix
        # opens the gate, it does not leak the definitions that pass through it.
        # The counters are per frame - prepare_parameters zeroes them at the top
        # of every one - so this is the final frame's balance, not the session's.
        assert report["param_checkouts_balanced"] is True
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)

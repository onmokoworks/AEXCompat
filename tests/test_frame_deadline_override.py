import json
import os
import subprocess
from pathlib import Path

from PIL import Image

from _render_session import HARNESS

ROOT = Path(__file__).resolve().parents[1]
CRASH_KIT = (ROOT / "target" / "classic-failure-probes-build" / "pf-crashkit" / "Release"
             / "pf_crashkit.aex")
# pf_crashkit slot 1 is its "Fault mode" popup; 3 selects the render hang.
HANG = ["1", "3"]


def _render_hang(tmp_path: Path, deadline: str):
    session_input = tmp_path / "input.png"
    Image.new("RGBA", (8, 4), (1, 2, 3, 255)).save(session_input)
    environment = dict(os.environ, AEXCOMPAT_FRAME_DEADLINE_MS=deadline)
    return subprocess.run(
        [str(HARNESS), "--headless", "--render-experimental-session-param", str(CRASH_KIT),
         str(session_input), str(tmp_path / "output.png"), "argb8", "classic", "0", "1",
         "1", *HANG],
        cwd=ROOT,
        env=environment,
        text=True,
        encoding="utf-8",
        errors="replace",
        capture_output=True,
        timeout=120,
    )


def test_frame_deadline_override_reaches_the_cli_session_route(tmp_path):
    # A render that never returns is cut off at the overridden deadline rather
    # than the 30 s default (issue #1769). This drives the CLI's length-one
    # session; the GUI's resident session reads the same resolver but has no
    # behavioral test here.
    assert HARNESS.is_file() and CRASH_KIT.is_file()
    completed = _render_hang(tmp_path, "2000")
    assert completed.returncode != 0
    assert "invalidated (frame_deadline)" in completed.stderr, completed.stderr
    assert "exceeded the 2000ms deadline" in completed.stderr, completed.stderr


def test_malformed_frame_deadline_override_fails_instead_of_defaulting(tmp_path):
    assert HARNESS.is_file() and CRASH_KIT.is_file()
    completed = _render_hang(tmp_path, "30s")
    # Rejected at CLI start, before any worker runs, as a structured headless
    # configuration error.
    assert completed.returncode == 64, completed.stderr
    failure = json.loads(completed.stderr)
    assert failure["classification"] == "configuration_error", failure
    assert failure["failure_stage"] == "environment_validation", failure
    assert "AEXCOMPAT_FRAME_DEADLINE_MS must be a whole number of milliseconds" in (
        failure["message"]), failure

"""The classic lifecycle must not dispatch RENDER into a refused frame.

`classic_execution::begin_lifecycle` used to hand the caller a clean
`LifecycleResult` even when the plug-in's own SEQUENCE_SETUP or FRAME_SETUP had
refused, because `begin` returns the lifecycle opaquely and nothing read the
refusal back out. `dispatch_render`'s short-circuit keys on that error, so it
did not fire and RENDER ran against frame-local state `begin_frame` returns
before transferring.

Eight AE 2026 effects (PSL_Drop_Shadow, Basic_3D, PSL_Inner_Glow,
PSL_Inner_Shadow, PSL_Outer_Glow, Bulge, Spherize, Corner_Pin) took an access
violation in RENDER that way, which masked the FRAME_SETUP fault underneath:
`last_seh_selector` reported RENDER because the second crash overwrote the
first. Issue #725.

The native self-test drives the same functions with counting fakes, so it needs
no plug-in and no AEX - only the build.
"""

import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SELFTEST = ROOT / "target" / "minihost-build" / "worker_classic_execution_selftest.exe"


def test_a_refused_setup_stops_the_classic_dispatch():
    assert SELFTEST.exists(), f"missing self-test binary: {SELFTEST}"
    completed = subprocess.run(
        [str(SELFTEST)],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert completed.returncode == 0, completed.stderr
    report = json.loads(completed.stdout)
    assert report["classic_execution_selftest"] == "passed"

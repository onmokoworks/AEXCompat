"""Parameter checkouts must be answerable at the frame the host is rendering.

`checkout_param` refuses a checkout whose time is not the frame's unless the
plug-in advertised wide time input. The smart path serves checkouts from the
hosted ledger rather than from a classic dispatch context, and nothing set that
ledger's frame time: it kept `current_time = 0` / `current_time_scale = 1`, so
the gate admitted t=0 and refused every other time.

Every SmartFX frame past t=0 therefore had its first parameter checkout answered
with 4, which the plug-in returned as PF_Err_OUT_OF_MEMORY. AviUtl2 renders at
the timeline cursor, so no smart effect rendered anywhere but frame 0 - the
symptom that survived the two fixes in issue #777 (issue #828).

The native self-test drives the gate directly, so it needs no plug-in and no
AEX - only the build.
"""

import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "target" / "minihost-build"
NAME = "worker_param_checkout_time_selftest.exe"
# Ninja puts the binary flat; a multi-config generator puts it under the config
# directory. CI uses Ninja, so the flat path comes first.
CANDIDATES = [BUILD / NAME, BUILD / "Release" / NAME, BUILD / "RelWithDebInfo" / NAME]


def locate() -> Path:
    for candidate in CANDIDATES:
        if candidate.exists():
            return candidate
    raise AssertionError(
        "missing self-test binary, looked in: "
        + ", ".join(str(candidate) for candidate in CANDIDATES)
    )


def test_a_checkout_is_answerable_at_the_frame_being_rendered():
    selftest = locate()
    completed = subprocess.run(
        [str(selftest)],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert completed.returncode == 0, completed.stderr
    report = json.loads(completed.stdout)
    assert report["param_checkout_time_selftest"] == "passed"

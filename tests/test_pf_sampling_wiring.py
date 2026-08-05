"""The sampling callbacks must accept a null effect_ref, and the utility table
must carry the 16-bit sampling pair.

Both defects were found together: AE's own Displacement could not render through
the SmartFX route at any depth (issue #777).

At 8 and 32 bits its SMART_RENDER pixel function called PF_SUBPIXEL_SAMPLE with
a null `effect_ref` - the host's `in_data->effect_ref` is populated, the plug-in
simply does not pass it - and the host refused with 4, which the plug-in
returned as its own PF_Err_OUT_OF_MEMORY.

At 16 bits it went further and access-violated at address 0, because
`subpixel_sample16` and `area_sample16` were never written into
`in_data->utils`. PF_UtilCallbacks places them between `host_resize_handle`
(offset 464) and `fill16` (488); slots 472 and 480 were left null.

The native self-test drives the sampling entry points and the bootstrap's table
installer directly, so it needs no plug-in and no AEX - only the build.
"""

import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "target" / "minihost-build"
NAME = "worker_pf_sampling_wiring_selftest.exe"
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


def test_sampling_accepts_a_null_effect_ref_and_the_16_bit_slots_are_wired():
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
    assert report["pf_sampling_wiring_selftest"] == "passed"

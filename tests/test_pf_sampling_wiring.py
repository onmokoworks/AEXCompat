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
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "target" / "minihost-build"
NAME = "worker_pf_sampling_wiring_selftest.exe"
# Ninja puts the binary flat; a multi-config generator puts it under the config
# directory. CI uses Ninja, so the flat path comes first.
CANDIDATES = [BUILD / NAME, BUILD / "Release" / NAME, BUILD / "RelWithDebInfo" / NAME]


def locate() -> Path | None:
    for candidate in CANDIDATES:
        if candidate.exists():
            return candidate
    return None


def build_portable_selftest(tmp_path: Path) -> Path:
    compiler = shutil.which("clang++") or shutil.which("c++")
    assert compiler is not None, "a C++17 compiler is required for the portable self-test"
    output = tmp_path / "worker_pf_sampling_wiring_selftest"
    subprocess.run(
        [
            compiler,
            "-std=c++17",
            "-D__cdecl=",
            "-Dstrnlen_s=strnlen",
            "-I",
            str(ROOT / "minihost" / "src"),
            str(ROOT / "tests" / "native" / "worker_pf_sampling_wiring_selftest.cpp"),
            str(ROOT / "minihost" / "src" / "worker_pf_sampling_runtime.cpp"),
            str(ROOT / "minihost" / "src" / "worker_effect_bootstrap.cpp"),
            "-o",
            str(output),
        ],
        check=True,
        cwd=ROOT,
    )
    return output


def test_sampling_accepts_a_null_effect_ref_and_the_utility_slots_are_wired(tmp_path):
    selftest = locate()
    if selftest is None:
        assert sys.platform != "win32", (
            "missing native self-test binary, looked in: "
            + ", ".join(str(candidate) for candidate in CANDIDATES)
        )
        selftest = build_portable_selftest(tmp_path)
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
    sampling = report["callback_diagnostics"]["sampling"]
    assert sampling["calls"] == sampling["successes"] + sampling["failures"]
    assert sampling["denials"]["invalid_arguments"] >= 2
    assert sampling["denials"]["missing_world"] >= 1

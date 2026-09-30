"""A plug-in-owned thread must be able to resolve the world the host handed it.

PSL_Adjustments hands its SmartFX body to `U_SuspendContext::CallOnThreadedExecutor`
and asks `PF_GetPixelFormat` about the host's own output world from a
"COR PSL Thread". The dispatch-world registry it resolves through was
`thread_local`, so that thread found an empty stack and the host answered
PF_Err_OUT_OF_MEMORY, which the plug-in returned as its own (issue #1299).

Reaching across threads may not relax the registry: the self-test also drives
the refusals - a geometry mismatch, a registration retargeted at another
registration's pixels, a layout two threads answer differently, a reference two
threads answer differently, and a dispatch that has already ended - and requires
them to stay refusals.

The self-test binary is preferred when the worker build produced one, because
that is the MSVC/`/EHsc` toolchain the workers themselves use; otherwise it is
compiled here with whatever portable C++17 compiler is around, so a checkout
with no minihost build still runs it.
"""

import json
import shutil
import subprocess
from pathlib import Path

import pytest

from _native_selftest import ROOT, locate_optional

NAME = "worker_cross_thread_dispatch_world_selftest.exe"
SOURCES = (
    Path("tests") / "native" / "worker_cross_thread_dispatch_world_selftest.cpp",
    Path("minihost") / "src" / "worker_world_safety.cpp",
    # world_safety's register_world gives each handed-out world AE's PF_World
    # shape behind reserved_long4 (issue #1276).
    Path("minihost") / "src" / "worker_pf_world_facade.cpp",
)


def _build_portable(tmp_path: Path) -> Path:
    compiler = shutil.which("clang++") or shutil.which("c++")
    if compiler is None:
        pytest.skip(
            "no self-test binary in the minihost build and no portable C++17 compiler"
        )
    output = tmp_path / "worker_cross_thread_dispatch_world_selftest"
    subprocess.run(
        [compiler, "-std=c++17", "-D__cdecl=", "-I", str(ROOT / "minihost" / "src")]
        + [str(ROOT / source) for source in SOURCES]
        + ["-o", str(output)],
        check=True,
        cwd=ROOT,
    )
    return output


def test_a_plugin_owned_thread_resolves_the_hosts_world(tmp_path):
    selftest = locate_optional(NAME) or _build_portable(tmp_path)
    completed = subprocess.run(
        [str(selftest)], cwd=ROOT, capture_output=True, text=True, timeout=60
    )
    assert completed.returncode == 0, completed.stderr
    assert json.loads(completed.stdout) == {"cross_thread_dispatch_world": "passed"}

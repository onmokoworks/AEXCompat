"""PF callback argument errors must not masquerade as allocation failures."""

import json
import shutil
import subprocess
from pathlib import Path

import pytest

from _compile_cache import compile_and_link


ROOT = Path(__file__).resolve().parents[1]


def test_world_transform_distinguishes_bad_arguments_from_allocation_failure(tmp_path):
    compiler = shutil.which("clang++") or shutil.which("c++")
    if compiler is None:
        pytest.skip("a portable C++17 compiler is unavailable")
    output = tmp_path / "worker_pf_bad_callback_param_selftest"
    compile_and_link(
        compiler,
        [
            ROOT / "tests" / "native" / "worker_pf_bad_callback_param_selftest.cpp",
            ROOT / "minihost" / "src" / "worker_pf_world_transform_runtime.cpp",
            ROOT / "minihost" / "src" / "worker_world_safety.cpp",
            # world_safety's register_world gives each handed-out world AE's
            # PF_World shape behind reserved_long4 (issue #1276).
            ROOT / "minihost" / "src" / "worker_pf_world_facade.cpp",
        ],
        output,
        compile_args=(
            "-std=c++17",
            "-D__cdecl=",
            "-I",
            str(ROOT / "minihost" / "src"),
        ),
        cwd=ROOT,
    )
    completed = subprocess.run(
        [str(output)], cwd=ROOT, capture_output=True, text=True, timeout=30
    )
    assert completed.returncode == 0, completed.stderr
    assert json.loads(completed.stdout) == {"pf_bad_callback_param": "passed"}

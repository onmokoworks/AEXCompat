"""PF callback argument errors must not masquerade as allocation failures."""

import json
import shutil
import subprocess
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]


def test_world_transform_distinguishes_bad_arguments_from_allocation_failure(tmp_path):
    compiler = shutil.which("clang++") or shutil.which("c++")
    if compiler is None:
        pytest.skip("a portable C++17 compiler is unavailable")
    output = tmp_path / "worker_pf_bad_callback_param_selftest"
    subprocess.run(
        [
            compiler,
            "-std=c++17",
            "-D__cdecl=",
            "-I",
            str(ROOT / "minihost" / "src"),
            str(ROOT / "tests" / "native" / "worker_pf_bad_callback_param_selftest.cpp"),
            str(ROOT / "minihost" / "src" / "worker_pf_world_transform_runtime.cpp"),
            str(ROOT / "minihost" / "src" / "worker_world_safety.cpp"),
            "-o",
            str(output),
        ],
        check=True,
        cwd=ROOT,
    )
    completed = subprocess.run(
        [str(output)], cwd=ROOT, capture_output=True, text=True, timeout=30
    )
    assert completed.returncode == 0, completed.stderr
    assert json.loads(completed.stdout) == {"pf_bad_callback_param": "passed"}


def test_all_three_pf_callback_translation_units_use_the_sdk_error_code():
    sampling = (ROOT / "minihost/src/worker_pf_sampling_runtime.cpp").read_text(
        encoding="utf-8"
    )
    suites = (ROOT / "minihost/src/worker_pf_suites.cpp").read_text(encoding="utf-8")
    world_transform = (
        ROOT / "minihost/src/worker_pf_world_transform_runtime.cpp"
    ).read_text(encoding="utf-8")

    assert "constexpr int32_t kPfBadCallbackParam = 516;" in sampling
    assert "constexpr int32_t kPfErrBadCallbackParam = 516;" in suites
    assert "constexpr int32_t kPfErrBadCallbackParam = 516;" in world_transform
    assert "constexpr int32_t kPfBadCallbackParam = 4;" not in (
        sampling + suites + world_transform
    )
    assert "iterate_generic(0, &state, generic_callback) != kPfErrBadCallbackParam" in suites

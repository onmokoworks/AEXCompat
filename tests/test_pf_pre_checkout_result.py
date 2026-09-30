import os
import subprocess
import tempfile
from pathlib import Path

from _native_selftest import worker_self_test

import pytest

from _msvc_compile import compile_driver

ROOT = Path(__file__).resolve().parents[1]
SDK_ROOT = os.environ.get("AFTER_EFFECTS_SDK_ROOT")
HEADERS = Path(SDK_ROOT) / "Examples" / "Headers" if SDK_ROOT else None


def _sdk_headers() -> Path:
    if HEADERS is None or not HEADERS.is_dir():
        pytest.skip("set AFTER_EFFECTS_SDK_ROOT to a valid After Effects SDK root")
    return HEADERS


def test_sdk_frozen_checkout_result_abi_compiles() -> None:
    headers = _sdk_headers()
    source = r"""
#include <cstddef>
#include "AEConfig.h"
#include "AE_Effect.h"

static_assert(sizeof(PF_CheckoutResult) == 76);
static_assert(offsetof(PF_CheckoutResult, result_rect) == 0);
static_assert(offsetof(PF_CheckoutResult, max_result_rect) == 16);
static_assert(offsetof(PF_CheckoutResult, par) == 32);
static_assert(offsetof(PF_CheckoutResult, solid) == 40);
static_assert(offsetof(PF_CheckoutResult, ref_width) == 44);
static_assert(offsetof(PF_CheckoutResult, ref_height) == 48);
static_assert(offsetof(PF_CheckoutResult, reserved) == 52);
static_assert(sizeof(PF_RationalScale) == 8);
int main() { return 0; }
"""
    program_files_x86 = os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")
    vswhere = (
        Path(program_files_x86)
        / "Microsoft Visual Studio"
        / "Installer"
        / "vswhere.exe"
    )
    installation = subprocess.check_output(
        [
            str(vswhere),
            "-latest",
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-property",
            "installationPath",
        ],
        text=True,
    ).strip()
    vcvars = Path(installation) / "VC" / "Auxiliary" / "Build" / "vcvars64.bat"
    with tempfile.TemporaryDirectory() as directory:
        cpp = Path(directory) / "pf_checkout_result_abi.cpp"
        obj = Path(directory) / "pf_checkout_result_abi.obj"
        batch = Path(directory) / "compile.bat"
        cpp.write_text(source, encoding="ascii")
        batch.write_text(
            f'@call "{vcvars}" >nul\n'
            f'@{compile_driver()} /nologo /std:c++17 /DWIN32 /D_WINDOWS /c /I"{headers}" '
            f'/I"{headers / "SP"}" /Fo"{obj}" "{cpp}"\n',
            encoding="ascii",
        )
        subprocess.run(["cmd", "/d", "/c", str(batch)], check=True, timeout=120)


def test_native_self_test_passes_all_three_workers() -> None:
    expected = {"pf_pre_checkout_result": "passed"}
    for report in worker_self_test("--self-test-pf-pre-checkout-result").values():
        assert report == expected

import json
import os
import subprocess
import tempfile
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"
SMART_RUNTIME_SOURCE = ROOT / "minihost" / "src" / "worker_smart_runtime.cpp"
BUILD = ROOT / "target" / "minihost-build"
SDK_ROOT = os.environ.get("AFTER_EFFECTS_SDK_ROOT")
HEADERS = Path(SDK_ROOT) / "Examples" / "Headers" if SDK_ROOT else None


def _sdk_headers() -> Path:
    if HEADERS is None or not HEADERS.is_dir():
        pytest.skip("set AFTER_EFFECTS_SDK_ROOT to a valid After Effects SDK root")
    return HEADERS


def test_sdk_frozen_checkout_result_abi_compiles() -> None:
    headers = _sdk_headers()
    source = r'''
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
'''
    program_files_x86 = os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")
    vswhere = Path(program_files_x86) / "Microsoft Visual Studio" / "Installer" / "vswhere.exe"
    installation = subprocess.check_output(
        [str(vswhere), "-latest", "-products", "*", "-requires",
         "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"],
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
            f'@cl /nologo /std:c++17 /DWIN32 /D_WINDOWS /c /I"{headers}" '
            f'/I"{headers / "SP"}" /Fo"{obj}" "{cpp}"\n',
            encoding="ascii",
        )
        subprocess.run(["cmd", "/d", "/c", str(batch)], check=True, timeout=120)


def test_l2_source_writes_full_checkout_result() -> None:
    source = SOURCE.read_text(encoding="utf-8")
    runtime_source = SMART_RUNTIME_SOURCE.read_text(encoding="utf-8")
    for marker in (
        "constexpr std::size_t kCheckoutResultBytes = 76;",
        "void write_checkout_result(void* destination",
        "std::memset(bytes, 0, kCheckoutResultBytes);",
        "const int32_t par[2] = {runtime.pixel_aspect_numerator,",
        "std::memcpy(bytes + 32, par, sizeof(par));",
        "std::memcpy(bytes + 44, reference_size, sizeof(reference_size));",
        'L"--self-test-pf-pre-checkout-result"',
        'L"--self-test-smart-runtime-concurrency"',
        'L"--self-test-smart-result-skipped"',
        "std::make_shared<aexcompat::worker_runtime::smart::Snapshot>()",
    ):
        assert marker in source + runtime_source
    for marker in (
        "runtime.full_resolution_width > 0",
        "write_checkout_result(result, runtime.width, runtime.height,",
        "thread_local State g_default_state;",
        "thread_local State* g_active_state{};",
        "if (!g_active_state || time_step <= 0 || time_scale == 0) return 4;",
        "int32_t __cdecl width() { return g_active_state ? g_active_state->width : 0; }",
    ):
        assert marker in runtime_source
    # No success path may write only the rects and leave par, ref_width, and
    # ref_height uninitialized for the caller.
    assert "write_rect(static_cast<std::byte*>(result) + 16" not in source + runtime_source


def test_native_self_test_passes_all_three_workers() -> None:
    expected = {"pf_pre_checkout_result": "passed"}
    for name in ("aex_l2_worker.exe", "aex_render_worker.exe", "aex_smart_worker.exe"):
        worker = BUILD / name
        assert worker.exists(), f"build {name} before running the native test"
        completed = subprocess.run(
            [str(worker), "--self-test-pf-pre-checkout-result"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert completed.returncode == 0, completed.stderr
        assert json.loads(completed.stdout) == expected

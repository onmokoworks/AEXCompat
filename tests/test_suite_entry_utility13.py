import json
import os
import subprocess
import tempfile
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
SDK_ROOT = os.environ.get("AFTER_EFFECTS_SDK_ROOT")
HEADERS = Path(SDK_ROOT) / "Examples" / "Headers" if SDK_ROOT else None


def _msvc_vcvars() -> Path:
    if os.name != "nt":
        pytest.skip("SDK ABI compile test requires the Windows MSVC toolchain")
    program_files_x86 = os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")
    vswhere = Path(program_files_x86) / "Microsoft Visual Studio" / "Installer" / "vswhere.exe"
    if not vswhere.is_file():
        pytest.skip("vswhere.exe is unavailable; install Visual Studio C++ tools")
    # -utf8 forces UTF-8 output, so decode it as UTF-8 explicitly rather than the
    # process locale (the SDK workflow does not set PYTHONUTF8), else a UTF-8
    # install path with non-ASCII characters would mojibake. errors="replace"
    # still guards any stray CP932 chatter (#237, follow-up to #58).
    installation = subprocess.run(
        [str(vswhere), "-latest", "-products", "*", "-requires",
         "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath",
         "-utf8"],
        capture_output=True, text=True, encoding="utf-8-sig", errors="replace",
    ).stdout.strip()
    if not installation:
        pytest.skip("no Visual Studio installation with the C++ x64 toolset")
    vcvars = Path(installation) / "VC" / "Auxiliary" / "Build" / "vcvars64.bat"
    if not vcvars.is_file():
        pytest.skip("vcvars64.bat is unavailable; install Visual Studio C++ tools")
    return vcvars


def test_sdk_frozen_utility_suite_hwnd_slots_compile() -> None:
    if HEADERS is None or not HEADERS.is_dir():
        pytest.skip("set AFTER_EFFECTS_SDK_ROOT to a valid After Effects SDK root")
    vcvars = _msvc_vcvars()
    source = r'''
#include <cstddef>
#include "AEConfig.h"
#include "AE_GeneralPlug.h"

static_assert(kAEGPUtilitySuiteVersion6 == 13);
static_assert(kAEGPUtilitySuiteVersion3 == 7);
static_assert(sizeof(AEGP_UtilitySuite6) == 33 * sizeof(void*));
static_assert(offsetof(AEGP_UtilitySuite6, AEGP_RegisterWithAEGP) == 9 * sizeof(void*));
static_assert(offsetof(AEGP_UtilitySuite6, AEGP_GetMainHWND) == 10 * sizeof(void*));
static_assert(sizeof(AEGP_UtilitySuite3) == 25 * sizeof(void*));
static_assert(offsetof(AEGP_UtilitySuite3, AEGP_RegisterWithAEGP) == 7 * sizeof(void*));
static_assert(offsetof(AEGP_UtilitySuite3, AEGP_GetMainHWND) == 8 * sizeof(void*));
int main() { return 0; }
'''
    with tempfile.TemporaryDirectory() as directory:
        cpp = Path(directory) / "utility_suite_hwnd_abi.cpp"
        obj = Path(directory) / "utility_suite_hwnd_abi.obj"
        batch = Path(directory) / "compile.bat"
        cpp.write_text(source, encoding="ascii")
        batch.write_text(
            f'@call "{vcvars}" >nul\n'
            f'@cl /nologo /std:c++17 /DWIN32 /D_WINDOWS /c /I"{HEADERS}" '
            f'/I"{HEADERS / "SP"}" /Fo"{obj}" "{cpp}"\n',
            encoding="ascii",
        )
        subprocess.run(["cmd", "/d", "/c", str(batch)], check=True, timeout=120)


def test_suite_entry_guards_and_utility13_native_contract(canonical_release_worker):
    result = subprocess.run(
        [str(canonical_release_worker), "--self-test-suite-entry-utility13"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
        errors="replace",
        timeout=60,
    )
    assert json.loads(result.stdout) == {
        "suite_entry_utility13": "passed",
        "utility_v7_acquired": True,
        "unsupported_slots_diagnosed": True,
        "normal_effect_available": True,
        "versions_12_14_rejected": True,
        "mask_callbacks_exposed": False,
        "suite_leases_balanced": True,
    }

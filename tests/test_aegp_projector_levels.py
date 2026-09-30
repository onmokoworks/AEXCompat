import json
import os
import subprocess
import tempfile
from pathlib import Path

import pytest

from _msvc_compile import compile_driver

ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "target" / "minihost-build"
SDK_ROOT = os.environ.get("AFTER_EFFECTS_SDK_ROOT")
HEADERS = Path(SDK_ROOT) / "Examples" / "Headers" if SDK_ROOT else None


def _sdk_headers() -> Path:
    if HEADERS is None or not HEADERS.is_dir():
        pytest.skip("set AFTER_EFFECTS_SDK_ROOT to a valid After Effects SDK root")
    return HEADERS


def test_sdk_frozen_projector_levels_abi_compiles() -> None:
    headers = _sdk_headers()

    program_files_x86 = os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")
    vswhere = (
        Path(program_files_x86)
        / "Microsoft Visual Studio"
        / "Installer"
        / "vswhere.exe"
    )
    if not vswhere.is_file():
        pytest.skip("Visual Studio discovery tool is not installed")

    installations = subprocess.run(
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
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    if not installations:
        pytest.skip("Visual Studio C++ tools are not installed")
    vcvars = Path(installations) / "VC" / "Auxiliary" / "Build" / "vcvars64.bat"

    source = r"""
#include <cstddef>
#include <type_traits>
#include "AEConfig.h"
#include "AE_Effect.h"
#include "AE_GeneralPlug.h"

using GetNumInstalledEffects = A_Err (SPAPI *)(A_long*);
using GetNextInstalledEffect = A_Err (SPAPI *)(AEGP_InstalledEffectKey,
    AEGP_InstalledEffectKey*);
using GetEffectMatchName = A_Err (SPAPI *)(AEGP_InstalledEffectKey, A_char*);
using ApplyEffect = A_Err (SPAPI *)(AEGP_PluginID, AEGP_LayerH,
    AEGP_InstalledEffectKey, AEGP_EffectRefH*);
using GetEffectNumParamStreams = A_Err (SPAPI *)(AEGP_EffectRefH, A_long*);
using GetNewEffectStreamByIndex = A_Err (SPAPI *)(AEGP_PluginID,
    AEGP_EffectRefH, PF_ParamIndex, AEGP_StreamRefH*);
using DisposeStream = A_Err (SPAPI *)(AEGP_StreamRefH);
using GetStreamName = A_Err (SPAPI *)(AEGP_StreamRefH, A_Boolean, A_char*);
using GetStreamType = A_Err (SPAPI *)(AEGP_StreamRefH, AEGP_StreamType*);
using GetNewStreamValue = A_Err (SPAPI *)(AEGP_PluginID, AEGP_StreamRefH,
    AEGP_LTimeMode, const A_Time*, A_Boolean, AEGP_StreamValue*);
using DisposeStreamValue = A_Err (SPAPI *)(AEGP_StreamValue*);
using SetStreamValue = A_Err (SPAPI *)(AEGP_PluginID, AEGP_StreamRefH,
    AEGP_StreamValue*);

static_assert(kAEGPEffectSuiteVersion2 == 2);
static_assert(sizeof(AEGP_EffectSuite2) == 17 * sizeof(void*));
static_assert(sizeof(AEGP_EffectSuite2) == 136);
static_assert(offsetof(AEGP_EffectSuite2, AEGP_ApplyEffect) == 9 * sizeof(void*));
static_assert(offsetof(AEGP_EffectSuite2, AEGP_GetNumInstalledEffects) == 11 * sizeof(void*));
static_assert(offsetof(AEGP_EffectSuite2, AEGP_GetNextInstalledEffect) == 12 * sizeof(void*));
static_assert(offsetof(AEGP_EffectSuite2, AEGP_GetEffectMatchName) == 14 * sizeof(void*));
static_assert(offsetof(AEGP_EffectSuite2, AEGP_ApplyEffect) == 72);
static_assert(offsetof(AEGP_EffectSuite2, AEGP_GetNumInstalledEffects) == 88);
static_assert(offsetof(AEGP_EffectSuite2, AEGP_GetNextInstalledEffect) == 96);
static_assert(offsetof(AEGP_EffectSuite2, AEGP_GetEffectMatchName) == 112);
static_assert(std::is_same_v<decltype(AEGP_EffectSuite2::AEGP_ApplyEffect), ApplyEffect>);
static_assert(std::is_same_v<decltype(AEGP_EffectSuite2::AEGP_GetNumInstalledEffects), GetNumInstalledEffects>);
static_assert(std::is_same_v<decltype(AEGP_EffectSuite2::AEGP_GetNextInstalledEffect), GetNextInstalledEffect>);
static_assert(std::is_same_v<decltype(AEGP_EffectSuite2::AEGP_GetEffectMatchName), GetEffectMatchName>);

static_assert(kAEGPStreamSuiteVersion2 == 7);
static_assert(sizeof(AEGP_StreamSuite2) == 22 * sizeof(void*));
static_assert(sizeof(AEGP_StreamSuite2) == 176);
static_assert(offsetof(AEGP_StreamSuite2, AEGP_GetEffectNumParamStreams) == 32);
static_assert(offsetof(AEGP_StreamSuite2, AEGP_GetNewEffectStreamByIndex) == 40);
static_assert(offsetof(AEGP_StreamSuite2, AEGP_DisposeStream) == 56);
static_assert(offsetof(AEGP_StreamSuite2, AEGP_GetStreamName) == 64);
static_assert(offsetof(AEGP_StreamSuite2, AEGP_GetStreamType) == 96);
static_assert(offsetof(AEGP_StreamSuite2, AEGP_GetNewStreamValue) == 104);
static_assert(offsetof(AEGP_StreamSuite2, AEGP_DisposeStreamValue) == 112);
static_assert(offsetof(AEGP_StreamSuite2, AEGP_SetStreamValue) == 120);
static_assert(std::is_same_v<decltype(AEGP_StreamSuite2::AEGP_GetEffectNumParamStreams), GetEffectNumParamStreams>);
static_assert(std::is_same_v<decltype(AEGP_StreamSuite2::AEGP_GetNewEffectStreamByIndex), GetNewEffectStreamByIndex>);
static_assert(std::is_same_v<decltype(AEGP_StreamSuite2::AEGP_DisposeStream), DisposeStream>);
static_assert(std::is_same_v<decltype(AEGP_StreamSuite2::AEGP_GetStreamName), GetStreamName>);
static_assert(std::is_same_v<decltype(AEGP_StreamSuite2::AEGP_GetStreamType), GetStreamType>);
static_assert(std::is_same_v<decltype(AEGP_StreamSuite2::AEGP_GetNewStreamValue), GetNewStreamValue>);
static_assert(std::is_same_v<decltype(AEGP_StreamSuite2::AEGP_DisposeStreamValue), DisposeStreamValue>);
static_assert(std::is_same_v<decltype(AEGP_StreamSuite2::AEGP_SetStreamValue), SetStreamValue>);
int main() { return 0; }
"""
    with tempfile.TemporaryDirectory() as directory:
        cpp = Path(directory) / "aegp_projector_levels_abi.cpp"
        obj = Path(directory) / "aegp_projector_levels_abi.obj"
        batch = Path(directory) / "compile.bat"
        cpp.write_text(source, encoding="ascii")
        batch.write_text(
            f'@call "{vcvars}" >nul\n'
            f'@{compile_driver()} /nologo /std:c++17 /DWIN32 /D_WINDOWS /c /I"{headers}" '
            f'/I"{headers / "SP"}" /Fo"{obj}" "{cpp}"\n',
            encoding="ascii",
        )
        subprocess.run(["cmd", "/d", "/c", str(batch)], check=True, timeout=120)


def test_native_projector_levels_self_test_passes_all_present_workers() -> None:
    worker = BUILD / "aex_worker.exe"
    if not worker.is_file():
        pytest.skip("the minihost worker is not present; run the native build first")

    expected = {
        "projector_levels": "passed",
        "catalog": ["ADBE Easy Levels", "ADBE Pro Levels"],
        "stream_suite": {"version": 7, "slots": 22, "size_x64": 176},
        "index_zero_input": True,
        "simultaneous_stream_refs": True,
        "parameters": ["Input", "Input Black", "Input White"],
        "value_roundtrip": True,
        "reverse_dispose": True,
        "fail_closed": True,
    }
    for kind in ("discovery", "classic", "smart"):
        completed = subprocess.run(
            [str(worker), "--kind", kind, "--self-test-aegp-projector-levels"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert completed.returncode == 0, completed.stderr
        assert json.loads(completed.stdout) == expected

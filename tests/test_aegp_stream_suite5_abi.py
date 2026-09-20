import os
import subprocess
import tempfile
from pathlib import Path

import pytest

from _msvc_compile import compile_driver


SDK_ROOT = os.environ.get("AFTER_EFFECTS_SDK_ROOT")
HEADERS = Path(SDK_ROOT) / "Examples" / "Headers" if SDK_ROOT else None


def _sdk_headers() -> Path:
    if HEADERS is None or not HEADERS.is_dir():
        pytest.skip("set AFTER_EFFECTS_SDK_ROOT to a valid After Effects SDK root")
    return HEADERS


def test_sdk_stream_suite5_is_the_exact_stream_suite6_prefix() -> None:
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

static_assert(sizeof(void*) == 8);
static_assert(kAEGPStreamSuiteVersion4 == 9);
static_assert(kAEGPStreamSuiteVersion5 == 10);
static_assert(kAEGPStreamSuiteVersion6 == 11);
static_assert(sizeof(AEGP_StreamSuite5) == 22 * sizeof(void*));
static_assert(sizeof(AEGP_StreamSuite5) == 176);
static_assert(sizeof(AEGP_StreamSuite6) == 23 * sizeof(void*));
static_assert(sizeof(AEGP_StreamSuite6) == 184);

#define CHECK_V5_V6_PREFIX_SLOT(member, slot)                                  \
    static_assert(offsetof(AEGP_StreamSuite5, member) ==                       \
                  (slot) * sizeof(void*));                                     \
    static_assert(offsetof(AEGP_StreamSuite6, member) ==                       \
                  offsetof(AEGP_StreamSuite5, member));                        \
    static_assert(std::is_same_v<decltype(AEGP_StreamSuite5::member),          \
                                 decltype(AEGP_StreamSuite6::member)>)

CHECK_V5_V6_PREFIX_SLOT(AEGP_IsStreamLegal, 0);
CHECK_V5_V6_PREFIX_SLOT(AEGP_CanVaryOverTime, 1);
CHECK_V5_V6_PREFIX_SLOT(AEGP_GetValidInterpolations, 2);
CHECK_V5_V6_PREFIX_SLOT(AEGP_GetNewLayerStream, 3);
CHECK_V5_V6_PREFIX_SLOT(AEGP_GetEffectNumParamStreams, 4);
CHECK_V5_V6_PREFIX_SLOT(AEGP_GetNewEffectStreamByIndex, 5);
CHECK_V5_V6_PREFIX_SLOT(AEGP_GetNewMaskStream, 6);
CHECK_V5_V6_PREFIX_SLOT(AEGP_DisposeStream, 7);
CHECK_V5_V6_PREFIX_SLOT(AEGP_GetStreamName, 8);
CHECK_V5_V6_PREFIX_SLOT(AEGP_GetStreamUnitsText, 9);
CHECK_V5_V6_PREFIX_SLOT(AEGP_GetStreamProperties, 10);
CHECK_V5_V6_PREFIX_SLOT(AEGP_IsStreamTimevarying, 11);
CHECK_V5_V6_PREFIX_SLOT(AEGP_GetStreamType, 12);
CHECK_V5_V6_PREFIX_SLOT(AEGP_GetNewStreamValue, 13);
CHECK_V5_V6_PREFIX_SLOT(AEGP_DisposeStreamValue, 14);
CHECK_V5_V6_PREFIX_SLOT(AEGP_SetStreamValue, 15);
CHECK_V5_V6_PREFIX_SLOT(AEGP_GetLayerStreamValue, 16);
CHECK_V5_V6_PREFIX_SLOT(AEGP_GetExpressionState, 17);
CHECK_V5_V6_PREFIX_SLOT(AEGP_SetExpressionState, 18);
CHECK_V5_V6_PREFIX_SLOT(AEGP_GetExpression, 19);
CHECK_V5_V6_PREFIX_SLOT(AEGP_SetExpression, 20);
CHECK_V5_V6_PREFIX_SLOT(AEGP_DuplicateStreamRef, 21);

static_assert(offsetof(AEGP_StreamSuite6, AEGP_GetUniqueStreamID) == 176);

using GetStreamNameUnicode = A_Err (SPAPI *)(
    AEGP_PluginID, AEGP_StreamRefH, A_Boolean, AEGP_MemHandle*);
using GetExpressionUnicode = A_Err (SPAPI *)(
    AEGP_PluginID, AEGP_StreamRefH, AEGP_MemHandle*);
using SetExpressionUnicode = A_Err (SPAPI *)(
    AEGP_PluginID, AEGP_StreamRefH, const A_UTF16Char*);
using SetExpressionAnsi = A_Err (SPAPI *)(
    AEGP_PluginID, AEGP_StreamRefH, const A_char*);

static_assert(std::is_same_v<decltype(AEGP_StreamSuite5::AEGP_GetStreamName),
                             GetStreamNameUnicode>);
static_assert(std::is_same_v<decltype(AEGP_StreamSuite5::AEGP_GetExpression),
                             GetExpressionUnicode>);
static_assert(std::is_same_v<decltype(AEGP_StreamSuite5::AEGP_SetExpression),
                             SetExpressionUnicode>);
static_assert(std::is_same_v<decltype(AEGP_StreamSuite6::AEGP_SetExpression),
                             SetExpressionUnicode>);
static_assert(std::is_same_v<decltype(AEGP_StreamSuite4::AEGP_SetExpression),
                             SetExpressionAnsi>);
static_assert(!std::is_same_v<decltype(AEGP_StreamSuite5::AEGP_SetExpression),
                              decltype(AEGP_StreamSuite4::AEGP_SetExpression)>);

// The host's bounded value adapter supports the primitive alternatives only.
// Their old/new types and x64 storage agree; marker ownership changed from a
// handle to a pointer and is deliberately not covered by that equivalence.
static_assert(sizeof(AEGP_StreamVal) == 32);
static_assert(sizeof(AEGP_StreamVal2) == 32);
static_assert(sizeof(AEGP_StreamValue) == 40);
static_assert(sizeof(AEGP_StreamValue2) == 40);
static_assert(offsetof(AEGP_StreamValue, streamH) == 0);
static_assert(offsetof(AEGP_StreamValue, val) == 8);
static_assert(offsetof(AEGP_StreamValue2, streamH) == 0);
static_assert(offsetof(AEGP_StreamValue2, val) == 8);
static_assert(std::is_same_v<decltype(AEGP_StreamVal::four_d),
                             decltype(AEGP_StreamVal2::four_d)>);
static_assert(std::is_same_v<decltype(AEGP_StreamVal::three_d),
                             decltype(AEGP_StreamVal2::three_d)>);
static_assert(std::is_same_v<decltype(AEGP_StreamVal::two_d),
                             decltype(AEGP_StreamVal2::two_d)>);
static_assert(std::is_same_v<decltype(AEGP_StreamVal::one_d),
                             decltype(AEGP_StreamVal2::one_d)>);
static_assert(std::is_same_v<decltype(AEGP_StreamVal::color),
                             decltype(AEGP_StreamVal2::color)>);
static_assert(std::is_same_v<decltype(AEGP_StreamVal::layer_id),
                             decltype(AEGP_StreamVal2::layer_id)>);
static_assert(std::is_same_v<decltype(AEGP_StreamVal::mask_id),
                             decltype(AEGP_StreamVal2::mask_id)>);
static_assert(std::is_same_v<decltype(AEGP_StreamVal::markerH),
                             AEGP_MarkerValH>);
static_assert(std::is_same_v<decltype(AEGP_StreamVal2::markerP),
                             AEGP_MarkerValP>);
static_assert(!std::is_same_v<decltype(AEGP_StreamVal::markerH),
                              decltype(AEGP_StreamVal2::markerP)>);
static_assert(sizeof(AEGP_StreamVal::markerH) == sizeof(void*));
static_assert(sizeof(AEGP_StreamVal2::markerP) == sizeof(void*));

int main() { return 0; }
"""
    with tempfile.TemporaryDirectory() as directory:
        cpp = Path(directory) / "aegp_stream_suite5_abi.cpp"
        obj = Path(directory) / "aegp_stream_suite5_abi.obj"
        batch = Path(directory) / "compile.bat"
        cpp.write_text(source, encoding="ascii")
        batch.write_text(
            f'@call "{vcvars}" >nul\n'
            f'@{compile_driver()} /nologo /std:c++17 /DWIN32 /D_WINDOWS /c /I"{headers}" '
            f'/I"{headers / "SP"}" /Fo"{obj}" "{cpp}"\n',
            encoding="ascii",
        )
        subprocess.run(["cmd", "/d", "/c", str(batch)], check=True, timeout=120)

import json
import os
import subprocess
import tempfile
from pathlib import Path

import pytest
import source_owners


ROOT = Path(__file__).resolve().parents[1]
SOURCE = source_owners.L2_MAIN
SCENE_SOURCE = ROOT / "minihost" / "src" / "worker_aegp_scene.cpp"
SCENE_RUNTIME_HEADER = ROOT / "minihost" / "src" / "worker_aegp_scene_runtime.hpp"
SCENE_RUNTIME_SOURCE = ROOT / "minihost" / "src" / "worker_aegp_scene_runtime.cpp"
SCENE_SELFTEST_SOURCE = ROOT / "minihost" / "src" / "worker_aegp_scene_selftests.cpp"
PF_SUITE_SOURCE = ROOT / "minihost" / "src" / "worker_pf_suites_internal.hpp"
SELFTEST_DISPATCH_SOURCE = ROOT / "minihost" / "src" / "worker_selftest_dispatch.cpp"
ENTRY_WIRING_SOURCE = ROOT / "minihost" / "src" / "worker_entry_wiring.cpp"
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
    vswhere = Path(program_files_x86) / "Microsoft Visual Studio" / "Installer" / "vswhere.exe"
    if not vswhere.is_file():
        pytest.skip("Visual Studio discovery tool is not installed")

    installations = subprocess.run(
        [str(vswhere), "-latest", "-products", "*", "-requires",
         "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    if not installations:
        pytest.skip("Visual Studio C++ tools are not installed")
    vcvars = Path(installations) / "VC" / "Auxiliary" / "Build" / "vcvars64.bat"

    source = r'''
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
'''
    with tempfile.TemporaryDirectory() as directory:
        cpp = Path(directory) / "aegp_projector_levels_abi.cpp"
        obj = Path(directory) / "aegp_projector_levels_abi.obj"
        batch = Path(directory) / "compile.bat"
        cpp.write_text(source, encoding="ascii")
        batch.write_text(
            f'@call "{vcvars}" >nul\n'
            f'@cl /nologo /std:c++17 /DWIN32 /D_WINDOWS /c /I"{headers}" '
            f'/I"{headers / "SP"}" /Fo"{obj}" "{cpp}"\n',
            encoding="ascii",
        )
        subprocess.run(["cmd", "/d", "/c", str(batch)], check=True, timeout=120)


def test_l2_source_exposes_projector_levels_contract() -> None:
    source = "\n".join(path.read_text(encoding="utf-8") for path in
                       (SOURCE, SCENE_SOURCE, SCENE_RUNTIME_HEADER,
                        SCENE_RUNTIME_SOURCE,
                            SCENE_SELFTEST_SOURCE, PF_SUITE_SOURCE,
                            SELFTEST_DISPATCH_SOURCE, ENTRY_WIRING_SOURCE))
    for marker in (
        '"ADBE Easy Levels"',
        '"ADBE Pro Levels"',
        '"Input Black"',
        '"Input White"',
        "std::array<void*, 22> g_aegp_stream_suite2{}",
        "g_aegp_stream_suite2[4] = reinterpret_cast<void*>(&aegp_get_effect_num_param_streams_v2)",
        "scene_factory.legacy_stream_callbacks = {{",
        "reinterpret_cast<void*>(&aegp_get_new_effect_stream_by_index_v2)",
        "reinterpret_cast<void*>(&aegp_dispose_stream_v2)",
        "reinterpret_cast<void*>(&aegp_get_stream_name_v2)",
        "reinterpret_cast<void*>(&aegp_get_stream_type_v2)",
        "reinterpret_cast<void*>(&aegp_get_new_stream_value_v2)",
        "reinterpret_cast<void*>(&aegp_dispose_stream_value_v2)",
        "reinterpret_cast<void*>(&aegp_set_stream_value_v2)",
        "g_aegp_stream_suite2[5] = factory.legacy_stream_callbacks[0]",
        "g_aegp_stream_suite2[15] = factory.legacy_stream_callbacks[6]",
        'L"--self-test-aegp-projector-levels"',
    ):
        assert marker in source


def test_native_projector_levels_self_test_passes_all_present_workers() -> None:
    workers = [
        BUILD / name
        for name in ("aex_l2_worker.exe", "aex_render_worker.exe", "aex_smart_worker.exe")
    ]
    if not any(worker.is_file() for worker in workers):
        pytest.skip("Release minihost workers are not present; run the native build first")
    missing = [worker.name for worker in workers if not worker.is_file()]
    assert not missing, f"native build is incomplete; missing: {', '.join(missing)}"

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
    for worker in workers:
        completed = subprocess.run(
            [str(worker), "--self-test-aegp-projector-levels"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert completed.returncode == 0, completed.stderr
        assert json.loads(completed.stdout) == expected

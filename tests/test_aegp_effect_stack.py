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


def test_sdk_frozen_effect_stack_abi_compiles() -> None:
    headers = _sdk_headers()
    source = r'''
#include <cstddef>
#include <type_traits>
#include "AEConfig.h"
#include "AE_Effect.h"
#include "AE_GeneralPlug.h"

using SetEffectFlags = A_Err (SPAPI *)(AEGP_EffectRefH, AEGP_EffectFlags,
    AEGP_EffectFlags);
using ReorderEffect = A_Err (SPAPI *)(AEGP_EffectRefH, A_long);
using DeleteLayerEffect = A_Err (SPAPI *)(AEGP_EffectRefH);
using DuplicateEffect = A_Err (SPAPI *)(AEGP_EffectRefH, AEGP_EffectRefH*);
using SetStreamValue = A_Err (SPAPI *)(AEGP_PluginID, AEGP_StreamRefH,
    AEGP_StreamValue*);

static_assert(kAEGPEffectSuiteVersion2 == 2);
static_assert(kAEGPEffectSuiteVersion3 == 3);
static_assert(kAEGPEffectSuiteVersion4 == 4);

#define CHECK_EFFECT_STACK_ABI(Suite) \
    static_assert(offsetof(Suite, AEGP_SetEffectFlags) == 5 * sizeof(void*)); \
    static_assert(offsetof(Suite, AEGP_ReorderEffect) == 6 * sizeof(void*)); \
    static_assert(offsetof(Suite, AEGP_DeleteLayerEffect) == 10 * sizeof(void*)); \
    static_assert(offsetof(Suite, AEGP_DuplicateEffect) == 16 * sizeof(void*)); \
    static_assert(std::is_same_v<decltype(Suite::AEGP_SetEffectFlags), SetEffectFlags>); \
    static_assert(std::is_same_v<decltype(Suite::AEGP_ReorderEffect), ReorderEffect>); \
    static_assert(std::is_same_v<decltype(Suite::AEGP_DeleteLayerEffect), DeleteLayerEffect>); \
    static_assert(std::is_same_v<decltype(Suite::AEGP_DuplicateEffect), DuplicateEffect>)

CHECK_EFFECT_STACK_ABI(AEGP_EffectSuite2);
CHECK_EFFECT_STACK_ABI(AEGP_EffectSuite3);
CHECK_EFFECT_STACK_ABI(AEGP_EffectSuite4);

static_assert(offsetof(AEGP_EffectSuite2, AEGP_SetEffectFlags) == 40);
static_assert(offsetof(AEGP_EffectSuite2, AEGP_ReorderEffect) == 48);
static_assert(offsetof(AEGP_EffectSuite2, AEGP_DeleteLayerEffect) == 80);
static_assert(offsetof(AEGP_EffectSuite2, AEGP_DuplicateEffect) == 128);
static_assert(offsetof(AEGP_EffectSuite3, AEGP_SetEffectFlags) == 40);
static_assert(offsetof(AEGP_EffectSuite3, AEGP_ReorderEffect) == 48);
static_assert(offsetof(AEGP_EffectSuite3, AEGP_DeleteLayerEffect) == 80);
static_assert(offsetof(AEGP_EffectSuite3, AEGP_DuplicateEffect) == 128);
static_assert(offsetof(AEGP_EffectSuite4, AEGP_SetEffectFlags) == 40);
static_assert(offsetof(AEGP_EffectSuite4, AEGP_ReorderEffect) == 48);
static_assert(offsetof(AEGP_EffectSuite4, AEGP_DeleteLayerEffect) == 80);
static_assert(offsetof(AEGP_EffectSuite4, AEGP_DuplicateEffect) == 128);

static_assert(sizeof(AEGP_EffectSuite2) == 17 * sizeof(void*));
static_assert(sizeof(AEGP_EffectSuite3) == 17 * sizeof(void*));
static_assert(sizeof(AEGP_EffectSuite4) == 22 * sizeof(void*));
static_assert(sizeof(AEGP_EffectSuite2) == 136);
static_assert(sizeof(AEGP_EffectSuite3) == 136);
static_assert(sizeof(AEGP_EffectSuite4) == 176);
static_assert(kAEGPStreamSuiteVersion2 == 7);
static_assert(sizeof(AEGP_StreamSuite2) == 22 * sizeof(void*));
static_assert(sizeof(AEGP_StreamSuite2) == 176);
static_assert(offsetof(AEGP_StreamSuite2, AEGP_GetEffectNumParamStreams) == 32);
static_assert(offsetof(AEGP_StreamSuite2, AEGP_GetNewEffectStreamByIndex) == 40);
static_assert(offsetof(AEGP_StreamSuite2, AEGP_GetStreamName) == 64);
static_assert(offsetof(AEGP_StreamSuite2, AEGP_GetStreamType) == 96);
static_assert(offsetof(AEGP_StreamSuite2, AEGP_GetNewStreamValue) == 104);
static_assert(offsetof(AEGP_StreamSuite2, AEGP_DisposeStreamValue) == 112);
static_assert(offsetof(AEGP_StreamSuite2, AEGP_SetStreamValue) == 120);
static_assert(std::is_same_v<decltype(AEGP_StreamSuite2::AEGP_SetStreamValue),
    SetStreamValue>);
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
        cpp = Path(directory) / "aegp_effect_stack_abi.cpp"
        obj = Path(directory) / "aegp_effect_stack_abi.obj"
        batch = Path(directory) / "compile.bat"
        cpp.write_text(source, encoding="ascii")
        batch.write_text(
            f'@call "{vcvars}" >nul\n'
            f'@cl /nologo /std:c++17 /DWIN32 /D_WINDOWS /c /I"{headers}" '
            f'/I"{headers / "SP"}" /Fo"{obj}" "{cpp}"\n',
            encoding="ascii",
        )
        subprocess.run(["cmd", "/d", "/c", str(batch)], check=True, timeout=120)




def test_native_self_test_passes_all_three_workers() -> None:
    expected = {
        "stack_mutation": "passed",
        "suite_versions": [2, 3, 4],
        "slots": {
            "set_flags": 5,
            "reorder": 6,
            "delete": 10,
            "duplicate": 16,
        },
        "fail_closed": True,
    }
    for name in ("aex_l2_worker.exe", "aex_render_worker.exe", "aex_smart_worker.exe"):
        worker = BUILD / name
        assert worker.exists(), f"build {name} before running the native test"
        completed = subprocess.run(
            [str(worker), "--self-test-aegp-effect-stack"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert completed.returncode == 0, completed.stderr
        assert json.loads(completed.stdout) == expected

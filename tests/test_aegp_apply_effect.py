import json
import os
import subprocess
import tempfile
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"
SCENE_SOURCE = ROOT / "minihost" / "src" / "worker_aegp_scene.cpp"
SCENE_SELFTEST_SOURCE = ROOT / "minihost" / "src" / "worker_aegp_scene_selftests.cpp"
SCENE_IMPL = ROOT / "minihost" / "src" / "worker_aegp_scene_impl.inc"
PF_SUITE_SOURCE = ROOT / "minihost" / "src" / "worker_pf_suites.hpp"
BUILD = ROOT / "target" / "minihost-build"
SDK_ROOT = os.environ.get("AFTER_EFFECTS_SDK_ROOT")
HEADERS = Path(SDK_ROOT) / "Examples" / "Headers" if SDK_ROOT else None


def _sdk_headers() -> Path:
    if HEADERS is None or not HEADERS.is_dir():
        pytest.skip("set AFTER_EFFECTS_SDK_ROOT to a valid After Effects SDK root")
    return HEADERS


def test_sdk_frozen_apply_effect_abi_compiles() -> None:
    headers = _sdk_headers()
    source = r'''
#include <cstddef>
#include <type_traits>
#include "AEConfig.h"
#include "AE_Effect.h"
#include "AE_GeneralPlug.h"

using ApplyEffect = A_Err (SPAPI *)(AEGP_PluginID, AEGP_LayerH,
    AEGP_InstalledEffectKey, AEGP_EffectRefH*);

static_assert(kAEGPEffectSuiteVersion2 == 2);
static_assert(kAEGPEffectSuiteVersion3 == 3);
static_assert(kAEGPEffectSuiteVersion4 == 4);
static_assert(offsetof(AEGP_EffectSuite2, AEGP_ApplyEffect) == 9 * sizeof(void*));
static_assert(offsetof(AEGP_EffectSuite3, AEGP_ApplyEffect) == 9 * sizeof(void*));
static_assert(offsetof(AEGP_EffectSuite4, AEGP_ApplyEffect) == 9 * sizeof(void*));
static_assert(offsetof(AEGP_EffectSuite2, AEGP_ApplyEffect) == 72);
static_assert(offsetof(AEGP_EffectSuite3, AEGP_ApplyEffect) == 72);
static_assert(offsetof(AEGP_EffectSuite4, AEGP_ApplyEffect) == 72);
static_assert(std::is_same_v<decltype(AEGP_EffectSuite2::AEGP_ApplyEffect), ApplyEffect>);
static_assert(std::is_same_v<decltype(AEGP_EffectSuite3::AEGP_ApplyEffect), ApplyEffect>);
static_assert(std::is_same_v<decltype(AEGP_EffectSuite4::AEGP_ApplyEffect), ApplyEffect>);
static_assert(sizeof(AEGP_EffectSuite2) == 17 * sizeof(void*));
static_assert(sizeof(AEGP_EffectSuite3) == 17 * sizeof(void*));
static_assert(sizeof(AEGP_EffectSuite4) == 22 * sizeof(void*));
static_assert(sizeof(AEGP_EffectSuite2) == 136);
static_assert(sizeof(AEGP_EffectSuite3) == 136);
static_assert(sizeof(AEGP_EffectSuite4) == 176);
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
        cpp = Path(directory) / "aegp_apply_effect_abi.cpp"
        obj = Path(directory) / "aegp_apply_effect_abi.obj"
        batch = Path(directory) / "compile.bat"
        cpp.write_text(source, encoding="ascii")
        batch.write_text(
            f'@call "{vcvars}" >nul\n'
            f'@cl /nologo /std:c++17 /DWIN32 /D_WINDOWS /c /I"{headers}" '
            f'/I"{headers / "SP"}" /Fo"{obj}" "{cpp}"\n',
            encoding="ascii",
        )
        subprocess.run(["cmd", "/d", "/c", str(batch)], check=True, timeout=120)


def test_l2_source_exposes_apply_effect_contract() -> None:
    source = "\n".join(path.read_text(encoding="utf-8") for path in
                       (SOURCE, SCENE_SOURCE, SCENE_IMPL, SCENE_SELFTEST_SOURCE,
                        PF_SUITE_SOURCE))
    for marker in (
        "int32_t __cdecl aegp_apply_effect(",
        "(version == 2 || version == 3)",
        "g_aegp_effect_suite3[9] = reinterpret_cast<void*>(&aegp_apply_effect)",
        "g_aegp_effect_suite4[9] = reinterpret_cast<void*>(&aegp_apply_effect)",
        'L"--self-test-aegp-apply-effect"',
    ):
        assert marker in source


def test_native_self_test_passes_all_three_workers() -> None:
    expected = {
        "aegp_apply_effect": "passed",
        "suite_versions": [2, 3, 4],
        "apply_slot": 9,
        "apply_offset_x64": 72,
        "table_sizes_x64": [136, 136, 176],
        "instance_capacity": 8,
        "lease_capacity": 16,
        "fail_closed": True,
    }
    for name in ("aex_l2_worker.exe", "aex_render_worker.exe", "aex_smart_worker.exe"):
        worker = BUILD / name
        assert worker.exists(), f"build {name} before running the native test"
        completed = subprocess.run(
            [str(worker), "--self-test-aegp-apply-effect"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert completed.returncode == 0, completed.stderr
        assert json.loads(completed.stdout) == expected

import os
import subprocess
import tempfile
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
SOURCES = (
    ROOT / "minihost" / "src" / "l2_main.cpp",
    ROOT / "minihost" / "src" / "worker_aegp_scene_callbacks.hpp",
    ROOT / "minihost" / "src" / "worker_pf_suites.cpp",
    ROOT / "minihost" / "src" / "worker_pf_suites.hpp",
)
SUITE_ABI = ROOT / "minihost" / "src" / "worker_suite_abi.hpp"


def source_text() -> str:
    return "\n".join(path.read_text(encoding="utf-8") for path in SOURCES)
SDK_ROOT = os.environ.get("AFTER_EFFECTS_SDK_ROOT")
SDK_HEADERS = Path(SDK_ROOT) / "Examples" / "Headers" if SDK_ROOT else None


def test_sdk_headers_confirm_legacy_suite_abis_and_signatures() -> None:
    if SDK_HEADERS is None or not SDK_HEADERS.is_dir():
        pytest.skip("set AFTER_EFFECTS_SDK_ROOT to a valid After Effects SDK root")
    source = r'''
#include <cstddef>
#include <type_traits>
#include "AEConfig.h"
#include "AE_Effect.h"
#include "AE_EffectCBSuites.h"
#include "AE_EffectSuites.h"
#include "AE_GeneralPlug.h"
#include "AE_EffectSuitesHelper.h"

using CompBG = A_Err (SPAPI *)(AEGP_CompH, AEGP_ColorVal*);
using ConvertTime = A_Err (SPAPI *)(PF_ProgPtr, A_long, A_u_long, A_Time*);
using GetCameraMatrix = A_Err (SPAPI *)(PF_ProgPtr, const A_Time*, A_Matrix4*, A_FpLong*, A_short*, A_short*);
using CurrentTool = PF_Err (SPAPI *)(PF_SuiteTool*);

static_assert(kAEGPCompSuiteVersion10 == 21);
static_assert(sizeof(AEGP_CompSuite10) >= 41 * sizeof(void*));
static_assert(offsetof(AEGP_CompSuite10, AEGP_GetCompBGColor) == 4 * sizeof(void*));
static_assert(std::is_same_v<decltype(AEGP_CompSuite10::AEGP_GetCompBGColor), CompBG>);
static_assert(sizeof(AEGP_PFInterfaceSuite1) == 5 * sizeof(void*));
static_assert(offsetof(AEGP_PFInterfaceSuite1, AEGP_ConvertEffectToCompTime) == 2 * sizeof(void*));
static_assert(std::is_same_v<decltype(AEGP_PFInterfaceSuite1::AEGP_ConvertEffectToCompTime), ConvertTime>);
static_assert(offsetof(AEGP_PFInterfaceSuite1, AEGP_GetEffectCameraMatrix) == 4 * sizeof(void*));
static_assert(std::is_same_v<decltype(AEGP_PFInterfaceSuite1::AEGP_GetEffectCameraMatrix), GetCameraMatrix>);
static_assert(kPFHelperSuiteVersion1 == 1);
static_assert(sizeof(PF_HelperSuite1) == sizeof(void*));
static_assert(offsetof(PF_HelperSuite1, PF_GetCurrentTool) == 0);
static_assert(std::is_same_v<decltype(PF_HelperSuite1::PF_GetCurrentTool), CurrentTool>);
int main() { return 0; }
'''
    program_files_x86 = os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")
    vswhere = Path(program_files_x86) / "Microsoft Visual Studio" / "Installer" / "vswhere.exe"
    installation = subprocess.check_output(
        [str(vswhere), "-latest", "-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"],
        text=True,
    ).strip()
    vcvars = Path(installation) / "VC" / "Auxiliary" / "Build" / "vcvars64.bat"
    with tempfile.TemporaryDirectory() as directory:
        cpp = Path(directory) / "legacy_abi.cpp"
        obj = Path(directory) / "legacy_abi.obj"
        batch = Path(directory) / "compile_legacy_abi.bat"
        cpp.write_text(source, encoding="ascii")
        batch.write_text(
            f'@call "{vcvars}" >nul\n'
            f'@cl /nologo /std:c++17 /DWIN32 /D_WINDOWS /c /I"{SDK_HEADERS}" '
            f'/I"{SDK_HEADERS / "SP"}" /Fo"{obj}" "{cpp}"\n',
            encoding="ascii",
        )
        subprocess.run(["cmd", "/d", "/c", str(batch)], check=True, timeout=120)


def test_minihost_publishes_typed_fail_closed_legacy_effect_suites() -> None:
    text = source_text()
    for marker in (
        "std::array<void*, 41> g_aegp_comp_suite10",
        "g_aegp_comp_suite10[4] = reinterpret_cast<void*>(&aegp_get_comp_bg_color)",
        'std::strcmp(name, "AEGP Comp Suite") == 0 && version == 21',
        "offsetof(PfInterfaceSuite, convert_effect_to_comp_time) == 2 * sizeof(void*)",
        "&convert_effect_to_comp_time",
        "std::array<void*, 1> g_pf_helper_suite1",
        'std::strcmp(name, "AE Plugin Helper Suite") == 0',
        "--self-test-legacy-effect-compat",
    ):
        assert marker in text

    bg = text[text.index("int32_t __cdecl aegp_get_comp_bg_color") :]
    bg = bg[: bg.index("\n}")]
    assert "comp != &g_aegp_comp || !color" in bg
    assert bg.index("return 4") < bg.index("*color = headless_color")

    assert "struct AegpTime {" in SUITE_ABI.read_text(encoding="utf-8")
    convert = text[text.rindex("int32_t __cdecl convert_effect_to_comp_time(") :]
    convert = convert[: convert.index("\n}")]
    assert "effect != &g_effect || time_scale == 0 || !comp_time" in convert
    assert convert.index("return 4") < convert.index("*comp_time = converted")


def test_helper_v1_has_independent_lease_and_headless_none_policy() -> None:
    text = source_text()
    helper = text[text.index("int32_t __cdecl pf_get_current_tool") :]
    helper = helper[: helper.index("\n}")]
    assert "if (!tool) return kPfBadCallbackParam;" in helper
    assert "*tool = kPfSuiteToolNone;" in helper
    assert "current_pf_helper_tool" not in helper
    assert "g_pf_helper_suite1.data()" in text
    assert "g_pf_helper_suite2.data()" in text

import os
import subprocess
import tempfile
from pathlib import Path

import pytest
import source_owners


ROOT = Path(__file__).resolve().parents[1]
SOURCES = source_owners.contract_files("legacy_effect_compat")
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




def test_helper_v1_has_independent_lease_and_headless_none_policy() -> None:
    text = source_text()
    helper = text[text.index("int32_t __cdecl get_current_tool") :]
    helper = helper[: helper.index("\n}")]
    assert "if (!tool) return kBadCallbackParam;" in helper
    assert "*tool = kToolNone;" in helper
    assert "current_tool().load" not in helper
    assert "aexcompat::pf_helper::suite1()" in text
    assert "aexcompat::pf_helper::suite2()" in text

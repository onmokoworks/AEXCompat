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


def test_sdk_frozen_layer_and_stream_slots_compile() -> None:
    headers = _sdk_headers()
    source = r"""
#include <cstddef>
#include <type_traits>
#include "AEConfig.h"
#include "AE_Effect.h"
#include "AE_GeneralPlug.h"
using LayerXform = A_Err (SPAPI *)(AEGP_LayerH, const A_Time*, A_Matrix4*);
using LayerValue = A_Err (SPAPI *)(AEGP_LayerH, AEGP_LayerStream, AEGP_LTimeMode,
    const A_Time*, A_Boolean, AEGP_StreamVal*, AEGP_StreamType*);
using ItemFromComp = A_Err (SPAPI *)(AEGP_CompH, AEGP_ItemH*);
using ItemDimensions = A_Err (SPAPI *)(AEGP_ItemH, A_long*, A_long*);
static_assert(kAEGPLayerSuiteVersion5 == 11);
static_assert(offsetof(AEGP_LayerSuite5, AEGP_GetLayerToWorldXform) == 38 * sizeof(void*));
static_assert(std::is_same_v<decltype(AEGP_LayerSuite5::AEGP_GetLayerToWorldXform), LayerXform>);
static_assert(kAEGPStreamSuiteVersion2 == 7);
static_assert(offsetof(AEGP_StreamSuite2, AEGP_GetLayerStreamValue) == 16 * sizeof(void*));
static_assert(std::is_same_v<decltype(AEGP_StreamSuite2::AEGP_GetLayerStreamValue), LayerValue>);
static_assert(kAEGPCompSuiteVersion4 == 9);
static_assert(offsetof(AEGP_CompSuite4, AEGP_GetItemFromComp) == sizeof(void*));
static_assert(std::is_same_v<decltype(AEGP_CompSuite4::AEGP_GetItemFromComp), ItemFromComp>);
static_assert(kAEGPItemSuiteVersion6 == 10);
static_assert(offsetof(AEGP_ItemSuite6, AEGP_GetItemDimensions) == 16 * sizeof(void*));
static_assert(std::is_same_v<decltype(AEGP_ItemSuite6::AEGP_GetItemDimensions), ItemDimensions>);
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
        cpp = Path(directory) / "resizer_3d_abi.cpp"
        obj = Path(directory) / "resizer_3d_abi.obj"
        batch = Path(directory) / "compile.bat"
        cpp.write_text(source, encoding="ascii")
        batch.write_text(
            f'@call "{vcvars}" >nul\n'
            f'@{compile_driver()} /nologo /std:c++17 /DWIN32 /D_WINDOWS /c /I"{headers}" '
            f'/I"{headers / "SP"}" /Fo"{obj}" "{cpp}"\n',
            encoding="ascii",
        )
        subprocess.run(["cmd", "/d", "/c", str(batch)], check=True, timeout=120)


def test_native_chain_passes_all_release_workers() -> None:
    expected = {
        "aegp_resizer_3d": "passed",
        "layer_slot": 38,
        "layer_offset_x64": 304,
        "stream_slot": 16,
        "stream_offset_x64": 128,
        "comp_slot": 1,
        "comp_offset_x64": 8,
        "item_slot": 16,
        "item_offset_x64": 128,
        "zoom": 1920,
        "dimensions": [1920, 1080],
    }
    for kind in ("discovery", "classic", "smart"):
        completed = subprocess.run(
            [
                str(BUILD / "aex_worker.exe"),
                "--kind",
                kind,
                "--self-test-aegp-resizer-3d",
            ],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert completed.returncode == 0, completed.stderr
        assert json.loads(completed.stdout) == expected

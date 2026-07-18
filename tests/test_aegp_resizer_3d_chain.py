import json
import os
import subprocess
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"
BUILD = ROOT / "target" / "minihost-build"
HEADERS = Path(r"C:\Program Files\Adobe\AfterEffectsSDK\Examples\Headers")


def test_sdk_frozen_layer_and_stream_slots_compile() -> None:
    source = r'''
#include <cstddef>
#include <type_traits>
#include "AEConfig.h"
#include "AE_Effect.h"
#include "AE_GeneralPlug.h"
using LayerXform = A_Err (SPAPI *)(AEGP_LayerH, const A_Time*, A_Matrix4*);
using LayerValue = A_Err (SPAPI *)(AEGP_LayerH, AEGP_LayerStream, AEGP_LTimeMode,
    const A_Time*, A_Boolean, AEGP_StreamVal*, AEGP_StreamType*);
static_assert(kAEGPLayerSuiteVersion5 == 11);
static_assert(offsetof(AEGP_LayerSuite5, AEGP_GetLayerToWorldXform) == 38 * sizeof(void*));
static_assert(std::is_same_v<decltype(AEGP_LayerSuite5::AEGP_GetLayerToWorldXform), LayerXform>);
static_assert(kAEGPStreamSuiteVersion2 == 7);
static_assert(offsetof(AEGP_StreamSuite2, AEGP_GetLayerStreamValue) == 16 * sizeof(void*));
static_assert(std::is_same_v<decltype(AEGP_StreamSuite2::AEGP_GetLayerStreamValue), LayerValue>);
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
        cpp = Path(directory) / "resizer_3d_abi.cpp"
        obj = Path(directory) / "resizer_3d_abi.obj"
        batch = Path(directory) / "compile.bat"
        cpp.write_text(source, encoding="ascii")
        batch.write_text(
            f'@call "{vcvars}" >nul\n'
            f'@cl /nologo /std:c++17 /DWIN32 /D_WINDOWS /c /I"{HEADERS}" '
            f'/I"{HEADERS / "SP"}" /Fo"{obj}" "{cpp}"\n',
            encoding="ascii",
        )
        subprocess.run(["cmd", "/d", "/c", str(batch)], check=True, timeout=120)


def test_host_chain_is_typed_bounded_and_atomic() -> None:
    source = SOURCE.read_text(encoding="utf-8")
    for marker in (
        "g_aegp_layer_suite5[38] = reinterpret_cast<void*>(&aegp_get_layer_to_world_xform)",
        "g_aegp_layer_suite8[38] = reinterpret_cast<void*>(&aegp_get_layer_to_world_xform)",
        "g_aegp_stream_suite2[16] = reinterpret_cast<void*>(&aegp_get_layer_stream_value_v2)",
        "which_stream != kLayerStreamZoom || time_mode != kCompTimeMode",
        "index != g_aegp_active_camera_layer_index",
        "!layer_active_at_time(static_cast<std::size_t>(index), *time)",
        "value->one_d = static_cast<double>(width)",
        "std::memcmp(&matrix, &matrix_sentinel, sizeof(matrix)) == 0",
    ):
        assert marker in source


def test_native_chain_passes_all_release_workers() -> None:
    expected = {
        "aegp_resizer_3d": "passed",
        "layer_slot": 38,
        "layer_offset_x64": 304,
        "stream_slot": 16,
        "stream_offset_x64": 128,
        "zoom": 1920,
    }
    for name in ("aex_l2_worker.exe", "aex_render_worker.exe", "aex_smart_worker.exe"):
        completed = subprocess.run(
            [str(BUILD / name), "--self-test-aegp-resizer-3d"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert completed.returncode == 0, completed.stderr
        assert json.loads(completed.stdout) == expected

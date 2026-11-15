import json
import subprocess
from pathlib import Path

import pytest

from _msvc_compile import compile_driver
from test_suite_entry_utility13 import HEADERS, _msvc_vcvars

ROOT = Path(__file__).resolve().parents[1]


@pytest.mark.parametrize("route", ["discovery", "classic", "smart"])
def test_render_queue_suite(canonical_release_worker, route):
    result = subprocess.run(
        [str(canonical_release_worker), "--kind", route, "--self-test-render-queue-suite"],
        cwd=ROOT, capture_output=True, text=True, errors="replace", timeout=60,
        check=True,
    )
    report = json.loads(result.stdout)
    assert report == {
        "passed": True, "state": 0, "null_error": 4, "add_error": 4,
        "set_errors": [4, 4, 4, 4], "plugin_id": 1,
        "acquired": 3, "released": 3, "balanced": True,
        "unsupported_suite_calls": [
            {"name": "AEGP Render Queue Suite", "version": 1, "slot": 0, "call_count": 1},
            {"name": "AEGP Render Queue Suite", "version": 1, "slot": 1, "call_count": 4},
        ],
        "unsupported_suite_calls_truncated": False,
    }


def test_render_queue_sdk_abi():
    if HEADERS is None or not HEADERS.is_dir():
        pytest.skip("set AFTER_EFFECTS_SDK_ROOT to a valid After Effects SDK root")
    vcvars = _msvc_vcvars()
    # Keep local compile products in the explicitly scoped output directory.
    directory = ROOT / "target" / "fs-out" / "render-queue-abi"
    directory.mkdir(parents=True, exist_ok=True)
    cpp = directory / "abi.cpp"
    obj = directory / "abi.obj"
    batch = directory / "compile.bat"
    cpp.write_text(r'''
#include <cstddef>
#include <type_traits>
#include "AEConfig.h"
#include "AE_GeneralPlug.h"
#include "worker_aegp_render_queue_suite.hpp"
using Host = aexcompat::l2_detail::RenderQueueSuite1;
static_assert(kAEGPRenderQueueSuiteVersion1 == 1);
static_assert(AEGP_RenderQueueState_STOPPED == aexcompat::l2_detail::render_queue_stopped);
static_assert(sizeof(AEGP_RenderQueueState) == sizeof(uint32_t));
static_assert(std::is_unsigned_v<AEGP_RenderQueueState>);
static_assert(sizeof(A_Err) == sizeof(int32_t));
static_assert(sizeof(AEGP_CompH) == sizeof(void*));
static_assert(sizeof(Host) == sizeof(AEGP_RenderQueueSuite1));
static_assert(offsetof(Host, add_comp) == offsetof(AEGP_RenderQueueSuite1, AEGP_AddCompToRenderQueue));
static_assert(offsetof(Host, set_state) == offsetof(AEGP_RenderQueueSuite1, AEGP_SetRenderQueueState));
static_assert(offsetof(Host, get_state) == offsetof(AEGP_RenderQueueSuite1, AEGP_GetRenderQueueState));
using Add = A_Err (SPAPI*)(AEGP_CompH, const A_char*);
using Set = A_Err (SPAPI*)(AEGP_RenderQueueState);
using Get = A_Err (SPAPI*)(AEGP_RenderQueueState*);
static_assert(std::is_same_v<decltype(AEGP_RenderQueueSuite1::AEGP_AddCompToRenderQueue), Add>);
static_assert(std::is_same_v<decltype(AEGP_RenderQueueSuite1::AEGP_SetRenderQueueState), Set>);
static_assert(std::is_same_v<decltype(AEGP_RenderQueueSuite1::AEGP_GetRenderQueueState), Get>);
''', encoding="ascii")
    batch.write_text(
        f'@call "{vcvars}" >nul\n'
        f'@{compile_driver()} /nologo /std:c++17 /DWIN32 /D_WINDOWS /c '
        f'/I"{HEADERS}" /I"{HEADERS / "SP"}" /I"{ROOT / "minihost/src"}" '
        f'/Fo"{obj}" "{cpp}"\n', encoding="ascii",
    )
    subprocess.run(["cmd", "/d", "/c", str(batch)], check=True, timeout=120)

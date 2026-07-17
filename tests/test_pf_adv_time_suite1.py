import json
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SDK_HEADERS = Path(r"C:\Program Files\Adobe\AfterEffectsSDK\Examples\Headers")
BUILD = ROOT / "target/minihost-build-adv-time-v1"
WORKER = BUILD / "Release/aex_render_worker.exe"
VS_ROOT = Path(r"C:\Program Files\Microsoft Visual Studio\18\Community")
CMAKE = VS_ROOT / r"Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe"


def run_in_vs_environment(command, *, cwd=None, timeout=420):
    with tempfile.TemporaryDirectory() as directory:
        script = Path(directory) / "run.bat"
        script.write_text(f'@call "{VS_ROOT}\\VC\\Auxiliary\\Build\\vcvars64.bat" >nul\n{command}\n',
                          encoding="ascii")
        subprocess.run([str(script)], cwd=cwd, check=True, timeout=timeout)


def test_sdk_declares_independent_v1_four_slot_abi():
    source = r'''#include "AEConfig.h"
#include "entry.h"
#include <type_traits>
#include "AE_Effect.h"
#include "AE_AdvEffectSuites.h"
static_assert(sizeof(PF_TimeDisplayPref) == 4);
static_assert(sizeof(PF_AdvTimeSuite1) == 4 * sizeof(void*));
static_assert(sizeof(PF_AdvTimeSuite4) == 5 * sizeof(void*));
static_assert(std::is_same_v<decltype(PF_AdvTimeSuite1::PF_FormatTimeActiveItem), decltype(PF_AdvTimeSuite4::PF_FormatTimeActiveItem)>);
static_assert(std::is_same_v<decltype(PF_AdvTimeSuite1::PF_FormatTime), decltype(PF_AdvTimeSuite4::PF_FormatTime)>);
static_assert(std::is_same_v<decltype(PF_AdvTimeSuite1::PF_FormatTimePlus), decltype(PF_AdvTimeSuite4::PF_FormatTimePlus)>);
int main() { return 0; }
'''
    with tempfile.TemporaryDirectory() as directory:
        probe = Path(directory) / "probe.cpp"
        probe.write_text(source, encoding="ascii")
        command = (f'cl /nologo /std:c++17 /D_WINDOWS /c /I"{SDK_HEADERS}" /I"{SDK_HEADERS / "SP"}" '
                   f'/I"{SDK_HEADERS.parent / "Util"}" "{probe}" /Fo"{directory}\\probe.obj"')
        run_in_vs_environment(command, timeout=120)


def test_release_worker_native_v1_v4_guard_and_lease_selftest():
    command = (f'"{CMAKE}" -S minihost -B "{BUILD}" -G "Visual Studio 18 2026" -A x64 && '
               f'"{CMAKE}" --build "{BUILD}" --config Release --target aex_render_worker"')
    run_in_vs_environment(command, cwd=ROOT)
    result = subprocess.run([str(WORKER), "--self-test-pf-adv-time-suite1"], cwd=ROOT,
                            check=True, capture_output=True, text=True, timeout=60)
    assert json.loads(result.stdout) == {
        "pf_adv_time_suite1": "passed", "v1_slots": 4, "v4_slots": 5,
        "independent_identity": True, "guard_intact": True,
        "reverse_release": True, "suite_leases_balanced": True,
    }

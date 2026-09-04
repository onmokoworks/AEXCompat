import json
import os
import subprocess
import tempfile
from pathlib import Path

import pytest

import _msvc_compile
from _msvc_compile import compile_driver

ROOT = Path(__file__).resolve().parents[1]
SDK_ROOT = os.environ.get("AFTER_EFFECTS_SDK_ROOT")
SDK_HEADERS = Path(SDK_ROOT) / "Examples" / "Headers" if SDK_ROOT else None


def _compile_command(probe: Path, directory: Path, headers: Path) -> str:
    return (
        f"{compile_driver()} /nologo /std:c++17 /D_WINDOWS /c "
        f'/I"{headers}" /I"{headers / "SP"}" '
        f'/I"{headers.parent / "Util"}" "{probe}" '
        f'/Fo"{directory}\\probe.obj"'
    )


def run_in_vs_environment(command, *, cwd=None, timeout=420):
    program_files_x86 = os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")
    vswhere = (
        Path(program_files_x86)
        / "Microsoft Visual Studio"
        / "Installer"
        / "vswhere.exe"
    )
    # -utf8 forces vswhere's output to UTF-8, so decode it as UTF-8 explicitly
    # rather than the process locale (the SDK workflow does not set PYTHONUTF8);
    # otherwise a UTF-8 install path with non-ASCII characters would mojibake.
    # errors="replace" still guards any stray CP932 chatter (#237, follow-up #58).
    result = subprocess.run(
        [
            str(vswhere),
            "-latest",
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-property",
            "installationPath",
            "-utf8",
        ],
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8-sig",
        errors="replace",
    )
    vs_root = Path(result.stdout.strip())
    if not vs_root.is_dir():
        pytest.fail("no Visual Studio installation with the C++ x64 toolset")
    with tempfile.TemporaryDirectory() as directory:
        script = Path(directory) / "run.bat"
        script.write_text(
            f'@call "{vs_root}\\VC\\Auxiliary\\Build\\vcvars64.bat" >nul\n{command}\n',
            encoding="ascii",
        )
        subprocess.run([str(script)], cwd=cwd, check=True, timeout=timeout)


def test_sdk_declares_independent_v1_four_slot_abi():
    if SDK_HEADERS is None or not SDK_HEADERS.is_dir():
        pytest.skip("set AFTER_EFFECTS_SDK_ROOT to a valid After Effects SDK root")
    source = r"""#include "AEConfig.h"
#include "entry.h"
#include <type_traits>
#include "AE_Effect.h"
#include "AE_AdvEffectSuites.h"
static_assert(sizeof(PF_TimeDisplayPref) == 4 && alignof(PF_TimeDisplayPref) == 1);
static_assert(sizeof(PF_TimeDisplayPrefVersion2) == 7 && alignof(PF_TimeDisplayPrefVersion2) == 1);
static_assert(sizeof(PF_TimeDisplayPrefVersion3) == 16 && alignof(PF_TimeDisplayPrefVersion3) == 4);
static_assert(sizeof(PF_AdvTimeSuite1) == 4 * sizeof(void*));
static_assert(sizeof(PF_AdvTimeSuite2) == 4 * sizeof(void*));
static_assert(sizeof(PF_AdvTimeSuite3) == 4 * sizeof(void*));
static_assert(sizeof(PF_AdvTimeSuite4) == 5 * sizeof(void*));
static_assert(std::is_same_v<decltype(PF_AdvTimeSuite1::PF_FormatTimeActiveItem), decltype(PF_AdvTimeSuite4::PF_FormatTimeActiveItem)>);
static_assert(std::is_same_v<decltype(PF_AdvTimeSuite1::PF_FormatTime), decltype(PF_AdvTimeSuite4::PF_FormatTime)>);
static_assert(std::is_same_v<decltype(PF_AdvTimeSuite1::PF_FormatTimePlus), decltype(PF_AdvTimeSuite4::PF_FormatTimePlus)>);
int main() { return 0; }
"""
    with tempfile.TemporaryDirectory() as directory:
        probe = Path(directory) / "probe.cpp"
        probe.write_text(source, encoding="ascii")
        command = _compile_command(probe, Path(directory), SDK_HEADERS)
        run_in_vs_environment(command, timeout=120)


def test_sdk_compile_command_honors_the_configured_cache(monkeypatch):
    monkeypatch.setenv("AEXCOMPAT_COMPILE_CACHE", "sccache")
    # The command shape is what is under test, not whether this machine has
    # sccache installed.
    monkeypatch.setattr(_msvc_compile.shutil, "which", lambda name: f"/fake/{name}")
    command = _compile_command(
        Path("probe.cpp"), Path("build"), Path("sdk/Examples/Headers")
    )
    assert command.startswith("sccache cl.exe ")
    assert " /c " in command


def test_release_worker_native_v1_v4_guard_and_lease_selftest(canonical_release_worker):
    result = subprocess.run(
        [
            str(canonical_release_worker),
            "--kind",
            "classic",
            "--self-test-pf-adv-time-suite1",
        ],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
        errors="replace",
        timeout=60,
    )
    assert json.loads(result.stdout) == {
        "pf_adv_time_suite_versions": "passed",
        "v1_slots": 4,
        "v2_slots": 4,
        "v3_slots": 4,
        "v4_slots": 5,
        "independent_identity": True,
        "guard_intact": True,
        "reverse_release": True,
        "suite_leases_balanced": True,
    }

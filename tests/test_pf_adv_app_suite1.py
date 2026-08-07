import json
import os
import subprocess
import tempfile
import time
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
SDK_ROOT = os.environ.get("AFTER_EFFECTS_SDK_ROOT")
SDK_HEADERS = Path(SDK_ROOT) / "Examples" / "Headers" if SDK_ROOT else None
WORKER = ROOT / "target" / "minihost-build-v18" / "Release" / "aex_render_worker.exe"


def _visual_studio_installation() -> Path:
    vswhere = Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")) / (
        "Microsoft Visual Studio/Installer/vswhere.exe"
    )
    result = subprocess.run(
        [str(vswhere), "-latest", "-products", "*", "-requires",
         "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"],
        check=True, capture_output=True, text=True, timeout=30,
    )
    return Path(result.stdout.strip())


def _cleanup_with_windows_lock_retry(cleanup, attempts: int = 4) -> None:
    """Retry only the short-lived Windows sharing violation after cl.exe exits."""
    for attempt in range(attempts):
        try:
            cleanup()
            return
        except PermissionError as exc:
            if getattr(exc, "winerror", None) != 32 or attempt == attempts - 1:
                raise
            time.sleep(0.1 * (attempt + 1))


def test_sdk_header_compiled_x64_probe_fixes_v1_to_ten_ordered_slots():
    if SDK_HEADERS is None or not SDK_HEADERS.is_dir():
        pytest.skip("set AFTER_EFFECTS_SDK_ROOT to a valid After Effects SDK root")
    source = r'''
#include <cstddef>
#include <iostream>
#include "AEConfig.h"
#include "AE_Effect.h"
#include "AE_EffectCB.h"
#include "AE_EffectCBSuites.h"
#include "AE_EffectUI.h"
#include "AE_AdvEffectSuites.h"
int main() {
  std::cout << "{\"pointer_size\":" << sizeof(void*)
    << ",\"suite_size\":" << sizeof(PF_AdvAppSuite1)
    << ",\"offsets\":["
    << offsetof(PF_AdvAppSuite1, PF_SetProjectDirty) << ','
    << offsetof(PF_AdvAppSuite1, PF_SaveProject) << ','
    << offsetof(PF_AdvAppSuite1, PF_SaveBackgroundState) << ','
    << offsetof(PF_AdvAppSuite1, PF_ForceForeground) << ','
    << offsetof(PF_AdvAppSuite1, PF_RestoreBackgroundState) << ','
    << offsetof(PF_AdvAppSuite1, PF_RefreshAllWindows) << ','
    << offsetof(PF_AdvAppSuite1, PF_InfoDrawText) << ','
    << offsetof(PF_AdvAppSuite1, PF_InfoDrawColor) << ','
    << offsetof(PF_AdvAppSuite1, PF_InfoDrawText3) << ','
    << offsetof(PF_AdvAppSuite1, PF_InfoDrawText3Plus) << "]}";
}
'''
    installation = _visual_studio_installation()
    vcvars = installation / "VC/Auxiliary/Build/vcvars64.bat"
    temporary_directory = tempfile.TemporaryDirectory(prefix="pf-adv-app-suite1-")
    try:
        directory = Path(temporary_directory.name)
        probe = directory / "probe.cpp"
        executable = directory / "probe.exe"
        build_script = directory / "build-probe.bat"
        probe.write_text(source, encoding="ascii")
        build_script.write_text(
            f'@call "{vcvars}" >nul\n@cl /nologo /EHsc /std:c++17 '
            f'/DWIN32 /D_WINDOWS /I"{SDK_HEADERS}" /I"{SDK_HEADERS / "SP"}" '
            f'/I"{SDK_HEADERS.parent / "Util"}" '
            f'"{probe}" /Fe:"{executable}"\n',
            encoding="ascii",
        )
        subprocess.run(["cmd", "/d", "/c", str(build_script)], check=True,
                       cwd=directory, timeout=180)
        result = subprocess.run([str(executable)], check=True, capture_output=True,
                                text=True, timeout=30)
    finally:
        _cleanup_with_windows_lock_retry(temporary_directory.cleanup)
    payload = json.loads(result.stdout)
    assert payload == {
        "pointer_size": 8,
        "suite_size": 80,
        "offsets": [index * 8 for index in range(10)],
    }


def test_temp_cleanup_retries_only_windows_sharing_violations(monkeypatch):
    calls = 0
    sleeps = []

    def cleanup():
        nonlocal calls
        calls += 1
        if calls < 3:
            error = PermissionError("sharing violation")
            error.winerror = 32
            raise error

    monkeypatch.setattr(time, "sleep", sleeps.append)
    _cleanup_with_windows_lock_retry(cleanup)

    assert calls == 3
    assert sleeps == [0.1, 0.2]


def test_temp_cleanup_does_not_retry_other_permission_errors():
    error = PermissionError("access denied")
    error.winerror = 5

    with pytest.raises(PermissionError, match="access denied"):
        _cleanup_with_windows_lock_retry(lambda: (_ for _ in ()).throw(error))


def test_worker_v1_v2_tables_are_independent_non_null_fail_closed_and_balanced():
    result = subprocess.run([str(WORKER), "--self-test-pf-adv-app-suite"], cwd=ROOT,
                            check=True, capture_output=True, text=True, timeout=30)
    assert json.loads(result.stdout.strip()) == {
        "pf_adv_app_suite_versions": "passed",
        "v1_slots": 10,
        "v2_slots": 11,
        "independent_identity": True,
        "suite_leases_balanced": True,
    }



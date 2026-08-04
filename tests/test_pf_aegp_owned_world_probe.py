import hashlib
import json
import subprocess
from pathlib import Path

from _render_session import HARNESS, assert_artifact_fresh, run_session_render


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments" / "pf-aegp-owned-world-probe" / "pf_aegp_owned_world_probe.cpp"
SCRIPT = ROOT / "tools" / "build-pf-aegp-owned-world-probe.ps1"
WORKER = ROOT / "target" / "minihost-build" / "aex_render_worker.exe"
PROBE = ROOT / "target" / "pf-aegp-owned-world-probe-build" / "Release" / "pf_aegp_owned_world_probe.aex"
INPUT = ROOT / "target" / "gpu-effects" / "opencl-input.rgba"


def test_probe_builds_against_world_suite3():
    subprocess.run(
        ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT)],
        cwd=ROOT, check=True, timeout=180,
    )
    source = SOURCE.read_text(encoding="utf-8")
    assert "sizeof(AEGP_WorldSuite3) == 13 * sizeof(void*)" in source
    assert "offsetof(AEGP_WorldSuite3, AEGP_New) == 0 * sizeof(void*)" in source
    assert "offsetof(AEGP_WorldSuite3, AEGP_Dispose) == 1 * sizeof(void*)" in source




def test_real_probe_exercises_owned_world_lifecycle(tmp_path):
    assert WORKER.is_file()
    assert PROBE.is_file()
    assert_artifact_fresh(PROBE, SOURCE, WORKER, HARNESS)
    assert INPUT.is_file()
    output = tmp_path / "owned-world-output.rgba"
    report = run_session_render(tmp_path, PROBE, INPUT, output, width=37, height=23)
    assert report["status"] == "render_completed"
    assert report["render_error"] == 0
    assert report["suite_leases_balanced"] is True
    assert report["suite_acquires"] == report["suite_releases"] == 2
    assert report["guard_bytes_intact"] is True
    assert output.read_bytes() == bytes(37 * 23 * 4)

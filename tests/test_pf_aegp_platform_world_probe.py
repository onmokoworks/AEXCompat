import hashlib
import json
import subprocess
from pathlib import Path

from _render_session import HARNESS, assert_artifact_fresh, run_session_render

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments" / "pf-aegp-platform-world-probe" / "pf_aegp_platform_world_probe.cpp"
SCRIPT = ROOT / "tools" / "build-pf-aegp-platform-world-probe.ps1"
WORKER = ROOT / "target" / "minihost-build" / "aex_render_worker.exe"
PROBE = ROOT / "target" / "pf-aegp-platform-world-probe-build" / "Release" / "pf_aegp_platform_world_probe.aex"
INPUT = ROOT / "target" / "gpu-effects" / "opencl-input.rgba"


def test_probe_builds_with_exact_sdk_tables():
    subprocess.run(
        ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT)],
        cwd=ROOT, check=True, timeout=180,
    )
    source = SOURCE.read_text(encoding="utf-8")
    assert "const AEGP_WorldSuite3* worlds" in source
    assert "const AEGP_RenderSuite5* render_suite" in source
    assert "sizeof(AEGP_WorldSuite3) == 13 * sizeof(void*)" in source
    assert "AEGP_CheckinRenderedFrame) == 12 * sizeof(void*)" in source
    assert "A_Err (*)(AEGP_RenderOptionsH, const AEGP_TimeStamp*" in source


def test_probe_covers_platform_world_lifecycle_and_safe_invalid_paths():
    source = SOURCE.read_text(encoding="utf-8")
    for operation in (
        "AEGP_NewPlatformWorld", "AEGP_NewReferenceFromPlatformWorld",
        "AEGP_GetType", "AEGP_GetSize", "AEGP_GetRowBytes", "AEGP_GetBaseAddr8",
        "AEGP_DisposePlatformWorld", "AEGP_CheckinRenderedFrame",
    ):
        assert f"->{operation}" in source
    assert "AEGP_WorldH stale_reference = reference" in source
    assert "AEGP_PlatformWorldH stale_platform = platform" in source
    assert "AEGP_WorldH adopted_reference = reference" in source
    assert "g_plugin_id, nullptr, &invalid_reference" in source
    assert "nullptr, &timestamp, 1, nullptr" in source


def test_real_probe_adopts_platform_world_and_rejects_stale_handles(tmp_path):
    assert WORKER.is_file()
    assert PROBE.is_file()
    assert_artifact_fresh(PROBE, SOURCE, WORKER, HARNESS)
    assert INPUT.is_file()
    output = tmp_path / "platform-world-output.rgba"
    report = run_session_render(tmp_path, PROBE, INPUT, output, width=37, height=23)
    assert report["status"] == "render_completed"
    assert report["render_error"] == 0
    assert report["suite_leases_balanced"] is True
    assert report["suite_acquires"] == report["suite_releases"] == 6
    assert report["guard_bytes_intact"] is True
    assert output.read_bytes() == bytes(37 * 23 * 4)

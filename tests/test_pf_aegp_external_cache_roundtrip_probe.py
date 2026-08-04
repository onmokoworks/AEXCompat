import hashlib
import json
import subprocess
from pathlib import Path

from _render_session import HARNESS, assert_artifact_fresh, run_session_render

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments" / "pf-aegp-external-cache-roundtrip-probe" / "pf_aegp_external_cache_roundtrip_probe.cpp"
SCRIPT = ROOT / "tools" / "build-pf-aegp-external-cache-roundtrip-probe.ps1"
WORKER = ROOT / "target" / "minihost-build" / "aex_render_worker.exe"
PROBE = ROOT / "target" / "pf-aegp-external-cache-roundtrip-probe-build" / "Release" / "pf_aegp_external_cache_roundtrip_probe.aex"
INPUT = ROOT / "target" / "gpu-effects" / "opencl-input.rgba"


def test_probe_builds_against_the_real_pf_aegp_sdk_tables():
    subprocess.run(
        ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT)],
        cwd=ROOT, check=True, timeout=180,
    )
    source = SOURCE.read_text(encoding="utf-8")
    assert "const AEGP_RenderOptionsSuite4* options_suite" in source
    assert "const AEGP_WorldSuite3* worlds" in source
    assert "const AEGP_RenderSuite5* renders" in source
    assert "AEGP_CheckinRenderedFrame) == 12 * sizeof(void*)" in source




def test_real_probe_roundtrips_external_cache_pixels_through_a_receipt(tmp_path):
    assert WORKER.is_file()
    assert PROBE.is_file()
    assert_artifact_fresh(PROBE, SOURCE, WORKER, HARNESS)
    assert INPUT.is_file()
    output = tmp_path / "external-cache-roundtrip-output.rgba"
    report = run_session_render(tmp_path, PROBE, INPUT, output, width=37, height=23)
    assert report["status"] == "render_completed"
    assert report["render_error"] == 0
    assert report["suite_leases_balanced"] is True
    assert report["suite_acquires"] == report["suite_releases"] == 6
    assert report["guard_bytes_intact"] is True
    assert output.read_bytes() == bytes(37 * 23 * 4)

import hashlib
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments" / "pf-aegp-render-suite5-probe" / "pf_aegp_render_suite5_probe.cpp"
WORKER = ROOT / "target" / "minihost-build" / "aex_render_worker.exe"
PROBE = ROOT / "target" / "pf-aegp-render-suite5-probe-build" / "Release" / "pf_aegp_render_suite5_probe.aex"
INPUT = ROOT / "target" / "gpu-effects" / "opencl-input.rgba"


def test_probe_uses_exact_typed_suite5_metadata_and_lifecycle_slots():
    source = SOURCE.read_text(encoding="utf-8")
    assert "const AEGP_RenderSuite5* suite" in source
    assert "sizeof(AEGP_RenderSuite5) == 14 * sizeof(void*)" in source
    assert "kAEGPRenderSuiteVersion5" in source
    for slot in (
        "AEGP_CheckinFrame", "AEGP_GetRenderedRegion",
        "AEGP_IsRenderedFrameSufficient", "AEGP_GetCurrentTimestamp",
        "AEGP_HasItemChangedSinceTimestamp", "AEGP_IsItemWorthwhileToRender",
        "AEGP_GetReceiptGuid",
    ):
        assert f"suite->{slot}" in source


def test_probe_covers_stale_handles_and_error_paths():
    source = SOURCE.read_text(encoding="utf-8")
    assert "AEGP_FrameReceiptH stale_receipt = receipt" in source
    assert "AEGP_IsRenderedFrameSufficient(nullptr, nullptr, &answer)" in source
    assert "AEGP_IsItemWorthwhileToRender(nullptr, &timestamp, &answer)" in source
    assert "AEGP_FreeMemHandle(stale_guid) == A_Err_NONE" in source
    assert "AEGP_GetCurrentTimestamp(nullptr) == A_Err_NONE" in source
    assert "nullptr, &start, &duration" in source


def test_real_probe_exercises_render_suite5_metadata_and_lifecycle(tmp_path):
    assert WORKER.is_file()
    assert PROBE.is_file()
    assert INPUT.is_file()
    output = tmp_path / "render-suite5-output.rgba"
    completed = subprocess.run(
        [str(WORKER), "--render-image", str(PROBE),
         hashlib.sha256(PROBE.read_bytes()).hexdigest(), "v5|",
         str(INPUT), str(output), "37", "23", "0", "1", "1", "1"],
        cwd=ROOT, text=True, encoding="utf-8", errors="replace",
        capture_output=True, timeout=30,
    )
    assert completed.returncode == 0, completed.stdout + completed.stderr
    report = json.loads(completed.stdout)
    assert report["status"] == "render_completed"
    assert report["render_error"] == 0
    assert report["suite_leases_balanced"] is True
    assert report["suite_acquires"] == report["suite_releases"] == 5
    assert report["guard_bytes_intact"] is True
    assert output.read_bytes() == bytes(37 * 23 * 4)

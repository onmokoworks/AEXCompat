import hashlib
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments" / "pf-aegp-layer-options-probe" / "pf_aegp_layer_options_probe.cpp"
WORKER = ROOT / "target" / "minihost-build" / "aex_render_worker.exe"
PROBE = ROOT / "target" / "pf-aegp-layer-options-probe-build" / "Release" / "pf_aegp_layer_options_probe.aex"
INPUT = ROOT / "target" / "gpu-effects" / "opencl-input.rgba"


def test_probe_acquires_suite1_and_exercises_all_14_slots():
    source = SOURCE.read_text(encoding="utf-8")
    assert "kAEGPLayerRenderOptionsSuiteVersion1" in source
    for slot in (
        "AEGP_NewFromLayer",
        "AEGP_NewFromUpstreamOfEffect",
        "AEGP_Duplicate",
        "AEGP_Dispose",
        "AEGP_SetTime",
        "AEGP_GetTime",
        "AEGP_SetTimeStep",
        "AEGP_GetTimeStep",
        "AEGP_SetWorldType",
        "AEGP_GetWorldType",
        "AEGP_SetDownsampleFactor",
        "AEGP_GetDownsampleFactor",
        "AEGP_SetMatteMode",
        "AEGP_GetMatteMode",
    ):
        assert f"suite->{slot}" in source


def test_probe_checks_duplicate_independence_and_rejects_stale_handles():
    source = SOURCE.read_text(encoding="utf-8")
    assert "copy == original" in source
    assert "AEGP_SetTime(copy, copy_time)" in source
    assert "AEGP_GetTime(original, &got_time)" in source
    assert "AEGP_WorldType_32" in source
    assert "AEGP_WorldType_16" in source
    assert "AEGP_LayerRenderOptionsH stale = original" in source
    assert "AEGP_GetTime(stale, &got_time) == A_Err_NONE" in source
    assert "AEGP_Dispose(stale) == A_Err_NONE" in source


def test_real_probe_validates_layer_options_suite_during_render(tmp_path):
    assert WORKER.is_file()
    assert PROBE.is_file()
    assert INPUT.is_file()
    output = tmp_path / "layer-options-output.rgba"
    completed = subprocess.run(
        [
            str(WORKER), "--render-image", str(PROBE),
            hashlib.sha256(PROBE.read_bytes()).hexdigest(), "v5|",
            str(INPUT), str(output), "37", "23", "0", "1", "1", "1",
        ],
        cwd=ROOT,
        text=True,
        encoding="utf-8",
        errors="replace",
        capture_output=True,
        timeout=30,
    )
    assert completed.returncode == 0, completed.stdout + completed.stderr
    report = json.loads(completed.stdout)
    assert report["status"] == "render_completed"
    assert report["render_error"] == 0
    assert report["suite_leases_balanced"] is True
    assert report["suite_acquires"] == report["suite_releases"] == 4
    assert report["guard_bytes_intact"] is True
    assert output.read_bytes() == bytes(37 * 23 * 4)

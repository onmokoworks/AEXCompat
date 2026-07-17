import hashlib
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments" / "pf-aegp-render-options4-tail-probe" / "pf_aegp_render_options4_tail_probe.cpp"
WORKER = ROOT / "target" / "minihost-build" / "aex_render_worker.exe"
PROBE = ROOT / "target" / "pf-aegp-render-options4-tail-probe-build" / "Release" / "pf_aegp_render_options4_tail_probe.aex"
INPUT = ROOT / "target" / "gpu-effects" / "opencl-input.rgba"


def test_probe_pins_typed_suite4_tail_slots_17_through_22():
    source = SOURCE.read_text(encoding="utf-8")
    assert "const AEGP_RenderOptionsSuite4* suite" in source
    assert "sizeof(AEGP_RenderOptionsSuite4) == 23 * sizeof(void*)" in source
    for slot, offset in (
        ("AEGP_SetChannelOrder", 17), ("AEGP_GetChannelOrder", 18),
        ("AEGP_GetRenderGuideLayers", 19), ("AEGP_SetRenderGuideLayers", 20),
        ("AEGP_GetRenderQuality", 21), ("AEGP_SetRenderQuality", 22),
    ):
        assert f"offsetof(AEGP_RenderOptionsSuite4, {slot}) == {offset} * sizeof(void*)" in source
        assert f"suite->{slot}" in source


def test_probe_covers_tail_roundtrips_invalid_and_stale_options():
    source = SOURCE.read_text(encoding="utf-8")
    assert "kAEGPRenderOptionsSuiteVersion4" in source
    assert "channel != AEGP_ChannelOrder_BGRA" in source
    assert "guides != TRUE" in source
    assert "quality != AEGP_ItemQuality_BEST" in source
    assert "AEGP_RenderOptionsH stale = options" in source
    for call in (
        "AEGP_SetChannelOrder(stale", "AEGP_GetChannelOrder(stale",
        "AEGP_GetRenderGuideLayers(stale", "AEGP_SetRenderGuideLayers(stale",
        "AEGP_GetRenderQuality(stale", "AEGP_SetRenderQuality(stale",
    ):
        assert call in source


def test_real_probe_exercises_render_options4_tail(tmp_path):
    assert WORKER.is_file()
    assert PROBE.is_file()
    assert INPUT.is_file()
    output = tmp_path / "render-options4-tail-output.rgba"
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
    assert report["suite_acquires"] == report["suite_releases"] == 4
    assert report["guard_bytes_intact"] is True
    assert output.read_bytes() == bytes(37 * 23 * 4)

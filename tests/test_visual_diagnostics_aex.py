"""Visual advisories must be present on the public session AEX render path."""

import json
import shutil
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from _render_session import HARNESS, ROOT, assert_artifact_fresh


SOURCE = ROOT / "instruments" / "pf-smart-geometry-probe" / "pf_smart_geometry_probe.cpp"
WORKER = ROOT / "target" / "minihost-build" / "aex_worker.exe"
PROBE_BUILD = ROOT / "target" / "pf-smart-geometry-probe-build"
PROBE_CANDIDATES = (
    PROBE_BUILD / "Release" / "pf_smart_geometry_probe.aex",
    PROBE_BUILD / "pf_smart_geometry_probe.aex",
)


@pytest.mark.parametrize("depth", ("argb8", "argb16", "argb32f"))
def test_public_aex_render_reports_bounded_advisory(tmp_path: Path, depth: str) -> None:
    available = [path for path in PROBE_CANDIDATES if path.is_file()]
    assert available, "build the smart geometry probe before running this test"
    probe = max(available, key=lambda path: path.stat().st_mtime_ns)
    assert_artifact_fresh(probe, SOURCE, WORKER, HARNESS)
    plugin = tmp_path / "geometry-classic-nop.aex"
    shutil.copyfile(probe, plugin)
    source = tmp_path / "source.png"
    output = tmp_path / "output.png"
    Image.new("RGBA", (16, 12), (17, 31, 47, 255)).save(source)
    completed = subprocess.run(
        [str(HARNESS), "--render-experimental-session", str(plugin),
         str(source), str(output), depth, "classic", "0", "5", "1"],
        cwd=ROOT, capture_output=True, text=True, encoding="utf-8",
        errors="replace", timeout=60,
    )
    assert completed.returncode == 0, completed.stdout + completed.stderr
    report = json.loads(completed.stdout)
    assert report["passed"] is True
    assert output.is_file()
    diagnosis = report["visual_diagnostics"]
    assert diagnosis["advisory"] is True
    assert diagnosis["status"] == "available"
    assert diagnosis["pixel_format"] == depth
    assert diagnosis["sample_count"] == 16 * 12
    assert diagnosis["comparison"]["status"] == "available"
    assert diagnosis["comparison"]["exact_equal"] is True
    assert "unchanged_from_input" in diagnosis["reasons"]
    assert len(json.dumps(diagnosis, separators=(",", ":"))) < 5000

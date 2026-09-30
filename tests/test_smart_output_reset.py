"""A failed SmartFX output resize must not expose stale-buffer geometry."""

import json
import os
import subprocess
from pathlib import Path

from PIL import Image

from _render_session import HARNESS, assert_artifact_fresh


ROOT = Path(__file__).resolve().parents[1]
WORKER = ROOT / "target" / "minihost-build" / "aex_worker.exe"
PROBE = (
    ROOT
    / "target"
    / "pf-smart-geometry-probe-build"
    / "Release"
    / "pf_smart_geometry_probe.aex"
)
SOURCE = (
    ROOT
    / "instruments"
    / "pf-smart-geometry-probe"
    / "pf_smart_geometry_probe.cpp"
)


def test_failed_output_reset_clears_final_report_geometry(tmp_path: Path) -> None:
    assert_artifact_fresh(PROBE, SOURCE, WORKER, HARNESS)
    input_path = tmp_path / "input.png"
    output_path = tmp_path / "output.png"
    Image.new("RGBA", (16, 12), (17, 31, 47, 255)).save(input_path)
    environment = os.environ.copy()
    # The guarded buffer constructor is call 1. Fail the SmartFX resize after
    # PreRender on call 2, while the old allocation is still live.
    environment["AEXCOMPAT_TEST_FAIL_OUTPUT_RESET_CALL"] = "2"
    completed = subprocess.run(
        [
            str(HARNESS),
            "--render-experimental-session",
            str(PROBE),
            str(input_path),
            str(output_path),
            "argb8",
            "smart",
            "1",
            "5",
            "1",
        ],
        cwd=ROOT,
        env=environment,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=60,
    )
    assert completed.returncode != 0
    marker = ", report="
    assert marker in completed.stderr, completed.stderr
    report, _ = json.JSONDecoder().raw_decode(
        completed.stderr.split(marker, 1)[1]
    )
    assert report["pre_render_error"] == -3
    assert report["result_rects_valid"] is False
    assert report["smart_render_selector_dispatched"] is False
    assert report["width"] == 0
    assert report["height"] == 0
    assert report["rowbytes"] == 0
    assert report["output_origin"] == [0, 0]
    assert report["output_world"] == {
        "extent_hint": {"bottom": 0, "left": 0, "right": 0, "top": 0},
        "height": 0,
        "pixel_format": "argb8",
        "premultiplication": "premultiplied",
        "row_bytes": 0,
        "width": 0,
    }
    assert not output_path.exists()

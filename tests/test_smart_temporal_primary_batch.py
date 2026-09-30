"""A resident SmartFX frame may check out the actual earlier primary input."""

import json
import os
import subprocess
from pathlib import Path

from PIL import Image

from _render_session import BROKER, ROOT


PROBE = Path(os.environ.get(
    "AEXCOMPAT_SMART_PAST_PROBE",
    ROOT / "target" / "instruments-build" / "pf-wide-time-probe"
    / "pf_smart_past_input_probe.aex",
))
WORKER = ROOT / "target" / "minihost-build" / "aex_worker.exe"


def test_smart_session_returns_prior_primary_pixels_not_current_pixels(tmp_path):
    for artifact in (BROKER, WORKER, PROBE):
        assert artifact.is_file(), f"built artifact is missing: {artifact}"

    colors = [(0, 0, 0, 255), (7, 13, 19, 255), (14, 26, 38, 255)]
    inputs = []
    for index, color in enumerate(colors):
        image = tmp_path / f"input-{index}.png"
        Image.new("RGBA", (16, 12), color).save(image)
        inputs.append(str(image))

    output = tmp_path / "output"
    request = tmp_path / "request.json"
    request.write_text(json.dumps({
        "schema_version": 1,
        "plugin": str(PROBE.resolve()),
        "input_frames": inputs,
        "output_directory": str(output),
        "pixel_format": "argb8",
        "time_scale": 30,
        "time_step": 1,
        "smart": True,
    }), encoding="utf-8")
    report_path = tmp_path / "report.json"
    completed = subprocess.run(
        [str(BROKER), "render-video-batch", str(request), str(report_path)],
        cwd=ROOT, capture_output=True, text=True, encoding="utf-8",
        errors="replace", timeout=60,
    )
    assert report_path.is_file(), completed.stderr
    report = json.loads(report_path.read_text(encoding="utf-8"))
    assert completed.returncode == 0, report
    assert report["passed"] is True
    assert report["frames_ok"] == 3
    assert report["session"]["session_clean"] is True
    assert report["session"]["worker"]["classification"] == "ok"
    assert report["session"]["final_report"]["rejected_temporal_layer_checkouts"] == 0

    for index, expected in enumerate((colors[0], colors[0], colors[1])):
        with Image.open(output / f"frame-{index:06}.png") as image:
            pixels = image.convert("RGBA")
            assert pixels.size == (16, 12)
            assert pixels.tobytes() == bytes(expected) * (16 * 12)
            if index > 0:
                assert expected != colors[index], "the probe must not copy the current frame"

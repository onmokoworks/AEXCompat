import json
import os
import pathlib
import subprocess

import pytest

ROOT = pathlib.Path(__file__).resolve().parents[1]

def _worker() -> pathlib.Path | None:
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        pathlib.Path(configured) if configured else None,
        ROOT / "target" / "minihost-build-v18" / "Release" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build-v18" / "aex_render_worker.exe",
    ]
    return next((candidate for candidate in candidates if candidate and candidate.is_file()), None)


def test_render_options_runtime_matrix(tmp_path):
    worker = _worker()
    assert worker is not None, "build aex_render_worker before running the focused runtime test"
    temp = ROOT / "target" / "tmp"
    temp.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env["TEMP"] = env["TMP"] = str(temp)
    result = subprocess.run(
        [str(worker), "--self-test-aegp-render-options-suite1"],
        cwd=ROOT,
        env=env,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stderr or result.stdout
    report = json.loads(result.stdout)
    assert report["aegp_render_options_suite1"] == "passed"
    assert report["created"] == report["disposed"] == 38
    assert report["live"] == 0
    assert report["receipts_created"] == report["receipts_checked_in"] == 7
    assert report["invalid_operations"] >= 9
    assert report["baseline_argb8"] == [123, 59, 177, 157]
    assert report["time_argb8"] == [72, 8, 24, 56]
    assert report["downsample_argb8"] == [170, 110, 235, 198]
    assert report["roi_outside_argb8"] == [0, 0, 0, 0]
    assert report["roi_inside_argb8"] == [134, 76, 177, 162]
    assert report["field_excluded_argb8"] == [0, 0, 0, 0]
    assert report["matte_argb8"] == [255, 59, 177, 157]
    assert report["argb16"] == [31611, 15163, 45489, 40349]
    assert report["argb32f"] == pytest.approx(
        [123 / 255, 59 / 255, 177 / 255, 157 / 255], abs=1e-7
    )

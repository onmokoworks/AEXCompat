"""SDK-built Effect AEX observes an authored camera through the shipping CLI."""

import copy
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from _render_session import HARNESS


ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "tools" / "build-pf-active-camera-probe.ps1"
PROBE = (
    ROOT
    / "target"
    / "pf-active-camera-probe-managed-build"
    / "Release"
    / "pf_active_camera_probe.aex"
)
WORKER = ROOT / "target" / "minihost-build" / "aex_worker.exe"


@pytest.fixture(scope="module")
def probe():
    assert WORKER.is_file(), "Release worker must be built before shipping probe"
    assert HARNESS.is_file(), "Release harness must be built before shipping probe"
    subprocess.run(
        ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(BUILD)],
        cwd=ROOT,
        check=True,
        timeout=180,
        env={**os.environ, "PYTHONUTF8": "1"},
    )
    assert PROBE.is_file()
    return PROBE


def camera_context():
    return {
        "mask_scene": {"masks": []},
        "active_camera": {
            "layer": {"project_id": 1, "object_id": 2807, "generation": 1, "index": 2},
            "anchor": [0.0, 0.0, 0.0],
            "position": [10.0, 20.0, 30.0],
            "scale": [100.0, 100.0, 100.0],
            "rotation_degrees": [0.0, 0.0, 0.0],
            "zoom": 800.0,
            "in_point": {"value": 30, "scale": 30},
            "duration": {"value": 60, "scale": 30},
        },
    }


def render(probe, tmp_path, name, frame, context=None):
    input_image = tmp_path / "input.png"
    if not input_image.exists():
        Image.new("RGBA", (4, 4), (50, 60, 70, 255)).save(input_image)
    request = {
        "schema_version": 1,
        "assignments": [],
        "timing": {"frame": frame, "fps": 30, "duration_frames": 300},
    }
    if context is not None:
        request["host_context"] = context
    request_path = tmp_path / f"{name}.json"
    request_path.write_text(json.dumps(request), encoding="utf-8")
    output = tmp_path / f"{name}.png"
    result = subprocess.run(
        [str(HARNESS), "--render-experimental-request", str(probe),
         str(input_image), str(output), str(request_path)],
        cwd=ROOT,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=60,
    )
    return result, output


def failure_summary(result):
    try:
        report = json.loads(result.stdout)
    except json.JSONDecodeError:
        report = {}
    probe_lines = [line for line in result.stderr.splitlines()
                   if "frame reported error" in line or "classic_render" in line]
    return (
        f"exit={result.returncode} stage={report.get('failure_stage')} "
        f"passed={report.get('passed')} probe={str(probe_lines)[:1500]}"
    )


def test_shipping_camera_absent_present_and_out_of_range(probe, tmp_path):
    absent, absent_output = render(probe, tmp_path, "absent", 45)
    assert absent.returncode == 0, failure_summary(absent)
    assert json.loads(absent.stdout)["passed"] is True
    assert Image.open(absent_output).convert("RGBA").getpixel((0, 0)) == (0, 0, 0, 255)

    present, present_output = render(probe, tmp_path, "present", 45, camera_context())
    assert present.returncode == 0, failure_summary(present)
    report = json.loads(present.stdout)
    assert report["passed"] is True
    assert report["suite_leases_balanced"] is True
    pixels = Image.open(present_output).convert("RGBA")
    assert pixels.getpixel((0, 0)) == (2, 247, 80, 255)
    assert pixels.getpixel((1, 0)) == (10, 20, 30, 255)
    assert pixels.getpixel((2, 0)) == (100, 0, 0, 255)

    outside, outside_output = render(probe, tmp_path, "outside", 0, camera_context())
    assert outside.returncode == 0, failure_summary(outside)
    assert json.loads(outside.stdout)["passed"] is True
    assert Image.open(outside_output).convert("RGBA").getpixel((0, 0)) == (0, 0, 0, 255)


def test_shipping_camera_scale_and_rotation_affect_matrix(probe, tmp_path):
    context = camera_context()
    context["active_camera"]["scale"] = [200.0, 100.0, 100.0]
    context["active_camera"]["rotation_degrees"] = [0.0, 0.0, 90.0]
    result, output = render(probe, tmp_path, "transformed", 45, context)
    assert result.returncode == 0, failure_summary(result)
    assert json.loads(result.stdout)["passed"] is True
    assert Image.open(output).convert("RGBA").getpixel((2, 0)) == (0, 50, 100, 255)


@pytest.mark.parametrize(
    "corruption", ["stale", "foreign", "collision", "singular", "near_singular", "nonfinite"]
)
def test_shipping_camera_rejects_invalid_context_without_output(probe, tmp_path, corruption):
    context = copy.deepcopy(camera_context())
    camera = context["active_camera"]
    if corruption == "stale":
        camera["layer"]["generation"] += 1
    elif corruption == "foreign":
        camera["layer"]["project_id"] += 1
    elif corruption == "collision":
        camera["layer"]["object_id"] = 2002
    elif corruption == "singular":
        camera["scale"][0] = 0.0
    elif corruption == "near_singular":
        camera["layer"]["object_id"] = 2003
        camera["scale"] = [0.01, 0.01, 0.01]
    else:
        camera["zoom"] = float("nan")
    result, output = render(probe, tmp_path, corruption, 45, context)
    assert result.returncode != 0
    assert not output.exists()
    assert "frame reported error" not in result.stderr

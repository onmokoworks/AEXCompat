"""SDK-built Effect AEX observes an authored camera through the shipping CLI."""

import copy
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from _render_session import BROKER, HARNESS


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


def build_probe():
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


@pytest.fixture(scope="module")
def probe(tmp_path_factory, request):
    if getattr(request.config, "workerinput", None) is None:
        return build_probe()

    # Module fixtures run once per xdist worker. Configure/build touches the
    # same Ninja files, so share one successful build across this test session.
    from filelock import FileLock

    marker = tmp_path_factory.getbasetemp().parent / "active-camera-probe.json"
    with FileLock(str(marker) + ".lock"):
        if marker.is_file():
            artifact = Path(json.loads(marker.read_text(encoding="utf-8"))["probe"])
            assert artifact == PROBE and artifact.is_file(), "stale camera probe marker"
            return artifact
        artifact = build_probe()
        # Publish only after the real build has succeeded; failure is not cached.
        marker.write_text(json.dumps({"probe": str(artifact)}), encoding="utf-8")
        return artifact


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
    assert Image.open(output).convert("RGBA").getpixel((2, 0)) == (0, 200, 100, 255)


def oriented_camera_context(animated=False, graph=False):
    context = camera_context()
    camera = context["active_camera"]
    camera["position"] = [0, 20, 30]
    camera["rotation_degrees"] = [0, 0, 90]
    camera["orientation_degrees"] = [90, 0, 0]
    camera["in_point"] = {"value": 0, "scale": 1}
    camera["duration"] = {"value": 10, "scale": 1}
    if animated:
        camera["keyframes"] = []
        for value, orientation in [(2, 0), (4, 90)]:
            camera["keyframes"].append({
                "time": {"value": value, "scale": 2},
                **{key: copy.deepcopy(camera[key]) for key in
                   ("anchor", "position", "scale", "rotation_degrees", "zoom")},
                "orientation_degrees": [orientation, 0, 0],
            })
    if graph:
        context["scene_layers"] = layer_context()["scene_layers"]
    return context


@pytest.mark.parametrize("animated", [False, True])
@pytest.mark.parametrize("graph", [False, True])
def test_shipping_camera_separate_orientation(probe, tmp_path, animated, graph):
    observations = [(45, (0, 20, 30, 255), (0, 100, 0, 255))]
    if animated:
        observations = [(30, (0, 20, 30, 255), (0, 100, 100, 255)),
                        (45, (0, 20, 30, 255), (0, 100, 71, 255)),
                        (60, (0, 20, 30, 255), (0, 100, 0, 255))]
    for frame, translation, basis in observations:
        result, output = render(probe, tmp_path, f"orientation-{frame}", frame,
                                oriented_camera_context(animated, graph))
        assert result.returncode == 0, failure_summary(result)
        report = json.loads(result.stdout)
        assert report["passed"] is True and report["suite_leases_balanced"] is True
        with Image.open(output) as image:
            pixels = image.convert("RGBA")
        assert pixels.size == (4, 4)
        assert pixels.getpixel((0, 0)) == (2, 247, 80, 255)
        assert pixels.getpixel((1, 0)) == translation
        assert pixels.getpixel((2, 0)) == basis
        if graph:
            assert pixels.getpixel((0, 1)) == (241, 0, 0, 255)
            assert pixels.getpixel((1, 1)) == (138, 148, 158, 255)


@pytest.mark.parametrize("animated", [False, True])
@pytest.mark.parametrize("value", [36001.0, float("inf"), float("nan")])
def test_shipping_camera_invalid_orientation_no_output(probe, tmp_path, animated, value):
    context = oriented_camera_context(animated, True)
    target = context["active_camera"]
    if animated:
        target = target["keyframes"][1]
    target["orientation_degrees"][0] = value
    result, output = render(probe, tmp_path, "invalid-orientation", 45, context)
    assert result.returncode != 0
    assert not output.exists()


def test_shipping_camera_orientation_in_one_resident_batch(probe, tmp_path):
    input_path = tmp_path / "input.png"
    Image.new("RGBA", (4, 4), (50, 60, 70, 255)).save(input_path)
    request_path, report_path = tmp_path / "batch.json", tmp_path / "report.json"
    output_dir = tmp_path / "frames"
    context = oriented_camera_context(True, True)
    request_path.write_text(json.dumps({
        "schema_version": 1, "plugin": str(probe),
        "input_frames": [str(input_path)] * 6, "output_directory": str(output_dir),
        "time_scale": 2, "time_step": 1,
        "active_camera": context["active_camera"], "scene_layers": context["scene_layers"],
    }), encoding="utf-8")
    result = subprocess.run(
        [str(BROKER), "render-video-batch", str(request_path), str(report_path)],
        cwd=ROOT, capture_output=True, text=True, encoding="utf-8",
        errors="replace", timeout=60,
    )
    assert result.returncode == 0, failure_summary(result)
    report = json.loads(report_path.read_text(encoding="utf-8"))
    assert report["passed"] is True and report["frames_ok"] == 6
    assert report["session"]["session_clean"] is True
    assert report["session"]["invalidated"] is False
    for index, (translation, basis) in enumerate([
        ((0, 20, 30, 255), (0, 100, 100, 255)),
        ((0, 20, 30, 255), (0, 100, 100, 255)),
        ((0, 20, 30, 255), (0, 100, 100, 255)),
        ((0, 20, 30, 255), (0, 100, 71, 255)),
        ((0, 20, 30, 255), (0, 100, 0, 255)),
        ((0, 20, 30, 255), (0, 100, 0, 255)),
    ]):
        assert report["frames"][index]["status"] == "ok"
        with Image.open(output_dir / f"frame-{index:06}.png") as image:
            pixels = image.convert("RGBA")
        assert pixels.getpixel((0, 0)) == (2, 247, 80, 255)
        assert pixels.getpixel((1, 0)) == translation
        assert pixels.getpixel((2, 0)) == basis
        assert pixels.getpixel((0, 1)) == (241, 0, 0, 255)


def animated_camera_context():
    context = camera_context()
    camera = context["active_camera"]
    camera["in_point"] = {"value": 0, "scale": 30}
    camera["duration"] = {"value": 300, "scale": 30}
    camera["keyframes"] = []
    for value, scale, position, zoom in [
        (1, 1, [10.0, 20.0, 30.0], 800.0),
        (4, 2, [30.0, 40.0, 50.0], 1200.0),
    ]:
        camera["keyframes"].append({
            "time": {"value": value, "scale": scale},
            "anchor": [0.0, 0.0, 0.0],
            "position": position,
            "scale": [100.0, 100.0, 100.0],
            "rotation_degrees": [0.0, 0.0, 0.0],
            "zoom": zoom,
        })
    return context


def test_shipping_camera_keyframes_hold_and_rational_midpoint(probe, tmp_path):
    for frame, translation, zoom in [
        (15, (10, 20, 30, 255), 80),
        (45, (20, 30, 40, 255), 100),
        (75, (30, 40, 50, 255), 120),
    ]:
        result, output = render(
            probe, tmp_path, f"animated-{frame}", frame, animated_camera_context()
        )
        assert result.returncode == 0, failure_summary(result)
        report = json.loads(result.stdout)
        assert report["passed"] is True
        assert report["suite_leases_balanced"] is True
        pixels = Image.open(output).convert("RGBA")
        assert pixels.getpixel((0, 0)) == (2, 247, zoom, 255)
        assert pixels.getpixel((1, 0)) == translation


def test_shipping_camera_animation_in_one_resident_batch(probe, tmp_path):
    assert BROKER.is_file(), "Release broker must be built before batch probe"
    input_path = tmp_path / "input.png"
    Image.new("RGBA", (4, 4), (50, 60, 70, 255)).save(input_path)
    output_dir = tmp_path / "frames"
    request_path = tmp_path / "batch.json"
    report_path = tmp_path / "report.json"
    request_path.write_text(json.dumps({
        "schema_version": 1, "plugin": str(probe),
        "input_frames": [str(input_path)] * 6,
        "output_directory": str(output_dir),
        "time_scale": 2, "time_step": 1,
        "active_camera": animated_camera_context()["active_camera"],
    }), encoding="utf-8")
    result = subprocess.run(
        [str(BROKER), "render-video-batch", str(request_path), str(report_path)],
        cwd=ROOT, capture_output=True, text=True, encoding="utf-8",
        errors="replace", timeout=60,
    )
    assert result.returncode == 0, failure_summary(result)
    report = json.loads(report_path.read_text(encoding="utf-8"))
    assert report["passed"] is True
    assert report["frames_ok"] == 6
    assert report["session"]["session_clean"] is True
    assert report["session"]["invalidated"] is False
    for index, (translation, zoom) in enumerate([
        ((10, 20, 30, 255), 80), ((10, 20, 30, 255), 80),
        ((10, 20, 30, 255), 80), ((20, 30, 40, 255), 100),
        ((30, 40, 50, 255), 120), ((30, 40, 50, 255), 120),
    ]):
        assert report["frames"][index]["status"] == "ok"
        pixels = Image.open(output_dir / f"frame-{index:06}.png").convert("RGBA")
        assert pixels.size == (4, 4)
        assert pixels.getpixel((0, 0)) == (2, 247, zoom, 255)
        assert pixels.getpixel((1, 0)) == translation


@pytest.mark.parametrize("corruption", [
    "one", "three", "equal", "reversed", "zero_scale", "negative_time",
    "past_bound", "end_bound", "singular", "near_singular", "nonfinite", "identity",
])
def test_shipping_camera_rejects_invalid_keyframes(probe, tmp_path, corruption):
    context = animated_camera_context()
    keys = context["active_camera"]["keyframes"]
    if corruption == "one":
        keys.pop()
    elif corruption == "three":
        keys.append(copy.deepcopy(keys[1]))
    elif corruption == "equal":
        keys[1]["time"] = {"value": 2, "scale": 2}
    elif corruption == "reversed":
        keys.reverse()
    elif corruption == "zero_scale":
        keys[1]["time"]["scale"] = 0
    elif corruption == "negative_time":
        keys[0]["time"]["value"] = -1
    elif corruption == "past_bound":
        keys[1]["time"] = {"value": 11, "scale": 1}
    elif corruption == "end_bound":
        keys[1]["time"] = {"value": 10, "scale": 1}
    elif corruption == "singular":
        keys[1]["scale"][0] = 0
    elif corruption == "near_singular":
        keys[1]["scale"] = [0.01, 0.01, 0.01]
    elif corruption == "nonfinite":
        keys[1]["zoom"] = float("inf")
    else:
        keys[1]["layer"] = {"project_id": 1, "object_id": 2808, "generation": 1}
    result, output = render(probe, tmp_path, corruption, 45, context)
    assert result.returncode != 0
    assert not output.exists()
    assert "frame reported error" not in result.stderr


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


def layer_context(chain=False):
    layers = [{
        "layer": {"project_id": 1, "object_id": 2801 + index, "generation": 1, "index": index},
        "parent": None, "anchor": [0, 0, 0], "position": [10, 20, 30],
        "scale": [100, 100, 100], "rotation_degrees": [0, 0, 0], "is_3d": True,
    } for index in range(3 if chain else 1)]
    if chain:
        layers[0]["parent"] = copy.deepcopy(layers[1]["layer"])
        layers[1]["parent"] = copy.deepcopy(layers[2]["layer"])
        layers[1]["position"] = [4, 5, 6]
        layers[1]["scale"] = [200, 300, 100]
        layers[2]["position"] = [7, 8, 9]
        layers[2]["rotation_degrees"] = [0, 0, 90]
    return {"mask_scene": {"masks": []}, "scene_layers": layers}


def animated_layer_context(parent=False):
    context = layer_context()
    child = context["scene_layers"][0]
    child["keyframes"] = []
    for time, position, orientation in [
        ({"value": 2, "scale": 2}, [10, 20, 30], [0, 0, 0]),
        ({"value": 4, "scale": 2}, [30, 40, 50], [0, 0, 0 if parent else 90]),
    ]:
        child["keyframes"].append({
            "time": time, "anchor": [0, 0, 0], "position": position,
            "scale": [100, 100, 100], "rotation_degrees": [0, 0, 0],
            "orientation_degrees": orientation,
        })
    if parent:
        ancestor = copy.deepcopy(layer_context()["scene_layers"][0])
        ancestor["layer"].update(object_id=2802, index=1)
        ancestor["keyframes"] = []
        for value, position, orientation in [(1, [4, 5, 6], 0), (5, [8, 9, 10], 180)]:
            ancestor["keyframes"].append({
                "time": {"value": value, "scale": 2}, "anchor": [0, 0, 0],
                "position": position, "scale": [100, 100, 100],
                "rotation_degrees": [0, 0, 0], "orientation_degrees": [0, 0, orientation],
            })
        child["parent"] = copy.deepcopy(ancestor["layer"])
        context["scene_layers"].append(ancestor)
    return context


def test_shipping_layer_keyframes_hold_and_fractional_orientation(probe, tmp_path):
    for frame, translation, basis in [
        (0, (138, 148, 158, 255), (10, 0, 0, 255)),
        (30, (138, 148, 158, 255), (10, 0, 0, 255)),
        (45, (148, 158, 168, 255), (7, 7, 7, 255)),
        (60, (158, 168, 178, 255), (0, 10, 10, 255)),
        (90, (158, 168, 178, 255), (0, 10, 10, 255)),
    ]:
        result, output = render(probe, tmp_path, f"layer-key-{frame}", frame,
                                animated_layer_context())
        assert result.returncode == 0, failure_summary(result)
        report = json.loads(result.stdout)
        assert report["passed"] is True and report["suite_leases_balanced"] is True
        with Image.open(output) as image:
            pixels = image.convert("RGBA")
        assert pixels.getpixel((0, 1)) == (241, 0, 0, 255)
        assert pixels.getpixel((1, 1)) == translation
        assert pixels.getpixel((2, 1)) == basis


@pytest.mark.parametrize("grandparent", [False, True])
def test_shipping_layer_and_parent_animate_independent_intervals(probe, tmp_path, grandparent):
    context = animated_layer_context(True)
    if grandparent:
        ancestor = copy.deepcopy(layer_context()["scene_layers"][0])
        ancestor["layer"].update(object_id=2803, index=2)
        ancestor["position"] = [1, 2, 3]
        ancestor["keyframes"] = []
        for value, position in [(1, [1, 2, 3]), (2, [3, 4, 5])]:
            ancestor["keyframes"].append({
                "time": {"value": value, "scale": 1}, "anchor": [0, 0, 0],
                "position": position, "scale": [100, 100, 100], "rotation_degrees": [0, 0, 0],
            })
        context["scene_layers"][1]["parent"] = copy.deepcopy(ancestor["layer"])
        context["scene_layers"].append(ancestor)
    for frame, translation, basis in [
        (0, (142, 153, 164, 255), (10, 0, 0, 255)),
        (45, (104, 155, 176, 255), (0, 10, 10, 255)),
        (90, (106, 97, 188, 255), (10, 0, 0, 255)),
    ]:
        result, output = render(probe, tmp_path, f"parent-key-{frame}", frame,
                                context)
        assert result.returncode == 0, failure_summary(result)
        assert json.loads(result.stdout)["suite_leases_balanced"] is True
        with Image.open(output) as image:
            pixels = image.convert("RGBA")
        assert pixels.getpixel((0, 1)) == (241, 242, 243 if grandparent else 0, 255)
        if grandparent:
            shift = {0: (1, 2, 3), 45: (2, 3, 4), 90: (3, 4, 5)}[frame]
            translation = tuple(translation[axis] + shift[axis] for axis in range(3)) + (255,)
        assert pixels.getpixel((1, 1)) == translation
        assert pixels.getpixel((2, 1)) == basis


def test_shipping_layer_keyframes_animate_anchor_scale_rotation(probe, tmp_path):
    context = animated_layer_context()
    keys = context["scene_layers"][0]["keyframes"]
    keys[1].update(anchor=[2, 4, 6], scale=[300, 500, 700],
                   rotation_degrees=[0, 0, 180], orientation_degrees=[0, 0, 0])
    for frame, translation, basis in [
        (0, (138, 148, 158, 255), (10, 0, 0, 255)),
        (45, (154, 156, 156, 255), (0, 30, 20, 255)),
        (90, (164, 188, 136, 255), (30, 0, 0, 255)),
    ]:
        result, output = render(probe, tmp_path, f"all-transform-{frame}", frame, context)
        assert result.returncode == 0, failure_summary(result)
        assert json.loads(result.stdout)["passed"] is True
        with Image.open(output) as image:
            pixels = image.convert("RGBA")
        assert pixels.getpixel((1, 1)) == translation
        assert pixels.getpixel((2, 1)) == basis


@pytest.mark.parametrize("corruption", ["one", "equal", "reversed", "zero_time_scale",
                                      "negative", "end_bound", "nonfinite", "scale", "2d"])
def test_shipping_layer_keyframes_reject_without_output(probe, tmp_path, corruption):
    context = animated_layer_context()
    layer = context["scene_layers"][0]
    keys = layer["keyframes"]
    if corruption == "one":
        layer["keyframes"] = keys[:1]
    elif corruption == "equal":
        keys[1]["time"] = {"value": 1, "scale": 1}
    elif corruption == "reversed":
        keys.reverse()
    elif corruption == "zero_time_scale":
        keys[1]["time"]["scale"] = 0
    elif corruption == "negative":
        keys[0]["time"]["value"] = -1
    elif corruption == "end_bound":
        keys[1]["time"] = {"value": 10, "scale": 1}
    elif corruption == "nonfinite":
        keys[1]["anchor"][0] = float("nan")
    elif corruption == "scale":
        keys[1]["scale"][0] = 0
    else:
        layer["is_3d"] = False
        layer["position"][2] = 0
        for key in keys:
            key["position"][2] = 0
    result, output = render(probe, tmp_path, f"invalid-layer-key-{corruption}", 45, context)
    assert result.returncode != 0
    assert not output.exists()
    assert "frame reported error" not in result.stderr


def test_shipping_layer_keyframes_and_camera_in_one_resident_batch(probe, tmp_path):
    context = animated_layer_context()
    context["active_camera"] = animated_camera_context()["active_camera"]
    input_path = tmp_path / "input.png"
    Image.new("RGBA", (4, 4), (50, 60, 70, 255)).save(input_path)
    output_dir, report_path, request_path = (tmp_path / name for name in
                                            ("frames", "report.json", "batch.json"))
    request_path.write_text(json.dumps({
        "schema_version": 1, "plugin": str(probe), "input_frames": [str(input_path)] * 6,
        "output_directory": str(output_dir), "time_scale": 2, "time_step": 1,
        "scene_layers": context["scene_layers"], "active_camera": context["active_camera"],
    }), encoding="utf-8")
    result = subprocess.run([str(BROKER), "render-video-batch", str(request_path), str(report_path)],
                            cwd=ROOT, capture_output=True, text=True, encoding="utf-8", timeout=60)
    assert result.returncode == 0, failure_summary(result)
    report = json.loads(report_path.read_text(encoding="utf-8"))
    assert report["passed"] is True and report["frames_ok"] == 6
    assert report["session"]["session_clean"] is True
    assert report["session"]["invalidated"] is False
    for index, (translation, basis, zoom) in enumerate([
        ((138, 148, 158, 255), (10, 0, 0, 255), 80),
        ((138, 148, 158, 255), (10, 0, 0, 255), 80),
        ((138, 148, 158, 255), (10, 0, 0, 255), 80),
        ((148, 158, 168, 255), (7, 7, 7, 255), 100),
        ((158, 168, 178, 255), (0, 10, 10, 255), 120),
        ((158, 168, 178, 255), (0, 10, 10, 255), 120),
    ]):
        assert report["frames"][index]["status"] == "ok"
        with Image.open(output_dir / f"frame-{index:06}.png") as image:
            pixels = image.convert("RGBA")
        assert pixels.getpixel((0, 0)) == (2, 247, zoom, 255)
        assert pixels.getpixel((0, 1)) == (241, 0, 0, 255)
        assert pixels.getpixel((1, 1)) == translation
        assert pixels.getpixel((2, 1)) == basis


@pytest.mark.parametrize("chain", [False, True])
def test_shipping_authored_layer_parent_matrix(probe, tmp_path, chain):
    result, output = render(probe, tmp_path, f"layer-{chain}", 45, layer_context(chain))
    assert result.returncode == 0, failure_summary(result)
    assert json.loads(result.stdout)["suite_leases_balanced"] is True
    pixels = Image.open(output).convert("RGBA")
    assert pixels.getpixel((0, 1)) == ((241, 242, 243, 255) if chain else (241, 0, 0, 255))
    assert pixels.getpixel((1, 1)) == ((70, 160, 173, 255) if chain else (138, 148, 158, 255))
    assert pixels.getpixel((2, 1)) == ((0, 30, 20, 255) if chain else (10, 0, 0, 255))
    assert pixels.getpixel((3, 1)) == (1, 0, 0, 255)


def test_shipping_authored_2d_layer_flags_and_transform(probe, tmp_path):
    context = layer_context()
    context["scene_layers"][0]["is_3d"] = False
    context["scene_layers"][0]["position"][2] = 0
    result, output = render(probe, tmp_path, "layer-2d", 45, context)
    assert result.returncode == 0, failure_summary(result)
    pixels = Image.open(output).convert("RGBA")
    assert pixels.getpixel((1, 1)) == (138, 148, 128, 255)
    assert pixels.getpixel((3, 1)) == (0, 0, 0, 255)


@pytest.mark.parametrize("oriented", [False, True])
def test_shipping_authored_layers_in_one_resident_batch(probe, tmp_path, oriented):
    input_path = tmp_path / "input.png"
    Image.new("RGBA", (4, 4), (50, 60, 70, 255)).save(input_path)
    output_dir = tmp_path / "frames"
    request_path = tmp_path / "batch.json"
    report_path = tmp_path / "report.json"
    layers = layer_context(True)["scene_layers"]
    if oriented:
        layers[2]["rotation_degrees"] = [0, 0, 0]
        layers[2]["orientation_degrees"] = [0, 0, 90]
    request_path.write_text(json.dumps({
        "schema_version": 1, "plugin": str(probe), "input_frames": [str(input_path)] * 3,
        "output_directory": str(output_dir), "time_scale": 30, "time_step": 1,
        "scene_layers": layers,
    }), encoding="utf-8")
    result = subprocess.run([str(BROKER), "render-video-batch", str(request_path), str(report_path)],
                            cwd=ROOT, capture_output=True, text=True, encoding="utf-8", timeout=60)
    assert result.returncode == 0, failure_summary(result)
    report = json.loads(report_path.read_text(encoding="utf-8"))
    assert report["passed"] is True
    assert report["frames_ok"] == 3
    assert report["session"]["session_clean"] is True
    assert report["session"]["invalidated"] is False
    for index in range(3):
        pixels = Image.open(output_dir / f"frame-{index:06}.png").convert("RGBA")
        assert pixels.getpixel((0, 1)) == (241, 242, 243, 255)
        assert pixels.getpixel((1, 1)) == (70, 160, 173, 255)


def test_shipping_authored_layer_and_animated_camera(probe, tmp_path):
    context = layer_context()
    context["active_camera"] = animated_camera_context()["active_camera"]
    result, output = render(probe, tmp_path, "layer-camera", 45, context)
    assert result.returncode == 0, failure_summary(result)
    pixels = Image.open(output).convert("RGBA")
    assert pixels.getpixel((0, 0)) == (2, 247, 100, 255)
    assert pixels.getpixel((1, 0)) == (20, 30, 40, 255)
    assert pixels.getpixel((0, 1)) == (241, 0, 0, 255)
    assert pixels.getpixel((1, 1)) == (138, 148, 158, 255)


@pytest.mark.parametrize("case", ["orientation", "noncommuting", "reverse", "all_axes", "parent"])
def test_shipping_authored_orientation_matrix(probe, tmp_path, case):
    context = layer_context(case == "parent")
    layer = context["scene_layers"][0]
    if case == "parent":
        parent = context["scene_layers"][2]
        parent["rotation_degrees"] = [0, 0, 0]
        parent["orientation_degrees"] = [0, 0, 90]
        translation, basis = (70, 160, 173, 255), (0, 30, 20, 255)
    elif case in ("noncommuting", "reverse", "all_axes"):
        layer.update(anchor=[1, 2, 3], scale=[200, 300, 400],
                     rotation_degrees=[90, 0, 0], orientation_degrees=[0, 90, 0])
        # Rx * Oy * S maps anchor (1,2,3) to (12,2,6), not Oy * Rx.
        translation, basis = (126, 146, 152, 255), (0, 0, 20, 255)
        if case == "reverse":
            layer.update(rotation_degrees=[0, 90, 0], orientation_degrees=[90, 0, 0])
            translation, basis = (132, 160, 160, 255), (0, 30, 0, 255)
        elif case == "all_axes":
            layer.update(rotation_degrees=[0, 0, 0], orientation_degrees=[90, 90, 90])
            translation, basis = (126, 142, 160, 255), (0, 0, 0, 255)
    else:
        layer["orientation_degrees"] = [0, 0, 90]
        context["active_camera"] = animated_camera_context()["active_camera"]
        translation, basis = (138, 148, 158, 255), (0, 10, 10, 255)
    result, output = render(probe, tmp_path, case, 45, context)
    assert result.returncode == 0, failure_summary(result)
    assert json.loads(result.stdout)["suite_leases_balanced"] is True
    pixels = Image.open(output).convert("RGBA")
    assert pixels.getpixel((1, 1)) == translation
    assert pixels.getpixel((2, 1)) == basis
    if case == "orientation":
        assert pixels.getpixel((0, 0)) == (2, 247, 100, 255)
        assert pixels.getpixel((1, 0)) == (20, 30, 40, 255)


@pytest.mark.parametrize("corruption", ["nonfinite", "range", "2d"])
def test_shipping_orientation_rejects_without_output(probe, tmp_path, corruption):
    context = layer_context()
    layer = context["scene_layers"][0]
    layer["orientation_degrees"] = [0, 0, 90]
    if corruption == "nonfinite":
        layer["orientation_degrees"][0] = float("nan")
    elif corruption == "range":
        layer["orientation_degrees"][0] = 36001
    else:
        layer["is_3d"] = False
        layer["position"][2] = 0
    result, output = render(probe, tmp_path, corruption, 45, context)
    assert result.returncode != 0
    assert not output.exists()
    assert "frame reported error" not in result.stderr


@pytest.mark.parametrize("corruption", ["cycle", "stale", "foreign", "parent_index", "duplicate",
                                        "singular", "nonfinite", "camera_collision"])
def test_shipping_authored_layer_graph_rejects_without_output(probe, tmp_path, corruption):
    context = layer_context(True)
    layers = context["scene_layers"]
    if corruption == "cycle":
        layers[2]["parent"] = copy.deepcopy(layers[0]["layer"])
    elif corruption == "stale":
        layers[2]["layer"]["generation"] = 2
        layers[1]["parent"]["generation"] = 2
    elif corruption == "foreign":
        layers[2]["layer"]["project_id"] = 2
    elif corruption == "parent_index":
        layers[0]["parent"]["index"] = 2
    elif corruption == "duplicate":
        layers[2]["layer"]["object_id"] = 2801
    elif corruption == "singular":
        layers[2]["scale"][0] = 0
    elif corruption == "nonfinite":
        layers[2]["position"][0] = float("nan")
    else:
        context["active_camera"] = camera_context()["active_camera"]
    result, output = render(probe, tmp_path, corruption, 45, context)
    assert result.returncode != 0
    assert not output.exists()
    assert "frame reported error" not in result.stderr

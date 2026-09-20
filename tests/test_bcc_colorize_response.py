"""BCC Colorize endpoint response, not blanket AE pixel parity."""

import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


CASES = (
    ("red", [255, 255, 0, 0], 0),
    ("blue", [255, 0, 0, 255], 0),
    ("neutral", [255, 255, 0, 0], 100),
)

FIXED_ASSIGNMENTS = (
    (3, 1),
    (8, 1),
    (11, 1),
    (12, 0),
    (13, 0),
    (14, 0),
    (15, 0),
    (22, 0),
    (23, 100),
    (24, 0),
    (26, 1),
    (28, 0),
    (29, 0),
    (37, 1),
    (39, 1),
)


def source_pixels():
    return bytes(
        component
        for y in range(HEIGHT)
        for x in range(WIDTH)
        for value in ((x + 37 * y) % 256,)
        for component in (value, value, value, 255)
    )


def tint(source, channel):
    return bytes(
        component
        for offset in range(0, len(source), 4)
        for component in (
            source[offset] if channel == 0 else 0,
            source[offset] if channel == 1 else 0,
            source[offset] if channel == 2 else 0,
            source[offset + 3],
        )
    )


def assert_colorize_response(red, blue, neutral):
    source = source_pixels()
    assert len(red) == len(blue) == len(neutral) == len(source)
    assert red == tint(source, 0)
    assert blue == tint(source, 2)
    assert red != blue
    assert neutral == source


@pytest.mark.parametrize(
    "fault",
    (
        "copy",
        "fixed",
        "spatial_gradient",
        "wrong_red",
        "red_leak",
        "blue_leak",
        "color_insensitive",
        "nonmonotonic",
        "neutral_changed",
        "alpha",
        "truncated",
    ),
)
def test_colorize_validator_rejects_corruption(fault):
    source = source_pixels()
    red = bytearray(tint(source, 0))
    blue = bytearray(tint(source, 2))
    neutral = bytearray(source)
    assert_colorize_response(red, blue, neutral)
    if fault == "copy":
        red = source
    elif fault == "fixed":
        red = red[:4] * (WIDTH * HEIGHT)
    elif fault == "spatial_gradient":
        red = bytearray(
            component
            for _y in range(HEIGHT)
            for x in range(WIDTH)
            for component in (x, 0, 0, 255)
        )
    elif fault == "wrong_red":
        red[255 * 4] = 254
    elif fault == "red_leak":
        red[1] = 1
    elif fault == "blue_leak":
        blue[0] = 1
    elif fault == "color_insensitive":
        blue = red
    elif fault == "nonmonotonic":
        red[64 * 4] = 62
    elif fault == "neutral_changed":
        neutral = red
    elif fault == "alpha":
        blue[3] = 0
    else:
        blue = blue[:-4]
    with pytest.raises(AssertionError):
        assert_colorize_response(red, blue, neutral)


def test_real_bcc_colorize_response(tmp_path):
    plugin = os.environ.get("AEXCOMPAT_TEST_BCC_COLORIZE")
    if not plugin:
        pytest.skip("set AEXCOMPAT_TEST_BCC_COLORIZE to the installed BCCColorize.aex")
    assert os.name == "nt"
    harness = ROOT / "broker/target/release/aexcompat-harness.exe"

    def run(*args):
        result = subprocess.run(
            [str(harness), "--headless", *map(str, args)],
            cwd=ROOT,
            capture_output=True,
            timeout=None if args[0] == "--inspect-experimental" else 90,
        )
        assert result.returncode == 0, result.stderr.decode("utf-8", errors="replace")
        return json.loads(result.stdout)

    params = {
        parameter["slot"]: parameter
        for parameter in run("--inspect-experimental", plugin)
    }
    assert params[6]["name"] == "Host Layer" and params[6]["kind"] == "layer"
    assert params[8]["name"] == "Input" and params[8]["choices"][0] == "Luma"
    assert params[11]["name"] == "Color Space"
    assert params[11]["choices"] == ["RGB", "HSL", "HSV"]
    assert params[16]["name"] == "Color 1" and params[16]["kind"] == "color"
    assert params[21]["name"] == "Color 6" and params[21]["kind"] == "color"
    assert params[36]["name"] == "Mix with Original"

    source = tmp_path / "source.png"
    Image.frombytes("RGBA", (WIDTH, HEIGHT), source_pixels()).save(source)
    outputs = []
    for name, endpoint, mix in CASES:
        request = tmp_path / f"{name}.json"
        output = tmp_path / f"{name}.png"
        assignments = [{"slot": 6, "layer": str(source)}]
        assignments.extend(
            {"slot": slot, "value": value} for slot, value in FIXED_ASSIGNMENTS
        )
        assignments.extend(
            (
                {"slot": 16, "color": [255, 0, 0, 0]},
                {"slot": 21, "color": endpoint},
                {"slot": 36, "value": mix},
            )
        )
        request.write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "timing": {"frame": 0, "fps": 30, "duration_frames": 300},
                    "assignments": assignments,
                }
            ),
            encoding="utf-8",
        )
        report = run(
            "--render-experimental-smart-request", plugin, source, output, request
        )
        assert report["passed"] and report["output_pixels_valid"]
        assert report["render_path"] == "smartfx"
        assert report["worker_classification"] == "ok"
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            pixels = image.convert("RGBA").tobytes()
        argb = bytearray(len(pixels))
        argb[0::4], argb[1::4] = pixels[3::4], pixels[0::4]
        argb[2::4], argb[3::4] = pixels[1::4], pixels[2::4]
        assert hashlib.sha256(argb).hexdigest() == report["output_sha256"]
        outputs.append(pixels)
    assert_colorize_response(*outputs)

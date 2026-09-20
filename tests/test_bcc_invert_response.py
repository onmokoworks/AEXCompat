"""Exact tested BCC Invert channel behavior, not blanket AE pixel parity."""

import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


CASES = (
    ("rgb", 1, 0),
    ("red", 4, 0),
    ("mix100", 1, 100),
)


def source_pixels():
    return bytes(
        component
        for y in range(HEIGHT)
        for x in range(WIDTH)
        for component in (x, (2 * x + y) % 256, (x + 3 * y) % 256, 255)
    )


def expected_rgb(source):
    return bytes(
        component
        for offset in range(0, len(source), 4)
        for component in (
            255 - source[offset],
            255 - source[offset + 1],
            255 - source[offset + 2],
            source[offset + 3],
        )
    )


def expected_red(source):
    return bytes(
        component
        for offset in range(0, len(source), 4)
        for component in (
            255 - source[offset],
            source[offset + 1],
            source[offset + 2],
            source[offset + 3],
        )
    )


def assert_invert_response(rgb, red, mix100):
    source = source_pixels()
    assert len(rgb) == len(red) == len(mix100) == len(source)
    assert rgb == expected_rgb(source)
    assert red == expected_red(source)
    assert mix100 == source


@pytest.mark.parametrize(
    "fault",
    (
        "copy",
        "fixed",
        "wrong_rgb",
        "red_leak",
        "red_ignored",
        "alpha",
        "mix_changed",
        "truncated",
    ),
)
def test_invert_validator_rejects_corruption(fault):
    source = source_pixels()
    rgb = bytearray(expected_rgb(source))
    red = bytearray(expected_red(source))
    mix100 = bytearray(source)
    assert_invert_response(rgb, red, mix100)
    if fault == "copy":
        rgb = source
    elif fault == "fixed":
        rgb = rgb[:4] * (WIDTH * HEIGHT)
    elif fault == "wrong_rgb":
        rgb[2] ^= 1
    elif fault == "red_leak":
        red[1] ^= 1
    elif fault == "red_ignored":
        red = source
    elif fault == "alpha":
        rgb[3] = 0
    elif fault == "mix_changed":
        mix100[0] ^= 1
    else:
        red = red[:-4]
    with pytest.raises(AssertionError):
        assert_invert_response(rgb, red, mix100)


def test_real_bcc_invert_response(tmp_path):
    plugin = os.environ.get("AEXCOMPAT_TEST_BCC_INVERT")
    if not plugin:
        pytest.skip("set AEXCOMPAT_TEST_BCC_INVERT to the installed BCCInvert.aex")
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

    params = {parameter["slot"]: parameter for parameter in run("--inspect-experimental", plugin)}
    assert params[6]["name"] == "Host Layer" and params[6]["kind"] == "layer"
    assert params[8]["name"] == "Channels" and params[8]["value"] == 1
    assert params[8]["choices"][:4] == ["RGB", "RGBA", "Alpha", "Red"]
    assert params[9]["name"] == "Mix with Original" and params[9]["value"] == 0

    source = tmp_path / "source.png"
    Image.frombytes("RGBA", (WIDTH, HEIGHT), source_pixels()).save(source)
    outputs = []
    for name, channels, mix in CASES:
        request = tmp_path / f"{name}.json"
        output = tmp_path / f"{name}.png"
        request.write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "timing": {"frame": 0, "fps": 30, "duration_frames": 300},
                    "assignments": [
                        {"slot": 6, "layer": str(source)},
                        {"slot": 8, "value": channels},
                        {"slot": 9, "value": mix},
                    ],
                }
            ),
            encoding="utf-8",
        )
        report = run("--render-experimental-smart-request", plugin, source, output, request)
        assert report["passed"] and report["output_pixels_valid"]
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            pixels = image.convert("RGBA").tobytes()
        argb = bytearray(len(pixels))
        argb[0::4], argb[1::4] = pixels[3::4], pixels[0::4]
        argb[2::4], argb[3::4] = pixels[1::4], pixels[2::4]
        assert hashlib.sha256(argb).hexdigest() == report["output_sha256"]
        outputs.append(pixels)
    assert_invert_response(*outputs)

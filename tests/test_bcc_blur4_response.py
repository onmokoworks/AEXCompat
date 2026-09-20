"""BCC VR Blur strength and axis response, not an exact kernel or AE oracle."""

import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image

from test_bcc_blur_response import source_pixels
from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


CASES = (
    ("zero", 0, 100, 100),
    ("small", 2, 100, 100),
    ("large", 20, 100, 100),
    ("horizontal", 20, 100, 0),
    ("vertical", 20, 0, 100),
)


def assert_frame(raw):
    assert len(raw) == WIDTH * HEIGHT * 4
    assert raw[3::4] == bytes([255]) * (WIDTH * HEIGHT)
    assert raw[0::4] == raw[1::4] == raw[2::4]


def transition_width(raw, horizontal):
    assert_frame(raw)
    red = raw[0::4]
    line = red[72 * WIDTH : 73 * WIDTH] if horizontal else red[128::WIDTH]
    start, end = (64, 192) if horizontal else (36, 108)
    assert line[0] == line[-1] == 0
    assert line[(start + end) // 2] == 255
    gray = [index for index, value in enumerate(line) if 0 < value < 255]
    assert gray
    assert all(
        start - 32 <= index < start + 32 or end - 32 <= index < end + 32
        for index in gray
    )
    assert any(index < start for index in gray)
    assert any(start <= index for index in gray if index < start + 32)
    assert any(end - 32 <= index < end for index in gray)
    assert any(index >= end for index in gray)
    rising = line[start - 32 : start + 32]
    falling = line[end - 32 : end + 32]
    assert all(left <= right for left, right in zip(rising, rising[1:]))
    assert all(left >= right for left, right in zip(falling, falling[1:]))
    return len(gray)


def assert_profile_coverage(raw, horizontal, index):
    red = raw[0::4]
    line = (
        red[index * WIDTH : (index + 1) * WIDTH]
        if horizontal
        else red[index::WIDTH]
    )
    center = 128 if horizontal else 72
    assert line[0] == line[-1] == 0
    assert line[center] == max(line) > 0
    assert any(0 < value < 255 for value in line)
    assert all(left <= right for left, right in zip(line[:center], line[1 : center + 1]))
    assert all(left >= right for left, right in zip(line[center:], line[center + 1 :]))


def assert_combined_coverage(raw):
    red = raw[0::4]
    for y in (36, 40, 60, 84, 103, 107):
        assert_profile_coverage(raw, True, y)
    for x in (64, 72, 96, 160, 176, 191):
        assert_profile_coverage(raw, False, x)
    for y in (0, HEIGHT - 1):
        assert set(red[y * WIDTH : (y + 1) * WIDTH]) == {0}
    for x in (0, WIDTH - 1):
        assert set(red[x::WIDTH]) == {0}


def assert_horizontal_coverage(raw):
    red = raw[0::4]
    for y in (36, 40, 60, 84, 103, 107):
        assert_profile_coverage(raw, True, y)
    for y in (0, 35, 108, HEIGHT - 1):
        assert set(red[y * WIDTH : (y + 1) * WIDTH]) == {0}


def assert_vertical_coverage(raw):
    red = raw[0::4]
    for x in (64, 72, 96, 160, 176, 191):
        assert_profile_coverage(raw, False, x)
    for x in (0, 63, 192, WIDTH - 1):
        assert set(red[x::WIDTH]) == {0}


def assert_axis_response(raw, horizontal):
    assert_frame(raw)
    red = raw[0::4]
    source = source_pixels()[0::4]
    inactive = red[128::WIDTH] if horizontal else red[72 * WIDTH : 73 * WIDTH]
    expected = source[128::WIDTH] if horizontal else source[72 * WIDTH : 73 * WIDTH]
    assert inactive == expected
    assert transition_width(raw, horizontal) > 0


def assert_blur4_response(zero, small, large, horizontal, vertical):
    assert zero == source_pixels()
    small_x = transition_width(small, True)
    small_y = transition_width(small, False)
    large_x = transition_width(large, True)
    large_y = transition_width(large, False)
    assert large_x > small_x > 0
    assert large_y > small_y > 0
    assert_combined_coverage(small)
    assert_combined_coverage(large)
    assert_axis_response(horizontal, True)
    assert_axis_response(vertical, False)
    assert_horizontal_coverage(horizontal)
    assert_vertical_coverage(vertical)


def _profile(position, start, end, width):
    if width == 0:
        return 255 if start <= position < end else 0
    if position < start - width or position >= end + width:
        return 0
    if position < start + width:
        return round(255 * (position - (start - width) + 1) / (2 * width + 1))
    if position < end - width:
        return 255
    return round(255 * (end + width - position) / (2 * width + 1))


def synthetic(horizontal_width, vertical_width):
    raw = bytearray()
    for y in range(HEIGHT):
        vertical = _profile(y, 36, 108, vertical_width)
        for x in range(WIDTH):
            horizontal = _profile(x, 64, 192, horizontal_width)
            value = (horizontal * vertical + 127) // 255
            raw.extend((value, value, value, 255))
    return raw


@pytest.mark.parametrize(
    "fault",
    (
        "zero_changed",
        "copy",
        "reversed",
        "alpha",
        "color",
        "cross_axis",
        "no_horizontal",
        "no_vertical",
        "center_cross",
        "fixed",
        "truncated",
    ),
)
def test_blur4_validator_rejects_corruption(fault):
    zero = source_pixels()
    small = synthetic(2, 2)
    large = synthetic(20, 20)
    horizontal = synthetic(20, 0)
    vertical = synthetic(0, 20)
    assert_blur4_response(zero, small, large, horizontal, vertical)
    if fault == "zero_changed":
        zero = small
    elif fault == "copy":
        large = small
    elif fault == "reversed":
        small, large = large, small
    elif fault == "alpha":
        large[3] = 0
    elif fault == "color":
        large[1] ^= 1
    elif fault == "cross_axis":
        offset = (35 * WIDTH + 128) * 4
        horizontal[offset : offset + 3] = bytes((64, 64, 64))
    elif fault == "no_horizontal":
        horizontal = source_pixels()
    elif fault == "no_vertical":
        vertical = source_pixels()
    elif fault == "center_cross":
        original = large
        large = bytearray((0, 0, 0, 255)) * (WIDTH * HEIGHT)
        row_start = 72 * WIDTH * 4
        large[row_start : row_start + WIDTH * 4] = original[
            row_start : row_start + WIDTH * 4
        ]
        for y in range(HEIGHT):
            offset = (y * WIDTH + 128) * 4
            large[offset : offset + 4] = original[offset : offset + 4]
    elif fault == "fixed":
        large = bytearray((0, 0, 0, 255)) * (WIDTH * HEIGHT)
    else:
        vertical = vertical[:-4]
    with pytest.raises(AssertionError):
        assert_blur4_response(zero, small, large, horizontal, vertical)


def test_real_bcc_blur4_response(tmp_path):
    plugin = os.environ.get("AEXCOMPAT_TEST_BCC_BLUR4")
    if not plugin:
        pytest.skip("set AEXCOMPAT_TEST_BCC_BLUR4 to the installed BCCBlur4.aex")
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
    for slot, name in ((12, "Master Blur"), (13, "Blur X"), (14, "Blur Y")):
        assert params[slot]["name"] == name
        assert params[slot]["minimum"] <= 0 < 20 <= params[slot]["maximum"]

    source = tmp_path / "source.png"
    Image.frombytes("RGBA", (WIDTH, HEIGHT), source_pixels()).save(source)
    outputs = []
    for name, master, blur_x, blur_y in CASES:
        request = tmp_path / f"{name}.json"
        output = tmp_path / f"{name}.png"
        request.write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "timing": {"frame": 0, "fps": 30, "duration_frames": 300},
                    "assignments": [
                        {"slot": 6, "layer": str(source)},
                        {"slot": 12, "value": master},
                        {"slot": 13, "value": blur_x},
                        {"slot": 14, "value": blur_y},
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
    assert_blur4_response(*outputs)

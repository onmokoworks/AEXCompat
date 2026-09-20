"""BCC Posterize level response, not an exact threshold formula or AE oracle."""

import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image

from test_bcc_invert_response import source_pixels
from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


CASES = (
    ("levels2", 2, 0),
    ("levels8", 8, 0),
    ("neutral", 8, 100),
)

FIXED_ASSIGNMENTS = (
    (8, 1),
    (12, 1),
    (13, 0),
    (14, 0),
    (15, 0),
    (16, 1),
    (17, 0),
    (18, 0),
    (19, 0),
    (20, 0),
    (21, 0),
    (23, 0),
    (24, 0),
    (25, 0),
    (26, 1),
    (27, 0),
)


def palette(levels):
    return [int(255 * index / (levels - 1)) for index in range(levels)]


def quantize(source, levels):
    values = palette(levels)
    return bytes(
        component
        for offset in range(0, len(source), 4)
        for component in (
            values[source[offset] * levels // 256],
            values[source[offset + 1] * levels // 256],
            values[source[offset + 2] * levels // 256],
            source[offset + 3],
        )
    )


def assert_quantized(source, output, levels):
    assert len(output) == len(source)
    assert output != source
    assert output[3::4] == source[3::4]
    expected_palette = palette(levels)
    for channel in range(3):
        assert sorted(set(output[channel::4])) == expected_palette
        mapping = {}
        for original, result in zip(source[channel::4], output[channel::4]):
            if original in mapping:
                assert mapping[original] == result
            else:
                mapping[original] = result
        assert sorted(mapping) == list(range(256))
        mapped = [mapping[value] for value in range(256)]
        assert mapped[0] == 0 and mapped[-1] == 255
        assert all(left <= right for left, right in zip(mapped, mapped[1:]))


def assert_posterize_response(levels2, levels8, neutral):
    source = source_pixels()
    assert_quantized(source, levels2, 2)
    assert_quantized(source, levels8, 8)
    assert levels2 != levels8
    assert neutral == source


@pytest.mark.parametrize(
    "fault",
    (
        "copy",
        "fixed",
        "wrong_palette",
        "channel_leak",
        "nonmonotonic",
        "level_insensitive",
        "neutral_changed",
        "alpha",
        "truncated",
    ),
)
def test_posterize_validator_rejects_corruption(fault):
    source = source_pixels()
    levels2 = bytearray(quantize(source, 2))
    levels8 = bytearray(quantize(source, 8))
    neutral = bytearray(source)
    assert_posterize_response(levels2, levels8, neutral)
    if fault == "copy":
        levels8 = source
    elif fault == "fixed":
        levels2 = levels2[:4] * (WIDTH * HEIGHT)
    elif fault == "wrong_palette":
        levels8[0] = 17
    elif fault == "channel_leak":
        levels8[0::4] = levels8[1::4]
    elif fault == "nonmonotonic":
        for offset in range(0, len(source), 4):
            if source[offset] == 64:
                levels8[offset] = 218
    elif fault == "level_insensitive":
        levels2 = levels8
    elif fault == "neutral_changed":
        neutral = levels8
    elif fault == "alpha":
        levels8[3] = 0
    else:
        levels8 = levels8[:-4]
    with pytest.raises(AssertionError):
        assert_posterize_response(levels2, levels8, neutral)


def test_real_bcc_posterize_response(tmp_path):
    plugin = os.environ.get("AEXCOMPAT_TEST_BCC_POSTERIZE")
    if not plugin:
        pytest.skip("set AEXCOMPAT_TEST_BCC_POSTERIZE to the installed BCCPosterize.aex")
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
    for slot, name in ((9, "Red Levels"), (10, "Green Levels"), (11, "Blue Levels")):
        assert params[slot]["name"] == name
        assert params[slot]["minimum"] <= 2 < 8 <= params[slot]["maximum"]
    assert params[29]["name"] == "Mix with Original"

    source = tmp_path / "source.png"
    Image.frombytes("RGBA", (WIDTH, HEIGHT), source_pixels()).save(source)
    outputs = []
    for name, levels, mix in CASES:
        request = tmp_path / f"{name}.json"
        output = tmp_path / f"{name}.png"
        assignments = [{"slot": 6, "layer": str(source)}]
        assignments.extend({"slot": slot, "value": value} for slot, value in FIXED_ASSIGNMENTS)
        assignments.extend({"slot": slot, "value": levels} for slot in (9, 10, 11))
        assignments.append({"slot": 29, "value": mix})
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
    assert_posterize_response(*outputs)

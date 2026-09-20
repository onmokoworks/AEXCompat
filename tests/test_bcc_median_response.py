"""Bounded BCC Median impulse removal, not every mode or exact AE parity."""

import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image

from test_median_pro_response import inputs
from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


FIXED_ASSIGNMENTS = (
    (8, 3),   # A R G B Separately
    (9, 1),   # Luminance selector (inactive in this mode)
    (10, 1),  # Preserve Alpha
    (11, 1),  # Lock Width to Length
    (12, 3),
    (13, 3),
    (15, 0),
    (16, 50),
    (17, 0),
    (18, 3),  # Compositing: None
    (20, 1),  # Apply: None
)


def assert_median_response(active, neutral):
    original, clean = inputs()
    assert len(active) == len(neutral) == len(original) == len(clean)
    assert active == clean
    assert neutral == original


@pytest.mark.parametrize(
    "fault",
    (
        "bypass",
        "background",
        "boundary",
        "impulse",
        "neutral",
        "alpha",
        "fixed",
        "truncated",
    ),
)
def test_median_validator_rejects_corruption(fault):
    original, clean = inputs()
    active = bytearray(clean)
    neutral = bytearray(original)
    assert_median_response(active, neutral)
    if fault == "bypass":
        active = original
    elif fault == "background":
        active[0] ^= 1
    elif fault == "boundary":
        active[(WIDTH // 2) * 4] ^= 1
    elif fault == "impulse":
        offset = (32 * WIDTH + 32) * 4
        active[offset : offset + 3] = bytes((0, 0, 0))
    elif fault == "neutral":
        neutral = clean
    elif fault == "alpha":
        active[3] = 0
    elif fault == "fixed":
        active = active[:4] * (WIDTH * HEIGHT)
    else:
        active = active[:-4]
    with pytest.raises(AssertionError):
        assert_median_response(active, neutral)


def test_real_bcc_median_response(tmp_path):
    plugin = os.environ.get("AEXCOMPAT_TEST_BCC_MEDIAN")
    if not plugin:
        pytest.skip("set AEXCOMPAT_TEST_BCC_MEDIAN to the installed BCCMedian.aex")
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
    for slot, name in (
        (8, "Mode"),
        (9, "Channel"),
        (10, "Preserve Alpha"),
        (11, "Lock Width to Length"),
        (12, "Median Length"),
        (13, "Median Width"),
        (16, "Median Level"),
        (17, "Softness"),
        (18, "Compositing"),
        (19, "Mix with Original"),
        (20, "Apply"),
    ):
        assert params[slot]["name"] == name
    assert params[8]["choices"][2] == "A R G B Separately"
    assert params[18]["choices"][2] == "None"

    original, _ = inputs()
    source = tmp_path / "source.png"
    Image.frombytes("RGBA", (WIDTH, HEIGHT), original).save(source)
    outputs = []
    for name, mix in (("active", 0), ("neutral", 100)):
        request = tmp_path / f"{name}.json"
        output = tmp_path / f"{name}.png"
        assignments = [{"slot": 6, "layer": str(source)}]
        assignments.extend({"slot": slot, "value": value} for slot, value in FIXED_ASSIGNMENTS)
        assignments.append({"slot": 19, "value": mix})
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
    assert_median_response(*outputs)

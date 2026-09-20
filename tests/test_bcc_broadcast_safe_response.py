"""BCC Broadcast Safe RGB clip response, not blanket AE pixel parity."""

import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image

from test_bcc_invert_response import source_pixels
from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


CASES = (
    ("clip25", 11, 25),
    ("clip50", 11, 50),
    ("neutral", 1, 50),
)

FIXED_ASSIGNMENTS = (
    (3, 1),
    (9, 1),
    (20, 0),
    (21, 0),
    (39, 2),
    (41, 1),
)


def hard_clip(source, limit):
    return bytes(
        component
        for offset in range(0, len(source), 4)
        for component in (
            min(source[offset], limit),
            min(source[offset + 1], limit),
            min(source[offset + 2], limit),
            source[offset + 3],
        )
    )


def assert_broadcast_safe_response(clip25, clip50, neutral):
    source = source_pixels()
    assert len(clip25) == len(clip50) == len(neutral) == len(source)
    assert clip25 == hard_clip(source, 64)
    assert clip50 == hard_clip(source, 128)
    assert clip25 != clip50 != source
    assert neutral == source


@pytest.mark.parametrize(
    "fault",
    (
        "copy",
        "fixed",
        "wrong_cap",
        "low_damage",
        "channel_leak",
        "nonmonotonic",
        "limit_insensitive",
        "neutral_changed",
        "alpha",
        "truncated",
    ),
)
def test_broadcast_safe_validator_rejects_corruption(fault):
    source = source_pixels()
    clip25 = bytearray(hard_clip(source, 64))
    clip50 = bytearray(hard_clip(source, 128))
    neutral = bytearray(source)
    assert_broadcast_safe_response(clip25, clip50, neutral)
    if fault == "copy":
        clip50 = source
    elif fault == "fixed":
        clip25 = clip25[:4] * (WIDTH * HEIGHT)
    elif fault == "wrong_cap":
        clip50[255 * 4] = 127
    elif fault == "low_damage":
        clip25[0] = 1
    elif fault == "channel_leak":
        clip50[0::4] = clip50[1::4]
    elif fault == "nonmonotonic":
        for offset in range(0, len(source), 4):
            if source[offset] == 64:
                clip25[offset] = 62
    elif fault == "limit_insensitive":
        clip25 = clip50
    elif fault == "neutral_changed":
        neutral = clip50
    elif fault == "alpha":
        clip50[3] = 0
    else:
        clip25 = clip25[:-4]
    with pytest.raises(AssertionError):
        assert_broadcast_safe_response(clip25, clip50, neutral)


def test_real_bcc_broadcast_safe_response(tmp_path):
    plugin = os.environ.get("AEXCOMPAT_TEST_BCC_BROADCAST_SAFE")
    if not plugin:
        pytest.skip(
            "set AEXCOMPAT_TEST_BCC_BROADCAST_SAFE to the installed "
            "BCCBroadcastSafe.aex"
        )
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
    assert params[8]["name"] == "Broadcast Standards"
    assert params[8]["choices"][0] == "Neutral"
    assert params[8]["choices"][-1] == "Custom"
    assert params[9]["name"] == "Color Mode" and params[9]["choices"][0] == "RGB"
    for slot, name in ((18, "High Clip"), (19, "High Knee")):
        assert params[slot]["name"] == name
        assert params[slot]["minimum"] <= 25 < 50 <= params[slot]["maximum"]
    assert params[41]["name"] == "View Mode"
    assert params[41]["choices"][0] == "Final Output"

    source = tmp_path / "source.png"
    Image.frombytes("RGBA", (WIDTH, HEIGHT), source_pixels()).save(source)
    outputs = []
    for name, standard, limit in CASES:
        request = tmp_path / f"{name}.json"
        output = tmp_path / f"{name}.png"
        assignments = [{"slot": 6, "layer": str(source)}]
        assignments.extend(
            {"slot": slot, "value": value} for slot, value in FIXED_ASSIGNMENTS
        )
        assignments.extend(
            (
                {"slot": 8, "value": standard},
                {"slot": 18, "value": limit},
                {"slot": 19, "value": limit},
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
    assert_broadcast_safe_response(*outputs)

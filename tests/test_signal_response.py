"""Deterministic Signal control response; not an exact AE oracle."""

import hashlib
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH, argb


NOISE_STRENGTH = 40
ROW_BYTES = WIDTH * 4
STABLE_PIXEL_COUNT = WIDTH * (HEIGHT - 1)

# Keep every unrelated stochastic or signal-degrading control neutral.  Popup
# values are one-based, so Low-Pass filter out=1 is the advertised Off choice.
COMMON_ASSIGNMENTS = {
    1: 1,
    2: 1,
    3: 1,
    4: 100000,
    7: 0,
    8: 0,
    9: 0,
    10: 0,
    11: 1,
    12: 0,
    14: 0,
    18: 0,
    19: 0,
    27: 0,
    28: 0,
    29: 0,
    30: 0,
    33: 0,
    34: 1,
    35: 0,
    36: 0,
    37: 0,
    38: 0,
    39: 0,
    40: 0,
}

PARAMETER_SCHEMA = {
    1: ("Random seed", "float", 1, 1000000),
    2: ("Signal strength", "float", 1, 200),
    3: ("Signal amplification", "float", 0, 100),
    4: ("Cutoff filter", "float", 1, 100000),
    5: ("Luma noise", "float", 0, 400),
    6: ("Chroma noise", "float", 0, 400),
    7: ("Hue Noise", "float", 0, 200),
    8: ("Chroma loss", "float", 0, 1),
    9: ("Sharpen", "float", 0, 100),
    10: ("Device-to-device connections", "float", 0, 100),
    11: ("Emulating VHS", "integer", 1, 3),
    12: ("Destruction button", "integer", 0, 1),
    14: ("Turn On/Off", "integer", 0, 1),
    18: ("Quantize", "float", 0, 100),
    19: ("Tape Errors", "integer", 0, 1),
    27: ("Thick distortion", "float", 0, 100),
    28: ("Fine distotrion", "float", 0, 10),
    29: ("Distortion speed", "float", 0, 200),
    30: ("Roll speed", "float", 0, 200),
    33: ("Low-Pass filter in", "integer", 0, 1),
    34: ("Low-Pass filter out", "integer", 1, 3),
    35: ("Phase shift", "float", 0, 360),
    36: ("Phase shift offset", "float", 0, 360),
    37: ("S-video out", "integer", 0, 1),
    38: ("Chroma vertical blend", "integer", 0, 1),
    39: ("Sharpen On/Off", "integer", 0, 1),
    40: ("Scanlines blur", "float", 0, 100),
}


def source_pixels(reverse=False):
    left, right = ((192, 64) if reverse else (64, 192))
    return bytes(
        component
        for _y in range(HEIGHT)
        for x in range(WIDTH)
        for component in ((left if x < WIDTH // 2 else right,) * 3 + (255,))
    )


def assignments(luma, chroma):
    values = {**COMMON_ASSIGNMENTS, 5: luma, 6: chroma}
    return ([{"slot": slot, "value": value} for slot, value in sorted(values.items())],
            values)


def stable_rgb(pixels):
    """Ignore only RGB row zero; alpha is validated over the complete frame."""
    return bytes(
        component
        for offset in range(ROW_BYTES, len(pixels), 4)
        for component in pixels[offset:offset + 3]
    )


def changed_rgb_pixels(actual, expected):
    return sum(
        actual[offset:offset + 3] != expected[offset:offset + 3]
        for offset in range(ROW_BYTES, len(expected), 4)
    )


def region_mean_luma(pixels, *, left):
    start, end = ((16, WIDTH // 2 - 16)
                  if left else (WIDTH // 2 + 16, WIDTH - 16))
    values = []
    for y in range(1, HEIGHT):
        for x in range(start, end):
            offset = (y * WIDTH + x) * 4
            red, green, blue = pixels[offset:offset + 3]
            values.append((77 * red + 150 * green + 29 * blue) / 256)
    return sum(values) / len(values)


def channel_spans(pixels):
    return [
        max(pixels[offset:offset + 3]) - min(pixels[offset:offset + 3])
        for offset in range(ROW_BYTES, len(pixels), 4)
    ]


def assert_signal_response(luma_forward, luma_reverse, luma_repeat, chroma_forward):
    expected_size = WIDTH * HEIGHT * 4
    frames = (luma_forward, luma_reverse, luma_repeat, chroma_forward)
    for pixels in frames:
        assert len(pixels) == expected_size
        assert pixels[3::4] == bytes([255]) * (WIDTH * HEIGHT)

    forward_source = source_pixels()
    reverse_source = source_pixels(reverse=True)
    assert stable_rgb(luma_forward) == stable_rgb(luma_repeat)
    assert stable_rgb(luma_forward) != stable_rgb(luma_reverse)
    assert changed_rgb_pixels(luma_forward, forward_source) > STABLE_PIXEL_COUNT // 20
    assert changed_rgb_pixels(luma_reverse, reverse_source) > STABLE_PIXEL_COUNT // 20
    assert changed_rgb_pixels(chroma_forward, forward_source) > STABLE_PIXEL_COUNT // 20

    forward_gap = (region_mean_luma(luma_forward, left=False)
                   - region_mean_luma(luma_forward, left=True))
    reverse_gap = (region_mean_luma(luma_reverse, left=True)
                   - region_mean_luma(luma_reverse, left=False))
    assert forward_gap > 8
    assert reverse_gap > 8

    # Signal routes both controls through its internal VHS/RGB pipeline, so
    # "Luma" is not a promise of grayscale output.  What is stable and useful
    # for compatibility is that equal-strength Luma and Chroma requests have
    # substantially different spatial/color footprints.
    luma_spans = channel_spans(luma_forward)
    chroma_spans = channel_spans(chroma_forward)
    control_difference = sum(
        luma_forward[offset:offset + 3] != chroma_forward[offset:offset + 3]
        for offset in range(ROW_BYTES, len(luma_forward), 4)
    )
    assert control_difference > STABLE_PIXEL_COUNT // 10
    luma_mean_span = sum(luma_spans) / len(luma_spans)
    chroma_mean_span = sum(chroma_spans) / len(chroma_spans)
    assert abs(luma_mean_span - chroma_mean_span) > 8
    assert stable_rgb(chroma_forward) != stable_rgb(luma_forward)


def synthetic_luma(reverse=False, phase=0):
    source = source_pixels(reverse)
    result = bytearray(source)
    for y in range(HEIGHT):
        for x in range(WIDTH):
            offset = (y * WIDTH + x) * 4
            value = source[offset]
            delta = 48 if (13 * x + 7 * y + phase) % 2 else -48
            result[offset:offset + 3] = bytes((value + delta, value, value - delta))
    return bytes(result)


def synthetic_chroma():
    source = source_pixels()
    result = bytearray(source)
    for y in range(HEIGHT):
        for x in range(WIDTH):
            offset = (y * WIDTH + x) * 4
            delta = 12 if (x + y) % 2 else -12
            value = source[offset]
            result[offset:offset + 3] = bytes((value + delta, value, value - delta))
    return bytes(result)


def synthetic_cases():
    forward = synthetic_luma()
    repeat = bytearray(forward)
    repeat[0] ^= 1  # The plug-in's known process-dependent difference is RGB row zero.
    return forward, synthetic_luma(reverse=True), bytes(repeat), synthetic_chroma()


def test_signal_validator_accepts_rgb_row_zero_repeat_difference():
    assert_signal_response(*synthetic_cases())


@pytest.mark.parametrize(
    "fault",
    (
        "copy",
        "source_ignored",
        "wrong_half_order",
        "chroma_ignored",
        "chroma_gray",
        "repeat_drift",
        "alpha",
        "flat",
        "truncated",
    ),
)
def test_signal_validator_rejects_corruption(fault):
    forward, reverse, repeat, chroma = synthetic_cases()
    assert_signal_response(forward, reverse, repeat, chroma)

    if fault == "copy":
        forward = source_pixels()
        repeat = bytearray(forward)
        repeat[0] ^= 1
    elif fault == "source_ignored":
        reverse = forward
    elif fault == "wrong_half_order":
        reverse = bytearray(forward)
        reverse[ROW_BYTES] ^= 1
    elif fault == "chroma_ignored":
        chroma = forward
    elif fault == "chroma_gray":
        chroma = synthetic_luma(phase=3)
    elif fault == "repeat_drift":
        repeat = bytearray(repeat)
        repeat[ROW_BYTES] ^= 1
    elif fault == "alpha":
        forward = bytearray(forward)
        forward[3] = 0
    elif fault == "flat":
        forward = bytes((128, 128, 128, 255)) * (WIDTH * HEIGHT)
        repeat = bytearray(forward)
        repeat[0] ^= 1
    else:
        chroma = chroma[:-4]

    with pytest.raises(AssertionError):
        assert_signal_response(forward, reverse, repeat, chroma)


def test_real_signal_response(tmp_path):
    plugin = os.environ.get("AEXCOMPAT_TEST_SIGNAL")
    if not plugin:
        pytest.skip("set AEXCOMPAT_TEST_SIGNAL to the installed signal.aex")
    assert os.name == "nt"
    plugin_path = Path(plugin).resolve(strict=True)
    assert plugin_path.is_file()
    assert plugin_path.name.casefold() == "signal.aex"
    assert plugin_path.parent.name.casefold() == "zaebects"
    plugin = str(plugin_path)
    harness = ROOT / "broker/target/release/aexcompat-harness.exe"

    def run(*args):
        result = subprocess.run(
            [str(harness), "--headless", *map(str, args)],
            cwd=ROOT,
            capture_output=True,
            timeout=90,
        )
        assert result.returncode == 0, result.stderr.decode("utf-8", errors="replace")
        return json.loads(result.stdout)

    parameters = {
        parameter["slot"]: parameter
        for parameter in run("--inspect-experimental", plugin)
    }
    for slot, (name, kind, minimum, maximum) in PARAMETER_SCHEMA.items():
        parameter = parameters[slot]
        assert parameter["name"] == name
        assert parameter["kind"] == kind
        assert parameter["minimum"] == minimum
        assert parameter["maximum"] == maximum
    assert parameters[1]["value"] == 0
    assert parameters[3]["value"] == 1
    assert parameters[34]["choices"] == ["Off", "Lite", "Strong"]

    sources = {}
    for label, reverse in (("forward", False), ("reverse", True)):
        path = tmp_path / f"source-{label}.png"
        Image.frombytes("RGBA", (WIDTH, HEIGHT), source_pixels(reverse)).save(path)
        sources[label] = path

    outputs = {}
    cases = (
        ("luma_forward", "forward", NOISE_STRENGTH, 0),
        ("luma_reverse", "reverse", NOISE_STRENGTH, 0),
        ("luma_repeat", "forward", NOISE_STRENGTH, 0),
        ("chroma_forward", "forward", 0, NOISE_STRENGTH),
    )
    for label, source_label, luma, chroma in cases:
        request = tmp_path / f"{label}.json"
        output = tmp_path / f"{label}.png"
        request_assignments, requested_values = assignments(luma, chroma)
        request.write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "timing": {"frame": 0, "fps": 30, "duration_frames": 300},
                    "assignments": request_assignments,
                }
            ),
            encoding="utf-8",
        )
        report = run(
            "--render-experimental-request",
            plugin,
            sources[source_label],
            output,
            request,
        )
        assert report["passed"] is True
        # Classic reports do not always publish the SmartFX-only validation
        # flag. A literal false is still a failure; decoded bytes and the
        # worker-reported hash are checked below for positive evidence.
        assert report.get("output_pixels_valid") is not False
        assert report["worker_classification"] == "ok"
        assert report["render_path"] == "classic"
        for receipt in (
            "guard_bytes_intact",
            "suite_leases_balanced",
            "handle_lifetimes_balanced",
            "world_lifetimes_balanced",
        ):
            assert report[receipt] is True
        receipt = report["requested_parameters"]
        assert len(receipt) == len(requested_values)
        assert len({parameter["slot"] for parameter in receipt}) == len(receipt)
        requested = {
            parameter["slot"]: parameter["value"]
            for parameter in receipt
        }
        assert requested == requested_values
        assert output.is_file() and output.stat().st_size > 0
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            pixels = image.convert("RGBA").tobytes()
        assert hashlib.sha256(argb(pixels)).hexdigest() == report["output_sha256"]
        outputs[label] = pixels

    assert_signal_response(
        outputs["luma_forward"],
        outputs["luma_reverse"],
        outputs["luma_repeat"],
        outputs["chroma_forward"],
    )

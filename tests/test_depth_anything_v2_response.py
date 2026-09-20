"""Bounded Depth Anything V2 response; not an AE or model-quality oracle."""

import hashlib
import json
import math
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH, argb


EXPECTED_PLUGIN = Path(
    r"C:\Program Files\Adobe\Common\Plug-ins\7.0\MediaCore"
    r"\DepthAnythingV2\DepthAnythingV2.aex"
)
EXPECTED_PLUGIN_SHA256 = (
    "a8cd9ea5d98af3d853f793f523afad9cc2cbb412f1cdb1ad1f26621bc64ad4ff"
)
MODEL_NAME = "Model"
RESOLUTION_NAME = "Resolution (DAv2 only)"
NORMALIZATION_NAME = "Normalization"
INVERT_NAME = "Invert"
MODEL_CHOICES = ["Depth Anything V2", "Depth Pro (1536, slow)"]
RESOLUTION_CHOICES = ["266 (preview)", "392 (preview HQ)", "518 (final)"]
NORMALIZATION_CHOICES = ["Per-frame", "Fixed range"]
MIRROR_EDGE_MARGIN = 8


def source_pixels():
    """Opaque asymmetric pseudo-scene with broad color and spatial structure."""
    output = bytearray()
    for y in range(HEIGHT):
        for x in range(WIDTH):
            left = x < WIDTH // 2
            circle = (x - 184) ** 2 + (y - 48) ** 2 < 29 ** 2
            stripe = 78 < y < 103 and 24 < x < 116
            red = (23 + 2 * x + 3 * y) % 256
            green = (191 - x // 2 + 2 * y) % 256
            blue = (37 + x // 3 + 5 * y) % 256
            if left:
                red //= 2
                blue = min(255, blue + 48)
            if circle:
                red, green, blue = 238, 214, 61
            if stripe:
                red, green, blue = 34, 196, 142
            output.extend((red, green, blue, 255))
    return bytes(output)


def _horizontal_flip_rgba(pixels):
    assert len(pixels) == WIDTH * HEIGHT * 4
    output = bytearray()
    row_bytes = WIDTH * 4
    for y in range(HEIGHT):
        row = pixels[y * row_bytes:(y + 1) * row_bytes]
        for x in range(WIDTH - 1, -1, -1):
            offset = x * 4
            output.extend(row[offset:offset + 4])
    return bytes(output)


def _gray_values(pixels):
    return list(pixels[0::4])


def _frame_metrics(pixels):
    grayscale_pixels = 0
    opaque_pixels = 0
    values = []
    for offset in range(0, len(pixels), 4):
        red, green, blue, alpha = pixels[offset:offset + 4]
        grayscale_pixels += red == green == blue
        opaque_pixels += alpha == 255
        values.append(red)

    horizontal_variation = sum(
        abs(values[y * WIDTH + x] - values[y * WIDTH + x - 1])
        for y in range(HEIGHT)
        for x in range(1, WIDTH)
    )
    vertical_variation = sum(
        abs(values[y * WIDTH + x] - values[(y - 1) * WIDTH + x])
        for y in range(1, HEIGHT)
        for x in range(WIDTH)
    )
    return {
        "grayscale_pixels": grayscale_pixels,
        "opaque_pixels": opaque_pixels,
        "unique_gray_values": len(set(values)),
        "gray_minimum": min(values),
        "gray_maximum": max(values),
        "gray_range": max(values) - min(values),
        "horizontal_variation": horizontal_variation,
        "vertical_variation": vertical_variation,
        "total_variation": horizontal_variation + vertical_variation,
    }


def _average_ranks(values):
    order = sorted(range(len(values)), key=values.__getitem__)
    ranks = [0.0] * len(values)
    cursor = 0
    while cursor < len(order):
        end = cursor + 1
        value = values[order[cursor]]
        while end < len(order) and values[order[end]] == value:
            end += 1
        average = (cursor + end - 1) / 2.0
        for position in range(cursor, end):
            ranks[order[position]] = average
        cursor = end
    return ranks


def _correlation(first, second):
    first_mean = sum(first) / len(first)
    second_mean = sum(second) / len(second)
    first_centered = [value - first_mean for value in first]
    second_centered = [value - second_mean for value in second]
    numerator = sum(
        left * right for left, right in zip(first_centered, second_centered)
    )
    denominator = math.sqrt(
        sum(value * value for value in first_centered)
        * sum(value * value for value in second_centered)
    )
    return numerator / denominator if denominator else 0.0


def _spatial_rank_correlation(normal, inverted):
    # A deterministic quarter-frame sample keeps the mutation suite quick while
    # retaining every row and all broad spatial structures in the 256x144 map.
    sampled_normal = normal[::4]
    sampled_inverted = inverted[::4]
    return _correlation(
        _average_ranks(sampled_normal), _average_ranks(sampled_inverted)
    )


def _median(values):
    ordered = sorted(values)
    midpoint = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[midpoint]
    return (ordered[midpoint - 1] + ordered[midpoint]) / 2


def _mirror_response_metrics(normal, mirrored):
    normal_values = _gray_values(normal)
    mirrored_values = _gray_values(mirrored)
    same_position_errors = [
        abs(before - after)
        for before, after in zip(normal_values, mirrored_values)
    ]
    expected_values = []
    observed_values = []
    for y in range(HEIGHT):
        for x in range(MIRROR_EDGE_MARGIN, WIDTH - MIRROR_EDGE_MARGIN):
            expected_values.append(normal_values[y * WIDTH + (WIDTH - 1 - x)])
            observed_values.append(mirrored_values[y * WIDTH + x])
    mirror_errors = [
        abs(expected - observed)
        for expected, observed in zip(expected_values, observed_values)
    ]
    return {
        "same_position_changed_fraction": sum(
            error >= 4 for error in same_position_errors
        ) / len(same_position_errors),
        "same_position_mean_absolute_difference": (
            sum(same_position_errors) / len(same_position_errors)
        ),
        "aligned_within_four_fraction": sum(
            error <= 4 for error in mirror_errors
        ) / len(mirror_errors),
        "aligned_median_error": _median(mirror_errors),
        "aligned_maximum_error": max(mirror_errors),
        "aligned_rank_correlation": _correlation(
            _average_ranks(expected_values[::4]),
            _average_ranks(observed_values[::4]),
        ),
    }


def depth_response_metrics(normal, repeat, inverted, mirrored):
    normal_values = _gray_values(normal)
    inverted_values = _gray_values(inverted)
    complement_errors = sorted(
        abs(before + after - 255)
        for before, after in zip(normal_values, inverted_values)
    )
    return {
        "repeat_exact": normal == repeat,
        "normal": _frame_metrics(normal),
        "inverted": _frame_metrics(inverted),
        "mirrored_source": _frame_metrics(mirrored),
        "complement_within_two_fraction": sum(
            error <= 2 for error in complement_errors
        ) / len(complement_errors),
        "complement_median_error": _median(complement_errors),
        "complement_maximum_error": max(complement_errors),
        "spatial_rank_correlation": _spatial_rank_correlation(
            normal_values, inverted_values
        ),
        "mirror_response": _mirror_response_metrics(normal, mirrored),
    }


def assert_depth_response(normal, repeat, inverted, mirrored):
    expected_size = WIDTH * HEIGHT * 4
    for pixels in (normal, repeat, inverted, mirrored):
        assert len(pixels) == expected_size

    metrics = depth_response_metrics(normal, repeat, inverted, mirrored)
    total_pixels = WIDTH * HEIGHT
    assert metrics["repeat_exact"], metrics
    for frame in (
            metrics["normal"], metrics["inverted"],
            metrics["mirrored_source"]):
        assert frame["opaque_pixels"] == total_pixels, metrics
        assert frame["grayscale_pixels"] == total_pixels, metrics
        assert frame["unique_gray_values"] >= 32, metrics
        assert frame["gray_range"] >= 64, metrics
        assert frame["total_variation"] > total_pixels, metrics

    assert normal != inverted, metrics
    assert metrics["complement_within_two_fraction"] >= 0.99, metrics
    assert metrics["complement_median_error"] <= 1, metrics
    assert metrics["spatial_rank_correlation"] <= -0.98, metrics
    mirror = metrics["mirror_response"]
    assert normal != mirrored, metrics
    assert mirror["same_position_changed_fraction"] >= 0.10, metrics
    assert mirror["same_position_mean_absolute_difference"] >= 4, metrics
    # Per-frame monocular inference is context-sensitive rather than exactly
    # mirror-equivariant.  Require a positive spatial relation to the mirrored
    # map, while the two source renders must remain materially different.
    assert mirror["aligned_rank_correlation"] >= 0.30, metrics
    return metrics


def _synthetic_depth(*, complement_noise=True, mirror_noise=False):
    normal_values = []
    for y in range(HEIGHT):
        for x in range(WIDTH):
            radial = int(
                min(1.0, math.hypot(x - 177, (y - 43) * 1.6) / 170) * 164
            )
            band = (y * 61) // max(1, HEIGHT - 1)
            normal_values.append(min(254, radial + band))

    inverted_values = [255 - value for value in normal_values]
    if complement_noise:
        for index in range(0, len(inverted_values), 211):
            inverted_values[index] += 1 if inverted_values[index] < 255 else -1

    mirrored_values = [
        normal_values[y * WIDTH + (WIDTH - 1 - x)]
        for y in range(HEIGHT)
        for x in range(WIDTH)
    ]
    if mirror_noise:
        for y in range(HEIGHT):
            for x in range(WIDTH):
                index = y * WIDTH + x
                if x < MIRROR_EDGE_MARGIN or x >= WIDTH - MIRROR_EDGE_MARGIN:
                    delta = 9 if (x + y) % 2 else -9
                elif index % 211 == 0:
                    delta = 2 if index % 2 else -2
                else:
                    continue
                mirrored_values[index] = max(
                    0, min(255, mirrored_values[index] + delta)
                )

    def rgba(values):
        return bytes(
            component
            for value in values
            for component in (value, value, value, 255)
        )

    normal = rgba(normal_values)
    return normal, normal, rgba(inverted_values), rgba(mirrored_values)


def test_depth_validator_accepts_robust_relational_response():
    metrics = assert_depth_response(*_synthetic_depth())
    assert metrics["complement_maximum_error"] == 1
    assert metrics["spatial_rank_correlation"] < -0.99


def test_depth_validator_accepts_mirror_rounding_and_edge_differences():
    metrics = assert_depth_response(*_synthetic_depth(mirror_noise=True))
    mirror = metrics["mirror_response"]
    assert mirror["aligned_median_error"] <= 2
    assert mirror["aligned_maximum_error"] <= 2
    assert mirror["aligned_rank_correlation"] > 0.99


@pytest.mark.parametrize(
    "fault",
    (
        "non_grayscale",
        "alpha",
        "flat",
        "low_range",
        "repeat_drift",
        "invert_ignored",
        "complement_drift",
        "spatial_shuffle",
        "fixed_source_map",
        "mirrored_spatial_shuffle",
        "truncated",
    ),
)
def test_depth_validator_rejects_corruption(fault):
    normal, repeat, inverted, mirrored = _synthetic_depth(
        complement_noise=False
    )
    assert_depth_response(normal, repeat, inverted, mirrored)

    if fault == "non_grayscale":
        normal = bytearray(normal)
        repeat = bytearray(repeat)
        normal[1] ^= 1
        repeat[1] ^= 1
    elif fault == "alpha":
        normal = bytearray(normal)
        normal[3] = 0
    elif fault == "flat":
        normal = repeat = bytes((128, 128, 128, 255)) * (WIDTH * HEIGHT)
        inverted = bytes((127, 127, 127, 255)) * (WIDTH * HEIGHT)
        mirrored = normal
    elif fault == "low_range":
        values = [120 + value % 8 for value in _gray_values(normal)]
        normal = repeat = bytes(
            component for value in values
            for component in (value, value, value, 255)
        )
        inverted = bytes(
            component for value in values
            for component in (255 - value, 255 - value, 255 - value, 255)
        )
        mirrored = _horizontal_flip_rgba(normal)
    elif fault == "repeat_drift":
        repeat = bytearray(repeat)
        repeat[0] ^= 1
    elif fault == "invert_ignored":
        inverted = normal
    elif fault == "complement_drift":
        inverted = bytearray(inverted)
        for pixel in range(0, WIDTH * HEIGHT, 10):
            offset = pixel * 4
            value = min(255, inverted[offset] + 12)
            inverted[offset:offset + 3] = bytes((value, value, value))
    elif fault == "spatial_shuffle":
        values = list(reversed(_gray_values(inverted)))
        inverted = bytes(
            component for value in values
            for component in (value, value, value, 255)
        )
    elif fault == "fixed_source_map":
        mirrored = normal
    elif fault == "mirrored_spatial_shuffle":
        values = _gray_values(mirrored)
        shuffled = [
            values[(index * 7919) % len(values)]
            for index in range(len(values))
        ]
        mirrored = bytes(
            component for value in shuffled
            for component in (value, value, value, 255)
        )
    else:
        inverted = inverted[:-4]

    with pytest.raises(AssertionError):
        assert_depth_response(
            bytes(normal), bytes(repeat), bytes(inverted), bytes(mirrored)
        )


def _bind_depth_parameters(parameters):
    assert len(parameters) == 4
    assert len({parameter["slot"] for parameter in parameters}) == 4
    by_name = {}
    for parameter in parameters:
        assert parameter["name"] not in by_name
        by_name[parameter["name"]] = parameter
    assert set(by_name) == {
        MODEL_NAME,
        RESOLUTION_NAME,
        NORMALIZATION_NAME,
        INVERT_NAME,
    }

    model = by_name[MODEL_NAME]
    resolution = by_name[RESOLUTION_NAME]
    normalization = by_name[NORMALIZATION_NAME]
    invert = by_name[INVERT_NAME]
    for parameter in (model, resolution, normalization, invert):
        assert parameter["kind"] == "integer"
        assert parameter["enabled"] is True
        assert parameter["visible"] is True
    assert model["choices"] == MODEL_CHOICES
    assert model["value"] == 1
    assert resolution["choices"] == RESOLUTION_CHOICES
    assert normalization["choices"] == NORMALIZATION_CHOICES
    for parameter in (resolution, normalization):
        assert parameter["value"] == int(parameter["value"])
        assert 1 <= parameter["value"] <= len(parameter["choices"])
    assert invert["minimum"] == 0
    assert invert["maximum"] == 1
    assert invert["value"] == 0
    return by_name


def _request_values(parameters, invert):
    values = {
        parameters[MODEL_NAME]["slot"]: 1,
        parameters[RESOLUTION_NAME]["slot"]: int(
            parameters[RESOLUTION_NAME]["value"]
        ),
        parameters[NORMALIZATION_NAME]["slot"]: int(
            parameters[NORMALIZATION_NAME]["value"]
        ),
        parameters[INVERT_NAME]["slot"]: invert,
    }
    return values


def test_real_depth_anything_v2_response(tmp_path):
    configured = os.environ.get("AEXCOMPAT_TEST_DEPTH_ANYTHING_V2")
    if not configured:
        pytest.skip(
            "set AEXCOMPAT_TEST_DEPTH_ANYTHING_V2 to installed DepthAnythingV2.aex"
        )
    assert os.name == "nt"

    # Bind one exact installed identity before the harness loads any module.
    plugin_path = Path(configured).resolve(strict=True)
    expected_path = EXPECTED_PLUGIN.resolve(strict=True)
    assert plugin_path == expected_path
    assert plugin_path.is_file()
    assert plugin_path.name.casefold() == "depthanythingv2.aex"
    assert plugin_path.parent.name.casefold() == "depthanythingv2"
    assert hashlib.sha256(plugin_path.read_bytes()).hexdigest() == (
        EXPECTED_PLUGIN_SHA256
    )
    plugin = str(plugin_path)
    harness = ROOT / "broker/target/release/aexcompat-harness.exe"

    def run(*args):
        completed = subprocess.run(
            [str(harness), "--headless", *map(str, args)],
            cwd=ROOT,
            capture_output=True,
            timeout=90,
        )
        assert completed.returncode == 0, completed.stderr.decode(
            "utf-8", errors="replace"
        )
        return json.loads(completed.stdout)

    parameters = _bind_depth_parameters(
        run("--inspect-experimental", plugin)
    )
    original_source_pixels = source_pixels()
    source = tmp_path / "source.png"
    mirrored_source = tmp_path / "source-mirrored.png"
    Image.frombytes(
        "RGBA", (WIDTH, HEIGHT), original_source_pixels
    ).save(source)
    Image.frombytes(
        "RGBA", (WIDTH, HEIGHT),
        _horizontal_flip_rgba(original_source_pixels),
    ).save(mirrored_source)

    outputs = {}
    cases = (
        ("normal", 0, source),
        ("repeat", 0, source),
        ("invert", 1, source),
        ("mirrored_source", 0, mirrored_source),
    )
    for label, invert, input_path in cases:
        values = _request_values(parameters, invert)
        request = tmp_path / f"{label}.json"
        output = tmp_path / f"{label}.png"
        request.write_text(
            json.dumps({
                "schema_version": 1,
                "timing": {"frame": 0, "fps": 30, "duration_frames": 300},
                "assignments": [
                    {"slot": slot, "value": value}
                    for slot, value in sorted(values.items())
                ],
            }),
            encoding="utf-8",
        )
        report = run(
            "--render-experimental-smart-request",
            plugin,
            input_path,
            output,
            request,
        )
        assert report["passed"] is True
        assert report["output_pixels_valid"] is True
        assert report["worker_classification"] == "ok"
        assert report["render_path"] == "smartfx"
        for receipt_name in (
            "guard_bytes_intact",
            "suite_leases_balanced",
            "handle_lifetimes_balanced",
            "world_lifetimes_balanced",
        ):
            assert report[receipt_name] is True

        receipts = report["requested_parameters"]
        assert len(receipts) == len(values)
        assert len({receipt["slot"] for receipt in receipts}) == len(receipts)
        assert {
            receipt["slot"]: receipt["value"] for receipt in receipts
        } == values

        assert output.is_file() and output.stat().st_size > 0
        with Image.open(output) as image:
            image.load()
            assert image.size == (WIDTH, HEIGHT)
            pixels = image.convert("RGBA").tobytes()
        assert hashlib.sha256(argb(pixels)).hexdigest() == report["output_sha256"]
        outputs[label] = pixels

    metrics = assert_depth_response(
        outputs["normal"], outputs["repeat"], outputs["invert"],
        outputs["mirrored_source"],
    )
    (tmp_path / "metrics.json").write_text(
        json.dumps(metrics, indent=2, sort_keys=True), encoding="utf-8"
    )

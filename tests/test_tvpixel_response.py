"""Bounded Rowbyte TV Pixel control response; not an exact AE oracle."""

import hashlib
import json
import os
import subprocess
from collections import Counter
from pathlib import Path

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH, argb


LEFT_RGB = (41, 83, 167)
RIGHT_RGB = (197, 109, 53)
BASE_BACKGROUND = (0, 0, 0)
ALT_BACKGROUND = (17, 29, 43)
BASE_PIXEL_WIDTH = 1
WIDE_PIXEL_WIDTH = 4
PIXEL_HEIGHT = 3
GAP_X = 2
GAP_Y = 1

PARAMETER_SCHEMA = {
    1: ("Pixel Width", "integer", 1, 1000, 1),
    2: ("Pixel Height", "integer", 2, 1000, 3),
    3: ("Fill Background", "integer", 0, 1, 1),
    4: ("BG Color", "color", None, None, None),
    5: ("Pixel Gap", "integer", 1, 3, 1),
    6: ("Pixel Gap X", "integer", 0, 1000, 1),
    7: ("Pixel Gap Y", "integer", 0, 1000, 1),
    8: ("Blend w/ Original", "float", 0, 100, 0),
    9: ("Dummy", "integer", 0, 100000, 0),
    10: ("Legacy Kernel", "integer", 0, 1, 0),
}


def source_pixels():
    return bytes(
        component
        for _y in range(HEIGHT)
        for x in range(WIDTH)
        for component in (
            (LEFT_RGB if x < WIDTH // 2 else RIGHT_RGB) + (255,)
        )
    )


def _rgb(pixels, x, y):
    offset = (y * WIDTH + x) * 4
    return tuple(pixels[offset:offset + 3])


def _background_mask(pixels, background):
    return tuple(
        tuple(pixels[offset:offset + 3]) == background
        for offset in range(0, len(pixels), 4)
    )


def _full_background_rows(mask):
    return tuple(
        all(mask[y * WIDTH:(y + 1) * WIDTH])
        for y in range(HEIGHT)
    )


def _row_activity_ranks(mask):
    counts = [
        sum(not value for value in mask[y * WIDTH:(y + 1) * WIDTH])
        for y in range(HEIGHT)
    ]
    ranks = {value: rank for rank, value in enumerate(sorted(set(counts)))}
    return tuple(ranks[value] for value in counts)


def _pixel_class(rgb, background):
    if rgb == background:
        return 0
    nonzero = tuple(index for index, component in enumerate(rgb) if component)
    if len(nonzero) == 1:
        return nonzero[0] + 1
    return 4


def _matches_8bit_roundtrip(rgb, expected):
    return all(abs(actual - target) <= 1 for actual, target in zip(rgb, expected))


def _horizontal_transitions(pixels, background, full_background_rows):
    transitions = 0
    for y, full_background in enumerate(full_background_rows):
        if full_background:
            continue
        previous = _pixel_class(_rgb(pixels, 0, y), background)
        for x in range(1, WIDTH):
            current = _pixel_class(_rgb(pixels, x, y), background)
            transitions += current != previous
            previous = current
    return transitions


def _source_region_metrics(pixels, background):
    regions = {
        "left": (16, WIDTH // 2 - 16, LEFT_RGB),
        "right": (WIDTH // 2 + 16, WIDTH - 16, RIGHT_RGB),
    }
    metrics = {}
    for label, (start_x, end_x, expected) in regions.items():
        area = (end_x - start_x) * HEIGHT
        channel_metrics = []
        for channel in range(3):
            pure_values = []
            for y in range(HEIGHT):
                for x in range(start_x, end_x):
                    rgb = _rgb(pixels, x, y)
                    if rgb == background:
                        continue
                    if rgb[channel] and all(
                            rgb[other] == 0 for other in range(3)
                            if other != channel):
                        pure_values.append(rgb[channel])
            matching = sum(
                abs(value - expected[channel]) <= 1
                for value in pure_values
            )
            most_common = Counter(pure_values).most_common(1)
            channel_metrics.append({
                "pure_pixels": len(pure_values),
                "matching_source_pixels": matching,
                "matching_source_fraction": (
                    matching / len(pure_values) if pure_values else 0.0
                ),
                "mode": most_common[0][0] if most_common else None,
                "region_area": area,
            })
        metrics[label] = channel_metrics
    return metrics


def tvpixel_metrics(baseline, repeat, width_variant, background_variant):
    baseline_mask = _background_mask(baseline, BASE_BACKGROUND)
    width_mask = _background_mask(width_variant, BASE_BACKGROUND)
    baseline_rows = _full_background_rows(baseline_mask)
    width_rows = _full_background_rows(width_mask)
    baseline_gap_indexes = [
        index for index, is_background in enumerate(baseline_mask)
        if is_background
    ]
    active_indexes = [
        index for index, is_background in enumerate(baseline_mask)
        if not is_background
    ]

    def rgb_at_index(pixels, index):
        offset = index * 4
        return tuple(pixels[offset:offset + 3])

    def rgba_at_index(pixels, index):
        offset = index * 4
        return tuple(pixels[offset:offset + 4])

    return {
        "repeat_exact": baseline == repeat,
        "baseline_gap_pixels": len(baseline_gap_indexes),
        "width_gap_mask_changed_pixels": sum(
            before != after for before, after in zip(baseline_mask, width_mask)
        ),
        "baseline_vertical_activity_ranks": _row_activity_ranks(baseline_mask),
        "width_vertical_activity_ranks": _row_activity_ranks(width_mask),
        "baseline_horizontal_transitions": _horizontal_transitions(
            baseline, BASE_BACKGROUND, baseline_rows
        ),
        "width_horizontal_transitions": _horizontal_transitions(
            width_variant, BASE_BACKGROUND, width_rows
        ),
        "background_gap_matches": sum(
            _matches_8bit_roundtrip(
                rgb_at_index(background_variant, index), ALT_BACKGROUND
            )
            for index in baseline_gap_indexes
        ),
        "background_active_unchanged": sum(
            rgba_at_index(background_variant, index)
            == rgba_at_index(baseline, index)
            for index in active_indexes
        ),
        "baseline_source_regions": _source_region_metrics(
            baseline, BASE_BACKGROUND
        ),
        "width_source_regions": _source_region_metrics(
            width_variant, BASE_BACKGROUND
        ),
    }


def assert_tvpixel_response(baseline, repeat, width_variant,
                            background_variant):
    frames = (baseline, repeat, width_variant, background_variant)
    expected_size = WIDTH * HEIGHT * 4
    for pixels in frames:
        assert len(pixels) == expected_size
        assert set(pixels[3::4]).issubset({254, 255})
        assert 255 in pixels[3::4]

    metrics = tvpixel_metrics(
        baseline, repeat, width_variant, background_variant
    )
    total_pixels = WIDTH * HEIGHT
    assert metrics["repeat_exact"], metrics
    assert total_pixels // 50 < metrics["baseline_gap_pixels"] < total_pixels * 3 // 4, metrics

    for frame_key in ("baseline_source_regions", "width_source_regions"):
        for region in metrics[frame_key].values():
            pure_counts = [channel["pure_pixels"] for channel in region]
            for channel in region:
                assert channel["pure_pixels"] > channel["region_area"] // 10, metrics
                assert channel["matching_source_fraction"] >= 0.95, metrics
            assert min(pure_counts) * 4 >= max(pure_counts) * 3, metrics

    baseline_vertical = metrics["baseline_vertical_activity_ranks"]
    width_vertical = metrics["width_vertical_activity_ranks"]
    assert len(set(baseline_vertical)) > 1, metrics
    assert baseline_vertical == width_vertical, metrics
    assert metrics["width_gap_mask_changed_pixels"] > total_pixels // 25, metrics
    baseline_transitions = metrics["baseline_horizontal_transitions"]
    width_transitions = metrics["width_horizontal_transitions"]
    assert baseline_transitions > 0, metrics
    assert width_transitions < baseline_transitions * 0.80, metrics

    assert metrics["background_gap_matches"] == metrics["baseline_gap_pixels"], metrics
    assert metrics["background_active_unchanged"] == (
        total_pixels - metrics["baseline_gap_pixels"]
    ), metrics
    assert background_variant != baseline, metrics
    return metrics


def _synthetic_frame(pixel_width, background, *, right_rgb=RIGHT_RGB,
                     pixel_height=PIXEL_HEIGHT):
    output = bytearray()
    horizontal_period = pixel_width * 3 + GAP_X
    vertical_period = pixel_height + GAP_Y
    for y in range(HEIGHT):
        vertical_gap = y % vertical_period >= pixel_height
        for x in range(WIDTH):
            phase = x % horizontal_period
            if vertical_gap or phase >= pixel_width * 3:
                rgb = background
            else:
                source = LEFT_RGB if x < WIDTH // 2 else right_rgb
                channel = phase // pixel_width
                rgb = tuple(
                    source[index] if index == channel else 0
                    for index in range(3)
                )
            output.extend((*rgb, 255))
    return bytes(output)


def _synthetic_cases(*, right_rgb=RIGHT_RGB):
    baseline = _synthetic_frame(
        BASE_PIXEL_WIDTH, BASE_BACKGROUND, right_rgb=right_rgb
    )
    return (
        baseline,
        baseline,
        _synthetic_frame(
            WIDE_PIXEL_WIDTH, BASE_BACKGROUND, right_rgb=right_rgb
        ),
        _synthetic_frame(
            BASE_PIXEL_WIDTH, ALT_BACKGROUND, right_rgb=right_rgb
        ),
    )


def test_tvpixel_validator_exposes_relational_metrics():
    metrics = assert_tvpixel_response(*_synthetic_cases())
    assert metrics["repeat_exact"]
    assert metrics["width_horizontal_transitions"] < metrics[
        "baseline_horizontal_transitions"
    ]
    for region in metrics["baseline_source_regions"].values():
        assert [channel["mode"] for channel in region] in (
            list(LEFT_RGB), list(RIGHT_RGB)
        )


def test_tvpixel_validator_accepts_one_lsb_source_roundtrip():
    adjusted = []
    for pixels, background in zip(
            _synthetic_cases(),
            (BASE_BACKGROUND, BASE_BACKGROUND, BASE_BACKGROUND,
             ALT_BACKGROUND)):
        pixels = bytearray(pixels)
        for offset in range(0, len(pixels), 4):
            rgb = tuple(pixels[offset:offset + 3])
            if rgb == background:
                continue
            nonzero = [index for index, value in enumerate(rgb) if value]
            if len(nonzero) == 1:
                channel = nonzero[0]
                pixels[offset + channel] += 1
        adjusted.append(bytes(pixels))

    assert_tvpixel_response(*adjusted)


@pytest.mark.parametrize(
    "fault",
    (
        "passthrough",
        "source_ignored",
        "red_subpixels_missing",
        "partial_red_subpixels_missing",
        "repeat_drift",
        "width_ignored",
        "width_changed_vertical_grid",
        "background_ignored",
        "background_leaks_into_active",
        "background_alpha_leaks_into_active",
        "alpha",
        "truncated",
    ),
)
def test_tvpixel_validator_rejects_corruption(fault):
    baseline, repeat, width_variant, background_variant = _synthetic_cases()
    assert_tvpixel_response(baseline, repeat, width_variant, background_variant)

    if fault == "passthrough":
        baseline = repeat = source_pixels()
    elif fault == "source_ignored":
        baseline, repeat, width_variant, background_variant = _synthetic_cases(
            right_rgb=LEFT_RGB
        )
    elif fault == "red_subpixels_missing":
        changed = []
        for pixels, background in (
                (baseline, BASE_BACKGROUND), (repeat, BASE_BACKGROUND),
                (width_variant, BASE_BACKGROUND),
                (background_variant, ALT_BACKGROUND)):
            pixels = bytearray(pixels)
            for offset in range(0, len(pixels), 4):
                if pixels[offset] and pixels[offset + 1] == pixels[offset + 2] == 0:
                    pixels[offset:offset + 3] = bytes(background)
            changed.append(bytes(pixels))
        baseline, repeat, width_variant, background_variant = changed
    elif fault == "partial_red_subpixels_missing":
        changed = []
        for pixels, background in (
                (baseline, BASE_BACKGROUND), (repeat, BASE_BACKGROUND),
                (width_variant, BASE_BACKGROUND),
                (background_variant, ALT_BACKGROUND)):
            pixels = bytearray(pixels)
            red_index = 0
            for offset in range(0, len(pixels), 4):
                if pixels[offset] and pixels[offset + 1] == pixels[offset + 2] == 0:
                    if red_index % 2 == 0:
                        pixels[offset:offset + 3] = bytes(background)
                    red_index += 1
            changed.append(bytes(pixels))
        baseline, repeat, width_variant, background_variant = changed
    elif fault == "repeat_drift":
        repeat = bytearray(repeat)
        repeat[0] ^= 1
    elif fault == "width_ignored":
        width_variant = baseline
    elif fault == "width_changed_vertical_grid":
        width_variant = _synthetic_frame(
            WIDE_PIXEL_WIDTH, BASE_BACKGROUND, pixel_height=2
        )
    elif fault == "background_ignored":
        background_variant = baseline
    elif fault == "background_leaks_into_active":
        background_variant = bytearray(background_variant)
        first_active = next(
            index for index, is_background in enumerate(
                _background_mask(baseline, BASE_BACKGROUND)
            ) if not is_background
        )
        offset = first_active * 4
        background_variant[offset:offset + 3] = bytes(ALT_BACKGROUND)
    elif fault == "background_alpha_leaks_into_active":
        background_variant = bytearray(background_variant)
        first_active = next(
            index for index, is_background in enumerate(
                _background_mask(baseline, BASE_BACKGROUND)
            ) if not is_background
        )
        background_variant[first_active * 4 + 3] = 254
    elif fault == "alpha":
        baseline = bytearray(baseline)
        baseline[3] = 0
    else:
        width_variant = width_variant[:-4]

    with pytest.raises(AssertionError):
        assert_tvpixel_response(
            bytes(baseline), bytes(repeat), bytes(width_variant),
            bytes(background_variant)
        )


def _assignments(pixel_width, background):
    numeric = {
        1: pixel_width,
        2: PIXEL_HEIGHT,
        3: 1,
        5: 2,  # Custom: makes the explicit X/Y gap controls applicable.
        6: GAP_X,
        7: GAP_Y,
        8: 0,
        9: 0,
        10: 0,
    }
    request = [
        {"slot": slot, "value": value}
        for slot, value in sorted(numeric.items())
    ]
    request.append({"slot": 4, "color": [255, *background]})
    request.sort(key=lambda item: item["slot"])
    expected = dict(numeric)
    expected[4] = {
        "alpha": 255,
        "red": background[0],
        "green": background[1],
        "blue": background[2],
    }
    return request, expected


def test_real_tvpixel_response(tmp_path):
    configured = os.environ.get("AEXCOMPAT_TEST_TVPIXEL")
    if not configured:
        pytest.skip("set AEXCOMPAT_TEST_TVPIXEL to the installed TVPixel64.aex")
    assert os.name == "nt"

    # Resolve the exact Rowbyte identity before the harness loads anything. A
    # basename-only search is not accepted because other excluded vendor trees
    # can contain similarly named effects.
    plugin_path = Path(configured).resolve(strict=True)
    assert plugin_path.is_file()
    assert plugin_path.name.casefold() == "tvpixel64.aex"
    assert plugin_path.parent.name.casefold() == "rowbyte"
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

    parameters = {
        parameter["slot"]: parameter
        for parameter in run("--inspect-experimental", plugin)
    }
    assert set(parameters) == set(PARAMETER_SCHEMA)
    for slot, (name, kind, minimum, maximum, default) in PARAMETER_SCHEMA.items():
        parameter = parameters[slot]
        assert parameter["name"] == name
        assert parameter["kind"] == kind
        if minimum is not None:
            assert parameter["minimum"] == minimum
        if maximum is not None:
            assert parameter["maximum"] == maximum
        if kind == "color":
            assert parameter["color"] == [255, 0, 0, 0]
        else:
            assert parameter["value"] == default
    assert parameters[5]["choices"] == [
        "Auto", "Custom", "Legacy (For Old Projects)"
    ]
    assert parameters[5]["supervised"] is True
    assert parameters[6]["enabled"] is False
    assert parameters[7]["enabled"] is False
    assert parameters[9]["visible"] is False
    assert parameters[10]["visible"] is False

    source = tmp_path / "source.png"
    Image.frombytes("RGBA", (WIDTH, HEIGHT), source_pixels()).save(source)
    cases = (
        ("baseline", BASE_PIXEL_WIDTH, BASE_BACKGROUND),
        ("repeat", BASE_PIXEL_WIDTH, BASE_BACKGROUND),
        ("width", WIDE_PIXEL_WIDTH, BASE_BACKGROUND),
        ("background", BASE_PIXEL_WIDTH, ALT_BACKGROUND),
    )
    outputs = {}
    for label, pixel_width, background in cases:
        request = tmp_path / f"{label}.json"
        output = tmp_path / f"{label}.png"
        request_assignments, expected_receipts = _assignments(
            pixel_width, background
        )
        request.write_text(
            json.dumps({
                "schema_version": 1,
                "timing": {"frame": 0, "fps": 30, "duration_frames": 300},
                "assignments": request_assignments,
            }),
            encoding="utf-8",
        )
        report = run(
            "--render-experimental-smart-request",
            plugin,
            source,
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
        assert len(receipts) == len(expected_receipts)
        assert len({receipt["slot"] for receipt in receipts}) == len(receipts)
        assert {
            receipt["slot"]: receipt["value"] for receipt in receipts
        } == expected_receipts

        assert output.is_file() and output.stat().st_size > 0
        with Image.open(output) as image:
            image.load()
            assert image.size == (WIDTH, HEIGHT)
            pixels = image.convert("RGBA").tobytes()
        assert hashlib.sha256(argb(pixels)).hexdigest() == report["output_sha256"]
        outputs[label] = pixels

    metrics = assert_tvpixel_response(
        outputs["baseline"],
        outputs["repeat"],
        outputs["width"],
        outputs["background"],
    )
    (tmp_path / "metrics.json").write_text(
        json.dumps(metrics, indent=2, sort_keys=True), encoding="utf-8"
    )

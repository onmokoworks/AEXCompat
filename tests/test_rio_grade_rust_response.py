"""Bounded Rio Grade color response; not an exact AE look oracle."""

import hashlib
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH, argb


EXPECTED_PLUGIN = Path(
    r"C:\Program Files\Adobe\Common\Plug-ins\7.0\MediaCore"
    r"\onmk\RioGradeRust.aex"
)
EXPECTED_PLUGIN_SHA256 = (
    "6272e0999ae4e9821ab14f67a036a70374e44cb6fd6daf1847053b2c2984b698"
)
EXPECTED_VENDOR = "onmk"
EXPECTED_NAME = "RioGradeRust.aex"

SOURCE_A_RGB = (32, 64, 128)
SOURCE_B_RGB = (128, 96, 48)
BAND_ROWS = 18

PARAMETER_SCHEMA = {
    1: ("Strength", "float", 0, 100, 100),
    2: ("Purple / Pink", "float", -100, 100, 92),
    3: ("Warm Bottom", "float", 0, 100, 78),
    4: ("Saturation", "float", 0, 200, 110),
    5: ("Contrast", "float", 0, 200, 88),
    6: ("Color Fade", "float", 0, 100, 24),
    7: ("Film Grain", "float", 0, 100, 6),
    8: ("Vignette", "float", 0, 100, 8),
}


def source_pixels(rgb):
    return bytes((*rgb, 255)) * (WIDTH * HEIGHT)


def _channel_means(pixels):
    total = WIDTH * HEIGHT
    return tuple(sum(pixels[channel::4]) / total for channel in range(3))


def _difference_metrics(first, second):
    changed = 0
    absolute_difference = 0
    for offset in range(0, len(first), 4):
        first_rgb = first[offset:offset + 3]
        second_rgb = second[offset:offset + 3]
        changed += first_rgb != second_rgb
        absolute_difference += sum(
            abs(before - after)
            for before, after in zip(first_rgb, second_rgb)
        )
    return {
        "changed_pixels": changed,
        "mean_absolute_rgb_difference": (
            absolute_difference / (WIDTH * HEIGHT * 3)
        ),
    }


def _band_warmth(pixels, start_y, end_y):
    warmth = 0
    count = 0
    for y in range(start_y, end_y):
        for x in range(WIDTH):
            offset = (y * WIDTH + x) * 4
            warmth += pixels[offset] - pixels[offset + 2]
            count += 1
    return warmth / count


def _spatial_variation(pixels):
    horizontal = 0
    horizontal_count = 0
    vertical = 0
    vertical_count = 0
    for y in range(HEIGHT):
        for x in range(1, WIDTH):
            offset = (y * WIDTH + x) * 4
            previous = offset - 4
            horizontal += sum(
                abs(pixels[offset + channel] - pixels[previous + channel])
                for channel in range(3)
            )
            horizontal_count += 3
    row_bytes = WIDTH * 4
    for y in range(1, HEIGHT):
        for x in range(WIDTH):
            offset = (y * WIDTH + x) * 4
            previous = offset - row_bytes
            vertical += sum(
                abs(pixels[offset + channel] - pixels[previous + channel])
                for channel in range(3)
            )
            vertical_count += 3
    return {
        "horizontal_mean_absolute_difference": horizontal / horizontal_count,
        "vertical_mean_absolute_difference": vertical / vertical_count,
    }


def _active_metrics(pixels, source):
    top_warmth = _band_warmth(pixels, 0, BAND_ROWS)
    bottom_warmth = _band_warmth(
        pixels, HEIGHT - BAND_ROWS, HEIGHT
    )
    return {
        **_difference_metrics(pixels, source),
        **_spatial_variation(pixels),
        "channel_means": _channel_means(pixels),
        "top_warmth": top_warmth,
        "bottom_warmth": bottom_warmth,
        "bottom_minus_top_warmth": bottom_warmth - top_warmth,
    }


def rio_grade_metrics(active_a, repeat_a, neutral_a, active_b):
    source_a = source_pixels(SOURCE_A_RGB)
    source_b = source_pixels(SOURCE_B_RGB)
    active_a_metrics = _active_metrics(active_a, source_a)
    active_b_metrics = _active_metrics(active_b, source_b)
    output_deltas = tuple(
        after - before
        for before, after in zip(
            active_a_metrics["channel_means"],
            active_b_metrics["channel_means"],
        )
    )
    source_deltas = tuple(
        after - before
        for before, after in zip(SOURCE_A_RGB, SOURCE_B_RGB)
    )
    tracking = tuple(
        source_delta * output_delta > 0 and abs(output_delta) >= 4
        for source_delta, output_delta in zip(source_deltas, output_deltas)
    )
    return {
        "repeat_exact": active_a == repeat_a,
        "neutral_identity": neutral_a == source_a,
        "active_a": active_a_metrics,
        "active_b": active_b_metrics,
        "active_source_difference": _difference_metrics(active_a, active_b),
        "source_channel_deltas": source_deltas,
        "output_channel_deltas": output_deltas,
        "input_tracking_channels": sum(tracking),
        "input_tracking": tracking,
    }


def assert_rio_grade_response(active_a, repeat_a, neutral_a, active_b):
    frames = (active_a, repeat_a, neutral_a, active_b)
    expected_size = WIDTH * HEIGHT * 4
    for pixels in frames:
        assert len(pixels) == expected_size
        assert pixels[3::4] == bytes([255]) * (WIDTH * HEIGHT)

    metrics = rio_grade_metrics(active_a, repeat_a, neutral_a, active_b)
    total_pixels = WIDTH * HEIGHT
    assert metrics["repeat_exact"], metrics
    assert metrics["neutral_identity"], metrics

    for active in (metrics["active_a"], metrics["active_b"]):
        assert active["changed_pixels"] >= total_pixels * 95 // 100, metrics
        assert active["mean_absolute_rgb_difference"] >= 4, metrics
        assert active["bottom_minus_top_warmth"] >= 8, metrics
        horizontal = active["horizontal_mean_absolute_difference"]
        vertical = active["vertical_mean_absolute_difference"]
        assert horizontal <= 0.75, metrics
        assert horizontal <= max(0.25, vertical * 0.75), metrics

    source_difference = metrics["active_source_difference"]
    assert source_difference["changed_pixels"] > total_pixels // 2, metrics
    assert source_difference["mean_absolute_rgb_difference"] >= 8, metrics
    assert metrics["input_tracking_channels"] == 3, metrics
    return metrics


def _clamp(value):
    return max(0, min(255, value))


def _synthetic_active(rgb, *, warm=True, horizontal_noise=False):
    output = bytearray()
    for y in range(HEIGHT):
        progress = y / (HEIGHT - 1)
        warm_red = round(34 * progress) if warm else 0
        warm_green = round(5 * progress) if warm else 0
        warm_blue = -round(20 * progress) if warm else 0
        for x in range(WIDTH):
            red = _clamp(rgb[0] + 20 + warm_red)
            green = _clamp(rgb[1] + 10 + warm_green)
            blue = _clamp(rgb[2] + 20 + warm_blue)
            if horizontal_noise:
                green = _clamp(green + (12 if x % 2 else -12))
            output.extend((red, green, blue, 255))
    return bytes(output)


def _bottom_only_active(rgb):
    """Mutation that the old 25%-changed threshold incorrectly accepted."""
    output = bytearray(source_pixels(rgb))
    for y in range(HEIGHT - 40, HEIGHT):
        for x in range(WIDTH):
            offset = (y * WIDTH + x) * 4
            output[offset] = _clamp(output[offset] + 40)
            output[offset + 2] = _clamp(output[offset + 2] - 20)
    return bytes(output)


def _sparse_roundtrip_noise(pixels):
    output = bytearray(pixels)
    for pixel in range(0, WIDTH * HEIGHT, 997):
        offset = pixel * 4 + (pixel % 3)
        output[offset] += 1 if output[offset] < 255 else -1
    return bytes(output)


def _synthetic_cases(*, sparse_rounding=False):
    active_a = _synthetic_active(SOURCE_A_RGB)
    active_b = _synthetic_active(SOURCE_B_RGB)
    if sparse_rounding:
        active_a = _sparse_roundtrip_noise(active_a)
        active_b = _sparse_roundtrip_noise(active_b)
    return (
        active_a,
        active_a,
        source_pixels(SOURCE_A_RGB),
        active_b,
    )


def test_rio_grade_validator_accepts_relational_response():
    metrics = assert_rio_grade_response(*_synthetic_cases())
    assert metrics["active_a"]["bottom_minus_top_warmth"] > 40
    assert metrics["input_tracking_channels"] == 3


def test_rio_grade_validator_accepts_sparse_roundtrip_noise():
    metrics = assert_rio_grade_response(
        *_synthetic_cases(sparse_rounding=True)
    )
    assert metrics["active_a"]["horizontal_mean_absolute_difference"] < 0.01


@pytest.mark.parametrize(
    "fault",
    (
        "passthrough",
        "repeat_drift",
        "neutral_drift",
        "no_vertical_warmth",
        "horizontal_banding",
        "bottom_only",
        "fixed_source_map",
        "one_channel_reversed",
        "wrong_source_relation",
        "source_b_passthrough",
        "alpha",
        "truncated",
    ),
)
def test_rio_grade_validator_rejects_corruption(fault):
    active_a, repeat_a, neutral_a, active_b = _synthetic_cases()
    assert_rio_grade_response(active_a, repeat_a, neutral_a, active_b)

    if fault == "passthrough":
        active_a = repeat_a = source_pixels(SOURCE_A_RGB)
    elif fault == "repeat_drift":
        repeat_a = bytearray(repeat_a)
        repeat_a[0] ^= 1
    elif fault == "neutral_drift":
        neutral_a = bytearray(neutral_a)
        neutral_a[0] ^= 1
    elif fault == "no_vertical_warmth":
        active_a = repeat_a = _synthetic_active(SOURCE_A_RGB, warm=False)
    elif fault == "horizontal_banding":
        active_a = repeat_a = _synthetic_active(
            SOURCE_A_RGB, horizontal_noise=True
        )
    elif fault == "bottom_only":
        active_a = repeat_a = _bottom_only_active(SOURCE_A_RGB)
    elif fault == "fixed_source_map":
        active_b = active_a
    elif fault == "one_channel_reversed":
        active_b = _synthetic_active(
            (16, SOURCE_B_RGB[1], SOURCE_B_RGB[2])
        )
    elif fault == "wrong_source_relation":
        active_b = _synthetic_active((16, 48, 160))
    elif fault == "source_b_passthrough":
        active_b = source_pixels(SOURCE_B_RGB)
    elif fault == "alpha":
        active_a = bytearray(active_a)
        active_a[3] = 0
    else:
        active_b = active_b[:-4]

    with pytest.raises(AssertionError):
        assert_rio_grade_response(
            bytes(active_a), bytes(repeat_a), bytes(neutral_a), bytes(active_b)
        )


def _bind_rio_grade_parameters(parameters):
    assert len(parameters) == len(PARAMETER_SCHEMA)
    assert len({parameter["slot"] for parameter in parameters}) == len(parameters)
    by_slot = {parameter["slot"]: parameter for parameter in parameters}
    assert set(by_slot) == set(PARAMETER_SCHEMA)
    for slot, (name, kind, minimum, maximum, default) in PARAMETER_SCHEMA.items():
        parameter = by_slot[slot]
        assert parameter["name"] == name
        assert parameter["kind"] == kind
        assert parameter["minimum"] == minimum
        assert parameter["maximum"] == maximum
        assert parameter["value"] == default
        assert parameter["enabled"] is True
        assert parameter["visible"] is True
        assert parameter["supervised"] is False
    return by_slot


def _synthetic_parameters():
    return [
        {
            "slot": slot,
            "name": name,
            "kind": kind,
            "minimum": minimum,
            "maximum": maximum,
            "value": default,
            "enabled": True,
            "visible": True,
            "supervised": False,
        }
        for slot, (name, kind, minimum, maximum, default)
        in PARAMETER_SCHEMA.items()
    ]


def test_rio_grade_schema_accepts_exact_eight_controls():
    parameters = _bind_rio_grade_parameters(_synthetic_parameters())
    assert [parameters[slot]["name"] for slot in sorted(parameters)] == [
        schema[0] for schema in PARAMETER_SCHEMA.values()
    ]


@pytest.mark.parametrize(
    "fault",
    ("duplicate_slot", "name", "kind", "minimum", "maximum", "default",
     "disabled", "hidden", "supervised"),
)
def test_rio_grade_schema_rejects_drift(fault):
    parameters = _synthetic_parameters()
    if fault == "duplicate_slot":
        parameters[-1]["slot"] = 1
    elif fault == "name":
        parameters[1]["name"] = "Purple Pink"
    elif fault == "kind":
        parameters[0]["kind"] = "integer"
    elif fault == "minimum":
        parameters[1]["minimum"] = 0
    elif fault == "maximum":
        parameters[3]["maximum"] = 100
    elif fault == "default":
        parameters[6]["value"] = 0
    elif fault == "disabled":
        parameters[2]["enabled"] = False
    elif fault == "hidden":
        parameters[3]["visible"] = False
    else:
        parameters[4]["supervised"] = True

    with pytest.raises(AssertionError):
        _bind_rio_grade_parameters(parameters)


def _assignments(strength):
    # Vignette is also neutralized so a constant source has no intended
    # horizontal structure; this isolates the advertised Warm Bottom response.
    values = {
        slot: default
        for slot, (_name, _kind, _minimum, _maximum, default)
        in PARAMETER_SCHEMA.items()
    }
    values[1] = strength
    values[7] = 0
    values[8] = 0
    return (
        [
            {"slot": slot, "value": value}
            for slot, value in sorted(values.items())
        ],
        values,
    )


def test_real_rio_grade_rust_response(tmp_path):
    configured = os.environ.get("AEXCOMPAT_TEST_RIO_GRADE_RUST")
    if not configured:
        pytest.skip(
            "set AEXCOMPAT_TEST_RIO_GRADE_RUST to the installed "
            "RioGradeRust.aex"
        )
    assert os.name == "nt"

    # Exact identity is resolved before the harness gets a chance to load a
    # similarly named module from another registered plug-in tree.
    plugin_path = Path(configured).resolve(strict=True)
    expected_path = EXPECTED_PLUGIN.resolve(strict=True)
    assert plugin_path == expected_path
    assert plugin_path.is_file()
    assert plugin_path.name == EXPECTED_NAME
    assert plugin_path.parent.name.casefold() == EXPECTED_VENDOR
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

    _bind_rio_grade_parameters(run("--inspect-experimental", plugin))

    sources = {}
    for label, rgb in (("a", SOURCE_A_RGB), ("b", SOURCE_B_RGB)):
        source = tmp_path / f"source-{label}.png"
        Image.frombytes("RGBA", (WIDTH, HEIGHT), source_pixels(rgb)).save(source)
        sources[label] = source

    outputs = {}
    cases = (
        ("active_a", "a", 100),
        ("repeat_a", "a", 100),
        ("neutral_a", "a", 0),
        ("active_b", "b", 100),
    )
    for label, source_label, strength in cases:
        request_assignments, requested_values = _assignments(strength)
        request = tmp_path / f"{label}.json"
        output = tmp_path / f"{label}.png"
        request.write_text(
            json.dumps({
                "schema_version": 1,
                "timing": {"frame": 0, "fps": 30, "duration_frames": 300},
                "assignments": request_assignments,
            }),
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
        assert report["schema_version"] == 1
        assert report["stage"] == "interactive_image_render"
        assert report["plugin_id"] == "experimental-timed-layers"
        assert report["worker_classification"] == "ok"
        assert report["render_path"] == "classic"
        assert report["pixel_format"] == "argb8"
        assert report["output_transport"] == "rgba8_png"
        assert report["width"] == report["input_width"] == WIDTH
        assert report["height"] == report["input_height"] == HEIGHT
        assert report["full_resolution_dimensions"] == [WIDTH, HEIGHT]
        assert report["current_time"] == 0
        assert report["time_step"] == 1
        assert report["time_scale"] == 30
        assert report["total_time"] == 300
        assert report["input_sha256"] == hashlib.sha256(
            argb(source_pixels(
                SOURCE_A_RGB if source_label == "a" else SOURCE_B_RGB
            ))
        ).hexdigest()
        assert "output_pixels_valid" in report
        assert report["output_pixels_valid"] is None
        assert Path(report["output_png"]).resolve(strict=True) == (
            output.resolve(strict=True)
        )
        assert report["output_raw"] is None
        assert report["parameter_count_contract_ok"] is True
        assert report["spatial_contract_ok"] is True
        assert report["output_origin_contract_ok"] is True
        assert report["param_checkouts_balanced"] is True
        for receipt_name in (
            "guard_bytes_intact",
            "handle_lifetimes_balanced",
            "world_lifetimes_balanced",
        ):
            assert report[receipt_name] is True

        # Rio Grade retains the PF Handle suite acquired in GLOBAL_SETUP for
        # the lifetime of its loaded module.  The isolated worker records this
        # bounded lease explicitly instead of misreporting it as balanced; all
        # selector-local acquisitions are still paired and the worker exits
        # after this one render.
        assert report["suite_leases_balanced"] is False
        assert report["suite_lease_warning"] is True
        assert report["host_contract_warning"] is False
        assert report["live_suite_leases"] == "PF Handle Suite@2=1"
        assert report["suite_acquires"] == report["suite_releases"] + 1

        (tmp_path / f"{label}.report.json").write_text(
            json.dumps(report, indent=2, sort_keys=True), encoding="utf-8"
        )

        receipts = report["requested_parameters"]
        assert len(receipts) == len(requested_values)
        assert len({receipt["slot"] for receipt in receipts}) == len(receipts)
        assert {
            receipt["slot"]: receipt["value"] for receipt in receipts
        } == requested_values

        assert output.is_file() and output.stat().st_size > 0
        with Image.open(output) as image:
            assert image.format == "PNG"
            assert image.mode == "RGBA"
            image.load()
            assert image.size == (WIDTH, HEIGHT)
            pixels = image.convert("RGBA").tobytes()
        assert hashlib.sha256(argb(pixels)).hexdigest() == report["output_sha256"]
        outputs[label] = pixels

    metrics = assert_rio_grade_response(
        outputs["active_a"],
        outputs["repeat_a"],
        outputs["neutral_a"],
        outputs["active_b"],
    )
    (tmp_path / "metrics.json").write_text(
        json.dumps(metrics, indent=2, sort_keys=True), encoding="utf-8"
    )

"""Exact BCC Fast Flipper pixel permutations for two source images."""

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
    r"\BorisFX\Continuum\BCCFastFlipper.aex"
)
EXPECTED_NAME = "BCCFastFlipper.aex"
EXPECTED_VENDOR = "BorisFX"
EXPECTED_PRODUCT = "Continuum"
EXPECTED_PLUGIN_SHA256 = (
    "0b634c30e8697fd0a23430ba6cf64b942ed459f266e6a7eb2441a011a63a2172"
)

FLIP_CHOICES = [
    "Horizontal",
    "Vertical",
    "Both",
    "Mirror Left To Right",
    "Mirror Right To Left",
    "Mirror Top To Bottom",
    "Mirror Bottom To Top",
]

PARAMETER_SCHEMA = {
    1: {
        "name": "Effect Presets & Docs",
        "kind": "no_data",
        "enabled": True,
        "visible": True,
        "supervised": True,
        "custom_ui_events": 5,
        "control_size": [349, 56],
    },
    2: {
        "name": "Flip",
        "kind": "integer",
        "minimum": 1,
        "maximum": 7,
        "value": 1,
        "choices": FLIP_CHOICES,
        "enabled": True,
        "visible": True,
        "supervised": True,
    },
    3: {
        "name": "Mirror Blend",
        "kind": "float",
        "minimum": 0,
        "maximum": 30000,
        "value": 10,
        "choices": [],
        "enabled": True,
        "visible": True,
        "supervised": True,
    },
    4: {
        "name": "Mirror Offset %",
        "kind": "float",
        "minimum": -99.99998474121094,
        "maximum": 100,
        "value": 0,
        "choices": [],
        "enabled": True,
        "visible": True,
        "supervised": True,
    },
}


def source_a_pixels():
    """Opaque coordinate-unique, horizontally and vertically asymmetric RGBA."""
    return bytes(
        component
        for y in range(HEIGHT)
        for x in range(WIDTH)
        for component in (
            x,
            y,
            (37 * x + 101 * y + 17) & 0xFF,
            255,
        )
    )


def source_b_pixels():
    """A second coordinate-unique source unrelated to a flip of source A."""
    return bytes(
        component
        for y in range(HEIGHT)
        for x in range(WIDTH)
        for component in (
            x ^ 0xA5,
            y + 71,
            (73 * x + 29 * y + 193) & 0xFF,
            255,
        )
    )


def _pixels_as_tuples(pixels):
    return {
        tuple(pixels[offset:offset + 4])
        for offset in range(0, len(pixels), 4)
    }


def horizontal_flip(pixels):
    assert len(pixels) == WIDTH * HEIGHT * 4
    output = bytearray(len(pixels))
    for y in range(HEIGHT):
        for x in range(WIDTH):
            source_offset = (y * WIDTH + (WIDTH - 1 - x)) * 4
            output_offset = (y * WIDTH + x) * 4
            output[output_offset:output_offset + 4] = pixels[
                source_offset:source_offset + 4
            ]
    return bytes(output)


def vertical_flip(pixels):
    assert len(pixels) == WIDTH * HEIGHT * 4
    row_bytes = WIDTH * 4
    return b"".join(
        pixels[y * row_bytes:(y + 1) * row_bytes]
        for y in reversed(range(HEIGHT))
    )


def _changed_pixels(left, right):
    assert len(left) == len(right) == WIDTH * HEIGHT * 4
    return sum(
        left[offset:offset + 4] != right[offset:offset + 4]
        for offset in range(0, len(left), 4)
    )


def assert_fast_flipper_response(horizontal_a, repeat_a, horizontal_b,
                                 vertical_a):
    source_a = source_a_pixels()
    source_b = source_b_pixels()
    expected_size = WIDTH * HEIGHT * 4
    outputs = (horizontal_a, repeat_a, horizontal_b, vertical_a)
    for pixels in outputs:
        assert len(pixels) == expected_size
        assert pixels[3::4] == bytes([255]) * (WIDTH * HEIGHT)

    # Each source encodes its coordinates in the first two channels. This
    # makes an off-by-one permutation observable at every pixel.
    assert len(_pixels_as_tuples(source_a)) == WIDTH * HEIGHT
    assert len(_pixels_as_tuples(source_b)) == WIDTH * HEIGHT

    expected_horizontal_a = horizontal_flip(source_a)
    expected_horizontal_b = horizontal_flip(source_b)
    expected_vertical_a = vertical_flip(source_a)
    assert horizontal_a == expected_horizontal_a
    assert repeat_a == horizontal_a
    assert horizontal_b == expected_horizontal_b
    assert vertical_a == expected_vertical_a
    assert horizontal_a != vertical_a

    metrics = {
        "repeat_exact": repeat_a == horizontal_a,
        "horizontal_a_changed_pixels": _changed_pixels(
            source_a, horizontal_a
        ),
        "horizontal_b_changed_pixels": _changed_pixels(
            source_b, horizontal_b
        ),
        "vertical_a_changed_pixels": _changed_pixels(source_a, vertical_a),
        "horizontal_a_rgba_sha256": hashlib.sha256(horizontal_a).hexdigest(),
        "horizontal_b_rgba_sha256": hashlib.sha256(horizontal_b).hexdigest(),
        "vertical_a_rgba_sha256": hashlib.sha256(vertical_a).hexdigest(),
    }
    assert metrics["horizontal_a_changed_pixels"] == WIDTH * HEIGHT
    assert metrics["horizontal_b_changed_pixels"] == WIDTH * HEIGHT
    assert metrics["vertical_a_changed_pixels"] == WIDTH * HEIGHT
    return metrics


def _synthetic_outputs():
    source_a = source_a_pixels()
    horizontal_a = horizontal_flip(source_a)
    return (
        horizontal_a,
        horizontal_a,
        horizontal_flip(source_b_pixels()),
        vertical_flip(source_a),
    )


def test_fast_flipper_validator_accepts_exact_permutations():
    metrics = assert_fast_flipper_response(*_synthetic_outputs())
    assert metrics["repeat_exact"] is True
    assert metrics["horizontal_a_changed_pixels"] == WIDTH * HEIGHT


def _roll_each_row_right(pixels):
    row_bytes = WIDTH * 4
    output = bytearray()
    for y in range(HEIGHT):
        row = pixels[y * row_bytes:(y + 1) * row_bytes]
        output.extend(row[-4:] + row[:-4])
    return bytes(output)


def _crop_top_and_pad(pixels):
    row_bytes = WIDTH * 4
    return pixels[row_bytes:] + bytes((0, 0, 0, 255)) * WIDTH


def _rotate_180(pixels):
    return vertical_flip(horizontal_flip(pixels))


@pytest.mark.parametrize(
    "fault",
    (
        "passthrough",
        "fixed",
        "repeat_drift",
        "horizontal_vertical_swap",
        "off_by_one",
        "crop",
        "rotation",
        "tint",
        "alpha",
        "truncated",
        "wrong_b",
    ),
)
def test_fast_flipper_validator_rejects_corruption(fault):
    horizontal_a, repeat_a, horizontal_b, vertical_a = _synthetic_outputs()
    assert_fast_flipper_response(
        horizontal_a, repeat_a, horizontal_b, vertical_a
    )

    if fault == "passthrough":
        horizontal_a = repeat_a = source_a_pixels()
    elif fault == "fixed":
        fixed = bytes((31, 67, 149, 255)) * (WIDTH * HEIGHT)
        horizontal_a = repeat_a = horizontal_b = vertical_a = fixed
    elif fault == "repeat_drift":
        repeat_a = bytearray(repeat_a)
        repeat_a[0] ^= 1
    elif fault == "horizontal_vertical_swap":
        horizontal_a, vertical_a = vertical_a, horizontal_a
    elif fault == "off_by_one":
        horizontal_a = repeat_a = _roll_each_row_right(horizontal_a)
    elif fault == "crop":
        horizontal_a = repeat_a = _crop_top_and_pad(horizontal_a)
    elif fault == "rotation":
        horizontal_a = repeat_a = _rotate_180(source_a_pixels())
    elif fault == "tint":
        changed = []
        for pixels in (horizontal_a, repeat_a, horizontal_b, vertical_a):
            pixels = bytearray(pixels)
            for offset in range(0, len(pixels), 4):
                pixels[offset] = (pixels[offset] + 1) & 0xFF
            changed.append(bytes(pixels))
        horizontal_a, repeat_a, horizontal_b, vertical_a = changed
    elif fault == "alpha":
        horizontal_a = bytearray(horizontal_a)
        horizontal_a[3] = 254
    elif fault == "truncated":
        vertical_a = vertical_a[:-4]
    else:
        horizontal_b = horizontal_flip(source_a_pixels())

    with pytest.raises(AssertionError):
        assert_fast_flipper_response(
            bytes(horizontal_a),
            bytes(repeat_a),
            bytes(horizontal_b),
            bytes(vertical_a),
        )


def _bind_parameters(parameters):
    assert len(parameters) == len(PARAMETER_SCHEMA)
    assert len({parameter["slot"] for parameter in parameters}) == len(
        parameters
    )
    by_slot = {parameter["slot"]: parameter for parameter in parameters}
    assert set(by_slot) == set(PARAMETER_SCHEMA)
    for slot, expected in PARAMETER_SCHEMA.items():
        parameter = by_slot[slot]
        for key, value in expected.items():
            assert parameter[key] == value
    return by_slot


def _synthetic_parameters():
    return [
        {"slot": slot, **schema}
        for slot, schema in PARAMETER_SCHEMA.items()
    ]


def test_fast_flipper_schema_accepts_exact_four_controls():
    parameters = _bind_parameters(_synthetic_parameters())
    assert parameters[2]["choices"] == FLIP_CHOICES


@pytest.mark.parametrize(
    "fault",
    (
        "duplicate_slot",
        "name",
        "kind",
        "minimum",
        "maximum",
        "default",
        "choices",
        "disabled",
        "hidden",
        "unsupervised",
        "custom_ui_events",
        "control_size",
    ),
)
def test_fast_flipper_schema_rejects_drift(fault):
    parameters = _synthetic_parameters()
    if fault == "duplicate_slot":
        parameters[-1]["slot"] = 3
    elif fault == "name":
        parameters[1]["name"] = "Flipper"
    elif fault == "kind":
        parameters[1]["kind"] = "float"
    elif fault == "minimum":
        parameters[3]["minimum"] = -100
    elif fault == "maximum":
        parameters[2]["maximum"] = 100
    elif fault == "default":
        parameters[2]["value"] = 0
    elif fault == "choices":
        parameters[1]["choices"] = list(reversed(FLIP_CHOICES))
    elif fault == "disabled":
        parameters[2]["enabled"] = False
    elif fault == "hidden":
        parameters[3]["visible"] = False
    elif fault == "unsupervised":
        parameters[1]["supervised"] = False
    elif fault == "custom_ui_events":
        parameters[0]["custom_ui_events"] = 4
    else:
        parameters[0]["control_size"] = [348, 56]

    with pytest.raises(AssertionError):
        _bind_parameters(parameters)


def _assignments(flip):
    requested = {2: flip, 3: 10, 4: 0}
    return (
        [
            {"slot": slot, "value": value}
            for slot, value in sorted(requested.items())
        ],
        requested,
    )


def _assert_report(report, source_pixels, output):
    assert report["passed"] is True
    assert report["schema_version"] == 1
    assert report["stage"] == "interactive_image_render"
    assert report["plugin_id"] == "experimental-timed-layers"
    assert report["worker_classification"] == "ok"
    assert report["render_path"] == "smartfx"
    assert report["pixel_format"] == "argb8"
    assert report["output_transport"] == "rgba8_png"
    assert report["width"] == report["input_width"] == WIDTH
    assert report["height"] == report["input_height"] == HEIGHT
    assert report["full_resolution_dimensions"] == [WIDTH, HEIGHT]
    assert report["in_data_dimensions"] == [WIDTH, HEIGHT]
    assert report["current_time"] == 0
    assert report["time_step"] == 1
    assert report["local_time_step"] == 1
    assert report["time_scale"] == 30
    assert report["total_time"] == 300
    assert report["field"] == 0
    assert report["quality"] == 1
    assert report["downsample_x"] == [1, 1]
    assert report["downsample_y"] == [1, 1]
    assert report["input_sha256"] == hashlib.sha256(
        argb(source_pixels)
    ).hexdigest()
    assert report["output_pixels_valid"] is True
    assert Path(report["output_png"]).resolve(strict=True) == (
        output.resolve(strict=True)
    )
    assert report["output_raw"] is None
    assert report["output_origin"] == [0, 0]
    assert report["pre_effect_source_origin"] == [0, 0]
    assert report["input_checkout_result_rect"] == [0, 0, WIDTH, HEIGHT]
    assert report["result_rect"] == [0, 0, WIDTH, HEIGHT]
    assert report["max_result_rect"] == [0, 0, WIDTH, HEIGHT]
    assert report["result_rects_valid"] is True
    assert report["result_within_request"] is True
    assert report["spatial_contract_ok"] is True
    assert report["output_origin_contract_ok"] is True
    assert report["param_checkouts_balanced"] is True
    assert report["smart_render_selector_dispatched"] is True
    assert report["pre_render_error"] == 0
    assert report["smart_render_error"] == 0
    assert report["smart_render_selector_error"] == 0
    assert report["secondary_layers"] == []

    # BCC reports four effect parameters while PF's in_data count includes
    # the implicit source at index zero. Preserve that known warning as a
    # classification instead of silently treating it as a clean contract.
    assert report["in_data_num_params"] == 5
    assert len(report["parameter_metadata"]) == 4
    assert [item["index"] for item in report["parameter_metadata"]] == [
        1, 2, 3, 4
    ]
    assert [item["type"] for item in report["parameter_metadata"]] == [
        "no_data", "popup", "fixed_slider", "fixed_slider"
    ]
    assert report["parameter_count_contract_ok"] is False
    assert report["host_contract_warning"] is True

    assert report["guard_bytes_intact"] is True
    assert report["handle_lifetimes_balanced"] is True
    assert report["world_lifetimes_balanced"] is True
    assert report["suite_leases_balanced"] is True
    assert report["suite_lease_warning"] is False
    assert report["live_suite_leases"] == ""
    assert report["suite_acquires"] == report["suite_releases"]

    diagnostics = report["worker_diagnostics"]
    assert diagnostics["classification"] == "ok"
    assert diagnostics["exit_code"] == 0
    assert diagnostics["active_stage"] is None
    assert diagnostics["failure_stage"] is None
    assert diagnostics["first_failure_stage"] is None
    assert diagnostics["load_failure"] is None
    assert diagnostics["unsupported_suite_calls"] == []
    assert diagnostics["unsupported_suite_calls_truncated"] is False
    assert diagnostics["callback_denials"] == []
    assert diagnostics["callback_denials_truncated"] is False
    # The direct interactive harness does not carry the sweep's secure module
    # audit receipt.  Classify that exact boundary instead of silently passing
    # arbitrary audit warnings; the rebuilt worker itself must still be fresh.
    assert diagnostics["module_audit_warning"] == (
        "secure worker module audit did not pass"
    )
    assert diagnostics["missing_suites"] == [
        {"name": "VDS App Suite", "version": 1}
    ]
    assert diagnostics["missing_suites_truncated"] is False
    assert diagnostics.get("worker_freshness_warning") is None


def test_real_bcc_fast_flipper_response(tmp_path):
    configured = os.environ.get("AEXCOMPAT_TEST_BCC_FAST_FLIPPER")
    if not configured:
        pytest.skip(
            "set AEXCOMPAT_TEST_BCC_FAST_FLIPPER to the exact installed "
            "BCCFastFlipper.aex"
        )
    assert os.name == "nt"

    # Resolve one exact installed identity before the harness loads any AEX.
    plugin_path = Path(configured).resolve(strict=True)
    expected_path = EXPECTED_PLUGIN.resolve(strict=True)
    assert plugin_path == expected_path
    assert plugin_path.is_file()
    assert plugin_path.name == EXPECTED_NAME
    assert plugin_path.parent.name == EXPECTED_PRODUCT
    assert plugin_path.parent.parent.name == EXPECTED_VENDOR
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

    inspection = run("--inspect-experimental", plugin)
    (tmp_path / "inspection.json").write_text(
        json.dumps(inspection, indent=2, sort_keys=True), encoding="utf-8"
    )
    _bind_parameters(inspection)

    source_pixels_by_label = {
        "a": source_a_pixels(),
        "b": source_b_pixels(),
    }
    sources = {}
    for label, pixels in source_pixels_by_label.items():
        source = tmp_path / f"source-{label}.png"
        Image.frombytes("RGBA", (WIDTH, HEIGHT), pixels).save(source)
        assert source.is_file() and source.stat().st_size > 0
        with Image.open(source) as image:
            assert image.format == "PNG"
            assert image.mode == "RGBA"
            image.load()
            assert image.size == (WIDTH, HEIGHT)
            assert image.tobytes() == pixels
        sources[label] = source

    cases = (
        ("horizontal_a", "a", 1),
        ("repeat_a", "a", 1),
        ("horizontal_b", "b", 1),
        ("vertical_a", "a", 2),
    )
    outputs = {}
    for label, source_label, flip in cases:
        assignments, requested_values = _assignments(flip)
        request = tmp_path / f"{label}.request.json"
        output = tmp_path / f"{label}.png"
        request.write_text(
            json.dumps({
                "schema_version": 1,
                "timing": {
                    "frame": 0,
                    "fps": 30,
                    "duration_frames": 300,
                },
                "assignments": assignments,
            }),
            encoding="utf-8",
        )
        report = run(
            "--render-experimental-smart-request",
            plugin,
            sources[source_label],
            output,
            request,
        )
        # Preserve the raw evidence even when a contract assertion below
        # rejects it; drift is exactly when the unmodified report is useful.
        (tmp_path / f"{label}.report.json").write_text(
            json.dumps(report, indent=2, sort_keys=True), encoding="utf-8"
        )
        _assert_report(
            report, source_pixels_by_label[source_label], output
        )

        expected_receipts = [
            {
                "id": f"param_{slot}",
                "kind": "integer" if slot == 2 else "float",
                "slot": slot,
                "value": value,
            }
            for slot, value in sorted(requested_values.items())
        ]
        receipts = report["requested_parameters"]
        assert receipts == expected_receipts
        assert len({receipt["slot"] for receipt in receipts}) == len(receipts)
        assert len({receipt["id"] for receipt in receipts}) == len(receipts)
        assert 1 not in {receipt["slot"] for receipt in receipts}

        assert output.is_file() and output.stat().st_size > 0
        with Image.open(output) as image:
            assert image.format == "PNG"
            assert image.mode == "RGBA"
            image.load()
            assert image.size == (WIDTH, HEIGHT)
            pixels = image.tobytes()
        assert hashlib.sha256(argb(pixels)).hexdigest() == (
            report["output_sha256"]
        )
        outputs[label] = pixels

    metrics = assert_fast_flipper_response(
        outputs["horizontal_a"],
        outputs["repeat_a"],
        outputs["horizontal_b"],
        outputs["vertical_a"],
    )
    (tmp_path / "metrics.json").write_text(
        json.dumps(metrics, indent=2, sort_keys=True), encoding="utf-8"
    )

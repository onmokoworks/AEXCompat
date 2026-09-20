"""Bounded BCC Drop Shadow source and distance response."""

import copy
import hashlib
import json
import math
import os
import subprocess
from itertools import product
from pathlib import Path

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH, argb


EXPECTED_PLUGIN = Path(
    r"C:\Program Files\Adobe\Common\Plug-ins\7.0\MediaCore"
    r"\BorisFX\Continuum\BCCDropShadow.aex"
)
EXPECTED_NAME = "BCCDropShadow.aex"
EXPECTED_VENDOR = "BorisFX"
EXPECTED_PRODUCT = "Continuum"
EXPECTED_PLUGIN_SIZE = 33792
EXPECTED_PATH_SHA256 = (
    "22a92da2278301382acd211cbdbfb40af1f919d339408b5e04b1a92403d2c829"
)
EXPECTED_PLUGIN_SHA256 = (
    "7cf5379efcd128e0cffca56966e5a90bbc6d07770baa8405f9937955acbdbcf7"
)

DEFAULT_DISTANCE = 20
VARIANT_DISTANCE = 60

BLUR_QUALITY_CHOICES = [
    "Gaussian Low",
    "Gaussian Medium",
    "Gaussian High",
    "Gaussian Higher",
    "Gaussian Highest",
    "Pyramid",
    "Pyramid Smoother",
]

_COMMON_SCHEMA = {
    "choices": [],
    "color": [255, 0, 0, 0],
    "components": [0.0, 0.0, 0.0],
    "component_count": 0,
    "layer_path": None,
    "enabled": True,
    "visible": True,
    "supervised": True,
    "debug_summary": None,
    "custom_ui_events": 5,
    "control_size": [0, 0],
}

PARAMETER_SCHEMA = {
    1: {
        **_COMMON_SCHEMA,
        "name": "Effect Presets & Docs",
        "kind": "no_data",
        "minimum": 0,
        "maximum": 0,
        "value": 0,
        "control_size": [349, 56],
    },
    2: {
        **_COMMON_SCHEMA,
        "name": "Source Opacity",
        "kind": "float",
        "minimum": 0,
        "maximum": 100,
        "value": 100,
    },
    3: {
        **_COMMON_SCHEMA,
        "name": "Avoid Clipping",
        "kind": "integer",
        "minimum": 0,
        "maximum": 1,
        "value": 1,
    },
    4: {
        **_COMMON_SCHEMA,
        "name": "Shadow Distance",
        "kind": "float",
        "minimum": 0,
        "maximum": 1000,
        "value": DEFAULT_DISTANCE,
    },
    5: {
        **_COMMON_SCHEMA,
        "name": "Shadow Intensity",
        "kind": "float",
        "minimum": 0,
        "maximum": 100,
        "value": 60,
    },
    6: {
        **_COMMON_SCHEMA,
        "name": "Shadow Angle",
        "kind": "angle",
        "minimum": 0,
        "maximum": 0,
        "value": 0,
        "components": [135.0, 0.0, 0.0],
        "component_count": 1,
    },
    7: {
        **_COMMON_SCHEMA,
        "name": "Shadow Color",
        "kind": "color",
        "minimum": 0,
        "maximum": 0,
        "value": 0,
    },
    8: {
        **_COMMON_SCHEMA,
        "name": "Shadow Softness",
        "kind": "float",
        "minimum": 0,
        "maximum": 100,
        "value": 10,
    },
    9: {
        **_COMMON_SCHEMA,
        "name": "Blur Quality",
        "kind": "integer",
        "minimum": 1,
        "maximum": 7,
        "value": 6,
        "choices": BLUR_QUALITY_CHOICES,
    },
    10: {
        **_COMMON_SCHEMA,
        "name": "Gamma",
        "kind": "float",
        "minimum": 0,
        "maximum": 10,
        "value": 1,
    },
}


def source_a_pixels():
    """Opaque coordinate-unique source with no reflection symmetry."""
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
    """Different RGB at every coordinate, with alpha identical to source A."""
    return bytes(
        component
        for y in range(HEIGHT)
        for x in range(WIDTH)
        for component in (
            x ^ 0xA5,
            (y + 71) & 0xFF,
            (73 * x + 29 * y + 193) & 0xFF,
            255,
        )
    )


def _frame(pixels, width, height, origin):
    return {
        "pixels": bytes(pixels),
        "width": width,
        "height": height,
        "origin": tuple(origin),
    }


def _assert_frame_shape(frame):
    assert set(frame) == {"pixels", "width", "height", "origin"}
    assert isinstance(frame["width"], int) and frame["width"] > 0
    assert isinstance(frame["height"], int) and frame["height"] > 0
    assert len(frame["origin"]) == 2
    assert all(isinstance(value, int) for value in frame["origin"])
    assert len(frame["pixels"]) == frame["width"] * frame["height"] * 4


def _source_crop(frame):
    origin_x, origin_y = frame["origin"]
    assert origin_x <= 0 and origin_y <= 0
    assert origin_x + frame["width"] >= WIDTH
    assert origin_y + frame["height"] >= HEIGHT
    rows = []
    for y in range(HEIGHT):
        pixel_x = -origin_x
        pixel_y = y - origin_y
        start = (pixel_y * frame["width"] + pixel_x) * 4
        rows.append(frame["pixels"][start:start + WIDTH * 4])
    return b"".join(rows)


def _outside_rgba(frame):
    outside = {}
    origin_x, origin_y = frame["origin"]
    for image_y in range(frame["height"]):
        world_y = origin_y + image_y
        for image_x in range(frame["width"]):
            world_x = origin_x + image_x
            if 0 <= world_x < WIDTH and 0 <= world_y < HEIGHT:
                continue
            offset = (image_y * frame["width"] + image_x) * 4
            outside[(world_x, world_y)] = tuple(
                frame["pixels"][offset:offset + 4]
            )
    return outside


def _shadow_support(frame):
    support = {}
    maximum_rgb = 0
    for coordinate, rgba in _outside_rgba(frame).items():
        red, green, blue, alpha = rgba
        if alpha:
            support[coordinate] = alpha
            maximum_rgb = max(maximum_rgb, red, green, blue)
    return support, maximum_rgb


def _support_bbox(support):
    assert support
    xs = [coordinate[0] for coordinate in support]
    ys = [coordinate[1] for coordinate in support]
    return min(xs), min(ys), max(xs), max(ys)


def _overflow_direction(bbox):
    left = max(0, -bbox[0])
    top = max(0, -bbox[1])
    right = max(0, bbox[2] - (WIDTH - 1))
    bottom = max(0, bbox[3] - (HEIGHT - 1))

    def direction(negative, positive):
        if negative == positive:
            return 0
        return 1 if positive > negative else -1

    return {
        "overflow": {
            "left": left,
            "right": right,
            "top": top,
            "bottom": bottom,
        },
        "x": direction(left, right),
        "y": direction(top, bottom),
    }


def _translation_quality(base, variant, dx, dy):
    forward = []
    for (x, y), alpha in base.items():
        target = x + dx, y + dy
        if 0 <= target[0] < WIDTH and 0 <= target[1] < HEIGHT:
            continue
        # A coordinate beyond the returned extent is transparent, not absent
        # evidence. Only an opaque source pixel can legitimately occlude the
        # translated shadow and be excluded from the comparison.
        forward.append(abs(alpha - variant.get(target, 0)))

    reverse = []
    for (x, y), alpha in variant.items():
        target = x - dx, y - dy
        if 0 <= target[0] < WIDTH and 0 <= target[1] < HEIGHT:
            continue
        reverse.append(abs(alpha - base.get(target, 0)))

    differences = forward + reverse
    if not forward or not reverse:
        return {
            "dx": dx,
            "dy": dy,
            "forward_samples": len(forward),
            "reverse_samples": len(reverse),
            "mean_alpha_error": float("inf"),
            "close_fraction": 0,
        }
    return {
        "dx": dx,
        "dy": dy,
        "forward_samples": len(forward),
        "reverse_samples": len(reverse),
        "mean_alpha_error": sum(differences) / len(differences),
        "close_fraction": (
            sum(difference <= 8 for difference in differences)
            / len(differences)
        ),
    }


def _response_analysis(active_a, repeat_a, active_b, distance_a):
    frames = (active_a, repeat_a, active_b, distance_a)
    for frame in frames:
        _assert_frame_shape(frame)

    source_a = source_a_pixels()
    source_b = source_b_pixels()
    source_crops = [
        _source_crop(active_a),
        _source_crop(repeat_a),
        _source_crop(active_b),
        _source_crop(distance_a),
    ]
    supports_and_rgb = [_shadow_support(frame) for frame in frames]
    supports = [item[0] for item in supports_and_rgb]
    rgb_maxima = [item[1] for item in supports_and_rgb]

    base_bbox = _support_bbox(supports[0])
    variant_bbox = _support_bbox(supports[3])
    baseline_direction = _overflow_direction(base_bbox)
    dx_options = {
        variant_bbox[0] - base_bbox[0],
        variant_bbox[2] - base_bbox[2],
    }
    dy_options = {
        variant_bbox[1] - base_bbox[1],
        variant_bbox[3] - base_bbox[3],
    }
    candidates = [
        _translation_quality(supports[0], supports[3], dx, dy)
        for dx, dy in product(sorted(dx_options), sorted(dy_options))
    ]
    translation = min(
        candidates,
        key=lambda item: (
            item["mean_alpha_error"],
            -item["close_fraction"],
            -(item["forward_samples"] + item["reverse_samples"]),
        ),
    )

    same_position_changed = sum(
        supports[0].get(coordinate, 0)
        != supports[3].get(coordinate, 0)
        for coordinate in set(supports[0]) | set(supports[3])
    )
    def extent(frame):
        return {
            "origin": list(frame["origin"]),
            "width": frame["width"],
            "height": frame["height"],
        }

    metrics = {
        "repeat_exact": repeat_a == active_a,
        "source_crops_exact": [
            source_crops[0] == source_a,
            source_crops[1] == source_a,
            source_crops[2] == source_b,
            source_crops[3] == source_a,
        ],
        "baseline_extents_equal": extent(active_a) == extent(active_b),
        "baseline_outside_equal": (
            _outside_rgba(active_a) == _outside_rgba(active_b)
        ),
        "baseline_shadow_support_equal": supports[0] == supports[2],
        "support_counts": [len(support) for support in supports],
        "shadow_rgb_maxima": rgb_maxima,
        "base_bbox": list(base_bbox),
        "variant_bbox": list(variant_bbox),
        "baseline_direction": baseline_direction,
        "translation_candidates": candidates,
        "translation": translation,
        "same_position_changed": same_position_changed,
        "extents": [extent(frame) for frame in frames],
    }
    return metrics


def assert_drop_shadow_response(
    active_a, repeat_a, active_b, distance_a, *, measured=None
):
    metrics = measured or _response_analysis(
        active_a, repeat_a, active_b, distance_a
    )
    assert metrics["repeat_exact"], metrics
    assert all(metrics["source_crops_exact"]), metrics
    assert metrics["baseline_extents_equal"], metrics
    assert metrics["baseline_outside_equal"], metrics
    assert metrics["baseline_shadow_support_equal"], metrics

    for count in metrics["support_counts"]:
        assert count > WIDTH + HEIGHT, metrics
    assert max(metrics["shadow_rgb_maxima"]) <= 1, metrics

    assert metrics["extents"][0] != metrics["extents"][3], metrics
    assert metrics["same_position_changed"] > WIDTH + HEIGHT, metrics
    translation = metrics["translation"]
    baseline_direction = metrics["baseline_direction"]
    assert baseline_direction["x"] in (-1, 1), metrics
    assert baseline_direction["y"] in (-1, 1), metrics
    assert abs(translation["dx"]) >= 8, metrics
    assert abs(translation["dy"]) >= 8, metrics
    # Do not encode BorisFX's angle sign convention. Derive the established
    # direction from which sides of the baseline shadow overflow the opaque
    # source, then require an increased distance to continue on both axes.
    assert translation["dx"] * baseline_direction["x"] > 0, metrics
    assert translation["dy"] * baseline_direction["y"] > 0, metrics
    assert (
        translation["dx"] * baseline_direction["x"]
        + translation["dy"] * baseline_direction["y"]
    ) > 0, metrics
    distance = math.hypot(translation["dx"], translation["dy"])
    distance_delta = VARIANT_DISTANCE - DEFAULT_DISTANCE
    assert distance_delta * 0.75 <= distance <= distance_delta * 1.25, metrics
    assert translation["forward_samples"] > WIDTH + HEIGHT, metrics
    assert translation["reverse_samples"] > WIDTH + HEIGHT, metrics
    # A distance-only change reuses the same softness kernel. Permit sparse
    # byte rounding at its edge, but reject a resized or otherwise rewritten
    # shadow that merely happens to move in the same general direction.
    assert translation["mean_alpha_error"] <= 2, metrics
    assert translation["close_fraction"] >= 0.95, metrics
    return metrics


def _synthetic_frame(
    source,
    shift,
    *,
    shadow_size=(WIDTH, HEIGHT),
    source_origin=(0, 0),
    sparse_rounding=False,
):
    shadow_x, shadow_y = shift
    source_x, source_y = source_origin
    shadow_width, shadow_height = shadow_size
    origin_x = min(source_x, shadow_x)
    origin_y = min(source_y, shadow_y)
    right = max(source_x + WIDTH, shadow_x + shadow_width)
    bottom = max(source_y + HEIGHT, shadow_y + shadow_height)
    width = right - origin_x
    height = bottom - origin_y
    pixels = bytearray(width * height * 4)

    outside_index = 0
    for local_y in range(shadow_height):
        for local_x in range(shadow_width):
            world_x = shadow_x + local_x
            world_y = shadow_y + local_y
            image_x = world_x - origin_x
            image_y = world_y - origin_y
            offset = (image_y * width + image_x) * 4
            alpha = 80 + (7 * local_x + 11 * local_y) % 120
            outside_source = not (
                source_x <= world_x < source_x + WIDTH
                and source_y <= world_y < source_y + HEIGHT
            )
            if sparse_rounding and outside_source:
                if outside_index % 997 == 0:
                    alpha += 2 if alpha <= 253 else -2
                outside_index += 1
            pixels[offset:offset + 4] = bytes((0, 0, 0, alpha))

    for y in range(HEIGHT):
        for x in range(WIDTH):
            source_offset = (y * WIDTH + x) * 4
            image_x = source_x + x - origin_x
            image_y = source_y + y - origin_y
            output_offset = (image_y * width + image_x) * 4
            pixels[output_offset:output_offset + 4] = source[
                source_offset:source_offset + 4
            ]
    return _frame(pixels, width, height, (origin_x, origin_y))


def _synthetic_frames(*, sparse_rounding=False):
    source_a = source_a_pixels()
    source_b = source_b_pixels()
    active_a = _synthetic_frame(source_a, (-18, 14))
    return (
        active_a,
        copy.deepcopy(active_a),
        _synthetic_frame(source_b, (-18, 14)),
        _synthetic_frame(
            source_a, (-46, 42), sparse_rounding=sparse_rounding
        ),
    )


def _mutate_outside(frame, mutator):
    changed = copy.deepcopy(frame)
    origin_x, origin_y = changed["origin"]
    pixels = bytearray(changed["pixels"])
    seen = 0
    for image_y in range(changed["height"]):
        world_y = origin_y + image_y
        for image_x in range(changed["width"]):
            world_x = origin_x + image_x
            if 0 <= world_x < WIDTH and 0 <= world_y < HEIGHT:
                continue
            offset = (image_y * changed["width"] + image_x) * 4
            if pixels[offset + 3]:
                mutator(pixels, offset, seen)
                seen += 1
    changed["pixels"] = bytes(pixels)
    return changed


def test_drop_shadow_validator_accepts_source_dependent_translation():
    metrics = assert_drop_shadow_response(*_synthetic_frames())
    assert metrics["translation"]["dx"] == -28
    assert metrics["translation"]["dy"] == 28


def test_drop_shadow_validator_accepts_sparse_alpha_rounding():
    metrics = assert_drop_shadow_response(
        *_synthetic_frames(sparse_rounding=True)
    )
    assert metrics["translation"]["mean_alpha_error"] < 0.1


@pytest.mark.parametrize(
    "fault",
    (
        "passthrough",
        "fixed_output",
        "repeat_drift",
        "source_corruption",
        "rgb_dependent_shadow",
        "no_distance_response",
        "partial_shadow_drift",
        "shadow_growth",
        "outward_shadow_growth",
        "x_direction_reversed",
        "y_direction_reversed",
        "under_scaled_shift",
        "over_scaled_shift",
        "global_translation",
        "shadow_tint",
        "alpha_corruption",
        "truncated",
    ),
)
def test_drop_shadow_validator_rejects_corruption(fault):
    active_a, repeat_a, active_b, distance_a = _synthetic_frames()
    assert_drop_shadow_response(active_a, repeat_a, active_b, distance_a)

    if fault == "passthrough":
        active_a = repeat_a = _frame(
            source_a_pixels(), WIDTH, HEIGHT, (0, 0)
        )
    elif fault == "fixed_output":
        active_b = copy.deepcopy(active_a)
    elif fault == "repeat_drift":
        pixels = bytearray(repeat_a["pixels"])
        pixels[0] ^= 1
        repeat_a["pixels"] = bytes(pixels)
    elif fault == "source_corruption":
        pixels = bytearray(active_a["pixels"])
        source_offset = (-active_a["origin"][0]) * 4
        pixels[source_offset] ^= 1
        active_a["pixels"] = bytes(pixels)
    elif fault == "rgb_dependent_shadow":
        active_b = _mutate_outside(
            active_b,
            lambda pixels, offset, _seen: pixels.__setitem__(offset, 12),
        )
    elif fault == "no_distance_response":
        distance_a = copy.deepcopy(active_a)
    elif fault == "partial_shadow_drift":
        def corrupt_alpha(pixels, offset, seen):
            if seen % 4 == 0:
                pixels[offset + 3] = max(1, pixels[offset + 3] - 40)

        distance_a = _mutate_outside(distance_a, corrupt_alpha)
    elif fault == "shadow_growth":
        distance_a = _synthetic_frame(
            source_a_pixels(), (-46, 42), shadow_size=(WIDTH + 24, HEIGHT)
        )
    elif fault == "outward_shadow_growth":
        distance_a = _synthetic_frame(
            source_a_pixels(),
            (-166, 42),
            shadow_size=(WIDTH + 120, HEIGHT),
        )
    elif fault == "x_direction_reversed":
        distance_a = _synthetic_frame(source_a_pixels(), (10, 42))
    elif fault == "y_direction_reversed":
        distance_a = _synthetic_frame(source_a_pixels(), (-46, -14))
    elif fault == "under_scaled_shift":
        distance_a = _synthetic_frame(source_a_pixels(), (-30, 26))
    elif fault == "over_scaled_shift":
        distance_a = _synthetic_frame(source_a_pixels(), (-68, 64))
    elif fault == "global_translation":
        distance_a = _synthetic_frame(
            source_a_pixels(), (-45, 43), source_origin=(1, 1)
        )
    elif fault == "shadow_tint":
        distance_a = _mutate_outside(
            distance_a,
            lambda pixels, offset, _seen: pixels.__setitem__(offset, 24),
        )
    elif fault == "alpha_corruption":
        pixels = bytearray(active_a["pixels"])
        source_offset = (-active_a["origin"][0]) * 4 + 3
        pixels[source_offset] = 254
        active_a["pixels"] = bytes(pixels)
    else:
        distance_a["pixels"] = distance_a["pixels"][:-4]

    with pytest.raises(AssertionError):
        assert_drop_shadow_response(
            active_a, repeat_a, active_b, distance_a
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
        assert set(parameter) == {"slot", *expected}
        for key, value in expected.items():
            assert parameter[key] == value
    return by_slot


def _synthetic_parameters():
    return [
        {"slot": slot, **copy.deepcopy(schema)}
        for slot, schema in PARAMETER_SCHEMA.items()
    ]


def test_drop_shadow_schema_accepts_exact_ten_controls():
    parameters = _bind_parameters(_synthetic_parameters())
    assert parameters[4]["name"] == "Shadow Distance"
    assert parameters[6]["components"] == [135.0, 0.0, 0.0]
    assert parameters[9]["choices"] == BLUR_QUALITY_CHOICES


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
        "color",
        "components",
        "component_count",
        "layer_path",
        "disabled",
        "hidden",
        "unsupervised",
        "debug_summary",
        "custom_ui_events",
        "control_size",
        "extra_field",
    ),
)
def test_drop_shadow_schema_rejects_drift(fault):
    parameters = _synthetic_parameters()
    if fault == "duplicate_slot":
        parameters[-1]["slot"] = 9
    elif fault == "name":
        parameters[3]["name"] = "Distance"
    elif fault == "kind":
        parameters[5]["kind"] = "float"
    elif fault == "minimum":
        parameters[3]["minimum"] = -1
    elif fault == "maximum":
        parameters[3]["maximum"] = 999
    elif fault == "default":
        parameters[3]["value"] = 21
    elif fault == "choices":
        parameters[8]["choices"] = list(reversed(BLUR_QUALITY_CHOICES))
    elif fault == "color":
        parameters[6]["color"] = [255, 1, 0, 0]
    elif fault == "components":
        parameters[5]["components"] = [134.0, 0.0, 0.0]
    elif fault == "component_count":
        parameters[5]["component_count"] = 0
    elif fault == "layer_path":
        parameters[1]["layer_path"] = "source.png"
    elif fault == "disabled":
        parameters[1]["enabled"] = False
    elif fault == "hidden":
        parameters[2]["visible"] = False
    elif fault == "unsupervised":
        parameters[3]["supervised"] = False
    elif fault == "debug_summary":
        parameters[4]["debug_summary"] = "drift"
    elif fault == "custom_ui_events":
        parameters[0]["custom_ui_events"] = 4
    elif fault == "control_size":
        parameters[0]["control_size"] = [348, 56]
    else:
        parameters[1]["unexpected"] = True

    with pytest.raises(AssertionError):
        _bind_parameters(parameters)


def _assignments(distance):
    values = {
        2: 100,
        3: 1,
        4: distance,
        5: 60,
        6: [135.0],
        7: [255, 0, 0, 0],
        8: 10,
        9: 6,
        10: 1,
    }
    assignments = []
    for slot, value in values.items():
        kind = PARAMETER_SCHEMA[slot]["kind"]
        if kind == "angle":
            assignments.append({"slot": slot, "components": value})
        elif kind == "color":
            assignments.append({"slot": slot, "color": value})
        else:
            assignments.append({"slot": slot, "value": value})
    return assignments, values


def _expected_receipts(values):
    receipts = []
    for slot, value in values.items():
        kind = PARAMETER_SCHEMA[slot]["kind"]
        if kind == "color":
            alpha, red, green, blue = value
            receipt_value = {
                "alpha": alpha,
                "blue": blue,
                "green": green,
                "red": red,
            }
        else:
            receipt_value = value
        receipts.append(
            {
                "id": f"param_{slot}",
                "kind": kind,
                "slot": slot,
                "value": receipt_value,
            }
        )
    return receipts


def _assert_report(report, source, output):
    assert report["passed"] is True
    assert report["schema_version"] == 1
    assert report["stage"] == "interactive_image_render"
    assert report["plugin_id"] == "experimental-timed-layers"
    assert report["worker_classification"] == "ok"
    assert report["render_path"] == "smartfx"
    assert report["pixel_format"] == "argb8"
    assert report["output_transport"] == "rgba8_png"
    assert report["input_width"] == WIDTH
    assert report["input_height"] == HEIGHT
    assert report["width"] > WIDTH
    assert report["height"] > HEIGHT
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
    assert report["input_sha256"] == hashlib.sha256(argb(source)).hexdigest()
    assert report["output_pixels_valid"] is True
    assert Path(report["output_png"]).resolve(strict=True) == (
        output.resolve(strict=True)
    )
    assert report["output_raw"] is None
    assert report["premultiplication"] == "premultiplied"
    assert report["row_bytes"] == report["width"] * 4

    origin_x, origin_y = report["output_origin"]
    assert all(isinstance(value, int) for value in (origin_x, origin_y))
    assert origin_x <= 0 and origin_y <= 0
    assert origin_x + report["width"] >= WIDTH
    assert origin_y + report["height"] >= HEIGHT
    assert report["pre_effect_source_origin"] == [0, 0]
    assert report["input_checkout_result_rect"] == [0, 0, WIDTH, HEIGHT]
    expected_rect = [
        origin_x,
        origin_y,
        origin_x + report["width"],
        origin_y + report["height"],
    ]
    assert report["result_rect"] == expected_rect
    assert report["max_result_rect"] == expected_rect
    assert report["result_rects_valid"] is True
    # Avoid Clipping deliberately expands beyond the 256x144 input request;
    # the host records that boundary while still validating the returned
    # result/max-result rectangles and their output allocation below.
    assert report["result_within_request"] is False
    assert report["returns_extra_pixels"] is True
    assert report["extra_pixels_contract_violation"] is False
    assert report["spatial_contract_ok"] is True
    assert report["output_origin_contract_ok"] is True
    assert report["param_checkouts_balanced"] is True
    assert report["smart_render_selector_dispatched"] is True
    assert report["pre_render_error"] == 0
    assert report["smart_render_error"] == 0
    assert report["smart_render_selector_error"] == 0
    assert report["secondary_layers"] == []

    assert report["in_data_num_params"] == 11
    assert len(report["parameter_metadata"]) == 10
    assert [item["index"] for item in report["parameter_metadata"]] == list(
        range(1, 11)
    )
    assert [item["type"] for item in report["parameter_metadata"]] == [
        "no_data",
        "fixed_slider",
        "checkbox",
        "fixed_slider",
        "fixed_slider",
        "angle",
        "color",
        "fixed_slider",
        "popup",
        "fixed_slider",
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
    assert diagnostics["kill_reason"] is None
    assert diagnostics["memory_limit_reached"] is False
    assert diagnostics["minidump"] is None
    assert diagnostics["unsupported_suite_calls"] == []
    assert diagnostics["unsupported_suite_calls_truncated"] is False
    assert diagnostics["callback_denials"] == []
    assert diagnostics["callback_denials_truncated"] is False
    assert diagnostics["callback_addr_denials"] == []
    assert diagnostics["callback_addr_denials_truncated"] is False
    assert diagnostics["module_audit_warning"] == (
        "secure worker module audit did not pass"
    )
    expected_missing = [{"name": "VDS App Suite", "version": 1}]
    assert diagnostics["missing_suites"] == expected_missing
    assert diagnostics["missing_suites_truncated"] is False
    assert diagnostics["suite_acquire_failures"] == expected_missing
    assert diagnostics["suite_acquire_failures_truncated"] is False
    assert diagnostics.get("worker_freshness_warning") is None
    assert diagnostics["stderr_truncated"] is False


def _sha256_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _write_process_evidence(tmp_path, stem, completed, *, timed_out=False):
    stdout = completed.stdout or b""
    stderr = completed.stderr or b""
    (tmp_path / f"{stem}.stdout.bin").write_bytes(stdout)
    (tmp_path / f"{stem}.stderr.bin").write_bytes(stderr)
    (tmp_path / f"{stem}.process.json").write_text(
        json.dumps(
            {
                "returncode": getattr(completed, "returncode", None),
                "stderr_bytes": len(stderr),
                "stdout_bytes": len(stdout),
                "timed_out": timed_out,
            },
            indent=2,
            sort_keys=True,
        ),
        encoding="utf-8",
    )


def test_real_bcc_drop_shadow_response(tmp_path):
    configured = os.environ.get("AEXCOMPAT_TEST_BCC_DROP_SHADOW")
    if not configured:
        pytest.skip(
            "set AEXCOMPAT_TEST_BCC_DROP_SHADOW to the exact installed "
            "BCCDropShadow.aex"
        )
    assert os.name == "nt"

    # Resolve and record the exact identity before any process can load the
    # selected module. A mismatch is evidence drift and stops before inspect.
    plugin_path = Path(configured).resolve(strict=True)
    expected_path = EXPECTED_PLUGIN.resolve(strict=True)
    harness = ROOT / "broker/target/release/aexcompat-harness.exe"
    worker = ROOT / "target/minihost-build/aex_worker.exe"
    identity = {
        "schema_version": 1,
        "plugin": {
            "name": plugin_path.name,
            "size_bytes": plugin_path.stat().st_size,
            "path_sha256": hashlib.sha256(
                str(plugin_path).lower().encode("utf-8")
            ).hexdigest(),
            "sha256": _sha256_file(plugin_path),
        },
        "build": {
            "harness_sha256": _sha256_file(harness),
            "harness_size_bytes": harness.stat().st_size,
            "worker_sha256": _sha256_file(worker),
            "worker_size_bytes": worker.stat().st_size,
        },
    }
    (tmp_path / "preflight-evidence.json").write_text(
        json.dumps(identity, indent=2, sort_keys=True), encoding="utf-8"
    )
    assert plugin_path == expected_path
    assert plugin_path.name == EXPECTED_NAME
    assert plugin_path.parent.name == EXPECTED_PRODUCT
    assert plugin_path.parent.parent.name == EXPECTED_VENDOR
    assert identity["plugin"] == {
        "name": EXPECTED_NAME,
        "size_bytes": EXPECTED_PLUGIN_SIZE,
        "path_sha256": EXPECTED_PATH_SHA256,
        "sha256": EXPECTED_PLUGIN_SHA256,
    }
    assert identity["build"]["harness_size_bytes"] > 0
    assert identity["build"]["worker_size_bytes"] > 0

    plugin = str(plugin_path)

    def run(stem, *args, timeout):
        command = [str(harness), "--headless", *map(str, args)]
        try:
            completed = subprocess.run(
                command,
                cwd=ROOT,
                capture_output=True,
                timeout=timeout,
            )
        except subprocess.TimeoutExpired as error:
            _write_process_evidence(
                tmp_path, stem, error, timed_out=True
            )
            pytest.fail(f"{stem} timed out after {timeout} seconds")
        _write_process_evidence(tmp_path, stem, completed)
        assert completed.returncode == 0, completed.stderr.decode(
            "utf-8", errors="replace"
        )
        payload = json.loads(completed.stdout)
        (tmp_path / f"{stem}.json").write_text(
            json.dumps(payload, indent=2, sort_keys=True), encoding="utf-8"
        )
        return payload

    inspection = run(
        "inspection", "--inspect-experimental", plugin, timeout=90
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
        ("active_a", "a", DEFAULT_DISTANCE),
        ("repeat_a", "a", DEFAULT_DISTANCE),
        ("active_b", "b", DEFAULT_DISTANCE),
        ("distance_a", "a", VARIANT_DISTANCE),
    )
    outputs = {}
    for label, source_label, distance in cases:
        assignments, requested_values = _assignments(distance)
        request = tmp_path / f"{label}.request.json"
        output = tmp_path / f"{label}.png"
        request.write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "timing": {
                        "frame": 0,
                        "fps": 30,
                        "duration_frames": 300,
                    },
                    "assignments": assignments,
                }
            ),
            encoding="utf-8",
        )
        report = run(
            f"{label}.report",
            "--render-experimental-smart-request",
            plugin,
            sources[source_label],
            output,
            request,
            timeout=90,
        )
        _assert_report(report, source_pixels_by_label[source_label], output)

        receipts = report["requested_parameters"]
        assert receipts == _expected_receipts(requested_values)
        assert len({receipt["slot"] for receipt in receipts}) == len(receipts)
        assert len({receipt["id"] for receipt in receipts}) == len(receipts)
        assert 1 not in {receipt["slot"] for receipt in receipts}

        assert output.is_file() and output.stat().st_size > 0
        with Image.open(output) as image:
            assert image.format == "PNG"
            assert image.mode == "RGBA"
            image.load()
            assert image.size == (report["width"], report["height"])
            pixels = image.tobytes()
        assert hashlib.sha256(argb(pixels)).hexdigest() == (
            report["output_sha256"]
        )
        outputs[label] = _frame(
            pixels,
            report["width"],
            report["height"],
            report["output_origin"],
        )

    postflight_identity = {
        "schema_version": 1,
        "plugin": {
            "name": plugin_path.name,
            "size_bytes": plugin_path.stat().st_size,
            "path_sha256": hashlib.sha256(
                str(plugin_path).lower().encode("utf-8")
            ).hexdigest(),
            "sha256": _sha256_file(plugin_path),
        },
        "build": {
            "harness_sha256": _sha256_file(harness),
            "harness_size_bytes": harness.stat().st_size,
            "worker_sha256": _sha256_file(worker),
            "worker_size_bytes": worker.stat().st_size,
        },
    }
    (tmp_path / "postflight-evidence.json").write_text(
        json.dumps(postflight_identity, indent=2, sort_keys=True),
        encoding="utf-8",
    )
    assert postflight_identity == identity

    metrics = _response_analysis(
        outputs["active_a"],
        outputs["repeat_a"],
        outputs["active_b"],
        outputs["distance_a"],
    )
    (tmp_path / "metrics.json").write_text(
        json.dumps(metrics, indent=2, sort_keys=True), encoding="utf-8"
    )
    assert_drop_shadow_response(
        outputs["active_a"],
        outputs["repeat_a"],
        outputs["active_b"],
        outputs["distance_a"],
        measured=metrics,
    )

from pathlib import Path

import pytest

from _render_session import HARNESS, assert_artifact_fresh, run_session_render


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments" / "pf-smart-geometry-probe" / "pf_smart_geometry_probe.cpp"
WORKER = ROOT / "target" / "minihost-build" / "aex_worker.exe"
PROBE = ROOT / "target" / "pf-smart-geometry-probe-build" / "Release" / "pf_smart_geometry_probe.aex"
WIDTH, HEIGHT = 16, 12
DEPTH_FORMATS = ("argb8", "argb16", "argb32f")

# The probe drives one geometry scenario per render time (current_time % 5),
# so every scenario is reached through the generic time argument with no
# probe-specific host branch.
MODES = {
    0: {  # verify-intersection: the probe itself asserts the checkout answers
        "result_rect": [0, 0, WIDTH, HEIGHT],
        "max_result_rect": [0, 0, WIDTH, HEIGHT],
        "returns_extra_pixels": False,
        "result_within_request": True,
        "extra_pixels_contract_violation": False,
        "empty_result_rect": False,
        "empty_result_passthrough": False,
        "smart_render_selector_dispatched": True,
        "width": WIDTH,
        "height": HEIGHT,
    },
    1: {  # RETURNS_EXTRA_PIXELS admits result > request
        "result_rect": [-2, -2, WIDTH + 2, HEIGHT + 2],
        "max_result_rect": [-2, -2, WIDTH + 2, HEIGHT + 2],
        "returns_extra_pixels": True,
        "result_within_request": False,
        "extra_pixels_contract_violation": False,
        "empty_result_rect": False,
        "empty_result_passthrough": False,
        "smart_render_selector_dispatched": True,
        "width": WIDTH + 4,
        "height": HEIGHT + 4,
    },
    2: {  # the same overrun without the flag is an explicit violation
        "result_rect": [-2, -2, WIDTH + 2, HEIGHT + 2],
        "max_result_rect": [-2, -2, WIDTH + 2, HEIGHT + 2],
        "returns_extra_pixels": False,
        "result_within_request": False,
        "extra_pixels_contract_violation": True,
        "empty_result_rect": False,
        "empty_result_passthrough": False,
        "smart_render_selector_dispatched": True,
        "width": WIDTH + 4,
        "height": HEIGHT + 4,
    },
    3: {  # a legally empty result skips the selector; the input passes through
        # The selector still never runs -- the effect promised no pixels and is
        # not asked for any. What changed in issue #1285 is what the host then
        # emits: AE 26.3x87 answers an empty SmartFX result with the effect's
        # input, unchanged (captured from Grow_Bounds.aex, which has no
        # SMART_PRE_RENDER/SMART_RENDER case at all, and from Set_Channels.aex,
        # which sets the empty rect explicitly). `empty_result_passthrough`
        # keeps the two apart, so a frame the host copied can never be read as
        # one the effect rendered.
        "result_rect": [0, 0, 0, 0],
        "max_result_rect": [0, 0, WIDTH, HEIGHT],
        "returns_extra_pixels": False,
        "result_within_request": True,
        "extra_pixels_contract_violation": False,
        "empty_result_rect": True,
        "empty_result_passthrough": True,
        "smart_render_selector_dispatched": False,
        "width": WIDTH,
        "height": HEIGHT,
    },
    4: {  # a large availability envelope does not size the output allocation
        "result_rect": [0, 0, WIDTH, HEIGHT],
        "max_result_rect": [-5000, -5000, 5000, 5000],
        "returns_extra_pixels": False,
        "result_within_request": True,
        "extra_pixels_contract_violation": False,
        "empty_result_rect": False,
        "empty_result_passthrough": False,
        "smart_render_selector_dispatched": True,
        "width": WIDTH,
        "height": HEIGHT,
    },
}


def _run(tmp_path, pixel_format, mode):
    assert WORKER.exists(), "build aex_worker.exe before running this test"
    assert PROBE.exists(), (
        "build the probe first: tools/build-pf-smart-geometry-probe.ps1"
    )
    assert_artifact_fresh(PROBE, SOURCE, WORKER, HARNESS)
    input_path = tmp_path / f"input-{pixel_format}-{mode}.rgba"
    input_path.write_bytes(bytes(index % 251 for index in range(WIDTH * HEIGHT * 4)))
    output_path = tmp_path / f"output-{pixel_format}-{mode}.bin"
    return run_session_render(
        tmp_path, PROBE, input_path, output_path, width=WIDTH, height=HEIGHT,
        pixel_format=pixel_format, smart=True, current_time=mode,
        total_time=5, time_scale=1,
    )


@pytest.mark.parametrize("mode", sorted(MODES))
def test_probe_modes_pin_the_geometry_contract(tmp_path, mode):
    report = _run(tmp_path, "argb8", mode)
    assert report["status"] == "render_completed"
    # Mode 0 verifies the checkout intersection answers inside the probe's own
    # Smart Pre-Render; a zero pre_render_error means every check passed.
    assert report["pre_render_error"] == 0
    assert report["smart_render_error"] == 0
    assert report["result_rects_valid"] is True
    for field, expected in MODES[mode].items():
        assert report[field] == expected, f"mode {mode} field {field}"
    # The final full-frame checkout is the one that backs the render.
    assert report["input_checkout_result_rect"] == [0, 0, WIDTH, HEIGHT]
    assert report["malformed_checkout_request_count"] == 0
    assert report["empty_checkout_pixel_denial_count"] == 0


def test_empty_result_emits_the_input_unchanged(tmp_path):
    """The empty-result passthrough copies pixels, it does not invent them."""
    input_path = tmp_path / "passthrough-input.rgba"
    output_path = tmp_path / "passthrough-output.bin"
    input_path.write_bytes(bytes(index % 251 for index in range(WIDTH * HEIGHT * 4)))
    assert WORKER.exists(), "build aex_worker.exe before running this test"
    assert PROBE.exists(), (
        "build the probe first: tools/build-pf-smart-geometry-probe.ps1"
    )
    assert_artifact_fresh(PROBE, SOURCE, WORKER, HARNESS)
    report = run_session_render(
        tmp_path, PROBE, input_path, output_path, width=WIDTH, height=HEIGHT,
        pixel_format="argb8", smart=True, current_time=3, total_time=5,
        time_scale=1,
    )
    assert report["empty_result_rect"] is True
    assert report["empty_result_passthrough"] is True
    assert report["smart_render_selector_dispatched"] is False
    # Byte equality, not "nonempty": a passthrough that altered a pixel would
    # be a silently wrong frame, which is worse than the empty one it replaced.
    assert output_path.read_bytes() == input_path.read_bytes()


@pytest.mark.parametrize("mode", sorted(MODES))
def test_geometry_contract_is_identical_across_depths(tmp_path, mode):
    geometry_fields = tuple(MODES[mode]) + (
        "pre_render_error", "smart_render_error", "result_rects_valid",
        "input_checkout_result_rect",
    )
    reports = [_run(tmp_path, pixel_format, mode) for pixel_format in DEPTH_FORMATS]
    assert [report["pixel_format"] for report in reports] == [
        "argb8", "argb16", "argb32f"
    ]
    baseline = {field: reports[0][field] for field in geometry_fields}
    for report in reports[1:]:
        assert {field: report[field] for field in geometry_fields} == baseline

import hashlib
import json
import subprocess
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
WORKER = ROOT / "target" / "minihost-build" / "aex_smart_worker.exe"
PROBE = ROOT / "target" / "pf-smart-geometry-probe-build" / "Release" / "pf_smart_geometry_probe.aex"
WIDTH, HEIGHT = 16, 12
DEPTH_COMMANDS = ("--smart-image", "--smart-image16", "--smart-image32")

# The probe drives one geometry scenario per render time (current_time % 4),
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
        "smart_render_selector_dispatched": True,
        "width": WIDTH + 4,
        "height": HEIGHT + 4,
    },
    3: {  # a legally empty result skips the selector and renders nothing
        "result_rect": [0, 0, 0, 0],
        "max_result_rect": [0, 0, WIDTH, HEIGHT],
        "returns_extra_pixels": False,
        "result_within_request": True,
        "extra_pixels_contract_violation": False,
        "empty_result_rect": True,
        "smart_render_selector_dispatched": False,
        "width": 0,
        "height": 0,
    },
}


def _run(tmp_path, command, mode):
    assert WORKER.exists(), "build aex_smart_worker.exe before running this test"
    assert PROBE.exists(), (
        "build the probe first: tools/build-pf-smart-geometry-probe.ps1"
    )
    probe_hash = hashlib.sha256(PROBE.read_bytes()).hexdigest()
    input_path = tmp_path / f"input-{command.strip('-')}-{mode}.rgba"
    input_path.write_bytes(bytes(index % 251 for index in range(WIDTH * HEIGHT * 4)))
    output_path = tmp_path / f"output-{command.strip('-')}-{mode}.bin"
    completed = subprocess.run(
        [str(WORKER), command, str(PROBE), probe_hash, "v2|", str(input_path),
         str(output_path), str(WIDTH), str(HEIGHT), str(mode), "1", "4", "1"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=60,
    )
    assert completed.returncode == 0, completed.stderr
    return json.loads(completed.stdout)


@pytest.mark.parametrize("mode", sorted(MODES))
def test_probe_modes_pin_the_geometry_contract(tmp_path, mode):
    report = _run(tmp_path, "--smart-image", mode)
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


@pytest.mark.parametrize("mode", sorted(MODES))
def test_geometry_contract_is_identical_across_depths(tmp_path, mode):
    geometry_fields = tuple(MODES[mode]) + (
        "pre_render_error", "smart_render_error", "result_rects_valid",
        "input_checkout_result_rect",
    )
    reports = [_run(tmp_path, command, mode) for command in DEPTH_COMMANDS]
    assert [report["pixel_format"] for report in reports] == [
        "argb8", "argb16", "argb32f"
    ]
    baseline = {field: reports[0][field] for field in geometry_fields}
    for report in reports[1:]:
        assert {field: report[field] for field in geometry_fields} == baseline

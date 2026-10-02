from pathlib import Path
import json
import shutil
import subprocess

import pytest
from PIL import Image

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


def test_shipping_json_can_render_expanded_request_without_extra_pixels(tmp_path):
    assert_artifact_fresh(PROBE, SOURCE, WORKER, HARNESS)
    plugin = tmp_path / "pf_smart_geometry_probe-requestexpand.aex"
    shutil.copy2(PROBE, plugin)
    source = tmp_path / "input.png"
    Image.new("RGBA", (64, 48), (13, 29, 47, 255)).save(source)
    # Default demand still crops to input; explicit demand includes negative
    # origin and also supports a smaller tile within the expanded availability.
    for command in ("--render-experimental-smart-request",
                    "--render-experimental-smart-request-16",
                    "--render-experimental-smart-request-32-cpu"):
        for index, rect in enumerate((None, [-32, -24, 96, 72], [-8, -6, 80, 60])):
            context = {"mask_scene": {"masks": []}}
            if rect is not None:
                context["smart_output_request_rect"] = rect
            request = tmp_path / "request.json"
            request.write_text(json.dumps({"schema_version": 1, "assignments": [], "host_context": context}),
                               encoding="utf-8")
            output = tmp_path / f"{command}-{index}.png"
            completed = subprocess.run(
                [str(HARNESS), command, str(plugin), str(source), str(output), str(request)],
                cwd=ROOT, capture_output=True, text=True, encoding="utf-8",
                errors="replace", timeout=60)
            assert completed.returncode == 0, completed.stdout + completed.stderr
            report = json.loads(completed.stdout)
            expected = rect or [0, 0, 64, 48]
            assert report["passed"] is True, report
            assert report["pre_render_error"] == report["smart_render_error"] == 0
            assert report["result_rect"] == expected
            assert report["max_result_rect"] == [-32, -24, 96, 72]
            # PF_InData reports layer origin inside the buffer (opposite of
            # PF_EffectWorld origin, checked by the compiled probe itself).
            assert report["output_origin"] == [-expected[0], -expected[1]]
            assert report["returns_extra_pixels"] is False
            assert report["result_within_request"] is True
            with Image.open(output) as image:
                assert image.size == (expected[2] - expected[0], expected[3] - expected[1])
                pixels = image.convert("RGBA")
                assert pixels.getpixel((0, 0)) == (255, 0, 255, 255)
                assert pixels.getpixel((image.width - 1, image.height - 1)) == (255, 0, 255, 255)


def test_shipping_json_rejects_invalid_or_classic_output_requests(tmp_path):
    assert_artifact_fresh(PROBE, SOURCE, WORKER, HARNESS)
    source = tmp_path / "input.png"
    Image.new("RGBA", (64, 48), (13, 29, 47, 255)).save(source)
    for index, (command, rect) in enumerate((
        ("--render-experimental-request", [-32, -24, 96, 72]),
        ("--render-experimental-smart-request", [0, 0, 4097, 1]),
        ("--render-experimental-smart-request", [-16777217, 0, -16777216, 1]),
        ("--render-experimental-smart-request", [0, 0, 0, 1]),
    )):
        request = tmp_path / "request.json"
        request.write_text(json.dumps({"schema_version": 1, "assignments": [], "host_context": {
            "mask_scene": {"masks": []}, "smart_output_request_rect": rect}}), encoding="utf-8")
        output = tmp_path / f"rejected-{index}.png"
        completed = subprocess.run(
            [str(HARNESS), command, str(PROBE), str(source), str(output), str(request)],
            cwd=ROOT, capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=60)
        assert completed.returncode != 0
        assert not output.exists()
        assert "output request" in completed.stdout + completed.stderr


def test_worker_rejects_malformed_or_ambiguous_output_demand_before_load():
    assert WORKER.is_file()
    invalid = (
        "render:v2|1,0,0,0,0,0,4097,1",
        "render:v2|1,0,0,0,-16777217,0,-16777216,1",
        "render:v2|1,0,0,0,16777216,0,16777217,1",
        "render:v2|1,0,0,0,0,0,0,1",
        "render:v2|1,0,0,0,0,0,1,0",
        "render:v2|1,0,0,0,1,0,0,1",
        "render:v2|2,0,0,0,-32,-24,96,72",
        "render:v2|1,0,0,0,-32,-24,96",
        "render:v2|1,0,0,0,-32,-24,96,72,1",
    )
    valid = "render:v2|1,0,0,0,-32,-24,96,72"
    cases = [("smart", "--smart-session-v1", trailer, []) for trailer in invalid]
    cases += [("classic", "--render-session-v1", valid, []),
              ("smart", "--smart-session-v1", valid,
               ["--render-diagnostic-layout-v1", "v1|0,0,0,0,0,0,64,48,-1,-1,-1,-1"])]
    for kind, command, trailer, options in cases:
        completed = subprocess.run(
            [str(WORKER), "--kind", kind, command, "absent.aex", "0" * 64,
             "v2|", "64", "48", "1", "1", "1", trailer, *options],
            cwd=ROOT, capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=10)
        # Malformed request exit, not unsupported-command or load/transport failure.
        assert completed.returncode == 3, (trailer, completed.stdout, completed.stderr)
        assert "stage:" not in completed.stderr

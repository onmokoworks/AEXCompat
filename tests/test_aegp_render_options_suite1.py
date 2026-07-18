import json
import os
import pathlib
import subprocess

import pytest


ROOT = pathlib.Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"
ABI = ROOT / "minihost" / "src" / "worker_suite_abi.hpp"
REGISTRY = ROOT / "minihost" / "src" / "worker_aegp_render_options.cpp"


def _worker() -> pathlib.Path | None:
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        pathlib.Path(configured) if configured else None,
        ROOT / "target" / "minihost-build-v18" / "Release" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build-v18" / "aex_render_worker.exe",
    ]
    return next((candidate for candidate in candidates if candidate and candidate.is_file()), None)


def test_render_options_suite1_has_exact_typed_17_slot_abi():
    text = ABI.read_text(encoding="utf-8")
    assert "struct AegpRenderOptionsSuite1" in text
    assert "sizeof(AegpRenderOptionsSuite1) == 17 * sizeof(void*)" in text
    expected_offsets = {
        "new_from_item": 0,
        "duplicate": 1,
        "dispose": 2,
        "set_time": 3,
        "get_time": 4,
        "set_time_step": 5,
        "get_time_step": 6,
        "set_field": 7,
        "get_field": 8,
        "set_world_type": 9,
        "get_world_type": 10,
        "set_downsample": 11,
        "get_downsample": 12,
        "set_roi": 13,
        "get_roi": 14,
        "set_matte": 15,
        "get_matte": 16,
    }
    for member, slot in expected_offsets.items():
        assert f"AEXCOMPAT_ASSERT_RENDER1_SLOT({member}, {slot})" in text
    assert "std::array<void*, 17> g_render_options_suite1" not in text
    assert "g_render_options_suite1.fill" not in text


def test_registry_is_bounded_aba_resistant_and_receipts_snapshot_options():
    text = SOURCE.read_text(encoding="utf-8")
    registry = REGISTRY.read_text(encoding="utf-8")
    for marker in (
        "receipt->render_options = *options",
        "snapshot.matte == 2",
        "kSyntheticCompWidth + options->downsample_x - 1",
        "kSyntheticCompHeight + options->downsample_y - 1",
        "synthetic_item_pixel",
        "x * options.downsample_x",
        "source_x >= source_roi.left",
        "converted[channel] = static_cast<uint16_t>(pixel[channel]) * 257u",
        "converted[channel] = static_cast<float>(pixel[channel]) / 255.0f",
    ):
        assert marker in text
    for marker in (
        "kMaxItemOptions = 32",
        "std::atomic<uint64_t> g_item_generation{1}",
        "next_handle(g_item_generation, 1, 1)",
        "std::unordered_map<uintptr_t, ItemValue> g_items",
        "g_items.size() >= kMaxItemOptions",
    ):
        assert marker in registry


def test_item_async_and_render_suite_slot_zero_publish_ready_receipts():
    text = SOURCE.read_text(encoding="utf-8")
    assert "return publish_item_receipt(options, receipt);" in text
    assert "return publish_item_receipt(options, out);" in text
    assert "&render_checkout_frame_reject, &render_checkout_layer_reject" in text
    assert "&checkin_frame, &get_receipt_world" in text


def test_render_options_runtime_matrix(tmp_path):
    worker = _worker()
    assert worker is not None, "build aex_render_worker before running the focused runtime test"
    temp = ROOT / "target" / "tmp"
    temp.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env["TEMP"] = env["TMP"] = str(temp)
    result = subprocess.run(
        [str(worker), "--self-test-aegp-render-options-suite1"],
        cwd=ROOT,
        env=env,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stderr or result.stdout
    report = json.loads(result.stdout)
    assert report["aegp_render_options_suite1"] == "passed"
    assert report["created"] == report["disposed"] == 38
    assert report["live"] == 0
    assert report["receipts_created"] == report["receipts_checked_in"] == 7
    assert report["invalid_operations"] >= 9
    assert report["baseline_argb8"] == [123, 59, 177, 157]
    assert report["time_argb8"] == [72, 8, 24, 56]
    assert report["downsample_argb8"] == [170, 110, 235, 198]
    assert report["roi_outside_argb8"] == [0, 0, 0, 0]
    assert report["roi_inside_argb8"] == [134, 76, 177, 162]
    assert report["field_excluded_argb8"] == [0, 0, 0, 0]
    assert report["matte_argb8"] == [255, 59, 177, 157]
    assert report["argb16"] == [31611, 15163, 45489, 40349]
    assert report["argb32f"] == pytest.approx(
        [123 / 255, 59 / 255, 177 / 255, 157 / 255], abs=1e-7
    )

"""Shipping CLI regression for native-depth metamorphic renders (#1593)."""

import hashlib
import json
import os
import shutil
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from _render_session import assert_artifact_fresh


ROOT = Path(__file__).resolve().parents[1]
BUILD_TARGET = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "broker" / "target"))
PROFILE = os.environ.get("AEXCOMPAT_CARGO_PROFILE", "release")
HARNESS = BUILD_TARGET / PROFILE / "aexcompat-harness.exe"
WORKER = ROOT / "target" / "minihost-build" / "aex_worker.exe"
SOURCE = ROOT / "instruments" / "pf-smart-geometry-probe" / "pf_smart_geometry_probe.cpp"
PROBES = (
    ROOT / "target" / "pf-smart-geometry-probe-build" / "Release" / "pf_smart_geometry_probe.aex",
    ROOT / "target" / "pf-smart-geometry-probe-build" / "pf_smart_geometry_probe.aex",
)
CLASSIC_ORIGIN_PROBE = (
    ROOT / "target" / "pf-frame-origin-probe-build" / "Release"
    / "pf_frame_origin_probe.aex"
)


def run_differential(tmp_path: Path, marker: str, depth: str = "argb8", route: str = "smart"):
    assert HARNESS.is_file(), "build the selected harness profile before this built-artifact test"
    assert WORKER.is_file(), "build the Release worker before this built-artifact test"
    available = [probe for probe in PROBES if probe.is_file()]
    assert available, "build pf_smart_geometry_probe.aex before this built-artifact test"
    probe = max(available, key=lambda path: path.stat().st_mtime_ns)
    assert_artifact_fresh(probe, SOURCE, WORKER)
    plugin = tmp_path / f"pf_smart_geometry_probe{marker}.aex"
    shutil.copyfile(probe, plugin)
    input_path = tmp_path / "input.png"
    input_bytes = bytes(index % 251 for index in range(17 * 13 * 4))
    Image.frombytes("RGBA", (17, 13), input_bytes).save(input_path)
    result = subprocess.run(
        [str(HARNESS), "--headless", "--render-differential", str(plugin),
         str(input_path), depth, route, "0", "5", "1"],
        cwd=ROOT, capture_output=True, text=True, encoding="utf-8",
        errors="replace", timeout=60,
    )
    assert result.stdout, result.stderr
    report = json.loads(result.stdout)
    assert report["schema_version"] == 1
    assert report["stage"] == "render_differential"
    assert report["conditions"]["width"] == 17
    assert report["conditions"]["height"] == 13
    assert report["conditions"]["pixel_format"] == depth
    for key in ("plugin_sha256", "plugin_path_sha256", "host_executable_sha256",
                "worker_executable_sha256", "input_rgba_sha256", "parameters_sha256"):
        assert len(report["provenance"][key]) == 64
    assert len(report["cases"]) == (6 if route == "smart" else 7)
    for case in report["cases"]:
        for receipt in case.get("session_receipts", []):
            assert receipt["worker_admitted_plugin_sha256"].lower() == (
                report["provenance"]["plugin_sha256"].lower()
            )
            assert len(receipt["final_report_sha256"]) == 64
            assert receipt["worker_classification"] == "ok"
            assert isinstance(receipt["module_audit"], dict)
            assert receipt["admitted_matches_requested"] is True
            assert receipt["plugin_file_stable"] is True
            assert receipt["worker_file_stable"] is True
            assert receipt["host_file_stable"] is True
    return result.returncode, report


@pytest.mark.parametrize("depth", ("argb8", "argb16", "argb32f"))
def test_odd_frame_matches_both_tile_axes_at_native_depth(tmp_path, depth):
    code, report = run_differential(tmp_path, "-difftile", depth)
    assert code == 0, report
    assert report["passed"] is True
    assert [case["status"] for case in report["cases"]] == [
        "rendered", "matched", "matched", "matched", "matched", "matched"
    ]
    assert all(case["comparison"]["differing_pixels"] == 0
               for case in report["cases"][1:]
               if case["status"] == "matched")
    if depth == "argb8":
        host_hash = hashlib.sha256(HARNESS.read_bytes()).hexdigest()
        worker_hash = hashlib.sha256(WORKER.read_bytes()).hexdigest()
        assert report["provenance"]["host_executable_sha256"] == host_hash
        assert report["provenance"]["worker_executable_sha256"] == worker_hash
        for case in report["cases"]:
            for receipt in case["session_receipts"]:
                assert receipt["host_file_sha256_before"] == host_hash
                assert receipt["host_file_sha256_after"] == host_hash
                assert receipt["worker_file_sha256_before"] == worker_hash
                assert receipt["worker_file_sha256_after"] == worker_hash


@pytest.mark.parametrize(
    ("route", "reason"),
    (("smart", "output_geometry_did_not_honor_request"),
     ("classic", "classic_has_no_request_rect")),
)
def test_unsupported_request_rect_is_not_counted_as_match(tmp_path, route, reason):
    code, report = run_differential(tmp_path, "", route=route)
    assert code != 0
    assert report["passed"] is False
    for case in report["cases"][-2:]:
        assert case["status"] == "unsupported"
        assert case["reason"] == reason
    if route == "classic":
        geometry = next(case for case in report["cases"]
                        if case["id"] == "classic_output_geometry")
        assert geometry["status"] == "unsupported"
        assert geometry["reason"] == "plugin_did_not_resize_or_shift_output"


def test_classic_resized_nonzero_origin_compares_at_native_depth(tmp_path):
    assert HARNESS.is_file()
    assert WORKER.is_file()
    assert CLASSIC_ORIGIN_PROBE.is_file(), "build the frame-origin probe first"
    assert_artifact_fresh(CLASSIC_ORIGIN_PROBE,
                          ROOT / "instruments" / "pf-frame-origin-probe"
                          / "pf_frame_origin_probe.cpp", WORKER)
    input_path = tmp_path / "input.png"
    Image.new("RGBA", (17, 13), (35, 70, 105, 255)).save(input_path)
    result = subprocess.run(
        [str(HARNESS), "--headless", "--render-differential",
         str(CLASSIC_ORIGIN_PROBE), str(input_path), "argb8", "classic",
         "0", "5", "1"],
        cwd=ROOT, capture_output=True, text=True, encoding="utf-8",
        errors="replace", timeout=60,
    )
    assert result.stdout, result.stderr
    report = json.loads(result.stdout)
    geometry = next(case for case in report["cases"]
                    if case["id"] == "classic_output_geometry")
    assert geometry["status"] == "matched", geometry
    output_rect = geometry["region"][0]
    assert output_rect[0] != 0
    assert output_rect[2] - output_rect[0] > 17
    assert geometry["comparison"]["differing_pixels"] == 0
    assert result.returncode != 0  # Classic has no request-rect tiling.


def test_extent_hint_may_leave_pixels_outside_promised_rect_unwritten(tmp_path):
    code, report = run_differential(tmp_path, "-difftile-hintonly")
    assert code == 0, report
    extent = next(case for case in report["cases"] if case["id"] == "extent_hint")
    assert extent["status"] == "matched", extent
    assert extent["comparison"]["comparison_rect"] == [1, 1, 16, 12]
    assert extent["comparison"]["compared_pixels"] == 15 * 11


@pytest.mark.parametrize(
    ("marker", "case_id", "expected_status"),
    (("-difftile-originbug", "horizontal_tiles", "different"),
     ("-difftile-stridebug", "padded_stride", "failed"),
     ("-difftile-extentbug", "extent_hint", "different")),
)
def test_mutated_world_contract_cannot_pass(tmp_path, marker, case_id, expected_status):
    code, report = run_differential(tmp_path, marker)
    assert code != 0
    assert report["passed"] is False
    case = next(case for case in report["cases"] if case["id"] == case_id)
    assert case["status"] == expected_status, case
    if expected_status == "different":
        assert case["comparison"]["differing_pixels"] > 0
        assert case["comparison"]["difference_bbox"] is not None
    else:
        assert case["failure"]["stage"] == "frame"

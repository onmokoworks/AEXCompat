"""Built AEX regression: selector success must not certify partial output."""

import json
import shutil
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from _render_session import HARNESS, assert_artifact_fresh


ROOT = Path(__file__).resolve().parents[1]
WORKER = ROOT / "target" / "minihost-build" / "aex_worker.exe"
SOURCE = ROOT / "instruments" / "pf-smart-geometry-probe" / "pf_smart_geometry_probe.cpp"
PROBE_BUILD = ROOT / "target" / "pf-smart-geometry-probe-build"
PROBE_CANDIDATES = (
    PROBE_BUILD / "Release" / "pf_smart_geometry_probe.aex",
    PROBE_BUILD / "pf_smart_geometry_probe.aex",  # single-config Ninja
)


@pytest.mark.parametrize("route", ("classic", "smart"))
@pytest.mark.parametrize("depth", ("argb8", "argb16", "argb32f"))
@pytest.mark.parametrize("partial", (False, True))
def test_promised_output_coverage(tmp_path: Path, route: str, depth: str, partial: bool) -> None:
    available = [candidate for candidate in PROBE_CANDIDATES if candidate.is_file()]
    assert available, "build the smart geometry probe before running this test"
    probe = max(available, key=lambda candidate: candidate.stat().st_mtime_ns)
    assert_artifact_fresh(probe, SOURCE, WORKER, HARNESS)
    suffix = "-partial" if partial else "-written"
    plugin = tmp_path / f"geometry-{route}{suffix}.aex"
    shutil.copyfile(probe, plugin)
    image = tmp_path / "input.png"
    output = tmp_path / "output.png"
    Image.new("RGBA", (16, 12), (17, 31, 47, 255)).save(image)
    completed = subprocess.run(
        [str(HARNESS), "--render-experimental-session", str(plugin),
         str(image), str(output), depth, route, "0", "1", "1"],
        cwd=ROOT, capture_output=True, text=True, encoding="utf-8",
        errors="replace", timeout=60,
    )
    if partial:
        assert completed.returncode != 0, completed.stdout + completed.stderr
        marker = ", report="
        assert marker in completed.stderr, completed.stderr
        report, _ = json.JSONDecoder().raw_decode(completed.stderr.split(marker, 1)[1])
        coverage = report["output_coverage"]
        assert coverage["inspected"] is True
        assert coverage["promised_pixels"] == 16 * 12
        assert coverage["unwritten_pixels"] == 8 * 12
        assert coverage["unwritten_percent"] == 50
        assert coverage["bbox"] == [8, 0, 16, 12]
        assert coverage["max_row_run"] == 8
        assert coverage["max_column_run"] == 12
        assert coverage["failure_reason"] == "partial_unwritten_output"
        if route == "classic":
            assert "session frame reported error -6" in completed.stderr
        else:
            assert report["smart_render_error"] == -6
        assert not output.exists()
    else:
        assert completed.returncode == 0, completed.stdout + completed.stderr
        report = json.loads(completed.stdout)
        assert report["passed"] is True
        assert report["output_coverage"]["unwritten_pixels"] == 0
        assert output.is_file()


@pytest.mark.parametrize(
    ("route", "variant", "current_time"),
    (("classic", "classic-nop", "0"), ("smart", "smart-written", "3")),
)
def test_host_passthrough_does_not_claim_plugin_writes(
    tmp_path: Path, route: str, variant: str, current_time: str,
) -> None:
    available = [candidate for candidate in PROBE_CANDIDATES if candidate.is_file()]
    assert available, "build the smart geometry probe before running this test"
    probe = max(available, key=lambda candidate: candidate.stat().st_mtime_ns)
    assert_artifact_fresh(probe, SOURCE, WORKER, HARNESS)
    plugin = tmp_path / f"geometry-{variant}.aex"
    shutil.copyfile(probe, plugin)
    image = tmp_path / "input.png"
    output = tmp_path / "output.png"
    Image.new("RGBA", (16, 12), (17, 31, 47, 255)).save(image)
    completed = subprocess.run(
        [str(HARNESS), "--render-experimental-session", str(plugin),
         str(image), str(output), "argb8", route, current_time, "5", "1"],
        cwd=ROOT, capture_output=True, text=True, encoding="utf-8",
        errors="replace", timeout=60,
    )
    assert completed.returncode == 0, completed.stdout + completed.stderr
    report = json.loads(completed.stdout)
    assert report["passed"] is True
    assert report["output_coverage"]["inspected"] is False
    assert Image.open(output).convert("RGBA").tobytes() == Image.open(image).convert("RGBA").tobytes()


@pytest.mark.parametrize("variant,route,depth,time", (
    ("smart-crop", "smart", "argb8", "0"),
    ("smart-solidcc", "smart", "argb8", "0"),
    ("classic-solidcc", "classic", "argb16", "0"),
    ("smart-selectorerror", "smart", "argb8", "0"),
    ("classic-selectorerror", "classic", "argb8", "0"),
))
def test_coverage_attribution_and_legal_pixels(
    tmp_path: Path, variant: str, route: str, depth: str, time: str,
) -> None:
    available = [candidate for candidate in PROBE_CANDIDATES if candidate.is_file()]
    assert available, "build the smart geometry probe before running this test"
    probe = max(available, key=lambda candidate: candidate.stat().st_mtime_ns)
    assert_artifact_fresh(probe, SOURCE, WORKER, HARNESS)
    plugin = tmp_path / f"geometry-{variant}.aex"
    shutil.copyfile(probe, plugin)
    image = tmp_path / "input.png"
    output = tmp_path / "output.png"
    Image.new("RGBA", (16, 12), (17, 31, 47, 255)).save(image)
    completed = subprocess.run(
        [str(HARNESS), "--render-experimental-session", str(plugin),
         str(image), str(output), depth, route, time, "1", "1"],
        cwd=ROOT, capture_output=True, text=True, encoding="utf-8",
        errors="replace", timeout=60,
    )
    if "selectorerror" in variant:
        assert completed.returncode != 0, completed.stdout + completed.stderr
        assert ", report=" in completed.stderr
        report, _ = json.JSONDecoder().raw_decode(
            completed.stderr.split(", report=", 1)[1]
        )
        assert report["output_coverage"]["validation_failed"] is False
        assert report["output_coverage"]["failure_reason"] is None
        assert report.get("failure_stage") != "output_validation"
        assert not output.exists()
    else:
        assert completed.returncode == 0, completed.stdout + completed.stderr
        report = json.loads(completed.stdout)
        assert report["passed"] is True
        assert report["output_coverage"]["unwritten_pixels"] == 0
        assert report["output_coverage"]["validation_failed"] is False
        if "crop" in variant:
            assert report["result_rect"] == [2, 1, 14, 11]
            assert report["output_coverage"]["promised_pixels"] == 12 * 10
        assert output.is_file()

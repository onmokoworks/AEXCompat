"""Behavioral native performance-family probe; no absolute-time oracle."""

import json
import os
import subprocess
import hashlib
from pathlib import Path

from _render_session import assert_artifact_fresh


ROOT = Path(__file__).resolve().parents[1]
PROFILE = os.environ.get("AEXCOMPAT_CARGO_PROFILE", "release")
HARNESS = Path(os.environ.get(
    "AEXCOMPAT_PERF_HARNESS",
    ROOT / "broker" / "target" / PROFILE / "aexcompat-harness.exe"))
WORKER = Path(os.environ.get(
    "AEXCOMPAT_PERF_WORKER",
    ROOT / "target" / "minihost-build" / "aex_worker.exe"))
LAUNCHED_WORKER = ROOT / "target" / "minihost-build" / "aex_worker.exe"
AEX = Path(os.environ.get(
    "AEXCOMPAT_PERF_AEX",
    ROOT / "target" / "pf-performance-probe-build" / "Release"
    / "pf_performance_probe.aex"))


def _run_mode(mode):
    assert WORKER.is_file() and LAUNCHED_WORKER.is_file()
    assert hashlib.sha256(WORKER.read_bytes()).digest() == hashlib.sha256(
        LAUNCHED_WORKER.read_bytes()).digest()
    completed = subprocess.run(
        [str(HARNESS), "--headless", "--render-performance-diagnostics",
         str(AEX), "classic", "argb8", "1", str(mode)],
        cwd=ROOT, text=True, encoding="utf-8", errors="replace",
        capture_output=True, timeout=120,
    )
    assert completed.returncode == 0, completed.stderr + completed.stdout
    report = json.loads(completed.stdout)
    assert report["status"] == "available"
    assert report["advisory"] is True
    assert report["sample_count"] == 16
    assert len(report["samples"]) == 16
    assert report["frames_per_resolution"] == 4
    assert report["aggregation"] == "median"
    assert report["outlier_policy"] == "none_removed"
    assert report["timer_resolution"]["reason"] == "not_calibrated"
    assert len(report["resolutions"]) == 4
    for resolution in report["resolutions"]:
        pixels = resolution["width"] * resolution["height"]
        assert resolution["output_bytes"]["median_bytes"] == pixels * 4
        assert resolution["warm_ms_per_megapixel"] is not None
        assert resolution["warm_worker_setup"]["status"] == "available"
        assert resolution["warm_selector_dispatch"]["status"] == "available"
        assert resolution["warm_worker_finalize"]["status"] == "available"
        assert resolution["worker_live_commit_bytes"]["status"] == "available"
        assert resolution["worker_job_peak_commit_bytes"] is not None
    assert "path" not in report["identity"]
    admitted = report["identity"]["worker_admitted_plugin_sha256_per_session"]
    assert len(admitted) == 4
    assert set(admitted) == {report["identity"]["plugin_sha256_before_and_after"]}
    assert str(ROOT) not in completed.stdout
    assert str(AEX) not in completed.stdout
    return report


def test_native_performance_modes_separate_scaling_and_live_memory():
    assert_artifact_fresh(AEX, ROOT / "instruments" / "pf-performance-probe"
                          / "pf_performance_probe.cpp")
    assert HARNESS.is_file() and WORKER.is_file()
    linear, fixed, quadratic, temporary, leaking = map(_run_mode, range(5))
    for report in (linear, fixed, temporary, leaking):
        assert "superlinear_candidate" not in report["reasons"]
    assert "superlinear_candidate" in quadratic["reasons"]
    assert quadratic["high_end_normalized_dispatch_ratio"] >= 2.25
    for report in (linear, fixed, quadratic, temporary):
        assert "memory_growth_candidate" not in report["reasons"]
    assert "memory_growth_candidate" in leaking["reasons"]
    assert sum(resolution["worker_live_commit_slope_bytes_per_frame"] >= 131_072
               for resolution in leaking["resolutions"]) >= 2
    assert (linear["identity"]["plugin_sha256_before_and_after"]
            == leaking["identity"]["plugin_sha256_before_and_after"])
    assert (linear["identity"]["parameter_sha256"]
            != leaking["identity"]["parameter_sha256"])


def test_unsupported_render_path_is_unavailable_not_a_performance_success():
    assert HARNESS.is_file() and WORKER.is_file() and AEX.is_file()
    assert LAUNCHED_WORKER.is_file()
    assert hashlib.sha256(WORKER.read_bytes()).digest() == hashlib.sha256(
        LAUNCHED_WORKER.read_bytes()).digest()
    completed = subprocess.run(
        [str(HARNESS), "--headless", "--render-performance-diagnostics",
         str(AEX), "smart", "argb8", "1", "0"],
        cwd=ROOT, text=True, encoding="utf-8", errors="replace",
        capture_output=True, timeout=60,
    )
    assert completed.returncode == 2, completed.stderr + completed.stdout
    report = json.loads(completed.stdout)
    assert report["status"] == "unavailable"
    assert report["reason"] not in (None, "")
    assert report["sample_count"] == 0
    assert report["session_index"] == 0
    assert report["identity"]["worker_admitted_plugin_sha256_per_session"] == []
    assert str(AEX) not in completed.stdout

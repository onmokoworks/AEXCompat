"""Behavioral resident advisory states from bounded public native fixtures."""
import json
import subprocess

from test_performance_diagnostics import ROOT, HARNESS, WORKER, AEX
from _render_session import assert_artifact_fresh


def _memory(mode, profile="repeat"):
    assert_artifact_fresh(AEX, ROOT / "instruments/pf-performance-probe/pf_performance_probe.cpp")
    assert HARNESS.is_file() and WORKER.is_file()
    completed = subprocess.run(
        [str(HARNESS), "--headless", "--render-memory-diagnostics", str(AEX),
         "classic", "argb8", profile, "1", str(mode)], cwd=ROOT,
        capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=180)
    assert completed.returncode == 0, completed.stderr + completed.stdout
    report = json.loads(completed.stdout)
    assert report["status"] == "available"
    assert report["sample_count"] == 64
    assert report["session_count"] == 2
    assert len(report["output_identities"]) == 64
    assert str(AEX) not in completed.stdout and str(ROOT) not in completed.stdout
    assert set(report["identity"]["worker_admitted_plugin_sha256_per_session"]) == {
        report["identity"]["plugin_sha256_before_and_after"]}
    for session in report["sessions"]:
        assert session["cleanup_verified"]
        memory = session["memory_advisory"]
        assert memory["advisory"]
        assert memory["sample_count"] == memory["window_sample_count"] == 32
        assert memory["process_limit_bytes"] == 2 * 1024**3
        assert memory["live_limit_ratio"] == memory["live_commit_bytes"] / memory["process_limit_bytes"]
        assert memory["observation_phase"] == "after_validated_frame_reply"
        assert memory["samples"][0]["current_time"] == 0
        assert memory["samples"][-1]["current_time"] == (0 if profile == "repeat" else 15)
        live = [s["live_commit_bytes"] for s in memory["samples"]]
        tail = live[-8:]
        baseline = sorted(live[4:8])[1]
        assert memory["baseline_after_warmup_bytes"] == baseline
        assert memory["live_slope_bytes_per_frame"] == (tail[-1] - tail[0]) / 7
        assert memory["tail_live_range_bytes"] == max(tail) - min(tail)
        assert memory["retained_commit_delta_bytes"] == sorted(tail)[3] - baseline
        outputs = [o for o in report["output_identities"] if o["session_index"] == session["session_index"]]
        assert len({o["output_sha256"] for o in outputs}) == 1
        assert all(o["output_bytes"] == session["width"] * session["height"] * 4 for o in outputs)
        assert all(o["changed_rgba_bytes"] is not None and o["changed_rgba_bytes"] > 0 for o in outputs)
        assert len({o["changed_rgba_bytes"] for o in outputs}) == 1
    return report


def test_resident_memory_advisories_separate_retention_from_recovered_peak():
    retained = _memory(5)
    recovered = _memory(6)
    for session in retained["sessions"]:
        memory = session["memory_advisory"]
        # Real allocator/world-cache fluctuations need not fit the plateau
        # tolerance. Assert the measured phase, not an OS timing oracle.
        assert "sustained_live_growth_candidate" not in memory["warnings"], memory
        if memory["trend"] == "stable_plateau":
            assert memory["warnings"] == ["post_frame_retention_candidate"], memory
        assert memory["retained_commit_delta_bytes"] >= 1024**2
    for session in recovered["sessions"]:
        memory = session["memory_advisory"]
        assert "sustained_live_growth_candidate" not in memory["warnings"], memory
        assert "temporary_peak_or_recovered_commit" in memory["observations"]


def test_resident_time_cache_growth_then_plateau_is_not_continuing_growth():
    report = _memory(0, "advance-then-repeat")
    for session in report["sessions"]:
        memory = session["memory_advisory"]
        assert "sustained_live_growth_candidate" not in memory["warnings"]

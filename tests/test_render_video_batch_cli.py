"""Broker batch video render CLI (issue #98 stage 1 PR-C).

Source-level wiring assertions always run. The runtime test drives
``broker.exe render-video-batch`` end to end against the session-capable
``aex_render_worker.exe`` (PR-B) and the pf_sampling_probe fixture, with
self-computed expectations only, so a fresh build on any machine satisfies
it.
"""
import json
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
BROKER_SOURCE = ROOT / "broker" / "crates" / "broker" / "src" / "render_session.rs"
BROKER_MAIN = ROOT / "broker" / "crates" / "broker" / "src" / "main.rs"
BROKER = ROOT / "broker" / "target" / "release" / "broker.exe"
WORKER = ROOT / "target" / "minihost-build" / "aex_render_worker.exe"
AEX = ROOT / "target" / "pf-sampling-probe-build" / "Release" / "pf_sampling_probe.aex"
GENERATOR = ROOT / "tools" / "generate-oracle-rgba-input.py"


def test_render_session_broker_wiring_is_fail_closed():
    source = BROKER_SOURCE.read_text(encoding="utf-8")
    # Transport handles are inherited and advertised by number; the worker
    # never receives a session path (issue #18 lesson).
    assert "SessionChildHandles" in source
    # Per-frame safety boundaries from the protocol: watchdog, generation,
    # static header, checksum, guard verification, all session-invalidating.
    for marker in (
        '"frame_deadline"',
        '"worker_exited"',
        '"frame_invariant_failure"',
        '"output_checksum_mismatch"',
        "terminate_job",
        "guards_intact",
    ):
        assert marker in source, marker
    # Frame-local errors continue the session; the batch aborts by default.
    assert "FrameError" in source
    assert "continue_on_frame_error" in source
    main = BROKER_MAIN.read_text(encoding="utf-8")
    assert '"render-video-batch"' in main


def _require_artifacts():
    for artifact in (BROKER, WORKER, AEX):
        if not artifact.is_file():
            pytest.skip(f"{artifact.name} is not built")


def test_video_batch_renders_a_sequence_through_one_resident_worker(tmp_path: Path) -> None:
    _require_artifacts()
    frames = []
    first = tmp_path / "input-0.png"
    subprocess.run(
        [sys.executable, str(GENERATOR), "--width", "64", "--height", "32",
         "--out", str(first)],
        check=True,
        capture_output=True,
        cwd=ROOT,
        timeout=60,
    )
    frames.append(str(first))
    for index in (1, 2):
        clone = tmp_path / f"input-{index}.png"
        clone.write_bytes(first.read_bytes())
        frames.append(str(clone))

    output_directory = tmp_path / "out"
    dump_directory = tmp_path / "world-dumps"
    dump_directory.mkdir()
    request = tmp_path / "request.json"
    request.write_text(json.dumps({
        "schema_version": 1,
        "plugin": str(AEX),
        "input_frames": frames,
        "output_directory": str(output_directory),
        "world_dump_dir": str(dump_directory),
        "output_checksum_detail": True,
    }), encoding="utf-8")
    report_path = tmp_path / "report.json"
    completed = subprocess.run(
        [str(BROKER), "render-video-batch", str(request), str(report_path)],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=300,
    )
    assert completed.returncode == 0, completed.stderr[-800:]
    report = json.loads(report_path.read_text(encoding="utf-8"))
    assert report["passed"] is True
    assert report["frame_count"] == 3
    assert report["frames_ok"] == 3
    assert report["aborted"] is False
    session = report["session"]
    assert session["session_clean"] is True
    assert session["invalidated"] is False
    assert session["worker"]["classification"] == "ok"
    final = session["final_report"]
    # One SEQUENCE_SETUP for the whole batch, torn down once at close: the
    # resident session, not three one-shot lifecycles.
    assert final["persistent_sequence_setup_error"] == 0
    assert final["persistent_sequence_setdown_error"] == 0
    assert final["render_error"] == 0
    assert final["guard_bytes_intact"] is True
    for index in range(3):
        assert (output_directory / f"frame-{index:06}.png").is_file()
    # Identical inputs through a stateless fixture produce identical
    # transferred outputs; the checksums are the worker's slot hashes.
    checksums = {frame["checksum"] for frame in report["frames"]}
    assert len(checksums) == 1
    # The auxiliary observation options reached the real worker: world
    # snapshots landed in the requested dump directory.
    assert any(dump_directory.iterdir()), "world dump directory received snapshots"

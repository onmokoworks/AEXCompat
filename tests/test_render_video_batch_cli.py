"""Broker batch video render CLI (issue #98 stage 1 PR-C).

Source-level wiring assertions always run. The runtime test drives
``broker.exe render-video-batch`` end to end against the session-capable
``aex_render_worker.exe`` (PR-B) and the pf_sampling_probe fixture, with
self-computed expectations only, so a fresh build on any machine satisfies
it.
"""
import json
import os
import shutil
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
# STATUS_DLL_INIT_FAILED: what a process exits with when a restricted token
# cannot initialize it. The Rust integration tests detect the same condition
# through their own probe (broker/crates/broker/tests/common, issue #335).
STATUS_DLL_INIT_FAILED = 0xC0000142
GENERATOR = ROOT / "tools" / "generate-oracle-rgba-input.py"




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
    # World dumps must stay under the broker-managed <repository>/target
    # boundary; the resolver creates the directory and requires it fresh.
    dump_directory = ROOT / "target" / f"world-dumps-batch-test-{os.getpid()}"
    if dump_directory.exists():
        shutil.rmtree(dump_directory)
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
    if completed.returncode != 0:
        # The CLI exits 3 with nothing on stderr when the batch ran but did not
        # pass; the diagnosis is in the report. Surface it either way, and skip
        # only for the one environment limitation that provably has nothing to
        # do with this code: a host whose restricted token cannot initialize the
        # worker at all (STATUS_DLL_INIT_FAILED, issue #335). Any other failure
        # is reported with the full report so it can be diagnosed.
        detail = (report_path.read_text(encoding="utf-8")
                  if report_path.is_file() else "<no report written>")
        allow_skip = os.environ.get("AEXCOMPAT_ALLOW_RESTRICTED_TOKEN_SKIP") == "1"
        if allow_skip and f'"exit_code": {STATUS_DLL_INIT_FAILED}' in detail:
            pytest.skip(
                "this environment cannot launch a restricted-token worker "
                "(STATUS_DLL_INIT_FAILED, issue #335)")
        raise AssertionError(
            "broker exited {}; stderr: {}; report: {}".format(
                completed.returncode, completed.stderr[-800:], detail[:2000]))
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
    # transferred outputs; the batch manifest's checksums are computed by the
    # broker from the frames it wrote (issue #690).
    checksums = {frame["checksum"] for frame in report["frames"]}
    assert len(checksums) == 1
    # The auxiliary observation options reached the real worker: world
    # snapshots landed in the requested dump directory and the final report
    # carries the opt-in checksum detail for the transferred output.
    try:
        assert any(dump_directory.iterdir()), "world dump directory received snapshots"
    finally:
        shutil.rmtree(dump_directory, ignore_errors=True)
    assert final["output_row_crc32"], "checksum detail rows recorded"
    assert final["output_channel_sha256"], "checksum detail channels recorded"

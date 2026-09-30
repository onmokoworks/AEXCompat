from __future__ import annotations

import sys
import os
import signal
import subprocess
import base64
import hashlib
import json
import struct
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
import resolve_ofx_macos_runner as RUNNER  # noqa: E402
from openfx_render_session_contract import build_bridge_packet  # noqa: E402


def test_runner_publishes_only_verified_pixels_and_compact_evidence(tmp_path, monkeypatch):
    pixels = bytes(range(16))
    rendered = bytes(reversed(pixels))
    source = tmp_path / "source.rgba"
    result = tmp_path / "result.rgba"
    packet_path = tmp_path / "packet.json"
    evidence_dir = tmp_path / "evidence"
    evidence_dir.mkdir()
    source.write_bytes(pixels)
    monkeypatch.setenv("AEXCOMPAT_RESOLVE_AEX_PATH", "effect.aex")
    monkeypatch.setenv("AEXCOMPAT_RESOLVE_EVIDENCE_DIR", str(evidence_dir))
    observed = []

    def backend(**kwargs):
        observed.append(kwargs)
        return build_bridge_packet(
            plugin_relative_path="effect.aex",
            plugin_sha256="a" * 64,
            worker_sha256="b" * 64,
            width=2, height=2, rowbytes=8, pixels=pixels,
            current_time=250, total_time=250,
            output_pixels=rendered,
        )

    monkeypatch.setattr(RUNNER.macos_session, "render_macos_frame", backend)
    args = [str(source), str(result), str(packet_path), "2", "2", "250"]
    assert RUNNER.run(args) == 0
    assert result.read_bytes() == rendered
    assert observed[0]["current_time"] == 250
    assert observed[0]["total_time"] == 250
    records = list(evidence_dir.glob("resolve-aex-*.json"))
    assert len(records) == 1
    record = records[0].read_text(encoding="utf-8")
    assert "data_base64" not in record
    assert '"status":"worker_rendered"' in record


@pytest.mark.parametrize("failure", ["timeout", "identity_mismatch"])
def test_runner_failure_never_publishes_pixels(tmp_path, monkeypatch, failure):
    pixels = bytes(range(16))
    source = tmp_path / "source.rgba"
    result = tmp_path / "result.rgba"
    packet_path = tmp_path / "packet.json"
    source.write_bytes(pixels)
    monkeypatch.setenv("AEXCOMPAT_RESOLVE_AEX_PATH", "effect.aex")
    monkeypatch.setattr(
        RUNNER.macos_session,
        "render_macos_frame",
        lambda **_kwargs: build_bridge_packet(
            plugin_relative_path="effect.aex",
            plugin_sha256="a" * 64,
            worker_sha256="b" * 64,
            width=2, height=2, rowbytes=8, pixels=pixels,
            current_time=250, total_time=250,
            response_status=failure,
        ),
    )
    assert RUNNER.run([str(source), str(result), str(packet_path), "2", "2", "250"]) == 1
    assert not result.exists()
    assert packet_path.exists()


def test_runner_rejects_short_frame_before_backend(tmp_path, monkeypatch):
    source = tmp_path / "source.rgba"
    source.write_bytes(b"short")
    monkeypatch.setenv("AEXCOMPAT_RESOLVE_AEX_PATH", "effect.aex")
    monkeypatch.setattr(
        RUNNER.macos_session, "render_macos_frame",
        lambda **_kwargs: pytest.fail("backend must not start"),
    )
    result = tmp_path / "result.rgba"
    assert RUNNER.run([str(source), str(result), str(tmp_path / "packet.json"), "2", "2", "0"]) == 2
    assert not result.exists()


def test_output_write_failure_does_not_publish_success_evidence(tmp_path, monkeypatch):
    pixels = bytes(range(16))
    source = tmp_path / "source.rgba"
    source.write_bytes(pixels)
    evidence_dir = tmp_path / "evidence"
    evidence_dir.mkdir()
    monkeypatch.setenv("AEXCOMPAT_RESOLVE_AEX_PATH", "effect.aex")
    monkeypatch.setenv("AEXCOMPAT_RESOLVE_EVIDENCE_DIR", str(evidence_dir))
    monkeypatch.setattr(
        RUNNER.macos_session,
        "render_macos_frame",
        lambda **_kwargs: build_bridge_packet(
            plugin_relative_path="effect.aex", plugin_sha256="a" * 64,
            worker_sha256="b" * 64, width=2, height=2, rowbytes=8,
            pixels=pixels, output_pixels=bytes(reversed(pixels)),
        ),
    )
    # An existing directory cannot be replaced by the binary output file.
    assert RUNNER.run([str(source), str(tmp_path), str(tmp_path / "packet.json"), "2", "2", "0"]) == 1
    assert list(evidence_dir.iterdir()) == []


@pytest.mark.skipif(sys.platform != "darwin", reason="macOS process-group cleanup")
def test_signal_terminates_and_reaps_active_harness(monkeypatch):
    child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"], start_new_session=True)
    monkeypatch.setattr(RUNNER.macos_session.macos_backend, "_ACTIVE_HARNESS_PID", child.pid)
    try:
        with pytest.raises(SystemExit) as error:
            RUNNER._terminate(signal.SIGTERM, None)
        assert error.value.code == 124
        with pytest.raises(ProcessLookupError):
            os.kill(child.pid, 0)
    finally:
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        try:
            child.wait(timeout=1)
        except ChildProcessError:
            pass


@pytest.mark.skipif(sys.platform != "darwin", reason="macOS AEX worker required")
def test_real_native_aex_receives_frame_time_and_straight_pixels(tmp_path, monkeypatch):
    smoke = os.environ.get("AEXCOMPAT_RESOLVE_NATIVE_SMOKE")
    plugin = os.environ.get("AEXCOMPAT_RESOLVE_NATIVE_PLUGIN")
    required = (
        smoke, plugin, os.environ.get("AEXCOMPAT_RESOLVE_AEX_PATH"),
        os.environ.get("AEXCOMPAT_PLUGIN_ROOT"), os.environ.get("AEXCOMPAT_HARNESS"),
        os.environ.get("AEXCOMPAT_GUEST_WORKER"),
        os.environ.get("AEXCOMPAT_RESOLVE_PYTHON"),
        os.environ.get("AEXCOMPAT_RESOLVE_RUNNER"),
    )
    if not all(required):
        pytest.skip("real AEX native fixture paths are not configured")

    def f32(value):
        return struct.unpack("<f", struct.pack("<f", value))[0]

    # Independent straight-alpha reference for the native fixture's padded
    # float source. The zero-alpha pixels must carry zero RGB to the worker.
    straight = bytearray()
    for y in range(64):
        for x in range(64):
            alpha = 0 if x % 4 == 0 else 128 if x % 4 == 1 else 255
            straight.extend((
                0 if alpha == 0 else (x * 17 + y * 3) % 256,
                0 if alpha == 0 else (x * 11 + y * 7) % 256,
                0 if alpha == 0 else (x * 5 + y * 13) % 256,
                alpha,
            ))

    evidence_dir = tmp_path / "evidence"
    evidence_dir.mkdir()
    monkeypatch.setenv("AEXCOMPAT_RESOLVE_EVIDENCE_DIR", str(evidence_dir))
    result = subprocess.run([smoke, plugin], capture_output=True, text=True, check=True)
    native = json.loads(result.stdout)
    assert native["aex_render_claim"] == "verified_mac_native_fixture"
    records = list(evidence_dir.glob("resolve-aex-*.json"))
    assert len(records) == 1
    receipt = json.loads(records[0].read_text(encoding="utf-8"))
    assert receipt["status"] == "worker_rendered"
    assert receipt["time_ms"] == 250  # OFX frame 6 at 24 fps, observed by runner
    assert receipt["input_sha256"] == hashlib.sha256(straight).hexdigest()

    # Render the same worker input directly and independently reconstruct the
    # padded premultiplied float output expected in the OFX host image.
    packet = RUNNER.macos_session.render_macos_frame(
        plugin_relative_path=os.environ["AEXCOMPAT_RESOLVE_AEX_PATH"],
        width=64, height=64, rowbytes=256, pixels=bytes(straight),
        current_time=250, total_time=250,
    )
    response = packet["frame_exchange"]["response"]
    assert response["status"] == "rendered"
    worker_pixels = base64.b64decode(response["output"]["data_base64"], validate=True)
    assert receipt["output_sha256"] == hashlib.sha256(worker_pixels).hexdigest()
    host_pixels = bytearray()
    for y in range(64):
        for x in range(64):
            r, g, b, a = worker_pixels[(y * 64 + x) * 4:(y * 64 + x + 1) * 4]
            alpha = f32(f32(a) / f32(255))
            for channel in (r, g, b):
                host_pixels.extend(struct.pack("<f", f32(f32(f32(channel) / f32(255)) * alpha)))
            host_pixels.extend(struct.pack("<f", alpha))
        host_pixels.extend(b"\xcd" * 32)
    assert native["output_sha256"].lower() == hashlib.sha256(host_pixels).hexdigest()

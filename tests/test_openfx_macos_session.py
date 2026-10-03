from __future__ import annotations

import base64
import hashlib
import json
import subprocess
import sys
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
import openfx_macos_session as SESSION  # noqa: E402


@pytest.fixture
def runtime(tmp_path, monkeypatch):
    root = tmp_path / "plugins"
    root.mkdir()
    plugin = root / "effect.aex"
    plugin.write_bytes(b"fixture plugin")
    harness = tmp_path / "harness"
    harness.write_bytes(b"fixture harness")
    worker = tmp_path / "worker"
    worker.write_bytes(b"fixture worker")
    monkeypatch.setenv("AEXCOMPAT_PLUGIN_ROOT", str(root))
    monkeypatch.setenv("AEXCOMPAT_HARNESS", str(harness))
    monkeypatch.setenv("AEXCOMPAT_GUEST_WORKER", str(worker))
    monkeypatch.setattr(SESSION.sys, "platform", "darwin")
    return plugin, harness, worker


def input_frame():
    # Two 2-pixel rows, each followed by four bytes of host padding.
    return bytes((10, 20, 30, 255, 40, 50, 60, 128, 91, 92, 93, 94,
                  70, 80, 90, 64, 100, 110, 120, 0, 95, 96, 97, 98))


def render(runtime, monkeypatch, *, before_backend=None, mutate=None, **changes):
    plugin, harness, worker = runtime
    seen = []

    def backend(request):
        seen.append(request)
        if before_backend is not None:
            before_backend()
        packed = base64.b64decode(request["input"]["data"])
        transformed = bytearray(packed)
        transformed[0] += 1
        response = {
            "status": "rendered", "aex_render_performed": True, "host_success": True,
            "render_path": request["render_path"],
            "frame": request["frame"],
            "plugin_identity": {
                "sha256": hashlib.sha256(plugin.read_bytes()).hexdigest(),
                "source_relative_path": "effect.aex",
            },
            "render_identity": {
                "guest_worker_sha256": hashlib.sha256(worker.read_bytes()).hexdigest(),
                "harness_sha256": hashlib.sha256(harness.read_bytes()).hexdigest(),
                "files_unchanged": True,
                "post_run": {
                    "plugin_sha256": hashlib.sha256(plugin.read_bytes()).hexdigest(),
                    "guest_worker_sha256": hashlib.sha256(worker.read_bytes()).hexdigest(),
                    "harness_sha256": hashlib.sha256(harness.read_bytes()).hexdigest(),
                },
            },
            "input": {"sha256": hashlib.sha256(packed).hexdigest(), "bytes": len(packed)},
            "output": {
                "encoding": "base64-rgba8",
                "data": base64.b64encode(transformed).decode("ascii"),
                "sha256": hashlib.sha256(transformed).hexdigest(),
                "bytes": len(transformed),
            },
        }
        if mutate is not None:
            mutate(response)
        return response

    monkeypatch.setattr(SESSION.macos_backend, "build_response", backend)
    arguments = dict(
        plugin_relative_path="effect.aex", width=2, height=2,
        rowbytes=12, pixels=input_frame(), current_time=1000,
        total_time=1000,
    )
    arguments.update(changes)
    return SESSION.render_macos_frame(**arguments), seen


def test_padded_host_rows_reach_aex_as_packed_rgba_and_publish_verified_packet(runtime, monkeypatch):
    packet, seen = render(runtime, monkeypatch)
    packed = input_frame()[:8] + input_frame()[12:20]
    assert len(seen) == 1
    assert seen[0]["timeout_ms"] == 30_000
    assert base64.b64decode(seen[0]["input"]["data"]) == packed
    assert seen[0]["frame"]["frame_time"] == {"seconds": 1.0}
    assert packet["session_open"]["geometry"]["rowbytes"] == 8
    request = packet["frame_exchange"]["request"]
    response = packet["frame_exchange"]["response"]
    assert request["input"]["rowbytes"] == 12
    assert base64.b64decode(request["input"]["data_base64"]) == input_frame()
    assert response["status"] == "rendered"
    assert response["output"]["rowbytes"] == 8
    output = base64.b64decode(response["output"]["data_base64"])
    assert output[0] == packed[0] + 1 and output[1:] == packed[1:]
    assert packet["close"] == {"status": "closed", "frames_ok": 1, "frames_errored": 0}
    assert SESSION.validate_bridge_packet(packet) == []


def test_changed_worker_identity_invalidates_without_publishing_output(runtime, monkeypatch):
    plugin, harness, worker = runtime

    def drift(request):
        packed = base64.b64decode(request["input"]["data"])
        return {
            "status": "rendered", "aex_render_performed": True, "host_success": True,
            "render_path": request["render_path"],
            "frame": request["frame"],
            "plugin_identity": {
                "sha256": hashlib.sha256(plugin.read_bytes()).hexdigest(),
                "source_relative_path": "effect.aex",
            },
            "render_identity": {
                "guest_worker_sha256": hashlib.sha256(worker.read_bytes()).hexdigest(),
                "harness_sha256": hashlib.sha256(harness.read_bytes()).hexdigest(),
                "files_unchanged": False,
                "post_run": {
                    "plugin_sha256": hashlib.sha256(plugin.read_bytes()).hexdigest(),
                    "guest_worker_sha256": hashlib.sha256(worker.read_bytes()).hexdigest(),
                    "harness_sha256": hashlib.sha256(harness.read_bytes()).hexdigest(),
                },
            },
            "input": {"sha256": hashlib.sha256(packed).hexdigest(), "bytes": len(packed)},
            "output": {
                "encoding": "base64-rgba8",
                "data": base64.b64encode(packed).decode(),
                "sha256": hashlib.sha256(packed).hexdigest(), "bytes": len(packed),
            },
        }
    monkeypatch.setattr(SESSION.macos_backend, "build_response", drift)
    result = SESSION.render_macos_frame(
        plugin_relative_path="effect.aex", width=2, height=2,
        rowbytes=12, pixels=input_frame(), current_time=1000, total_time=1000,
    )
    assert result["frame_exchange"]["response"]["status"] == "identity_mismatch"
    assert "output" not in result["frame_exchange"]["response"]
    assert result["close"]["status"] == "invalidated"
    assert SESSION.validate_bridge_packet(result) == []


def test_corrupt_backend_pixels_are_never_published(runtime, monkeypatch):
    def corrupt(response):
        response["output"]["data"] = "not valid base64!"
    packet, _ = render(runtime, monkeypatch, mutate=corrupt)
    assert packet["frame_exchange"]["response"]["status"] == "protocol_error"
    assert "output" not in packet["frame_exchange"]["response"]


def test_backend_route_and_encoding_must_match_observed_rgba(runtime, monkeypatch):
    packet, _ = render(
        runtime, monkeypatch, render_path="smart",
        mutate=lambda response: response.update(render_path="classic"),
    )
    assert packet["frame_exchange"]["response"]["status"] == "protocol_error"
    assert "output" not in packet["frame_exchange"]["response"]

    packet, _ = render(
        runtime, monkeypatch,
        mutate=lambda response: response["output"].update(encoding="base64-argb8"),
    )
    assert packet["frame_exchange"]["response"]["status"] == "protocol_error"
    assert "output" not in packet["frame_exchange"]["response"]


def test_worker_file_change_during_render_invalidates_record(runtime, monkeypatch):
    def change_worker(_response):
        runtime[2].write_bytes(b"worker changed while frame ran")
    packet, _ = render(runtime, monkeypatch, mutate=change_worker)
    assert packet["frame_exchange"]["response"]["status"] == "identity_mismatch"
    assert "output" not in packet["frame_exchange"]["response"]


def test_rebuild_before_backend_observation_records_new_worker_identity(runtime, monkeypatch):
    def rebuild():
        runtime[2].write_bytes(b"new stable worker before backend observation")
    packet, _ = render(runtime, monkeypatch, before_backend=rebuild)
    assert packet["frame_exchange"]["response"]["status"] == "rendered"
    assert packet["session_open"]["worker"]["sha256"] == hashlib.sha256(runtime[2].read_bytes()).hexdigest()
    assert SESSION.validate_bridge_packet(packet) == []


def test_backend_timeout_is_explicit_and_cannot_close_healthy(runtime, monkeypatch):
    def timeout(_request):
        raise SESSION.macos_backend.SessionRequestError("timed out", "session_timeout")
    monkeypatch.setattr(SESSION.macos_backend, "build_response", timeout)
    result = SESSION.render_macos_frame(
        plugin_relative_path="effect.aex", width=2, height=2,
        rowbytes=12, pixels=input_frame(), current_time=1000, total_time=1000,
    )
    assert result["frame_exchange"]["response"] == {"status": "timeout", "classification": "session_timeout"}
    assert result["close"] == {"status": "invalidated", "frames_ok": 0, "frames_errored": 1}


def test_harness_nonzero_exit_is_not_labeled_worker_crash(runtime, monkeypatch):
    def nonzero(_request):
        raise SESSION.macos_backend.SessionRequestError("harness command failed", "worker_failure")
    monkeypatch.setattr(SESSION.macos_backend, "build_response", nonzero)
    packet = SESSION.render_macos_frame(
        plugin_relative_path="effect.aex", width=2, height=2,
        rowbytes=12, pixels=input_frame(), current_time=1000, total_time=1000,
    )
    assert packet["frame_exchange"]["response"] == {
        "status": "unsupported", "classification": "worker_failure",
    }
    assert "output" not in packet["frame_exchange"]["response"]


def test_backend_io_failure_after_worker_disappears_is_fail_closed(runtime, monkeypatch):
    def lost_file(_request):
        runtime[2].unlink()
        raise OSError("worker disappeared during post-render hash")
    monkeypatch.setattr(SESSION.macos_backend, "build_response", lost_file)
    packet = SESSION.render_macos_frame(
        plugin_relative_path="effect.aex", width=2, height=2,
        rowbytes=12, pixels=input_frame(), current_time=1000, total_time=1000,
    )
    assert packet["frame_exchange"]["response"] == {
        "status": "identity_mismatch", "classification": "runtime_file_unavailable",
    }
    assert "output" not in packet["frame_exchange"]["response"]


def test_host_json_request_reaches_the_same_verified_frame(runtime, monkeypatch):
    _, seen = render(runtime, monkeypatch)
    request = {
        "plugin_relative_path": "effect.aex", "width": 2, "height": 2,
        "rowbytes": 12, "pixels_base64": base64.b64encode(input_frame()).decode(),
        "current_time": 1000, "total_time": 1000,
    }
    packet = SESSION.process_request(request)
    assert len(seen) == 2
    assert packet["frame_exchange"]["response"]["status"] == "rendered"
    assert SESSION.validate_bridge_packet(packet) == []
    with pytest.raises(ValueError):
        SESSION.process_request({**request, "unexpected": 1})


def test_cli_rejects_duplicate_request_keys_before_a_worker_runs():
    completed = subprocess.run(
        [sys.executable, str(ROOT / "tools" / "openfx_macos_session.py")],
        input='{"plugin_relative_path":"safe.aex","plugin_relative_path":"other.aex"}',
        text=True, capture_output=True, check=False,
    )
    assert completed.returncode == 2
    assert json.loads(completed.stdout) == {"status": "rejected", "classification": "invalid_request"}


@pytest.mark.parametrize("changes", [
    {"rowbytes": 7},
    {"pixels": b"short"},
    {"plugin_relative_path": "../escape.aex"},
    {"alpha_mode": "premultiplied"},
    {"time_scale": 30},
])
def test_unsupported_inputs_fail_before_worker_launch(runtime, monkeypatch, changes):
    def launch(_request):
        pytest.fail("worker must not launch for an unsupported frame")
    monkeypatch.setattr(SESSION.macos_backend, "build_response", launch)
    arguments = dict(
        plugin_relative_path="effect.aex", width=2, height=2,
        rowbytes=12, pixels=input_frame(), current_time=1000, total_time=1000,
    )
    arguments.update(changes)
    with pytest.raises(ValueError):
        SESSION.render_macos_frame(**arguments)


def test_existing_plugin_path_over_contract_length_rejected_before_launch(runtime, monkeypatch):
    directory = runtime[0].parent / ("d" * 130)
    directory.mkdir()
    filename = "e" * 127 + ".aex"
    (directory / filename).write_bytes(b"existing but contract path is too long")
    relative = directory.name + "/" + filename
    assert len(relative) > 260
    monkeypatch.setattr(SESSION.macos_backend, "build_response", lambda _request: pytest.fail("worker launched"))
    with pytest.raises(ValueError, match="relative path"):
        SESSION.render_macos_frame(
            plugin_relative_path=relative, width=2, height=2,
            rowbytes=12, pixels=input_frame(), current_time=1000, total_time=1000,
        )

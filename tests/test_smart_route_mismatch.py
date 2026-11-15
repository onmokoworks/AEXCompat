"""A classic-only native fixture must reject smart without dispatching selectors."""
import hashlib
import ctypes
from ctypes import wintypes
import json
import os
import subprocess
from pathlib import Path

import pytest

from test_render_session_worker import (
    HEIGHT, WIDTH, OUTPUT_GENERATION_OFFSET, SessionTransport, render_frame_message,
)

ROOT = Path(__file__).resolve().parents[1]
pytestmark = pytest.mark.skipif(os.name != "nt", reason="Windows native transport")
DIAGNOSTIC = {"reason": "smart_render_not_advertised", "advertised_out_flags2": 0x00800009}


def artifacts(params_error):
    name = "worker_non_smart_params_error_fixture.aex" if params_error else "worker_non_smart_fixture.aex"
    plugin = ROOT / "target" / "minihost-build" / name
    assert plugin.is_file(), "build the native non-SmartFX fixture"
    return plugin, hashlib.sha256(plugin.read_bytes()).hexdigest()


def check_report(stdout, stderr, params_error):
    report = json.loads(stdout)
    assert report["status"] == "render_failed"
    assert report["route_mismatch"] == DIAGNOSTIC
    assert report["advertised_out_flags2"] == 0x00800009
    assert report["smart_pre_render_dispatched"] is False
    assert report["smart_render_selector_dispatched"] is False
    assert report["global_setdown_error"] == 0
    assert report["params_setup_error"] == params_error
    assert "fixture_dispatch_counts:0,0,0,0" in stderr


@pytest.mark.parametrize("depth,pixel_bytes,command", [
    (8, 4, "--smart-session-v1"),
    (16, 8, "--smart-session16-v1"),
    (32, 16, "--smart-session32-cpu-v1"),
])
@pytest.mark.parametrize("params_error", [0, 4])
def test_smart_route_mismatch_session(canonical_release_worker, depth, pixel_bytes, command, params_error):
    plugin, digest = artifacts(params_error)
    transport = SessionTransport(depth, pixel_bytes)
    process = subprocess.Popen(
        [str(canonical_release_worker), "--kind", "smart", command, str(plugin),
         digest, "v2|", str(WIDTH), str(HEIGHT), "1", "300", "30"],
        cwd=ROOT, env=transport.environment(), close_fds=False,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, errors="replace",
    )
    transport.attach_process(process)
    transport.close_child_ends()
    try:
        transport.write_input(11, 1)
        transport.send(render_frame_message(0, 0))
        done = transport.receive()
        assert done == {"v": 1, "type": "frame_done", "frame_index": 0,
                        "status": "error", "render_error": -49, "route_mismatch": DIAGNOSTIC}
        assert transport.read_header(OUTPUT_GENERATION_OFFSET) == 0
        stdout, stderr = process.communicate(timeout=30)
        assert process.returncode == 22
        check_report(stdout, stderr, params_error)
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)
        transport.kernel32.UnmapViewOfFile.argtypes = [wintypes.LPVOID]
        assert transport.kernel32.UnmapViewOfFile(transport.view_address)
        for handle in (transport.request_write, transport.response_read, transport.section):
            assert transport.kernel32.CloseHandle(wintypes.HANDLE(handle))


@pytest.mark.parametrize("params_error", [0, 4])
def test_smart_route_mismatch_single_shot(canonical_release_worker, params_error):
    plugin, digest = artifacts(params_error)
    result = subprocess.run(
        [str(canonical_release_worker), "--kind", "smart", "--smart",
         str(plugin), digest, "default"], cwd=ROOT,
        capture_output=True, text=True, errors="replace", timeout=30,
    )
    assert result.returncode == 22
    check_report(result.stdout, result.stderr, params_error)

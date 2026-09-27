import hashlib
import json
import os
import subprocess
from pathlib import Path

import pytest

from _native_selftest import run


def test_dvacore_main_thread_registration_preserves_existing_owner():
    report = run(
        "worker_dvacore_main_thread_selftest.exe",
        "worker_dvacore_main_thread_selftest",
        "AEXCOMPAT_DVACORE_MAIN_THREAD_SELFTEST",
    )
    assert report["checks"] == 5


def test_real_cor_aex_birth_returns_before_parameter_inspection():
    """Optional installed-AEX gate: exercise the actual worker/export/callback path.

    CI without Adobe's support DLLs cannot run this test. A configured local
    run must observe both a completed COR_Birth and a valid params report; a
    hang, export typo, or removed worker call fails rather than passing.
    """
    aex_name = os.environ.get("AEXCOMPAT_TEST_COR_AEX")
    worker_name = os.environ.get("AEXCOMPAT_TEST_WORKER_PATH")
    roots = os.environ.get("AEXCOMPAT_TEST_COR_DEPENDENCY_DIRS")
    if not (aex_name and worker_name and roots):
        pytest.skip("set COR AEX, worker, and dependency-dir paths for local gate")
    aex = Path(aex_name)
    worker = Path(worker_name)
    assert aex.is_file() and worker.is_file()
    result = subprocess.run(
        [str(worker), "--kind", "discovery", "--l2-params-only",
         str(aex), hashlib.sha256(aex.read_bytes()).hexdigest(),
         "--dependency-dirs-v1", roots],
        capture_output=True, text=True, timeout=20, errors="replace",
    )
    assert result.returncode == 0, result.stderr
    report = json.loads(result.stdout)
    assert report["status"] == "parameters_inspected"
    assert report["reported_num_params"] > 1
    assert "entry=?COR_Birth@@YAH_N00@Z status=called result=0" in result.stderr
    assert "stage:pf_host_layer_teardown result=0" in result.stderr
    assert "stage:cor_birth_teardown result=0" in result.stderr


def test_real_cor_aex_discovery_session_closes_after_paired_teardown(tmp_path):
    """The resident discovery route has its own finish path, unlike one-shot."""
    aex_name = os.environ.get("AEXCOMPAT_TEST_COR_AEX")
    worker_name = os.environ.get("AEXCOMPAT_TEST_WORKER_PATH")
    roots = os.environ.get("AEXCOMPAT_TEST_COR_DEPENDENCY_DIRS")
    if not (aex_name and worker_name and roots):
        pytest.skip("set COR AEX, worker, and dependency-dir paths for local gate")
    from test_render_session_worker import SessionTransport

    aex = Path(aex_name)
    worker = Path(worker_name)
    assert aex.is_file() and worker.is_file()
    manifest_path = tmp_path / "cor-discovery-manifest.json"
    manifest_path.write_text(json.dumps({
        "schema": "cluster-manifest-v2",
        "plugins": [{"path": str(aex),
                     "sha256": hashlib.sha256(aex.read_bytes()).hexdigest()}],
        "search_dirs": roots.split(";"),
        "module_bound": 400,
    }), encoding="utf-8")
    transport = SessionTransport()
    process = subprocess.Popen(
        [str(worker), "--kind", "discovery", "--discovery-session-v1",
         "--cluster-manifest-v2", str(manifest_path)],
        env=transport.environment(), close_fds=False,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
    )
    transport.attach_process(process)
    transport.close_child_ends()
    try:
        transport.send({"v": 1, "type": "inspect_plugin",
                        "plugin_index": 0, "request_index": 0})
        checkpoint = transport.receive(timeout=20)
        done = transport.receive(timeout=20)
        assert checkpoint["type"] == "inspect_checkpoint"
        assert done["type"] == "inspect_done"
        assert done["status"] == "ok"
        assert done["report"]["status"] == "parameters_inspected"
        transport.send({"v": 1, "type": "close"})
        stdout, stderr = process.communicate(timeout=20)
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=5)
    assert process.returncode == 0, stderr
    assert json.loads(stdout)["status"] == "discovery_session_completed"
    assert "stage:pf_host_layer_teardown result=0" in stderr
    assert "stage:cor_birth_teardown result=0" in stderr

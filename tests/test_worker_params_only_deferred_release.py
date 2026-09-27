"""The params-only worker keeps its loaded module until process detach."""

import hashlib
import os
import subprocess
from pathlib import Path

from _native_selftest import locate


def test_params_only_does_not_explicitly_unload_module(tmp_path):
    build = os.environ.get("AEXCOMPAT_TEST_NATIVE_BUILD")
    worker = (
        os.path.join(build, "aex_worker.exe") if build else locate("aex_worker.exe")
    )
    fixture = (
        os.path.join(build, "worker_session_detach_fixture.dll")
        if build
        else locate("worker_session_detach_fixture.dll")
    )
    marker = tmp_path / "detach.txt"
    digest = hashlib.sha256(Path(fixture).read_bytes()).hexdigest()
    environment = os.environ.copy()
    environment["AEXCOMPAT_DETACH_MARKER"] = str(marker)
    completed = subprocess.run(
        [worker, "--kind", "discovery", "--l2-params-only", fixture, digest],
        env=environment,
        capture_output=True,
        text=True,
        timeout=20,
        check=False,
    )
    assert completed.returncode == 12, completed.stderr or completed.stdout
    assert "unknown_no_effect_entrypoint" in completed.stderr
    assert marker.read_text(encoding="ascii") == "process"

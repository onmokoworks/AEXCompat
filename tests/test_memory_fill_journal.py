"""Exercise the production inherited-handle journal across abrupt worker exits.

The self-test route loads no AEX and spawns no descendants. Only the retained
Popen process is terminated; wait() verifies it exited before the file closes.
"""
import json
import os
import subprocess
import time

import pytest

msvcrt = pytest.importorskip("msvcrt")
from _native_selftest import locate


@pytest.mark.parametrize("mode", ["normal", "failure", "interrupt"])
def test_fill_journal_survives_owned_worker_exit(tmp_path, mode):
    path = tmp_path / "journal.jsonl"
    with path.open("wb") as journal:
        handle = msvcrt.get_osfhandle(journal.fileno())
        os.set_handle_inheritable(handle, True)
        startup = subprocess.STARTUPINFO()
        startup.lpAttributeList = {"handle_list": [handle]}
        process = subprocess.Popen(
            [str(locate("aex_worker.exe")), "--kind", "classic",
             "--self-test-memory-fill-journal", str(handle), mode],
            startupinfo=startup, close_fds=True, creationflags=subprocess.CREATE_NO_WINDOW,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        try:
            if mode == "interrupt":
                deadline = time.monotonic() + 10
                while path.stat().st_size == 0 and process.poll() is None:
                    assert time.monotonic() < deadline, "worker did not write journal"
                    time.sleep(0.01)
                assert process.poll() is None
                process.kill()
            stdout, stderr = process.communicate(timeout=10)
            assert process.returncode == (83 if mode == "failure" else 1 if mode == "interrupt" else 0), (stdout, stderr)
        finally:
            if process.poll() is None:
                process.kill()
            process.wait(timeout=10)
            os.set_handle_inheritable(handle, False)
    events = [json.loads(line) for line in path.read_text().splitlines()]
    assert len(events) == 1
    event = events[0]
    assert event["status"] == "applied"
    assert event["size"] == event["range"]["length"] == 32
    assert event["allocation_id"] > 0
    assert event["before_hash"] != event["after_hash"]
    assert event["initialized_before_hash"] == event["initialized_after_hash"]

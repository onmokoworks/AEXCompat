import json
import os
import struct
import subprocess
import threading
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
WORKERS = [
    ROOT / "target" / "minihost-build" / "aex_l2_worker.exe",
    ROOT / "target" / "minihost-build" / "aex_render_worker.exe",
    ROOT / "target" / "minihost-build" / "aex_smart_worker.exe",
]


@pytest.mark.parametrize("worker", WORKERS, ids=lambda p: p.name)
def test_guarded_crash_writes_a_minidump(worker: Path, tmp_path: Path) -> None:
    if os.name != "nt":
        pytest.skip("crash minidump capture is Windows-only")
    if not worker.is_file():
        pytest.skip(f"{worker.name} is not built; run the minihost build")

    import msvcrt

    dump_dir = tmp_path / "dumps"
    dump_dir.mkdir()
    dump_path = dump_dir / "crash-test.dmp"
    dump_read, dump_write = os.pipe()
    ack_read, ack_write = os.pipe()
    os.set_handle_inheritable(msvcrt.get_osfhandle(dump_write), True)
    os.set_handle_inheritable(msvcrt.get_osfhandle(ack_read), True)

    def broker_copy() -> None:
        total = 0
        overflow = False
        try:
            with dump_path.open("wb") as output:
                while True:
                    chunk = os.read(dump_read, 64 * 1024)
                    if not chunk:
                        break
                    remaining = (64 * 1024 * 1024) - total
                    if len(chunk) > remaining:
                        overflow = True
                        continue
                    output.write(chunk)
                    total += len(chunk)
            try:
                os.write(ack_write, struct.pack("<QB", total, int(overflow)))
            except OSError:
                pass
        finally:
            os.close(dump_read)
            os.close(ack_write)

    broker_thread = threading.Thread(target=broker_copy, daemon=True)
    broker_thread.start()
    process = None
    dump_write_open = True
    ack_read_open = True
    try:
        environment = os.environ.copy()
        environment.pop("AEXCOMPAT_MINIDUMP_DIR", None)
        environment["AEXCOMPAT_MINIDUMP_HANDLE"] = str(
            msvcrt.get_osfhandle(dump_write)
        )
        environment["AEXCOMPAT_MINIDUMP_ACK_HANDLE"] = str(
            msvcrt.get_osfhandle(ack_read)
        )
        process = subprocess.Popen(
            [str(worker), "--self-test-crash-minidump"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            close_fds=False,
            env=environment,
        )
        os.close(dump_write)
        dump_write_open = False
        os.close(ack_read)
        ack_read_open = False
        stdout, stderr = process.communicate(timeout=60)
    finally:
        if dump_write_open:
            os.close(dump_write)
        if ack_read_open:
            os.close(ack_read)
        if process is not None and process.poll() is None:
            process.kill()
            process.communicate()
        broker_thread.join(timeout=60)
    assert not broker_thread.is_alive(), "broker pipe reader did not finish"
    assert process is not None
    assert process.returncode == 0, stdout + stderr
    report = json.loads(stdout.strip())
    assert report["crash_minidump"] == "passed"
    assert report["attempted"] is True
    # 0xC0000005 is STATUS_ACCESS_VIOLATION.
    assert report["exception_code"] == 0xC0000005
    assert report["dump_bytes"] > 0

    assert dump_path.is_file()
    # Minidump files begin with the "MDMP" signature.
    assert dump_path.read_bytes()[:4] == b"MDMP"
    assert dump_path.stat().st_size == report["dump_bytes"]


@pytest.mark.parametrize("worker", WORKERS, ids=lambda p: p.name)
def test_crash_writes_nothing_without_opt_in(worker: Path, tmp_path: Path) -> None:
    if os.name != "nt":
        pytest.skip("crash minidump capture is Windows-only")
    if not worker.is_file():
        pytest.skip(f"{worker.name} is not built; run the minihost build")

    environment = os.environ.copy()
    environment.pop("AEXCOMPAT_MINIDUMP_DIR", None)
    environment.pop("AEXCOMPAT_MINIDUMP_HANDLE", None)
    environment.pop("AEXCOMPAT_MINIDUMP_ACK_HANDLE", None)
    result = subprocess.run(
        [str(worker), "--self-test-crash-no-minidump"],
        capture_output=True,
        text=True,
        timeout=60,
        env=environment,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    report = json.loads(result.stdout.strip())
    assert report["crash_minidump"] == "disabled"
    assert report["attempted"] is False
    assert not list(tmp_path.glob("**/*.dmp"))

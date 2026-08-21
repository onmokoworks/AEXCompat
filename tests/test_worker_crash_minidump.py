import json
import os
import struct
import subprocess
import threading
import time
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
WORKER = ROOT / "target" / "minihost-build" / "aex_worker.exe"
# The routes still parametrise this even though one binary serves them all
# (#1495): the crash guard runs inside the worker and the route gates behaviour
# in about forty places, so a dump written on one route says nothing about
# another.
KINDS = ["discovery", "classic", "smart"]


@pytest.mark.parametrize("kind", KINDS)
def test_guarded_crash_writes_a_minidump(kind: str, tmp_path: Path) -> None:
    if os.name != "nt":
        pytest.skip("crash minidump capture is Windows-only")
    if not WORKER.is_file():
        pytest.skip(f"{WORKER.name} is not built; run tools\\build-native.ps1")

    import msvcrt

    dump_dir = tmp_path / "dumps"
    dump_dir.mkdir()
    dump_path = dump_dir / "crash-test.dmp"
    # The worker never receives a directory or dump-file handle: it is handed an
    # inheritable pipe writer (dump) and an ack pipe reader, exactly as the
    # broker launch boundary does in production. This test stands in for the
    # broker's bounded-copy reader on the read side of that pipe.
    dump_read, dump_write = os.pipe()
    ack_read, ack_write = os.pipe()
    os.set_handle_inheritable(msvcrt.get_osfhandle(dump_write), True)
    os.set_handle_inheritable(msvcrt.get_osfhandle(ack_read), True)

    def broker_copy() -> None:
        total = 0
        overflow = False
        completion_marker = b"AEXDUMP-COMPLETE"
        pending = bytearray()
        try:
            with dump_path.open("wb") as output:
                while True:
                    chunk = os.read(dump_read, 64 * 1024)
                    if not chunk:
                        break
                    pending.extend(chunk)
                    flush_count = max(0, len(pending) - len(completion_marker))
                    remaining = (64 * 1024 * 1024) - total
                    if flush_count > remaining:
                        overflow = True
                        continue
                    output.write(pending[:flush_count])
                    del pending[:flush_count]
                    total += flush_count
                if pending != completion_marker:
                    overflow = True
            try:
                # A loaded CI runner can finish the dump but delay the broker
                # acknowledgement beyond the old two-second writer wait. Keep
                # one route deterministically beyond that boundary while the
                # other routes retain the ordinary transport timing.
                if kind == "discovery":
                    time.sleep(3)
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
        environment["AEXCOMPAT_MINIDUMP_HANDLE"] = str(msvcrt.get_osfhandle(dump_write))
        environment["AEXCOMPAT_MINIDUMP_ACK_HANDLE"] = str(
            msvcrt.get_osfhandle(ack_read)
        )
        process = subprocess.Popen(
            [str(WORKER), "--kind", kind, "--self-test-crash-minidump"],
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
    dump = dump_path.read_bytes()
    # Validate the seek-sensitive header and stream directory, not merely the
    # signature. DbgHelp emits directory entries at explicit offsets through
    # the alternate-I/O callback before the worker streams the completed image.
    assert dump[:4] == b"MDMP"
    assert len(dump) >= 32
    stream_count, directory_rva = struct.unpack_from("<II", dump, 8)
    assert 0 < stream_count <= 64
    assert 32 <= directory_rva <= len(dump)
    assert stream_count * 12 <= len(dump) - directory_rva
    stream_types = set()
    for index in range(stream_count):
        stream_type, data_size, data_rva = struct.unpack_from(
            "<III", dump, directory_rva + index * 12
        )
        stream_types.add(stream_type)
        if data_size:
            assert 32 <= data_rva <= len(dump)
            assert data_size <= len(dump) - data_rva
    # ExceptionStream is required when exception information was supplied.
    assert 6 in stream_types
    assert dump_path.stat().st_size == report["dump_bytes"]


@pytest.mark.parametrize("kind", KINDS)
def test_crash_writes_nothing_without_opt_in(kind: str, tmp_path: Path) -> None:
    if os.name != "nt":
        pytest.skip("crash minidump capture is Windows-only")
    if not WORKER.is_file():
        pytest.skip(f"{WORKER.name} is not built; run tools\\build-native.ps1")

    environment = os.environ.copy()
    environment.pop("AEXCOMPAT_MINIDUMP_DIR", None)
    environment.pop("AEXCOMPAT_MINIDUMP_HANDLE", None)
    environment.pop("AEXCOMPAT_MINIDUMP_ACK_HANDLE", None)
    result = subprocess.run(
        [str(WORKER), "--kind", kind, "--self-test-crash-no-minidump"],
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

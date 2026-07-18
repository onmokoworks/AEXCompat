import json
import os
import subprocess
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
    descriptor = os.open(
        dump_path,
        os.O_RDWR | os.O_CREAT | os.O_EXCL,
        0o600,
    )
    handle = msvcrt.get_osfhandle(descriptor)
    os.set_handle_inheritable(handle, True)
    try:
        environment = os.environ.copy()
        environment.pop("AEXCOMPAT_MINIDUMP_DIR", None)
        environment["AEXCOMPAT_MINIDUMP_HANDLE"] = str(handle)
        result = subprocess.run(
            [str(worker), "--self-test-crash-minidump"],
            capture_output=True,
            text=True,
            timeout=60,
            close_fds=False,
            env=environment,
        )
    finally:
        os.close(descriptor)
    assert result.returncode == 0, result.stdout + result.stderr
    report = json.loads(result.stdout.strip())
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

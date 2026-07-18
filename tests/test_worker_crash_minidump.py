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

    dump_dir = tmp_path / "dumps"
    dump_dir.mkdir()
    result = subprocess.run(
        [str(worker), "--self-test-crash-minidump", str(dump_dir)],
        capture_output=True,
        text=True,
        timeout=60,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    report = json.loads(result.stdout.strip())
    assert report["crash_minidump"] == "passed"
    assert report["attempted"] is True
    # 0xC0000005 is STATUS_ACCESS_VIOLATION.
    assert report["exception_code"] == 0xC0000005
    assert report["dump_bytes"] > 0

    dumps = list(dump_dir.glob("crash-*.dmp"))
    assert len(dumps) == 1
    # Minidump files begin with the "MDMP" signature.
    assert dumps[0].read_bytes()[:4] == b"MDMP"
    assert dumps[0].stat().st_size == report["dump_bytes"]


@pytest.mark.parametrize("worker", WORKERS, ids=lambda p: p.name)
def test_crash_writes_nothing_without_opt_in(worker: Path, tmp_path: Path) -> None:
    if os.name != "nt":
        pytest.skip("crash minidump capture is Windows-only")
    if not worker.is_file():
        pytest.skip(f"{worker.name} is not built; run the minihost build")

    # The self-test requires an explicit directory; a missing argument must be
    # rejected outright rather than defaulting to writing a dump somewhere.
    result = subprocess.run(
        [str(worker), "--self-test-crash-minidump"],
        capture_output=True,
        text=True,
        timeout=60,
    )
    assert result.returncode != 0
    assert not list(tmp_path.glob("**/*.dmp"))

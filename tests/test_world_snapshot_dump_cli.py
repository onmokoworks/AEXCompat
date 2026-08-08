import hashlib
import json
import os
import shutil
import subprocess
import zlib
from pathlib import Path

from _render_session import HARNESS

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "target" / "sdk-fixtures" / "shifter" / "Shifter.aex"
INPUT = ROOT / "target" / "ae-oracle-colorgrid-input.png"

def test_world_dumps_and_row_channel_checksums_match_the_raw_output(
    tmp_path: Path,
) -> None:
    dump_dir = ROOT / "target" / f"world-dump-test-{os.getpid()}"
    if dump_dir.exists():
        shutil.rmtree(dump_dir)
    output = tmp_path / "shifter-16-dumped.png"
    environment = dict(os.environ)
    environment["AEXCOMPAT_DUMP_WORLDS_DIR"] = f"target/{dump_dir.name}"
    environment["AEXCOMPAT_CHECKSUM_DETAIL"] = "1"
    try:
        completed = subprocess.run(
            [str(HARNESS), "--render-experimental-smart-16", str(FIXTURE),
             str(INPUT), str(output)],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=60,
            env=environment,
        )
        assert completed.returncode == 0, completed.stderr
        report = json.loads(completed.stdout)
        assert report["passed"] is True

        dumps = report["world_dumps"]
        assert dumps["directory"] == f"target/{dump_dir.name}"
        assert dumps["written"] >= 2
        assert dumps["skipped"] == 0

        names = sorted(item.name for item in dump_dir.iterdir())
        assert len(names) == dumps["written"]
        assert sum((dump_dir / name).stat().st_size for name in names) == dumps["bytes"]
        assert all(name.endswith(".rgba16le") for name in names)
        smart_input = [name for name in names if "-smart-input-" in name]
        smart_output = [name for name in names if "-smart-output-" in name]
        assert len(smart_input) == 1 and len(smart_output) == 1

        # The smart-output snapshot is byte-identical to the depth-preserving
        # raw sidecar: both are the RGBA-ordered native output transport.
        raw = Path(report["output_raw"]).read_bytes()
        assert (dump_dir / smart_output[0]).read_bytes() == raw

        # Row CRCs and channel digests must recompute from the raw bytes.
        width = report["width"]
        height = report["height"]
        row_bytes = width * 8
        assert len(raw) == row_bytes * height
        expected_rows = [
            f"{zlib.crc32(raw[row * row_bytes:(row + 1) * row_bytes]):08x}"
            for row in range(height)
        ]
        assert report["output_row_crc32"] == expected_rows
        expected_channels = []
        for channel in range(4):
            plane = bytearray()
            for pixel in range(width * height):
                offset = pixel * 8 + channel * 2
                plane += raw[offset:offset + 2]
            expected_channels.append(hashlib.sha256(bytes(plane)).hexdigest())
        assert [value.lower() for value in report["output_channel_sha256"]] == (
            expected_channels
        )
    finally:
        if dump_dir.exists():
            shutil.rmtree(dump_dir)

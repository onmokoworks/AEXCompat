import hashlib
import json
import os
import shutil
import subprocess
import zlib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKER_SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"
BROKER_SOURCE = ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"
HARNESS = ROOT / "broker" / "target" / "release" / "aexcompat-harness.exe"
FIXTURE = ROOT / "target" / "sdk-fixtures" / "shifter" / "Shifter.aex"
INPUT = ROOT / "target" / "ae-oracle-colorgrid-input.png"


def test_world_dump_and_checksum_detail_are_opt_in_and_fail_closed():
    worker = WORKER_SOURCE.read_text(encoding="utf-8")
    # Opt-in trailers, default off, with hard caps on count and total bytes.
    assert 'flag == L"--dump-worlds-v1"' in worker
    assert 'flag == L"--output-checksum-detail-v1"' in worker
    assert "constexpr uint32_t kMaxWorldDumps = 32;" in worker
    assert "constexpr uint64_t kMaxWorldDumpBytes = 1ull << 30;" in worker
    # Dump names carry stage and dimensions in the raw formats the comparison
    # tool consumes directly.
    assert '"%03u-%s-%dx%d.%s"' in worker
    assert '"rgba32f-le" : (pixel_bytes == 8 ? "rgba16le" : "rgba8")' in worker

    broker = BROKER_SOURCE.read_text(encoding="utf-8")
    assert '"AEXCOMPAT_DUMP_WORLDS_DIR"' in broker
    assert '"AEXCOMPAT_CHECKSUM_DETAIL"' in broker
    assert "world dump directory must stay under the repository target tree" in broker
    assert "world dump directory must start empty" in broker
    assert "world dump directory must not contain traversal components" in broker


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

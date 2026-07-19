"""issue #98 段階0 項目4: 現行 one-shot worker 経路のフレームあたり固定コスト計測。

worker を直接起動する (broker の sealed staging は含まない)。broker が加える
hash コストは sha256 計測で近似する。結果は JSON で stdout に出す。

Usage:
    uv run python tools/bench_oneshot_render.py <repository_root> <work_dir>

<work_dir> は入出力 raw の一時置き場 (任意の空きディレクトリ)。事前に
`aex_render_worker.exe` と `pf_sampling_probe.aex` のビルドが必要。
計測値は機材・AV スキャン状態に依存するため、frozen evidence ではなく
調査ノート (docs/RENDER_SESSION_INVESTIGATION_2026-07-19.md) の参考値。
"""
import hashlib
import json
import statistics
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(sys.argv[1]).resolve()
OUT_DIR = Path(sys.argv[2]).resolve()
WORKER = ROOT / "target/minihost-build/aex_render_worker.exe"
AEX = ROOT / "target/pf-sampling-probe-build/Release/pf_sampling_probe.aex"

N = 12


def run_worker(args, timeout=60):
    start = time.perf_counter()
    completed = subprocess.run(
        [str(WORKER), *args], cwd=ROOT, text=True, capture_output=True,
        timeout=timeout,
    )
    return time.perf_counter() - start, completed


def bench_render(width, height, label):
    aex_sha = hashlib.sha256(AEX.read_bytes()).hexdigest()
    input_path = OUT_DIR / f"input-{label}.rgba"
    write_start = time.perf_counter()
    input_path.write_bytes(bytes(width * height * 4))
    input_write_s = time.perf_counter() - write_start
    samples = []
    for index in range(N):
        output_path = OUT_DIR / f"output-{label}-{index}.rgba"
        if output_path.exists():
            output_path.unlink()
        elapsed, completed = run_worker([
            "--render-image", str(AEX), aex_sha, "v5|",
            str(input_path), str(output_path),
            str(width), str(height), "0", "1", "1", "1",
        ])
        if completed.returncode != 0:
            return {"label": label, "error": completed.returncode,
                    "stderr": completed.stderr[-500:]}
        samples.append(elapsed)
        output_path.unlink()
    return {
        "label": label,
        "width": width,
        "height": height,
        "n": N,
        "input_write_ms": round(input_write_s * 1e3, 2),
        "min_ms": round(min(samples) * 1e3, 1),
        "median_ms": round(statistics.median(samples) * 1e3, 1),
        "max_ms": round(max(samples) * 1e3, 1),
    }


def bench_spawn_floor():
    samples = []
    for _ in range(N):
        elapsed, completed = run_worker(["--render-image"], timeout=15)
        assert completed.returncode == 2, completed.returncode
        samples.append(elapsed)
    return {
        "label": "spawn_floor_exit2",
        "n": N,
        "min_ms": round(min(samples) * 1e3, 1),
        "median_ms": round(statistics.median(samples) * 1e3, 1),
        "max_ms": round(max(samples) * 1e3, 1),
    }


def stream_hash(path):
    # admit_local_worker (secure_image_dispatch.rs) と同じく、毎回ディスクから
    # open して 1MiB チャンクで stream しながら hash する。read_bytes 後の
    # CPU-only 計測ではディスクキャッシュ/AV スキャンのコストが抜けるため
    # (Codex PR #101 P2 指摘)、本番 admission と同じ経路を測る。
    hasher = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def bench_hash():
    result = {}
    for name, path in (("worker_exe", WORKER), ("probe_aex", AEX)):
        size = path.stat().st_size
        samples = []
        for _ in range(N):
            start = time.perf_counter()
            stream_hash(path)
            samples.append(time.perf_counter() - start)
        result[name] = {
            "size_bytes": size,
            "note": "open+stream+hash per iteration (mirrors admit_local_worker); "
                    "includes disk/AV overhead, warm cache",
            "n": N,
            "min_ms": round(min(samples) * 1e3, 3),
            "median_ms": round(statistics.median(samples) * 1e3, 3),
            "max_ms": round(max(samples) * 1e3, 3),
        }
    return result


def main():
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    report = {
        "spawn_floor": bench_spawn_floor(),
        "hash": bench_hash(),
        "tiny_37x23": bench_render(37, 23, "tiny"),
        "fhd_1920x1080": bench_render(1920, 1080, "fhd"),
    }
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()

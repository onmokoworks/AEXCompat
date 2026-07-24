"""Benchmark the current session-harness render transport.

The former one-shot worker render verbs were removed in #365. This benchmark
keeps its historical filename for evidence continuity, but enters through
``aexcompat-harness.exe --render-experimental-session`` and emits JSON to
stdout.

Usage:
    uv run python tools/bench_oneshot_render.py <repository_root> <work_dir>
    uv run python tools/bench_oneshot_render.py <repository_root> <work_dir> <harness_exe>

The work directory is a temporary input/output directory. Before running, build
the release session harness and ``pf_sampling_probe.aex``.
"""

from __future__ import annotations

import hashlib
import json
import statistics
import subprocess
import sys
import time
from pathlib import Path

from PIL import Image

N = 12


def run_harness(harness: Path, root: Path, args: list[str], timeout: int = 60):
    start = time.perf_counter()
    completed = subprocess.run(
        [str(harness), *args],
        cwd=root,
        text=True,
        capture_output=True,
        timeout=timeout,
    )
    return time.perf_counter() - start, completed


def session_failure(label: str, completed: subprocess.CompletedProcess[str]) -> dict:
    return {
        "label": label,
        "status": "error",
        "error": completed.returncode,
        "stderr": completed.stderr[-500:],
    }


def bench_render(
    root: Path,
    out_dir: Path,
    harness: Path,
    aex: Path,
    width: int,
    height: int,
    label: str,
) -> dict:
    aex_sha = hashlib.sha256(aex.read_bytes()).hexdigest()
    input_path = out_dir / f"input-{label}.png"
    write_start = time.perf_counter()
    Image.new("RGBA", (width, height), (0, 0, 0, 0)).save(input_path)
    input_write_s = time.perf_counter() - write_start
    samples = []
    for index in range(N):
        output_path = out_dir / f"output-{label}-{index}.png"
        if output_path.exists():
            output_path.unlink()
        elapsed, completed = run_harness(
            harness,
            root,
            [
                "--render-experimental-session",
                str(aex),
                str(input_path),
                str(output_path),
                "argb8",
                "classic",
                "0",
                "1",
                "1",
            ],
        )
        if completed.returncode != 0:
            return session_failure(label, completed)
        try:
            report = json.loads(completed.stdout)
        except json.JSONDecodeError:
            return {
                "label": label,
                "status": "error",
                "error": "session harness returned invalid JSON",
                "stdout": completed.stdout[-500:],
            }
        if report.get("passed") is not True or not output_path.is_file():
            return {
                "label": label,
                "status": "error",
                "error": "session harness did not report a successful PNG render",
                "report": report,
            }
        samples.append(elapsed)
        output_path.unlink()
    return {
        "status": "ok",
        "transport": "session-harness",
        "label": label,
        "width": width,
        "height": height,
        "n": N,
        "input_png_write_ms": round(input_write_s * 1e3, 2),
        "min_ms": round(min(samples) * 1e3, 1),
        "median_ms": round(statistics.median(samples) * 1e3, 1),
        "max_ms": round(max(samples) * 1e3, 1),
    }


def bench_spawn_floor(root: Path, harness: Path) -> dict:
    samples = []
    for _ in range(N):
        elapsed, completed = run_harness(
            harness,
            root,
            ["--print-cli-contract"],
            timeout=15,
        )
        if completed.returncode != 0:
            return session_failure("session_harness_contract_floor", completed)
        samples.append(elapsed)
    return {
        "label": "session_harness_contract_floor",
        "status": "ok",
        "transport": "session-harness",
        "n": N,
        "min_ms": round(min(samples) * 1e3, 1),
        "median_ms": round(statistics.median(samples) * 1e3, 1),
        "max_ms": round(max(samples) * 1e3, 1),
    }


def stream_hash(path: Path) -> str:
    """Hash one file through the same chunked admission shape used at runtime."""

    hasher = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def bench_hash(harness: Path, aex: Path) -> dict:
    result = {}
    for name, path in (("harness_exe", harness), ("probe_aex", aex)):
        size = path.stat().st_size
        samples = []
        for _ in range(N):
            start = time.perf_counter()
            stream_hash(path)
            samples.append(time.perf_counter() - start)
        result[name] = {
            "size_bytes": size,
            "note": "open+stream+hash per iteration (mirrors session admission); includes disk/AV overhead, warm cache",
            "n": N,
            "min_ms": round(min(samples) * 1e3, 3),
            "median_ms": round(statistics.median(samples) * 1e3, 3),
            "max_ms": round(max(samples) * 1e3, 3),
        }
    return result


def main() -> int:
    if len(sys.argv) not in (3, 4):
        print(__doc__, file=sys.stderr)
        return 2
    root = Path(sys.argv[1]).resolve()
    out_dir = Path(sys.argv[2]).resolve()
    harness = (
        Path(sys.argv[3]).resolve()
        if len(sys.argv) == 4
        else root / "broker/target/release/aexcompat-harness.exe"
    )
    aex = root / "target/pf-sampling-probe-build/Release/pf_sampling_probe.aex"
    missing = [str(path) for path in (harness, aex) if not path.is_file()]
    if missing:
        print(
            json.dumps(
                {
                    "status": "blocked",
                    "blocker": {
                        "blocker_id": "benchmark_artifact_missing",
                        "missing": missing,
                        "restart_condition": "build the release session harness and pf_sampling_probe.aex",
                    },
                },
                indent=2,
            )
        )
        return 3
    out_dir.mkdir(parents=True, exist_ok=True)
    report = {
        "transport": "session-harness",
        "spawn_floor": bench_spawn_floor(root, harness),
        "hash": bench_hash(harness, aex),
        "tiny_37x23": bench_render(root, out_dir, harness, aex, 37, 23, "tiny"),
        "fhd_1920x1080": bench_render(root, out_dir, harness, aex, 1920, 1080, "fhd"),
    }
    print(json.dumps(report, indent=2))
    render_results = [
        value
        for key, value in report.items()
        if key not in {"transport", "hash"}
    ]
    return 0 if all(value.get("status") == "ok" for value in render_results) else 1


if __name__ == "__main__":
    raise SystemExit(main())

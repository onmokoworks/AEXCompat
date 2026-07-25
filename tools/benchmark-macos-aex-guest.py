#!/usr/bin/env python3
"""Benchmark the macOS x64 AEX guest worker without guest instruction hooks."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import platform
import statistics
import subprocess
import time
from pathlib import Path


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def percentile_nearest_rank(values: list[float], percentile: float) -> float:
    ordered = sorted(values)
    rank = max(1, math.ceil(percentile * len(ordered)))
    return ordered[rank - 1]


def parse_input(value: str) -> tuple[str, Path]:
    label, separator, path = value.partition("=")
    if not separator or not label or not path:
        raise argparse.ArgumentTypeError("input must be label=/path/to/input.png")
    return label, Path(path)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--worker", type=Path, required=True)
    parser.add_argument("--aex", type=Path, required=True)
    parser.add_argument("--input", action="append", type=parse_input, required=True)
    parser.add_argument("--parameter", action="append", default=[])
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--output-directory", type=Path, required=True)
    args = parser.parse_args()

    if args.runs < 1:
        parser.error("--runs must be positive")
    for path in [args.worker, args.aex, *(path for _, path in args.input)]:
        if not path.is_file():
            parser.error(f"file does not exist: {path}")

    args.output_directory.mkdir(parents=True, exist_ok=True)
    cases = []
    for label, input_path in args.input:
        durations = []
        output_hashes = []
        reports = []
        case_directory = args.output_directory / label
        case_directory.mkdir(parents=True, exist_ok=True)
        for run in range(1, args.runs + 1):
            output_path = case_directory / f"output-{run}.png"
            command = [
                str(args.worker),
                "render-png",
                str(args.aex),
                str(input_path),
                str(output_path),
                *args.parameter,
            ]
            started = time.perf_counter()
            process = subprocess.run(command, capture_output=True, text=True)
            duration = time.perf_counter() - started
            if process.returncode != 0:
                raise SystemExit(
                    f"{label} run {run} exited {process.returncode}: "
                    f"{process.stderr.strip()}"
                )
            report = json.loads(process.stdout)
            if report.get("render_error") != 0:
                raise SystemExit(f"{label} run {run} reported failure: {report}")
            durations.append(duration)
            output_hashes.append(sha256(output_path))
            reports.append(
                {
                    "render_mode": report.get("render_mode"),
                    "width": report.get("width"),
                    "height": report.get("height"),
                    "parameter_values": report.get("parameter_values"),
                }
            )
        if len(set(output_hashes)) != 1:
            raise SystemExit(f"{label} output changed between runs: {output_hashes}")
        cases.append(
            {
                "label": label,
                "input": str(input_path),
                "input_sha256": sha256(input_path),
                "runs": args.runs,
                "wall_seconds": durations,
                "median_seconds": statistics.median(durations),
                "p95_seconds_nearest_rank": percentile_nearest_rank(durations, 0.95),
                "output_sha256": output_hashes[0],
                "report": reports[0],
            }
        )

    result = {
        "schema": "aexcompat.macos-guest-benchmark",
        "version": 1,
        "platform": platform.platform(),
        "machine": platform.machine(),
        "worker": str(args.worker),
        "worker_sha256": sha256(args.worker),
        "aex": str(args.aex),
        "aex_sha256": sha256(args.aex),
        "parameters": args.parameter,
        "cases": cases,
    }
    result_path = args.output_directory / "benchmark.json"
    result_path.write_text(
        json.dumps(result, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print(result_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

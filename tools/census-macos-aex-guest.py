#!/usr/bin/env python3
"""Run paired normal/census renders for the bounded Apple Silicon guest spike."""

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


def run_worker(command: list[str]) -> tuple[float, dict]:
    started = time.perf_counter()
    process = subprocess.run(command, capture_output=True, text=True)
    elapsed = time.perf_counter() - started
    if process.returncode != 0:
        raise SystemExit(
            f"worker exited {process.returncode}: {process.stderr.strip()}"
        )
    report = json.loads(process.stdout)
    if report.get("render_error") != 0:
        raise SystemExit(f"worker reported a render failure: {report}")
    return elapsed, report


def p95_nearest_rank(values: list[float]) -> float:
    ordered = sorted(values)
    return ordered[max(1, math.ceil(0.95 * len(ordered))) - 1]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--worker", required=True, type=Path)
    parser.add_argument("--aex", required=True, type=Path)
    parser.add_argument("--input-small", required=True, type=Path)
    parser.add_argument("--input-large", required=True, type=Path)
    parser.add_argument("--high-parameter", required=True)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--output-directory", required=True, type=Path)
    args = parser.parse_args()

    if args.runs < 1:
        parser.error("--runs must be positive")
    for path in [args.worker, args.aex, args.input_small, args.input_large]:
        if not path.is_file():
            parser.error(f"file does not exist: {path}")

    args.output_directory.mkdir(parents=True, exist_ok=True)
    cases = [
        ("default-small", args.input_small, []),
        ("high-small", args.input_small, [args.high_parameter]),
        ("default-large", args.input_large, []),
        ("high-large", args.input_large, [args.high_parameter]),
    ]
    results = []
    for label, input_path, parameters in cases:
        case_directory = args.output_directory / label
        case_directory.mkdir(parents=True, exist_ok=True)
        normal_times: list[float] = []
        census_times: list[float] = []
        normal_hashes: list[str] = []
        census_hashes: list[str] = []
        census_summaries: list[dict] = []
        first_census_report = None
        for run in range(1, args.runs + 1):
            normal_output = case_directory / f"normal-{run}.png"
            census_output = case_directory / f"census-{run}.png"
            common = [str(args.aex), str(input_path)]
            normal_elapsed, _ = run_worker(
                [
                    str(args.worker),
                    "render-png",
                    *common,
                    str(normal_output),
                    *parameters,
                ]
            )
            census_elapsed, census_report = run_worker(
                [
                    str(args.worker),
                    "census-png",
                    *common,
                    str(census_output),
                    *parameters,
                ]
            )
            normal_times.append(normal_elapsed)
            census_times.append(census_elapsed)
            normal_hashes.append(sha256(normal_output))
            census_hashes.append(sha256(census_output))
            census = census_report["census"]
            census_summaries.append(
                {
                    key: value
                    for key, value in census.items()
                    if key not in {"blocks", "extents"}
                }
            )
            if first_census_report is None:
                first_census_report = census_report

        if len(set(normal_hashes + census_hashes)) != 1:
            raise SystemExit(
                f"{label} normal/census output hashes differ: "
                f"{normal_hashes + census_hashes}"
            )
        encoded_summaries = {
            json.dumps(summary, sort_keys=True) for summary in census_summaries
        }
        if len(encoded_summaries) != 1:
            raise SystemExit(f"{label} census changed between runs")
        assert first_census_report is not None
        (case_directory / "census-report.json").write_text(
            json.dumps(first_census_report, indent=2) + "\n", encoding="utf-8"
        )
        results.append(
            {
                "label": label,
                "input": str(input_path),
                "input_sha256": sha256(input_path),
                "parameters": parameters,
                "runs": args.runs,
                "normal_wall_seconds": normal_times,
                "normal_median_seconds": statistics.median(normal_times),
                "normal_p95_seconds_nearest_rank": p95_nearest_rank(normal_times),
                "census_wall_seconds": census_times,
                "census_median_seconds": statistics.median(census_times),
                "census_p95_seconds_nearest_rank": p95_nearest_rank(census_times),
                "output_sha256": normal_hashes[0],
                "census": census_summaries[0],
            }
        )

    result = {
        "schema": "aexcompat.macos-guest-census",
        "version": 1,
        "platform": platform.platform(),
        "machine": platform.machine(),
        "worker": str(args.worker),
        "worker_sha256": sha256(args.worker),
        "aex": str(args.aex),
        "aex_sha256": sha256(args.aex),
        "high_parameter": args.high_parameter,
        "cases": results,
    }
    result_path = args.output_directory / "census.json"
    result_path.write_text(
        json.dumps(result, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print(result_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

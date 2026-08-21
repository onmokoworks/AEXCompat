#!/usr/bin/env python3
from __future__ import annotations

import argparse
import os
from pathlib import Path
import subprocess
import sys


CLASSIC_FAILURE_EVIDENCE_NODE = (
    "tests/test_classic_failure_stage_evidence.py::"
    "test_refresh_replays_three_classic_failures_before_updating_evidence"
)
SDK_BACKWARDS_BUILD_NODE = (
    "tests/test_sdk_backwards_fixture_build.py::"
    "test_official_sdk_backwards_builds_unchanged_and_records_hash"
)


def pytest_arguments(partition: str, *, sdk_ready: bool) -> list[str]:
    common = ["-q", "-rs"]
    if partition == "classic-evidence":
        if not sdk_ready:
            raise ValueError("classic evidence requires trusted built artifacts")
        return common + [
            CLASSIC_FAILURE_EVIDENCE_NODE,
            "--run-built-artifact-tests",
        ]
    if partition != "main":
        raise ValueError(f"unknown Python CI partition: {partition}")

    arguments = common + [
        "-n",
        "auto",
        "--dist",
        "worksteal",
        "--durations=25",
    ]
    if sdk_ready:
        arguments += [
            "--run-sdk-tests",
            "--run-built-artifact-tests",
            "--deselect",
            CLASSIC_FAILURE_EVIDENCE_NODE,
            "--deselect",
            SDK_BACKWARDS_BUILD_NODE,
        ]
    arguments.append("--validate-local-artifact-manifest")
    return arguments


def run_partition(
    partition: str,
    *,
    sdk_ready: bool,
    output_path: Path | None,
    runner=subprocess.run,
) -> int:
    command = [
        sys.executable,
        "-m",
        "pytest",
        *pytest_arguments(partition, sdk_ready=sdk_ready),
    ]
    completed = runner(command, text=True, capture_output=True)
    output = completed.stdout + completed.stderr
    if output_path is not None:
        output_path.write_text(output, encoding="utf-8")
    sys.stdout.write(output)
    return completed.returncode


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("partition", choices=("main", "classic-evidence"))
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    sdk_ready = os.environ.get("SDK_READY", "false").lower() == "true"
    return run_partition(
        args.partition,
        sdk_ready=sdk_ready,
        output_path=args.output,
    )


if __name__ == "__main__":
    raise SystemExit(main())

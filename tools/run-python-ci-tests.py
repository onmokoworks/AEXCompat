#!/usr/bin/env python3
"""Run one complete, deterministic file-level shard of the Python test suite."""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
TESTS = ROOT / "tests"
SHARD_COUNT = 2


def python_test_files() -> tuple[Path, ...]:
    return tuple(
        sorted(
            TESTS.rglob("test_*.py"),
            key=lambda path: path.relative_to(TESTS).as_posix(),
        )
    )


def test_shards(
    files: tuple[Path, ...], *, shard_count: int = SHARD_COUNT
) -> tuple[tuple[Path, ...], ...]:
    if shard_count < 2:
        raise SystemExit("Python CI requires at least two test shards")
    if not files:
        raise SystemExit("no Python test files were discovered")
    if len(set(files)) != len(files):
        raise SystemExit("duplicate Python test files were discovered")
    files = tuple(sorted(files, key=lambda path: path.as_posix()))

    shards = tuple(
        tuple(path for index, path in enumerate(files) if index % shard_count == shard)
        for shard in range(shard_count)
    )
    selected = [path for shard in shards for path in shard]
    if any(not shard for shard in shards):
        raise SystemExit("Python test shard is empty")
    if len(selected) != len(set(selected)) or set(selected) != set(files):
        raise SystemExit("Python test shards are not complete and disjoint")
    return shards


def run_shard(shard: int, pytest_args: list[str]) -> None:
    shards = test_shards(python_test_files())
    if shard < 0 or shard >= len(shards):
        raise SystemExit(f"invalid Python test shard: {shard}")
    relative_files = [str(path.relative_to(ROOT)) for path in shards[shard]]
    command = [sys.executable, "-m", "pytest", *pytest_args, *relative_files]
    print(
        f"python_test_shard={shard}/{len(shards)} files={len(relative_files)}",
        flush=True,
    )
    print("+", subprocess.list2cmdline(command), flush=True)
    raise SystemExit(subprocess.run(command, cwd=ROOT).returncode)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("shard", type=int)
    parser.add_argument("pytest_args", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    run_shard(args.shard, args.pytest_args)


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Run approved local AEX fixtures through inspect and redact missing-suite data."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import time
from collections import defaultdict
from pathlib import Path


SCHEMA = "aexcompat.harness-diagnostic"
SUITE_RE = re.compile(r"^[A-Za-z0-9 ._-]{1,128}$")
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
MARKER = "diagnostics="


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def approved_fixtures(root: Path) -> list[Path]:
    root = root.resolve(strict=True)
    fixtures = []
    for candidate in root.rglob("*.aex"):
        resolved = candidate.resolve(strict=True)
        if resolved == root or root not in resolved.parents:
            raise ValueError("fixture escaped the approved root")
        if candidate.is_symlink() or not candidate.is_file():
            raise ValueError("fixture must be a plain file")
        fixtures.append(resolved)
    return sorted(fixtures, key=lambda item: item.relative_to(root).as_posix().lower())


def json_after_marker(text: str) -> dict:
    start = text.find(MARKER)
    if start < 0:
        return {}
    decoder = json.JSONDecoder()
    try:
        value, _ = decoder.raw_decode(text[start + len(MARKER) :].lstrip())
    except (json.JSONDecodeError, TypeError):
        return {}
    return value if isinstance(value, dict) else {}


def missing_suites(stderr: str) -> list[dict]:
    diagnostics = json_after_marker(stderr)
    output = []
    seen = set()
    for suite in diagnostics.get("missing_suites", [])[:16]:
        if not isinstance(suite, dict):
            continue
        name, version = suite.get("name"), suite.get("version")
        key = (name, version)
        if (
            isinstance(name, str)
            and SUITE_RE.fullmatch(name)
            and isinstance(version, int)
            and not isinstance(version, bool)
            and version > 0
            and key not in seen
        ):
            seen.add(key)
            output.append({"name": name, "version": version})
    return output


def persist_event(repository: Path, sha: str, size: int, suites: list[dict], nonce: str) -> Path:
    if not SHA_RE.fullmatch(sha):
        raise ValueError("invalid SHA-256")
    directory = repository / "target" / "harness-diagnostics" / sha
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / f"inspect-{nonce}.local.json"
    event = {
        "schema": SCHEMA,
        "version": 1,
        "timestamp": int(time.time() * 1000),
        "success": False,
        "operation": "inspect_experimental",
        "summary": "classification=inspect_failed; stage=unknown; exit_code=1",
        "identity": {"sha256": sha, "size": size},
        "diagnostics": {"missing_suites": suites},
    }
    with path.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(event, stream, indent=2, sort_keys=True)
        stream.write("\n")
    return path


def aggregate(events: list[tuple[str, list[dict]]]) -> list[dict]:
    sha_sets: dict[tuple[str, int], set[str]] = defaultdict(set)
    event_counts: dict[tuple[str, int], int] = defaultdict(int)
    for sha, suites in events:
        for suite in suites:
            key = (suite["name"], suite["version"])
            sha_sets[key].add(sha)
            event_counts[key] += 1
    rows = [
        {"name": key[0], "version": key[1], "sha_count": len(shas), "event_count": event_counts[key]}
        for key, shas in sha_sets.items()
    ]
    rows.sort(key=lambda row: (-row["sha_count"], -row["event_count"], row["name"], row["version"]))
    return rows[:10]


def run(repository: Path, fixture_root: Path, harness: Path) -> dict:
    fixtures = approved_fixtures(fixture_root)
    cases = []
    failures = []
    nonce_base = str(int(time.time() * 1000))
    for index, fixture in enumerate(fixtures):
        sha = sha256_file(fixture)
        process = subprocess.run(
            [str(harness), "--inspect-experimental", str(fixture)],
            cwd=repository,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=30,
            check=False,
        )
        suites = missing_suites(process.stderr)
        if process.returncode != 0:
            persist_event(repository, sha, fixture.stat().st_size, suites, f"{nonce_base}-{index:03d}")
            failures.append((sha, suites))
        cases.append({
            "fixture": fixture.stem,
            "sha256": sha,
            "inspect_succeeded": process.returncode == 0,
            "missing_suite_count": len(suites),
        })
    top = aggregate(failures)
    return {
        "schema_version": 1,
        "stage": "aex_missing_suite_diagnostic_summary",
        "fixture_count": len(cases),
        "inspect_success_count": sum(case["inspect_succeeded"] for case in cases),
        "inspect_failure_count": sum(not case["inspect_succeeded"] for case in cases),
        "missing_suite_sha_count": len({sha for sha, suites in failures if suites}),
        "top_missing_suites": top,
        "next_unimplemented_suite_candidates": top,
        "candidate_ranking_state": (
            "ranked_from_observed_missing_suites" if top else "no_missing_suite_observed_no_candidate_ranked"
        ),
        "cases": cases,
        "privacy": {"local_paths_exported": False, "private_stderr_exported": False},
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repository", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--fixture-root", type=Path)
    parser.add_argument("--harness", type=Path)
    parser.add_argument("--summary-out", type=Path, required=True)
    args = parser.parse_args()
    repository = args.repository.resolve(strict=True)
    fixture_root = (args.fixture_root or repository / "target" / "sdk-fixtures").resolve(strict=True)
    harness = (args.harness or repository / "broker" / "target" / "release" / "aexcompat-harness.exe").resolve(strict=True)
    report = run(repository, fixture_root, harness)
    args.summary_out.parent.mkdir(parents=True, exist_ok=True)
    with args.summary_out.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(report, stream, indent=2, sort_keys=True)
        stream.write("\n")
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

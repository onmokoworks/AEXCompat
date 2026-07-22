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
WORKER_FAILURE_MARKER = "AEX parameter inspection worker failed safely:"
MAX_UNSUPPORTED_SUITE_CALLS = 32
MAX_UNSUPPORTED_SUITE_SLOT = 1023


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
    diagnostics = worker_failure(stderr)
    if not diagnostics:
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


def unsupported_suite_calls(stderr: str) -> list[dict]:
    diagnostics = worker_failure(stderr)
    if not diagnostics:
        diagnostics = json_after_marker(stderr)
    output = []
    seen = set()
    raw_calls = diagnostics.get("unsupported_suite_calls")
    if not isinstance(raw_calls, list):
        return output
    for call in raw_calls[:MAX_UNSUPPORTED_SUITE_CALLS]:
        if not isinstance(call, dict):
            continue
        name = call.get("name")
        version = call.get("version")
        slot = call.get("slot")
        call_count = call.get("call_count")
        key = (name, version, slot)
        if (
            isinstance(name, str)
            and SUITE_RE.fullmatch(name)
            and isinstance(version, int)
            and not isinstance(version, bool)
            and version > 0
            and isinstance(slot, int)
            and not isinstance(slot, bool)
            and 0 <= slot <= MAX_UNSUPPORTED_SUITE_SLOT
            and isinstance(call_count, int)
            and not isinstance(call_count, bool)
            and 0 < call_count <= 0xFFFFFFFF
            and key not in seen
        ):
            seen.add(key)
            output.append({
                "name": name,
                "version": version,
                "slot": slot,
                "call_count": call_count,
            })
    return output


def worker_failure(stderr: str) -> dict:
    start = stderr.find(WORKER_FAILURE_MARKER)
    if start < 0:
        return {}
    decoder = json.JSONDecoder()
    try:
        value, _ = decoder.raw_decode(stderr[start + len(WORKER_FAILURE_MARKER) :].lstrip())
    except (json.JSONDecodeError, TypeError):
        return {}
    return value if isinstance(value, dict) else {}


def classify_failure(stderr: str, process_exit: int) -> dict:
    diagnostics = worker_failure(stderr)
    worker_exit = diagnostics.get("exit_code")
    failure_stage = diagnostics.get("first_failure_stage", diagnostics.get("failure_stage"))
    events = diagnostics.get("stage_events")
    if not isinstance(worker_exit, int) or isinstance(worker_exit, bool):
        worker_exit = None
    if not isinstance(failure_stage, str) or failure_stage not in {
        "global_setup", "params_setup", "global_setdown"
    }:
        failure_stage = None
    selector_error = None
    if isinstance(events, list):
        for event in events[:16]:
            if not isinstance(event, dict) or event.get("state") != "end":
                continue
            errors = event.get("errors")
            error = errors.get("error") if isinstance(errors, dict) else None
            if isinstance(error, int) and not isinstance(error, bool) and error != 0 and error != -1:
                selector_error = error
                break
    plugin_kind = diagnostics.get("plugin_kind")
    if plugin_kind not in {"aegp_candidate", "invalid_pipl", "unknown_no_effect_entrypoint"}:
        plugin_kind = None
    if worker_exit == 12 and plugin_kind == "aegp_candidate" and not events:
        kind = "unsupported_plugin_kind_for_pf_inspect"
    elif worker_exit == 12 and plugin_kind == "invalid_pipl" and not events:
        kind = "invalid_pipl_for_pf_inspect"
    elif worker_exit == 12 and not events:
        kind = "effect_entrypoint_missing"
    elif missing_suites(stderr):
        kind = "missing_suite"
    elif unsupported_suite_calls(stderr):
        kind = "unsupported_suite_call"
    elif selector_error is not None:
        kind = "selector_error"
    else:
        kind = "worker_failure"
    return {
        "kind": kind,
        "process_exit_code": process_exit,
        "worker_exit_code": worker_exit,
        "failure_stage": failure_stage,
        "selector_error": selector_error,
        "plugin_kind": plugin_kind,
    }


def persist_event(
    repository: Path,
    sha: str,
    size: int,
    suites: list[dict],
    failure: dict,
    nonce: str,
    unsupported_calls: list[dict] | None = None,
) -> Path:
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
        "summary": (
            f"classification={failure['kind']}; stage={failure['failure_stage'] or 'none'}; "
            f"exit_code={failure['worker_exit_code'] if failure['worker_exit_code'] is not None else failure['process_exit_code']}"
        ),
        "identity": {"sha256": sha, "size": size},
        "diagnostics": {
            "missing_suites": suites,
            "unsupported_suite_calls": unsupported_calls or [],
            **failure,
        },
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


def aggregate_unsupported_calls(events: list[tuple[str, list[dict]]]) -> list[dict]:
    sha_sets: dict[tuple[str, int, int], set[str]] = defaultdict(set)
    event_counts: dict[tuple[str, int, int], int] = defaultdict(int)
    call_counts: dict[tuple[str, int, int], int] = defaultdict(int)
    for sha, calls in events:
        for call in calls:
            key = (call["name"], call["version"], call["slot"])
            sha_sets[key].add(sha)
            event_counts[key] += 1
            call_counts[key] += call["call_count"]
    rows = [
        {
            "name": key[0],
            "version": key[1],
            "slot": key[2],
            "sha_count": len(shas),
            "event_count": event_counts[key],
            "call_count": call_counts[key],
        }
        for key, shas in sha_sets.items()
    ]
    rows.sort(key=lambda row: (
        -row["sha_count"],
        -row["event_count"],
        -row["call_count"],
        row["name"],
        row["version"],
        row["slot"],
    ))
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
        unsupported_calls = unsupported_suite_calls(process.stderr)
        failure = classify_failure(process.stderr, process.returncode)
        if process.returncode != 0:
            persist_event(
                repository,
                sha,
                fixture.stat().st_size,
                suites,
                failure,
                f"{nonce_base}-{index:03d}",
                unsupported_calls,
            )
            failures.append((sha, suites, unsupported_calls))
        cases.append({
            "fixture": fixture.stem,
            "sha256": sha,
            "inspect_succeeded": process.returncode == 0,
            "missing_suite_count": len(suites),
            "unsupported_suite_call_count": len(unsupported_calls),
            "failure": failure if process.returncode != 0 else None,
        })
    top = aggregate([(sha, suites) for sha, suites, _ in failures])
    top_slots = aggregate_unsupported_calls(
        [(sha, calls) for sha, _, calls in failures]
    )
    out_of_scope_count = sum(
        (case.get("failure") or {}).get("kind") == "unsupported_plugin_kind_for_pf_inspect"
        for case in cases
    )
    return {
        "schema_version": 1,
        "stage": "aex_missing_suite_diagnostic_summary",
        "fixture_count": len(cases),
        "inspect_success_count": sum(case["inspect_succeeded"] for case in cases),
        "inspect_failure_count": sum(not case["inspect_succeeded"] for case in cases),
        "effect_fixture_count": len(cases) - out_of_scope_count,
        "effect_inspect_failure_count": sum(
            not case["inspect_succeeded"]
            and (case.get("failure") or {}).get("kind") != "unsupported_plugin_kind_for_pf_inspect"
            for case in cases
        ),
        "out_of_scope_plugin_count": out_of_scope_count,
        "missing_suite_sha_count": len({sha for sha, suites, _ in failures if suites}),
        "unsupported_suite_call_sha_count": len(
            {sha for sha, _, calls in failures if calls}
        ),
        "top_missing_suites": top,
        "next_unimplemented_suite_candidates": top,
        "candidate_ranking_state": (
            "ranked_from_observed_missing_suites" if top else "no_missing_suite_observed_no_candidate_ranked"
        ),
        "top_unsupported_suite_calls": top_slots,
        "next_unimplemented_slot_candidates": top_slots,
        "slot_candidate_ranking_state": (
            "ranked_from_observed_unsupported_suite_calls"
            if top_slots
            else "no_unsupported_suite_call_observed_no_slot_candidate_ranked"
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

#!/usr/bin/env python3
"""Delete Actions caches that belong exclusively to closed pull requests."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
from dataclasses import dataclass
from typing import Callable, Sequence
from urllib.parse import quote


REPOSITORY_RE = re.compile(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+\Z")
PULL_REF_RE = re.compile(r"refs/pull/([1-9][0-9]*)/merge\Z")
MAX_CACHE_PAGES = 100


class CleanupError(RuntimeError):
    """The cleanup could not prove that a requested operation was safe."""


RunCommand = Callable[[Sequence[str]], str]


def run_gh(arguments: Sequence[str]) -> str:
    completed = subprocess.run(
        ["gh", *arguments],
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    return completed.stdout


@dataclass(frozen=True)
class CacheRecord:
    cache_id: int
    ref: str
    size_in_bytes: int


class GitHubApi:
    def __init__(self, repository: str, run: RunCommand = run_gh) -> None:
        if not REPOSITORY_RE.fullmatch(repository):
            raise CleanupError(f"invalid repository: {repository!r}")
        self.repository = repository
        self.run = run

    def list_caches(self, ref: str | None = None) -> list[CacheRecord]:
        endpoint = f"repos/{self.repository}/actions/caches?per_page=100"
        if ref is not None:
            endpoint += f"&ref={quote(ref, safe='')}"
        raw = self.run(["api", "--paginate", "--slurp", endpoint])
        try:
            pages = json.loads(raw)
        except json.JSONDecodeError as error:
            raise CleanupError("cache inventory was not JSON") from error
        if not isinstance(pages, list) or len(pages) > MAX_CACHE_PAGES:
            raise CleanupError("cache inventory exceeded its bounded page contract")

        records: list[CacheRecord] = []
        seen_payloads: dict[int, dict[str, object]] = {}
        for page in pages:
            if not isinstance(page, dict) or not isinstance(
                page.get("actions_caches"), list
            ):
                raise CleanupError("cache inventory page has an invalid shape")
            for value in page["actions_caches"]:
                if not isinstance(value, dict):
                    raise CleanupError("cache inventory record has an invalid shape")
                cache_id = value.get("id")
                cache_ref = value.get("ref")
                size = value.get("size_in_bytes")
                if (
                    not isinstance(cache_id, int)
                    or cache_id <= 0
                    or not isinstance(cache_ref, str)
                    or not isinstance(size, int)
                    or size < 0
                ):
                    raise CleanupError("cache inventory record is invalid")
                if ref is not None and cache_ref != ref:
                    raise CleanupError(
                        "ref-filtered cache inventory returned another ref"
                    )
                record = CacheRecord(cache_id, cache_ref, size)
                previous = seen_payloads.get(cache_id)
                if previous is not None:
                    if previous != value:
                        raise CleanupError(
                            "cache inventory duplicated an id with conflicting data"
                        )
                    continue
                seen_payloads[cache_id] = value
                records.append(record)
        return records

    def pull_state(self, number: int) -> str:
        raw = self.run(["api", f"repos/{self.repository}/pulls/{number}"])
        try:
            value = json.loads(raw)
        except json.JSONDecodeError as error:
            raise CleanupError(
                f"pull request #{number} response was not JSON"
            ) from error
        state = value.get("state") if isinstance(value, dict) else None
        if state not in {"open", "closed"}:
            raise CleanupError(f"pull request #{number} has an invalid state")
        return state

    def delete_cache(self, cache_id: int) -> None:
        if not isinstance(cache_id, int) or cache_id <= 0:
            raise CleanupError(f"invalid cache id: {cache_id!r}")
        endpoint = f"repos/{self.repository}/actions/caches/{cache_id}"
        self.run(["api", "--method", "DELETE", endpoint])


@dataclass(frozen=True)
class CleanupResult:
    deleted_refs: tuple[str, ...]
    deleted_entries: int
    deleted_bytes: int
    open_refs: tuple[str, ...]


def cleanup_closed_pull_caches(
    api: GitHubApi, requested_pull: int | None = None
) -> CleanupResult:
    if requested_pull is not None:
        if requested_pull <= 0:
            raise CleanupError("pull request number must be positive")
        candidate_refs = [f"refs/pull/{requested_pull}/merge"]
    else:
        candidate_refs = sorted(
            {
                record.ref
                for record in api.list_caches()
                if PULL_REF_RE.fullmatch(record.ref) is not None
            },
            key=lambda ref: int(PULL_REF_RE.fullmatch(ref).group(1)),  # type: ignore[union-attr]
        )

    deleted_refs: list[str] = []
    open_refs: list[str] = []
    deleted_entries = 0
    deleted_bytes = 0
    for ref in candidate_refs:
        match = PULL_REF_RE.fullmatch(ref)
        if match is None:
            raise CleanupError("internal candidate ref validation failed")
        number = int(match.group(1))
        if api.pull_state(number) == "open":
            open_refs.append(ref)
            continue
        records = api.list_caches(ref)
        if not records:
            continue
        # Inventory may take several pages. Close the most material reopen
        # window by re-reading state immediately before the first mutation.
        if api.pull_state(number) == "open":
            open_refs.append(ref)
            continue
        for record in records:
            api.delete_cache(record.cache_id)
        if api.list_caches(ref):
            raise CleanupError(f"cache deletion did not empty {ref}")
        deleted_refs.append(ref)
        deleted_entries += len(records)
        deleted_bytes += sum(record.size_in_bytes for record in records)

    return CleanupResult(
        tuple(deleted_refs), deleted_entries, deleted_bytes, tuple(open_refs)
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repository", required=True)
    parser.add_argument("--pull-request", type=int)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    result = cleanup_closed_pull_caches(
        GitHubApi(args.repository), requested_pull=args.pull_request
    )
    print(
        json.dumps(
            {
                "deleted_refs": list(result.deleted_refs),
                "deleted_entries": result.deleted_entries,
                "deleted_bytes": result.deleted_bytes,
                "open_refs": list(result.open_refs),
            },
            separators=(",", ":"),
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

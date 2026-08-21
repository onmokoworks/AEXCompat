from __future__ import annotations

import importlib.util
import json
import sys
from pathlib import Path

import pytest


SCRIPT = Path(__file__).parents[1] / "tools" / "cleanup-closed-pr-caches.py"
SPEC = importlib.util.spec_from_file_location("cleanup_closed_pr_caches", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
cleanup = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = cleanup
SPEC.loader.exec_module(cleanup)


class FakeApi:
    def __init__(self) -> None:
        self.records = [
            cleanup.CacheRecord(1, "refs/heads/main", 50),
            cleanup.CacheRecord(2, "refs/pull/7/merge", 70),
            cleanup.CacheRecord(3, "refs/pull/7/merge", 30),
            cleanup.CacheRecord(4, "refs/pull/8/merge", 80),
            cleanup.CacheRecord(5, "refs/pull/not-a-number/merge", 90),
        ]
        self.states = {7: "closed", 8: "open"}
        self.deleted: list[int] = []

    def list_caches(self, ref: str | None = None):
        return [record for record in self.records if ref is None or record.ref == ref]

    def pull_state(self, number: int) -> str:
        return self.states[number]

    def delete_cache(self, cache_id: int) -> None:
        self.deleted.append(cache_id)
        self.records = [
            record for record in self.records if record.cache_id != cache_id
        ]


def test_reconciliation_deletes_only_closed_pull_request_refs() -> None:
    api = FakeApi()
    result = cleanup.cleanup_closed_pull_caches(api)

    assert result == cleanup.CleanupResult(
        deleted_refs=("refs/pull/7/merge",),
        deleted_entries=2,
        deleted_bytes=100,
        open_refs=("refs/pull/8/merge",),
    )
    assert api.deleted == [2, 3]
    assert {record.ref for record in api.records} == {
        "refs/heads/main",
        "refs/pull/8/merge",
        "refs/pull/not-a-number/merge",
    }


def test_close_event_still_confirms_pr_state_and_exact_ref() -> None:
    api = FakeApi()
    result = cleanup.cleanup_closed_pull_caches(api, requested_pull=7)
    assert result.deleted_refs == ("refs/pull/7/merge",)
    assert api.deleted == [2, 3]

    api = FakeApi()
    result = cleanup.cleanup_closed_pull_caches(api, requested_pull=8)
    assert result.deleted_refs == ()
    assert result.open_refs == ("refs/pull/8/merge",)
    assert api.deleted == []


def test_inventory_parser_is_paginated_bounded_and_ref_exact() -> None:
    pages = [
        {
            "total_count": 2,
            "actions_caches": [
                {"id": 11, "ref": "refs/pull/9/merge", "size_in_bytes": 10}
            ],
        },
        {
            "total_count": 2,
            "actions_caches": [
                {"id": 12, "ref": "refs/pull/9/merge", "size_in_bytes": 20}
            ],
        },
    ]
    calls: list[list[str]] = []

    def run(arguments):
        calls.append(list(arguments))
        return json.dumps(pages)

    api = cleanup.GitHubApi("owner/repo", run=run)
    assert [record.cache_id for record in api.list_caches("refs/pull/9/merge")] == [
        11,
        12,
    ]
    assert calls == [
        [
            "api",
            "--paginate",
            "--slurp",
            "repos/owner/repo/actions/caches?per_page=100&ref=refs%2Fpull%2F9%2Fmerge",
        ]
    ]

    pages[1]["actions_caches"][0]["ref"] = "refs/heads/main"
    with pytest.raises(cleanup.CleanupError, match="another ref"):
        api.list_caches("refs/pull/9/merge")


def test_api_failures_and_incomplete_deletion_do_not_become_success() -> None:
    class IncompleteDelete(FakeApi):
        def delete_cache(self, cache_id: int) -> None:
            self.deleted.append(cache_id)

    with pytest.raises(cleanup.CleanupError, match="did not empty"):
        cleanup.cleanup_closed_pull_caches(IncompleteDelete(), requested_pull=7)

    with pytest.raises(cleanup.CleanupError, match="invalid state"):
        # Exercise the production state validator rather than trusting a fake's value.
        cleanup.GitHubApi("owner/repo", run=lambda _: '{"state":"unknown"}').pull_state(
            7
        )

    with pytest.raises(cleanup.CleanupError, match="invalid repository"):
        cleanup.GitHubApi("owner/repo/extra")


def test_reopened_pull_request_is_rejected_before_any_delete() -> None:
    class Reopened(FakeApi):
        def __init__(self) -> None:
            super().__init__()
            self.states_seen = 0

        def pull_state(self, number: int) -> str:
            if number != 7:
                return super().pull_state(number)
            self.states_seen += 1
            return "closed" if self.states_seen == 1 else "open"

    api = Reopened()
    result = cleanup.cleanup_closed_pull_caches(api, requested_pull=7)
    assert result.deleted_refs == ()
    assert result.open_refs == ("refs/pull/7/merge",)
    assert api.deleted == []
    assert len(api.list_caches("refs/pull/7/merge")) == 2


def test_production_delete_uses_the_exact_cache_id_endpoint() -> None:
    calls: list[list[str]] = []

    def run(arguments):
        calls.append(list(arguments))
        return ""

    api = cleanup.GitHubApi("owner/repo", run=run)
    api.delete_cache(12345)
    assert calls == [
        ["api", "--method", "DELETE", "repos/owner/repo/actions/caches/12345"]
    ]

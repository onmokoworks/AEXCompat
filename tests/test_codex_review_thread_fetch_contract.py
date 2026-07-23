from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPTS = (
    ROOT / ".claude" / "skills" / "codex-review-loop" / "codex-merge-guard.sh",
    ROOT / ".claude" / "skills" / "codex-review-loop" / "codex-review-monitor.sh",
)


def fetch_function(path: Path) -> str:
    source = path.read_text(encoding="utf-8")
    start = source.index("fetch_review_threads()")
    end = source.index("\n}\n", start) + 2
    return source[start:end]


def test_both_review_loops_batch_nested_comments_with_thread_pages():
    for path in SCRIPTS:
        function = fetch_function(path)
        assert "reviewThreads(first:100,after:$endCursor)" in function
        assert "comments(first:100)" in function
        assert "pageInfo{hasNextPage}" in function
        assert "truncated:" in function
        assert "for encoded in" not in function
        assert 'gh api graphql --paginate -F id="$id"' not in function


def test_both_review_loops_emit_the_same_normalized_thread_shape():
    merge_guard = fetch_function(SCRIPTS[0])
    monitor = fetch_function(SCRIPTS[1])
    for field in (
        "isResolved",
        "truncated:",
        "comments:",
        "id:.databaseId",
        "created_at:.createdAt",
        "in_reply_to_id:(.replyTo.databaseId // null)",
    ):
        assert field in merge_guard
        assert field in monitor

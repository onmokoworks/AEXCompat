"""Tests for the codex-review-loop selection predicates.

These exercise the ACTUAL shell functions in
`.claude/skills/codex-review-loop/codex-review-lib.sh` (sourced per call), not a
copy of the jq strings, so changing the skill's logic without updating behavior
fails here. Requires bash + jq; skips cleanly when either is missing.
"""

import json
import shutil
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
LIB = ROOT / ".claude" / "skills" / "codex-review-loop" / "codex-review-lib.sh"


def _call(func: str, payload: list, *args: str) -> str:
    bash = shutil.which("bash")
    jq = shutil.which("jq")
    if bash is None or jq is None:
        pytest.skip("bash and jq are required")
    if not LIB.is_file():
        pytest.skip(f"{LIB} not found")
    quoted_args = " ".join(f'"{a}"' for a in args)
    script = f'. "{LIB.as_posix()}"; {func} {quoted_args}'
    result = subprocess.run(
        [bash, "-c", script],
        input=json.dumps(payload).encode("utf-8"),
        capture_output=True,
        timeout=30,
    )
    assert result.returncode == 0, result.stderr.decode("utf-8", "replace")
    return result.stdout.decode("utf-8").strip()


# --- codex_clean_for_head: clean must be bound to the current head SHA --------

def test_clean_requires_matching_head_sha() -> None:
    head = "6de58c3dbb095c277dca598cb5621d28cbed723a"
    payload = [
        {"user": {"login": "chatgpt-codex-connector[bot]"},
         "body": f"Codex Review: Didn't find any major issues.\nReviewed commit: `{head[:10]}`"}
    ]
    assert _call("codex_clean_for_head", payload, head) == "CLEAN"


def test_clean_for_a_stale_sha_is_rejected() -> None:
    payload = [
        {"user": {"login": "chatgpt-codex-connector[bot]"},
         "body": "Didn't find any major issues.\nReviewed commit: `31bef1dfc1`"}
    ]
    # Current head differs from the reviewed (stale) commit.
    assert _call("codex_clean_for_head", payload, "6de58c3dbb095c277dca598cb5621d28cbed723a") == ""


@pytest.mark.parametrize("login", ["naari3", "some-bot[bot]", "chatgpt-codex-connector-fake"])
def test_clean_only_from_exact_codex_identities(login: str) -> None:
    head = "6de58c3dbb"
    payload = [{"user": {"login": login}, "body": f"Didn't find any major issues {head}"}]
    assert _call("codex_clean_for_head", payload, head) == ""


# --- codex_reaction_clean_ts: +1 on the PR body is a clean signal -------------

def test_reaction_clean_when_bot_plus_one_covers_head() -> None:
    # Auto first-review with no findings: Codex signals clean only via a +1 on
    # the PR body (PR #44). The reaction is at/after the head commit date.
    head_date = "2026-07-18T15:00:00Z"
    payload = [{"user": {"login": "chatgpt-codex-connector[bot]"}, "content": "+1",
                "created_at": "2026-07-18T15:30:20Z"}]
    assert _call("codex_reaction_clean_ts", payload, head_date) == "2026-07-18T15:30:20Z"


def test_reaction_clean_is_stale_when_a_commit_landed_after_it() -> None:
    # A commit pushed after Codex reacted moves the head commit date past the
    # +1, so the reaction no longer covers the head.
    head_date = "2026-07-18T16:00:00Z"
    payload = [{"user": {"login": "chatgpt-codex-connector[bot]"}, "content": "+1",
                "created_at": "2026-07-18T15:30:20Z"}]
    assert _call("codex_reaction_clean_ts", payload, head_date) == ""


@pytest.mark.parametrize("login", ["naari3", "onmokoworks", "someone"])
def test_reaction_clean_only_from_codex_bot(login: str) -> None:
    # A human thumbs-up is not a Codex verdict.
    head_date = "2026-07-18T15:00:00Z"
    payload = [{"user": {"login": login}, "content": "+1", "created_at": "2026-07-18T15:30:20Z"}]
    assert _call("codex_reaction_clean_ts", payload, head_date) == ""


def test_reaction_clean_ignores_non_plus_one_reactions() -> None:
    # 👀 (eyes) is an ack, not clean.
    head_date = "2026-07-18T15:00:00Z"
    payload = [{"user": {"login": "chatgpt-codex-connector[bot]"}, "content": "eyes",
                "created_at": "2026-07-18T15:30:20Z"}]
    assert _call("codex_reaction_clean_ts", payload, head_date) == ""


# --- codex_error: error/onboarding messages mean the review did not run -------

@pytest.mark.parametrize(
    "body",
    ["Codex Review: Something went wrong. Try again later.",
     "Codex Review: Unknown error.",
     "To use Codex here, create a Codex account and connect to github."],
)
def test_codex_error_is_detected(body: str) -> None:
    payload = [{"user": {"login": "chatgpt-codex-connector[bot]"}, "body": body}]
    assert _call("codex_error", payload) == "CODEX-ERROR"


def test_codex_clean_is_not_an_error() -> None:
    payload = [{"user": {"login": "chatgpt-codex-connector[bot]"}, "body": "Didn't find any major issues"}]
    assert _call("codex_error", payload) == ""


# --- owner_blocking_reviews: state-based gate, incl. bodyless CHANGES_REQUESTED

def test_bodyless_changes_requested_blocks() -> None:
    payload = [{"user": {"login": "onmokoworks"}, "state": "CHANGES_REQUESTED", "body": ""}]
    assert "OWNER-REVIEW CHANGES_REQUESTED" in _call("owner_blocking_reviews", payload)


def test_approved_review_does_not_block() -> None:
    payload = [{"user": {"login": "onmokoworks"}, "state": "APPROVED", "body": "looks good"}]
    assert _call("owner_blocking_reviews", payload) == ""


def test_bodied_commented_review_blocks() -> None:
    payload = [{"user": {"login": "naari3"}, "state": "COMMENTED", "body": "[P1] fix this"}]
    assert "OWNER-REVIEW COMMENTED" in _call("owner_blocking_reviews", payload)


# --- owner_comments: exclude only a bare @codex review trigger ----------------

@pytest.mark.parametrize("body", ["@codex review", "  @codex review  ", "@Codex Review"])
def test_bare_trigger_comment_is_excluded(body: str) -> None:
    payload = [{"user": {"login": "naari3"}, "body": body}]
    assert _call("owner_comments", payload) == ""


@pytest.mark.parametrize("body", ["fix X, then @codex review again", "[P1] これ直して"])
def test_owner_feedback_is_kept_even_with_trigger_phrase(body: str) -> None:
    payload = [{"user": {"login": "onmokoworks"}, "body": body}]
    assert _call("owner_comments", payload) == f"OWNER-COMMENT: {body}"


# --- owner_review_gate: latest review state, not history ----------------------

def test_latest_approved_dismisses_earlier_changes_requested() -> None:
    payload = [
        {"user": {"login": "onmokoworks"}, "state": "CHANGES_REQUESTED", "submitted_at": "2026-07-18T19:19:00Z"},
        {"user": {"login": "onmokoworks"}, "state": "APPROVED", "submitted_at": "2026-07-18T19:40:00Z"},
    ]
    assert _call("owner_review_gate", payload) == ""


def test_latest_changes_requested_blocks() -> None:
    payload = [
        {"user": {"login": "onmokoworks"}, "state": "APPROVED", "submitted_at": "2026-07-18T18:35:00Z"},
        {"user": {"login": "onmokoworks"}, "state": "CHANGES_REQUESTED", "submitted_at": "2026-07-18T19:19:00Z"},
    ]
    assert _call("owner_review_gate", payload) == "BLOCK"


# --- latest-verdict timestamps ------------------------------------------------

def test_clean_ts_is_the_newest_head_bound_clean() -> None:
    head = "6de58c3dbb095c277dca598cb5621d28cbed723a"
    payload = [
        {"user": {"login": "chatgpt-codex-connector[bot]"},
         "body": "Didn't find any major issues `6de58c3dbb`", "created_at": "2026-07-18T19:16:44Z"},
        {"user": {"login": "chatgpt-codex-connector[bot]"},
         "body": "Didn't find any major issues `31bef1dfc1`", "created_at": "2026-07-18T18:37:23Z"},
    ]
    assert _call("codex_clean_ts_for_head", payload, head) == "2026-07-18T19:16:44Z"


def test_finding_newer_than_clean_supersedes() -> None:
    # A finding after the clean has a later timestamp; the script compares them.
    clean = [{"user": {"login": "chatgpt-codex-connector[bot]"},
              "body": "Didn't find any major issues `6de58c3dbb`", "created_at": "2026-07-18T19:16:44Z"}]
    findings = [{"user": {"login": "chatgpt-codex-connector[bot]"}, "created_at": "2026-07-18T19:26:00Z"}]
    clean_ts = _call("codex_clean_ts_for_head", clean, "6de58c3dbb095c277dca598cb5621d28cbed723a")
    find_ts = _call("codex_finding_max_ts", findings)
    assert find_ts > clean_ts


def test_commented_review_does_not_clear_changes_requested() -> None:
    # Replying to an inline comment posts a bodyless COMMENTED review as the
    # owner; it must NOT dismiss an earlier CHANGES_REQUESTED.
    payload = [
        {"user": {"login": "onmokoworks"}, "state": "CHANGES_REQUESTED", "submitted_at": "2026-07-18T19:19:00Z"},
        {"user": {"login": "naari3"}, "state": "COMMENTED", "submitted_at": "2026-07-18T19:29:00Z"},
    ]
    assert _call("owner_review_gate", payload) == "BLOCK"


def test_one_owner_approved_does_not_clear_another_owners_changes_requested() -> None:
    # Per-reviewer gate: naari3's later APPROVED must NOT dismiss onmokoworks's
    # CHANGES_REQUESTED. An aggregate-max clear would wrongly unblock here.
    payload = [
        {"user": {"login": "onmokoworks"}, "state": "CHANGES_REQUESTED", "submitted_at": "2026-07-18T19:19:00Z"},
        {"user": {"login": "naari3"}, "state": "APPROVED", "submitted_at": "2026-07-18T19:40:00Z"},
    ]
    assert _call("owner_review_gate", payload) == "BLOCK"


def test_same_owner_later_approved_clears_own_changes_requested() -> None:
    payload = [
        {"user": {"login": "onmokoworks"}, "state": "CHANGES_REQUESTED", "submitted_at": "2026-07-18T19:19:00Z"},
        {"user": {"login": "onmokoworks"}, "state": "APPROVED", "submitted_at": "2026-07-18T19:40:00Z"},
    ]
    assert _call("owner_review_gate", payload) == ""


# --- owner_*_after: owner feedback on the current head (bound = head commit
# --- date; second arg = this session's login, excluded to avoid self-block) ---

HEAD_DATE = "2026-07-18T19:00:00Z"  # when the current head was pushed
ME = "naari3"                       # this session's authenticated login


def test_owner_inline_after_head_blocks() -> None:
    payload = [{"user": {"login": "onmokoworks"}, "id": 9, "path": "x.sh", "line": 3,
                "created_at": "2026-07-18T19:20:00Z", "body": "wait, this is wrong"}]
    assert "OWNER-INLINE" in _call("owner_inline_after", payload, HEAD_DATE, ME)


def test_owner_inline_before_head_is_ignored() -> None:
    # Feedback before the current head was pushed was about a superseded state.
    payload = [{"user": {"login": "onmokoworks"}, "id": 9, "path": "x.sh", "line": 3,
                "created_at": "2026-07-18T18:50:00Z", "body": "old, on the previous head"}]
    assert _call("owner_inline_after", payload, HEAD_DATE, ME) == ""


def test_owner_inline_before_the_clean_but_after_head_blocks() -> None:
    # The P1 race: owner feedback after the head was pushed but seconds before
    # the Codex clean must still block (bound is the head date, not the clean).
    payload = [{"user": {"login": "onmokoworks"}, "id": 9, "path": "x.sh", "line": 3,
                "created_at": "2026-07-18T19:16:40Z", "body": "this is wrong"}]
    assert "OWNER-INLINE" in _call("owner_inline_after", payload, HEAD_DATE, ME)


def test_owner_inline_reply_from_other_owner_blocks() -> None:
    # A genuine owner reply on an existing thread ("still not fixed") blocks,
    # even with in_reply_to_id set, because it is not this session's login.
    payload = [{"user": {"login": "onmokoworks"}, "id": 9, "in_reply_to_id": 8, "path": "x.sh", "line": 3,
                "created_at": "2026-07-18T19:20:00Z", "body": "still not fixed"}]
    assert "OWNER-INLINE" in _call("owner_inline_after", payload, HEAD_DATE, ME)


def test_session_own_inline_reply_is_excluded_by_login() -> None:
    # This session's own ack reply (posted under its owner login) must not
    # self-block, regardless of timing — the exclusion is by authorship now.
    payload = [{"user": {"login": ME}, "id": 9, "in_reply_to_id": 8, "path": "x.sh", "line": 3,
                "created_at": "2026-07-18T19:20:00Z", "body": "対応済み"}]
    assert _call("owner_inline_after", payload, HEAD_DATE, ME) == ""


def test_session_own_toplevel_summary_is_excluded_by_login() -> None:
    payload = [{"user": {"login": ME},
                "created_at": "2026-07-18T19:20:00Z", "body": "対応済みの要約"}]
    assert _call("owner_comments_after", payload, HEAD_DATE, ME) == ""


def test_owner_inline_same_second_as_head_blocks() -> None:
    # Inclusive lower bound: same-second-as-head activity fails closed.
    payload = [{"user": {"login": "onmokoworks"}, "id": 9, "path": "x.sh", "line": 3,
                "created_at": HEAD_DATE, "body": "actually this is wrong"}]
    assert "OWNER-INLINE" in _call("owner_inline_after", payload, HEAD_DATE, ME)


def test_owner_comment_after_head_blocks() -> None:
    payload = [{"user": {"login": "onmokoworks"},
                "created_at": "2026-07-18T19:20:00Z", "body": "hold on, don't merge yet"}]
    assert "OWNER-COMMENT" in _call("owner_comments_after", payload, HEAD_DATE, ME)


def test_bare_trigger_comment_is_ignored() -> None:
    payload = [{"user": {"login": "onmokoworks"},
                "created_at": "2026-07-18T19:20:00Z", "body": "@codex review"}]
    assert _call("owner_comments_after", payload, HEAD_DATE, ME) == ""


def test_owner_bodied_commented_review_after_head_blocks() -> None:
    payload = [{"user": {"login": "onmokoworks"}, "state": "COMMENTED",
                "submitted_at": "2026-07-18T19:20:00Z", "body": "[P1] hold on, this is wrong"}]
    assert "OWNER-REVIEW COMMENTED" in _call("owner_reviews_after", payload, HEAD_DATE, ME)


def test_owner_bodyless_commented_review_is_ignored() -> None:
    # A bodyless COMMENTED review (this session's inline reply) is excluded both
    # by having no body and by the session-login filter.
    payload = [{"user": {"login": ME}, "state": "COMMENTED",
                "submitted_at": "2026-07-18T19:20:00Z", "body": ""}]
    assert _call("owner_reviews_after", payload, HEAD_DATE, ME) == ""


def test_owner_changes_requested_review_after_head_blocks() -> None:
    payload = [{"user": {"login": "onmokoworks"}, "state": "CHANGES_REQUESTED",
                "submitted_at": "2026-07-18T19:20:00Z", "body": ""}]
    assert "OWNER-REVIEW CHANGES_REQUESTED" in _call("owner_reviews_after", payload, HEAD_DATE, ME)


def test_owner_review_before_head_is_ignored() -> None:
    payload = [{"user": {"login": "onmokoworks"}, "state": "COMMENTED",
                "submitted_at": "2026-07-18T18:50:00Z", "body": "[P1] old, on previous head"}]
    assert _call("owner_reviews_after", payload, HEAD_DATE, ME) == ""


def test_owner_approved_review_after_head_does_not_block() -> None:
    payload = [{"user": {"login": "onmokoworks"}, "state": "APPROVED",
                "submitted_at": "2026-07-18T19:20:00Z", "body": "looks good"}]
    assert _call("owner_reviews_after", payload, HEAD_DATE, ME) == ""


def test_inline_error_message_is_not_a_finding() -> None:
    # The "To use Codex here" onboarding/error can arrive as an inline comment;
    # it must be classified as an error, not a finding.
    payload = [{"user": {"login": "chatgpt-codex-connector[bot]"},
                "path": "x.sh", "line": 31, "id": 1,
                "body": "To use Codex here, create a Codex account and connect to github."}]
    assert _call("codex_findings", payload) == ""
    assert _call("codex_error", payload) == "CODEX-ERROR"
    assert _call("codex_finding_max_ts", payload) == ""


def test_error_after_clean_does_not_invalidate_a_head_bound_clean() -> None:
    # A transient error is not a verdict: with a head-bound clean present and a
    # later error, the clean still stands (find_ts stays empty, clean_ts set).
    head = "6de58c3dbb095c277dca598cb5621d28cbed723a"
    issue = [
        {"user": {"login": "chatgpt-codex-connector[bot]"},
         "body": "Didn't find any major issues `6de58c3dbb`", "created_at": "2026-07-18T19:16:44Z"},
        {"user": {"login": "chatgpt-codex-connector[bot]"},
         "body": "Something went wrong. Try again later.", "created_at": "2026-07-18T19:40:00Z"},
    ]
    inline_error = [{"user": {"login": "chatgpt-codex-connector[bot]"},
                     "body": "To use Codex here, connect to github.", "created_at": "2026-07-18T19:41:00Z"}]
    assert _call("codex_clean_ts_for_head", issue, head) == "2026-07-18T19:16:44Z"
    # The inline error is not counted as a finding, so nothing supersedes the clean.
    assert _call("codex_finding_max_ts", inline_error) == ""

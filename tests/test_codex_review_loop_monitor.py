"""Tests for the codex-review-loop selection predicates.

These exercise the ACTUAL shell functions in
`.claude/skills/codex-review-loop/codex-review-lib.sh` (sourced per call), not a
copy of the jq strings, so changing the skill's logic without updating behavior
fails here. Requires bash + jq; skips cleanly when either is missing.
"""

import json
import shlex
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
    quoted_args = " ".join(shlex.quote(a) for a in args)
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


# --- owner feedback on the current head, by SHA association --------------------
# owner_inline_on_head(head_sha, me, clearances); owner_reviews_on_head(head_sha,
# clearances). A comment/review is "on the current head" iff its .commit_id ==
# head (GitHub does not move that stamp to later commits). No timestamps. Narrow
# self-ack exemption (only this session's inline replies). A later owner
# approval/dismissal clears an addressed comment. Top-level PR comments carry no
# commit association; the merge guard binds them to the accepted clean's
# timestamp instead (owner_comments_after, below).

HEAD = "6de58c3dbb095c277dca598cb5621d28cbed723a"  # current head SHA
OLD = "31bef1dfc1aabbccddeeff00112233445566778899"  # a superseded commit SHA
ME = "naari3"                                        # this session's login
NO_CLEAR = "{}"                                      # no owner has approved/dismissed


def _inline(login, sha, **kw):
    d = {"user": {"login": login}, "id": 9, "path": "x.sh", "line": 3,
         "commit_id": sha, "created_at": "2026-07-18T19:20:00Z", "body": "wrong"}
    d.update(kw)
    return [d]


def test_owner_inline_on_head_blocks() -> None:
    assert "OWNER-INLINE" in _call("owner_inline_on_head", _inline("onmokoworks", HEAD), HEAD, ME, NO_CLEAR)


def test_owner_inline_on_superseded_commit_is_ignored() -> None:
    # A comment stamped with an old commit_id is outdated (superseded by a push).
    assert _call("owner_inline_on_head", _inline("onmokoworks", OLD), HEAD, ME, NO_CLEAR) == ""


def test_owner_inline_reply_from_other_owner_blocks() -> None:
    # A genuine owner reply ("still not fixed") on the head commit blocks even
    # with in_reply_to_id set, because it is not this session's login.
    payload = _inline("onmokoworks", HEAD, in_reply_to_id=8, body="still not fixed")
    assert "OWNER-INLINE" in _call("owner_inline_on_head", payload, HEAD, ME, NO_CLEAR)


def test_session_own_inline_reply_is_excluded() -> None:
    # This session's own ack REPLY (in_reply_to_id + session login) is exempt.
    payload = _inline(ME, HEAD, in_reply_to_id=8, body="対応済み")
    assert _call("owner_inline_on_head", payload, HEAD, ME, NO_CLEAR) == ""


def test_session_own_non_reply_inline_still_blocks() -> None:
    # A NON-reply inline comment from this session's login is genuine feedback,
    # not an ack, and must still block — the exemption is replies only.
    payload = _inline(ME, HEAD, body="actually, reconsider this")
    assert "OWNER-INLINE" in _call("owner_inline_on_head", payload, HEAD, ME, NO_CLEAR)


def test_owner_inline_uses_original_commit_id_when_commit_id_absent() -> None:
    payload = [{"user": {"login": "onmokoworks"}, "id": 9, "path": "x.sh", "line": 3,
                "original_commit_id": HEAD, "created_at": "2026-07-18T19:20:00Z", "body": "wrong"}]
    assert "OWNER-INLINE" in _call("owner_inline_on_head", payload, HEAD, ME, NO_CLEAR)


def test_owner_inline_cleared_by_later_approval() -> None:
    clr = json.dumps({"onmokoworks": "2026-07-18T19:30:00Z"})
    payload = _inline("onmokoworks", HEAD, created_at="2026-07-18T19:20:00Z")
    assert _call("owner_inline_on_head", payload, HEAD, ME, clr) == ""


def test_owner_inline_after_approval_still_blocks() -> None:
    clr = json.dumps({"onmokoworks": "2026-07-18T19:30:00Z"})
    payload = _inline("onmokoworks", HEAD, created_at="2026-07-18T19:40:00Z")
    assert "OWNER-INLINE" in _call("owner_inline_on_head", payload, HEAD, ME, clr)


def test_owner_bodied_review_on_head_blocks() -> None:
    payload = [{"user": {"login": "onmokoworks"}, "state": "COMMENTED", "commit_id": HEAD,
                "submitted_at": "2026-07-18T19:20:00Z", "body": "[P1] hold on"}]
    assert "OWNER-REVIEW COMMENTED" in _call("owner_reviews_on_head", payload, HEAD, NO_CLEAR)


def test_owner_bodied_review_on_superseded_commit_is_ignored() -> None:
    payload = [{"user": {"login": "onmokoworks"}, "state": "COMMENTED", "commit_id": OLD,
                "submitted_at": "2026-07-18T19:20:00Z", "body": "[P1] old"}]
    assert _call("owner_reviews_on_head", payload, HEAD, NO_CLEAR) == ""


def test_owner_bodyless_review_on_head_is_ignored() -> None:
    # A bodyless COMMENTED review (this session's inline reply) has no body.
    payload = [{"user": {"login": ME}, "state": "COMMENTED", "commit_id": HEAD,
                "submitted_at": "2026-07-18T19:20:00Z", "body": ""}]
    assert _call("owner_reviews_on_head", payload, HEAD, NO_CLEAR) == ""


def test_owner_approved_review_does_not_block() -> None:
    payload = [{"user": {"login": "onmokoworks"}, "state": "APPROVED", "commit_id": HEAD,
                "submitted_at": "2026-07-18T19:20:00Z", "body": "looks good"}]
    assert _call("owner_reviews_on_head", payload, HEAD, NO_CLEAR) == ""


def test_owner_bodied_review_cleared_by_later_approval() -> None:
    clr = json.dumps({"onmokoworks": "2026-07-18T19:30:00Z"})
    payload = [{"user": {"login": "onmokoworks"}, "state": "COMMENTED", "commit_id": HEAD,
                "submitted_at": "2026-07-18T19:20:00Z", "body": "[P1] concern"}]
    assert _call("owner_reviews_on_head", payload, HEAD, clr) == ""


# --- owner_clearances ----------------------------------------------------------

def test_owner_clearances_reports_latest_approval_per_login() -> None:
    payload = [
        {"user": {"login": "onmokoworks"}, "state": "APPROVED", "submitted_at": "2026-07-18T19:30:00Z"},
        {"user": {"login": "onmokoworks"}, "state": "DISMISSED", "submitted_at": "2026-07-18T19:10:00Z"},
    ]
    out = json.loads(_call("owner_clearances", payload))
    assert out == {"onmokoworks": "2026-07-18T19:30:00Z"}


def test_one_owner_approval_does_not_clear_another_owners_inline() -> None:
    # Per-reviewer: naari3's approval must not clear onmokoworks's comment.
    clr = json.dumps({"naari3": "2026-07-18T19:30:00Z"})
    payload = _inline("onmokoworks", HEAD, created_at="2026-07-18T19:20:00Z")
    assert "OWNER-INLINE" in _call("owner_inline_on_head", payload, HEAD, ME, clr)


def test_inline_error_message_is_not_a_finding() -> None:
    # The "To use Codex here" onboarding/error can arrive as an inline comment;
    # it must be classified as an error, not a finding.
    payload = [{"user": {"login": "chatgpt-codex-connector[bot]"},
                "path": "x.sh", "line": 31, "id": 1,
                "body": "To use Codex here, create a Codex account and connect to github."}]
    assert _call("codex_findings", payload) == ""
    assert _call("codex_error", payload) == "CODEX-ERROR"
    assert _call("codex_finding_max_ts", payload) == ""


# --- owner_comments_after: last-minute merge gate for top-level comments -------
# Top-level comments cannot be SHA-bound, so the guard blocks on any non-trigger
# owner comment at/after the accepted clean (the monitor exits on CLEAN, so
# nothing else would surface such a comment before the merge).

CLEAN_TS = "2026-07-18T20:00:00Z"


def _toplevel(login, created_at, body):
    return [{"user": {"login": login}, "id": 77, "created_at": created_at, "body": body}]


def test_owner_comment_after_clean_blocks() -> None:
    payload = _toplevel("onmokoworks", "2026-07-18T20:00:30Z", "wait, one more thing")
    assert "OWNER-COMMENT" in _call("owner_comments_after", payload, CLEAN_TS, NO_CLEAR)


def test_owner_comment_in_same_second_as_clean_blocks() -> None:
    # Second-resolution timestamps: the race window includes the clean's second.
    payload = _toplevel("onmokoworks", CLEAN_TS, "hold the merge")
    assert "OWNER-COMMENT" in _call("owner_comments_after", payload, CLEAN_TS, NO_CLEAR)


def test_owner_comment_before_clean_is_ignored() -> None:
    # An older comment was surfaced by a monitor cycle and handled there; a
    # fresh clean issued after it supersedes it.
    payload = _toplevel("onmokoworks", "2026-07-18T19:59:00Z", "earlier note")
    assert _call("owner_comments_after", payload, CLEAN_TS, NO_CLEAR) == ""


def test_bare_trigger_after_clean_is_ignored() -> None:
    payload = _toplevel("naari3", "2026-07-18T20:00:30Z", "@codex review")
    assert _call("owner_comments_after", payload, CLEAN_TS, NO_CLEAR) == ""


def test_owner_comment_after_clean_cleared_by_later_approval() -> None:
    clr = json.dumps({"onmokoworks": "2026-07-18T20:05:00Z"})
    payload = _toplevel("onmokoworks", "2026-07-18T20:00:30Z", "resolved concern")
    assert _call("owner_comments_after", payload, CLEAN_TS, clr) == ""


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

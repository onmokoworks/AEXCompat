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


def test_finding_quoting_an_error_phrase_stays_a_finding() -> None:
    # The error check is anchored to the start of the body: a genuine finding
    # that merely QUOTES an error phrase (this repo's own files contain
    # `Unknown error` / `Something went wrong`) must stay a finding, or a stale
    # same-head clean would outlive it in the monitor and the merge guard.
    body = ("**P2 Badge  Narrow Codex error filtering**\n\nWhen a review body "
            "contains `Unknown error` or `Something went wrong`, ...")
    payload = [{"user": {"login": "chatgpt-codex-connector[bot]"}, "id": 5,
                "path": "x.sh", "line": 14, "created_at": "2026-07-18T23:17:48Z", "body": body}]
    assert _call("codex_error", payload) == ""
    assert "FINDING id=5" in _call("codex_findings", payload)
    assert _call("codex_finding_max_ts", payload) == "2026-07-18T23:17:48Z"


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


# --- owner_comments_unresolved: exclude only a bare @codex review trigger -----

@pytest.mark.parametrize("body", ["@codex review", "  @codex review  ", "@Codex Review"])
def test_bare_trigger_comment_is_excluded(body: str) -> None:
    payload = [{"user": {"login": "onmokoworks"}, "id": 1,
                "created_at": "2026-07-18T10:00:00Z", "body": body}]
    assert _call("owner_comments_unresolved", payload, "naari3", "{}") == ""


@pytest.mark.parametrize("body", ["fix X, then @codex review again", "[P1] これ直して"])
def test_owner_feedback_is_kept_even_with_trigger_phrase(body: str) -> None:
    payload = [{"user": {"login": "onmokoworks"}, "id": 1,
                "created_at": "2026-07-18T10:00:00Z", "body": body}]
    assert _call("owner_comments_unresolved", payload, "naari3", "{}") == f"OWNER-COMMENT id=1: {body}"


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


def test_same_second_finding_supersedes_clean() -> None:
    ts = "2026-07-19T01:05:26Z"
    assert _call("codex_finding_supersedes_clean", [], ts, ts) == "BLOCK"


def test_strictly_later_clean_clears_finding() -> None:
    assert _call("codex_finding_supersedes_clean", [],
                 "2026-07-19T01:05:26Z", "2026-07-19T01:05:27Z") == ""


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
# owner_inline_unresolved(me, clearances); owner_reviews_unresolved(clearances,
# ack_ts); owner_comments_unresolved(me, clearances); me_ack_ts(me). Explicit
# resolution only: a push or a later Codex clean never resolves owner feedback.
# Inline threads resolve when the session replied after the owner's last
# message; bodied reviews and top-level comments resolve via a later non-trigger
# top-level ack comment by the session; anything resolves via its author's
# later approval.

ME = "naari3"                                        # this session's login
NO_CLEAR = "{}"                                      # no owner has approved


def _inline(login, **kw):
    d = {"user": {"login": login}, "id": 9, "path": "x.sh", "line": 3,
         "commit_id": "31bef1dfc1aabbccddeeff0011223344556677",
         "created_at": "2026-07-18T19:20:00Z", "body": "wrong"}
    d.update(kw)
    return d


def test_owner_inline_with_no_reply_blocks() -> None:
    assert "OWNER-INLINE" in _call("owner_inline_unresolved", [_inline("onmokoworks")], ME, NO_CLEAR)


def test_owner_inline_survives_a_push_until_replied() -> None:
    # Finding: a follow-up push leaves .commit_id on the old commit; that must
    # NOT resolve the feedback. No commit/SHA input exists at all — an
    # unanswered owner comment blocks regardless of how far the head advanced.
    payload = [_inline("onmokoworks", commit_id="0000000000000000000000000000000000000000")]
    assert "OWNER-INLINE" in _call("owner_inline_unresolved", payload, ME, NO_CLEAR)


def test_owner_inline_resolved_by_later_session_reply() -> None:
    payload = [_inline("onmokoworks"),
               _inline(ME, id=11, in_reply_to_id=9, created_at="2026-07-18T19:25:00Z", body="[ack] 対応済み")]
    assert _call("owner_inline_unresolved", payload, ME, NO_CLEAR) == ""


def test_owner_reply_after_ack_reblocks() -> None:
    # The owner speaking last in the thread ("still not fixed") re-blocks.
    payload = [_inline("onmokoworks"),
               _inline(ME, id=11, in_reply_to_id=9, created_at="2026-07-18T19:25:00Z", body="[ack] 対応済み"),
               _inline("onmokoworks", id=12, in_reply_to_id=9,
                       created_at="2026-07-18T19:30:00Z", body="still not fixed")]
    out = _call("owner_inline_unresolved", payload, ME, NO_CLEAR)
    assert "OWNER-INLINE id=12" in out


def test_same_second_owner_message_and_reply_blocks() -> None:
    # Second-resolution race, matching the review/top-level paths: a reply in
    # the same second as the owner's message may not have seen it; fail closed.
    payload = [_inline("onmokoworks"),
               _inline(ME, id=11, in_reply_to_id=9,
                       created_at="2026-07-18T19:20:00Z", body="[ack] 対応済み")]
    assert "OWNER-INLINE" in _call("owner_inline_unresolved", payload, ME, NO_CLEAR)


def test_session_own_reply_alone_does_not_block() -> None:
    # A thread that contains only this session's ack reply has no owner message.
    payload = [_inline(ME, in_reply_to_id=8, body="[ack] 対応済み")]
    assert _call("owner_inline_unresolved", payload, ME, NO_CLEAR) == ""


def test_session_own_non_reply_inline_still_blocks() -> None:
    # A NON-reply inline comment from this session's login is genuine feedback,
    # not an ack, and must still block — the exemption is replies only.
    payload = [_inline(ME, body="actually, reconsider this")]
    assert "OWNER-INLINE" in _call("owner_inline_unresolved", payload, ME, NO_CLEAR)


def test_owner_inline_cleared_by_later_approval() -> None:
    clr = json.dumps({"onmokoworks": "2026-07-18T19:30:00Z"})
    assert _call("owner_inline_unresolved", [_inline("onmokoworks")], ME, clr) == ""


def test_owner_inline_after_approval_still_blocks() -> None:
    clr = json.dumps({"onmokoworks": "2026-07-18T19:30:00Z"})
    payload = [_inline("onmokoworks", created_at="2026-07-18T19:40:00Z")]
    assert "OWNER-INLINE" in _call("owner_inline_unresolved", payload, ME, clr)


def test_unresolved_graphql_owner_thread_blocks() -> None:
    payload = [{"isResolved": False, "comments": [_inline("onmokoworks")]}]
    assert "OWNER-INLINE id=9" in _call("owner_threads_unresolved", payload, ME, NO_CLEAR)


def test_resolved_graphql_owner_thread_does_not_block() -> None:
    payload = [{"isResolved": True, "comments": [_inline("onmokoworks")]}]
    assert _call("owner_threads_unresolved", payload, ME, NO_CLEAR) == ""


def test_unresolved_codex_only_thread_is_not_an_owner_blocker() -> None:
    payload = [{"isResolved": False, "comments": [_inline("chatgpt-codex-connector")]}]
    assert _call("owner_threads_unresolved", payload, ME, NO_CLEAR) == ""


def test_session_reply_on_codex_only_thread_is_not_owner_feedback() -> None:
    payload = [{"isResolved": False, "comments": [
        _inline("chatgpt-codex-connector", id=8),
        _inline(ME, id=9, in_reply_to_id=8, body="[ack] 対応済み")]}]
    assert _call("owner_threads_unresolved", payload, ME, NO_CLEAR) == ""


def test_session_root_comment_still_blocks_as_owner_feedback() -> None:
    payload = [{"isResolved": False, "comments": [_inline(ME, id=9)]}]
    assert "OWNER-INLINE id=9" in _call("owner_threads_unresolved", payload, ME, NO_CLEAR)


def test_session_unmarked_reply_still_blocks_as_owner_feedback() -> None:
    payload = [{"isResolved": False, "comments": [
        _inline("chatgpt-codex-connector", id=8),
        _inline(ME, id=9, in_reply_to_id=8, body="still broken")]}]
    assert "OWNER-INLINE id=9" in _call("owner_threads_unresolved", payload, ME, NO_CLEAR)


def test_mixed_thread_keeps_other_owner_reply_blocking() -> None:
    payload = [{"isResolved": False, "comments": [
        _inline("chatgpt-codex-connector", id=8),
        _inline(ME, id=9, in_reply_to_id=8, body="[ack] 対応済み"),
        _inline("onmokoworks", id=10, in_reply_to_id=8, body="still broken")]}]
    assert "OWNER-INLINE id=10" in _call("owner_threads_unresolved", payload, ME, NO_CLEAR)


def test_graphql_owner_thread_cleared_by_later_same_owner_approval() -> None:
    payload = [{"isResolved": False, "comments": [
        _inline("onmokoworks", created_at="2026-07-18T19:20:00Z")]}]
    clr = json.dumps({"onmokoworks": "2026-07-18T19:30:00Z"})
    assert _call("owner_threads_unresolved", payload, ME, clr) == ""


def test_graphql_owner_thread_not_cleared_by_other_owner_approval() -> None:
    payload = [{"isResolved": False, "comments": [
        _inline("onmokoworks", created_at="2026-07-18T19:20:00Z")]}]
    clr = json.dumps({"naari3": "2026-07-18T19:30:00Z"})
    assert "OWNER-INLINE" in _call("owner_threads_unresolved", payload, ME, clr)


def test_each_owner_must_clear_their_own_thread_feedback() -> None:
    payload = [{"isResolved": False, "comments": [
        _inline("onmokoworks", created_at="2026-07-18T19:20:00Z"),
        _inline("naari3", id=10, created_at="2026-07-18T19:21:00Z")]}]
    clr = json.dumps({"naari3": "2026-07-18T19:30:00Z"})
    assert "OWNER-INLINE id=9" in _call("owner_threads_unresolved", payload, ME, clr)


def test_graphql_owner_reply_after_approval_reblocks() -> None:
    payload = [{"isResolved": False, "comments": [
        _inline("onmokoworks", created_at="2026-07-18T19:40:00Z")]}]
    clr = json.dumps({"onmokoworks": "2026-07-18T19:30:00Z"})
    assert "OWNER-INLINE" in _call("owner_threads_unresolved", payload, ME, clr)


ACK = "2026-07-18T19:26:49Z"  # the session's newest non-trigger ack comment


def test_owner_bodied_review_with_no_later_ack_blocks() -> None:
    payload = [{"user": {"login": "onmokoworks"}, "state": "COMMENTED",
                "submitted_at": "2026-07-18T19:30:00Z", "body": "[P1] hold on"}]
    assert "OWNER-REVIEW COMMENTED" in _call("owner_reviews_unresolved", payload, NO_CLEAR, ACK)


def test_owner_bodied_review_before_ack_is_resolved() -> None:
    payload = [{"user": {"login": "onmokoworks"}, "state": "COMMENTED",
                "submitted_at": "2026-07-18T19:20:00Z", "body": "[P1] answered"}]
    assert _call("owner_reviews_unresolved", payload, NO_CLEAR, ACK) == ""


def test_owner_bodied_review_in_same_second_as_ack_blocks() -> None:
    # Second-resolution race: an ack in the same second may not have seen it.
    payload = [{"user": {"login": "onmokoworks"}, "state": "COMMENTED",
                "submitted_at": ACK, "body": "[P1] race"}]
    assert "OWNER-REVIEW COMMENTED" in _call("owner_reviews_unresolved", payload, NO_CLEAR, ACK)


def test_owner_bodyless_review_is_ignored() -> None:
    # A bodyless COMMENTED review (this session's inline reply) has no body.
    payload = [{"user": {"login": ME}, "state": "COMMENTED",
                "submitted_at": "2026-07-18T19:30:00Z", "body": ""}]
    assert _call("owner_reviews_unresolved", payload, NO_CLEAR, ACK) == ""


def test_owner_approved_review_does_not_block() -> None:
    payload = [{"user": {"login": "onmokoworks"}, "state": "APPROVED",
                "submitted_at": "2026-07-18T19:30:00Z", "body": "looks good"}]
    assert _call("owner_reviews_unresolved", payload, NO_CLEAR, ACK) == ""


def test_owner_bodied_review_cleared_by_later_approval() -> None:
    clr = json.dumps({"onmokoworks": "2026-07-18T19:40:00Z"})
    payload = [{"user": {"login": "onmokoworks"}, "state": "COMMENTED",
                "submitted_at": "2026-07-18T19:30:00Z", "body": "[P1] concern"}]
    assert _call("owner_reviews_unresolved", payload, clr, ACK) == ""


# --- owner_clearances ----------------------------------------------------------

def test_owner_clearances_reports_latest_approval_per_login() -> None:
    payload = [
        {"user": {"login": "onmokoworks"}, "state": "APPROVED", "submitted_at": "2026-07-18T19:30:00Z"},
        {"user": {"login": "onmokoworks"}, "state": "DISMISSED", "submitted_at": "2026-07-18T19:10:00Z"},
    ]
    out = json.loads(_call("owner_clearances", payload))
    assert out == {"onmokoworks": "2026-07-18T19:30:00Z"}


def test_dismissed_review_is_not_a_timestamped_clearance() -> None:
    # GitHub REST preserves the original submitted_at after dismissal; it does
    # not expose when the dismissal happened, so using it could under-clear or
    # misorder later feedback.
    payload = [{"user": {"login": "onmokoworks"}, "state": "DISMISSED",
                "submitted_at": "2026-07-18T19:10:00Z"}]
    assert json.loads(_call("owner_clearances", payload)) == {}


def test_one_owner_approval_does_not_clear_another_owners_inline() -> None:
    # Per-reviewer: naari3's approval must not clear onmokoworks's comment.
    clr = json.dumps({"naari3": "2026-07-18T19:30:00Z"})
    assert "OWNER-INLINE" in _call("owner_inline_unresolved", [_inline("onmokoworks")], ME, clr)


def test_inline_error_message_is_not_a_finding() -> None:
    # The "To use Codex here" onboarding/error can arrive as an inline comment;
    # it must be classified as an error, not a finding.
    payload = [{"user": {"login": "chatgpt-codex-connector[bot]"},
                "path": "x.sh", "line": 31, "id": 1,
                "body": "To use Codex here, create a Codex account and connect to github."}]
    assert _call("codex_findings", payload) == ""
    assert _call("codex_error", payload) == "CODEX-ERROR"
    assert _call("codex_finding_max_ts", payload) == ""


# --- owner_comments_unresolved / me_ack_ts: top-level explicit resolution -----
# Top-level comments have no reply threading; they resolve only via a later
# non-trigger ack comment by the session or their author's approval.
# A pre-existing unaddressed owner comment fails closed — a fresh Codex clean
# never supersedes it (finding on 39a1c5f).


def _toplevel(login, created_at, body, id=77):
    return {"user": {"login": login}, "id": id, "created_at": created_at, "body": body}


def test_preexisting_owner_comment_with_no_ack_blocks() -> None:
    # Loop started after the owner had already commented: still blocks.
    payload = [_toplevel("onmokoworks", "2026-07-18T10:00:00Z", "old unaddressed")]
    assert "OWNER-COMMENT" in _call("owner_comments_unresolved", payload, ME, NO_CLEAR)


def test_owner_comment_before_session_ack_is_resolved() -> None:
    payload = [_toplevel("onmokoworks", "2026-07-18T10:00:00Z", "note"),
               _toplevel(ME, "2026-07-18T11:00:00Z", "[ack] addressed in abc123", id=78)]
    assert _call("owner_comments_unresolved", payload, ME, NO_CLEAR) == ""


def test_owner_comment_in_same_second_as_ack_blocks() -> None:
    # Second-resolution race: the ack may not have seen a same-second comment.
    payload = [_toplevel("onmokoworks", "2026-07-18T11:00:00Z", "hold the merge"),
               _toplevel(ME, "2026-07-18T11:00:00Z", "[ack] addressed", id=78)]
    assert "OWNER-COMMENT" in _call("owner_comments_unresolved", payload, ME, NO_CLEAR)


def test_session_trigger_is_not_an_ack() -> None:
    # A bare "@codex review" by the session must not resolve owner comments.
    payload = [_toplevel("onmokoworks", "2026-07-18T10:00:00Z", "unaddressed"),
               _toplevel(ME, "2026-07-18T11:00:00Z", "@codex review", id=78)]
    assert "OWNER-COMMENT" in _call("owner_comments_unresolved", payload, ME, NO_CLEAR)


def test_session_status_comment_is_not_an_ack() -> None:
    # An ordinary status comment by the session ("確認します") must not resolve
    # owner feedback — only a comment carrying the explicit [ack] marker does.
    payload = [_toplevel("onmokoworks", "2026-07-18T10:00:00Z", "not mergeable yet"),
               _toplevel(ME, "2026-07-18T11:00:00Z", "確認します", id=78)]
    assert "OWNER-COMMENT" in _call("owner_comments_unresolved", payload, ME, NO_CLEAR)


def test_truncated_unresolved_thread_fails_closed() -> None:
    # An unresolved thread whose nested comments(first:100) page overflowed may
    # hide owner feedback beyond the fetched window; it must block even when
    # the fetched comments are all non-owner.
    payload = [{"isResolved": False, "truncated": True,
                "comments": [{"id": 1, "user": {"login": "chatgpt-codex-connector[bot]"},
                              "path": "x.sh", "line": 3, "created_at": "2026-07-18T19:20:00Z",
                              "body": "some finding"}]}]
    assert "fail closed" in _call("owner_threads_unresolved", payload, ME, NO_CLEAR)


def test_truncated_resolved_thread_does_not_block() -> None:
    # isResolved is thread-level and authoritative; truncation is irrelevant
    # once the thread is resolved.
    payload = [{"isResolved": True, "truncated": True,
                "comments": [{"id": 1, "user": {"login": "onmokoworks"},
                              "path": "x.sh", "line": 3, "created_at": "2026-07-18T19:20:00Z",
                              "body": "addressed"}]}]
    assert _call("owner_threads_unresolved", payload, ME, NO_CLEAR) == ""


def test_session_ack_comment_does_not_block() -> None:
    # The [ack] comment is the resolution signal itself, never a blocker.
    payload = [_toplevel(ME, "2026-07-18T11:00:00Z", "[ack] all owner feedback addressed")]
    assert _call("owner_comments_unresolved", payload, ME, NO_CLEAR) == ""


def test_session_markerless_comment_blocks_like_any_owner() -> None:
    # With an owner-authenticated token, a human comment from the same login
    # ("merge不可") must not be dropped; it blocks until a LATER [ack].
    payload = [_toplevel(ME, "2026-07-18T11:00:00Z", "merge不可、先に直して")]
    assert "OWNER-COMMENT" in _call("owner_comments_unresolved", payload, ME, NO_CLEAR)
    resolved = payload + [_toplevel(ME, "2026-07-18T12:00:00Z", "[ack] 対応した", id=78)]
    assert _call("owner_comments_unresolved", resolved, ME, NO_CLEAR) == ""


def test_owner_comment_cleared_by_later_approval() -> None:
    clr = json.dumps({"onmokoworks": "2026-07-18T12:00:00Z"})
    payload = [_toplevel("onmokoworks", "2026-07-18T10:00:00Z", "resolved concern")]
    assert _call("owner_comments_unresolved", payload, ME, clr) == ""


def test_both_review_thread_connections_are_paginated() -> None:
    # A single GraphQL cursor cannot advance both nested connections. The
    # scripts must enumerate threads, then issue a separately paginated query
    # for every thread's comments (finding on d62ad17).
    root = Path(__file__).resolve().parents[1] / ".claude" / "skills" / "codex-review-loop"
    for name in ("codex-review-monitor.sh", "codex-merge-guard.sh"):
        script = (root / name).read_text(encoding="utf-8")
        assert "nodes{id isResolved}" in script
        assert "node(id:$id)" in script
        assert "comments(first:100,after:$endCursor)" in script
        assert "replyTo{databaseId}" in script
        assert "in_reply_to_id:(.replyTo.databaseId // null)" in script
        assert script.count("gh api graphql --paginate") >= 2


def test_me_ack_ts_requires_the_marker() -> None:
    payload = [_toplevel(ME, "2026-07-18T11:00:00Z", "[ACK] 対応完了"),
               _toplevel(ME, "2026-07-18T12:00:00Z", "@codex review", id=78),
               _toplevel(ME, "2026-07-18T13:00:00Z", "status: still working", id=79)]
    assert _call("me_ack_ts", payload, ME) == "2026-07-18T11:00:00Z"


def test_me_ack_ts_empty_without_acks() -> None:
    payload = [_toplevel(ME, "2026-07-18T12:00:00Z", "@codex review"),
               _toplevel(ME, "2026-07-18T13:00:00Z", "確認します", id=78)]
    assert _call("me_ack_ts", payload, ME) == ""


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

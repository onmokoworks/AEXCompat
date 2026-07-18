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

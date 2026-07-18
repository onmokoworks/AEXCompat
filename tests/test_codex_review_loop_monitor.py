"""Fixture test for the codex-review-loop monitor's actor selection.

The `.claude/skills/codex-review-loop/SKILL.md` monitor selects Codex responses
with an exact two-identity allowlist:
`select(.user.login == "chatgpt-codex-connector" or .user.login == "chatgpt-codex-connector[bot]")`.
Both forms occur across GitHub REST surfaces (`issues/{PR}/comments` and
`pulls/{PR}/reviews` return the `[bot]` suffix; `gh pr view --json author`
returns it without), so both must be accepted. A `startswith` prefix would also
accept a spoofed login like `chatgpt-codex-connector-fake`, so this test pins
the exact allowlist: both real forms are accepted and prefix spoofs rejected.
"""

import json
import shutil
import subprocess

import pytest

# Must mirror the predicate used in the skill's monitor snippet: an exact
# allowlist of the two known Codex logins (not a prefix, which would accept a
# spoofed login like "chatgpt-codex-connector-fake").
CLEAN_FILTER = (
    '.[] | select(.user.login == "chatgpt-codex-connector" '
    'or .user.login == "chatgpt-codex-connector[bot]") '
    '| select(.body | test("Didn.t find any major issues")) | "CLEAN"'
)


def _run_jq(filter_expr: str, payload: list) -> str:
    jq = shutil.which("jq")
    if jq is None:
        pytest.skip("jq is not installed")
    result = subprocess.run(
        [jq, "-r", filter_expr],
        input=json.dumps(payload),
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert result.returncode == 0, result.stderr
    return result.stdout.strip()


@pytest.mark.parametrize("login", ["chatgpt-codex-connector", "chatgpt-codex-connector[bot]"])
def test_clean_detected_for_both_login_forms(login: str) -> None:
    payload = [{"user": {"login": login}, "body": "Codex Review: Didn't find any major issues. Bravo."}]
    assert _run_jq(CLEAN_FILTER, payload) == "CLEAN"


def test_non_codex_author_is_not_treated_as_clean() -> None:
    payload = [
        {"user": {"login": "naari3"}, "body": "Didn't find any major issues"},
        {"user": {"login": "some-other-bot[bot]"}, "body": "Didn't find any major issues"},
    ]
    assert _run_jq(CLEAN_FILTER, payload) == ""


@pytest.mark.parametrize(
    "login",
    ["chatgpt-codex-connector-fake", "chatgpt-codex-connector2", "chatgpt-codex-connector[bot]-x"],
)
def test_prefix_spoof_login_is_rejected(login: str) -> None:
    # A login that merely starts with the Codex prefix must not be accepted;
    # only the two exact identities count.
    payload = [{"user": {"login": login}, "body": "Didn't find any major issues"}]
    assert _run_jq(CLEAN_FILTER, payload) == ""


def test_codex_without_clean_phrase_is_not_clean() -> None:
    payload = [{"user": {"login": "chatgpt-codex-connector[bot]"}, "body": "Here are some suggestions."}]
    assert _run_jq(CLEAN_FILTER, payload) == ""

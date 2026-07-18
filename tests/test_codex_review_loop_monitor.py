"""Fixture test for the codex-review-loop monitor's actor selection.

The `.claude/skills/codex-review-loop/SKILL.md` monitor selects Codex responses
with the jq predicate `select(.user.login | startswith("chatgpt-codex-connector"))`.
The GitHub REST surfaces return the login as `chatgpt-codex-connector[bot]` on
`issues/{PR}/comments` and `pulls/{PR}/reviews`, but other paths (e.g.
`gh pr view --json author`) return it without the `[bot]` suffix. An exact-match
predicate would silently detect nothing on the second form and stall the loop,
so this test pins the prefix match against both forms.
"""

import json
import shutil
import subprocess

import pytest

# Must mirror the predicate used in the skill's monitor snippet.
CLEAN_FILTER = (
    '.[] | select(.user.login | startswith("chatgpt-codex-connector")) '
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


def test_codex_without_clean_phrase_is_not_clean() -> None:
    payload = [{"user": {"login": "chatgpt-codex-connector[bot]"}, "body": "Here are some suggestions."}]
    assert _run_jq(CLEAN_FILTER, payload) == ""

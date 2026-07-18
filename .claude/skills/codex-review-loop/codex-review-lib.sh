#!/usr/bin/env bash
# Pure selection predicates for the codex-review-loop, split out so both the
# monitor/merge scripts AND tests/test_codex_review_loop_monitor.py exercise the
# exact same logic (no duplicated jq strings). Every function reads a JSON array
# on stdin and writes its result to stdout. jq is required.
set -uo pipefail

# The two exact Codex identities. A prefix match would accept a spoofed login
# like "chatgpt-codex-connector-fake", so this is an allowlist, not startswith.
CODEX_LOGINS='["chatgpt-codex-connector","chatgpt-codex-connector[bot]"]'
OWNER_LOGINS='["onmokoworks","naari3"]'

# CLEAN only when a Codex issue-comment says "Didn't find any major issues" AND
# its "Reviewed commit" SHA is a prefix of the given (full) head SHA. Codex
# prints a short SHA, so match by prefix, not equality. History holds cleans for
# older SHAs, so an unbound match would pass after head advances (finding 1).
codex_clean_for_head() {
  local head="$1"
  [ -n "$head" ] || { echo ""; return 0; }
  jq -r --arg head "$head" --argjson codex "$CODEX_LOGINS" '
    ($head | ascii_downcase) as $h
    | [ .[]
        | select([.user.login] | inside($codex))
        | select(.body | test("Didn.t find any major issues"))
        # Any hex token in the body (the reviewed short SHA) must be a prefix of
        # the current head; a random hex will not prefix the real head SHA.
        | select([ .body | ascii_downcase | scan("[0-9a-f]{7,40}") ]
                 | any(. as $s | $h | startswith($s))) ]
    | if length > 0 then "CLEAN" else "" end'
}

# A Codex error/onboarding message means the review did NOT run. It is neither
# clean nor a finding; the caller must treat it as an error (retry or stop),
# never proceed to merge.
codex_error() {
  jq -r --argjson codex "$CODEX_LOGINS" '
    [ .[]
      | select([.user.login] | inside($codex))
      | select(.body | test("Something went wrong|Unknown error|To use Codex here")) ]
    | if length > 0 then "CODEX-ERROR" else "" end'
}

# Codex inline findings (input: a pull review-comments array).
codex_findings() {
  jq -r --argjson codex "$CODEX_LOGINS" '
    .[] | select([.user.login] | inside($codex))
    | "FINDING id=\(.id) \(.path):\(.line // .original_line): \((.body | split("\n")[0]))"'
}

# Owner reviews that BLOCK merge, by submission state (input: reviews array).
# CHANGES_REQUESTED always blocks, even with an empty body; a COMMENTED review
# blocks when it carries a body. Bodyless COMMENTED reviews with inline findings
# are caught separately by owner_inline.
owner_blocking_reviews() {
  jq -r --argjson owner "$OWNER_LOGINS" '
    .[] | select([.user.login] | inside($owner))
    | select(.state == "CHANGES_REQUESTED" or (.state == "COMMENTED" and ((.body // "") | length) > 0))
    | "OWNER-REVIEW \(.state): \(((.body // "") | split("\n"))[0])"'
}

# Owner inline review comments (input: pull review-comments array).
owner_inline() {
  jq -r --argjson owner "$OWNER_LOGINS" '
    .[] | select([.user.login] | inside($owner))
    | "OWNER-FINDING id=\(.id) \(.path):\(.line // .original_line): \((.body | split("\n")[0]))"'
}

# Owner top-level PR comments, excluding ONLY a bare "@codex review" trigger.
# Real feedback that merely contains the phrase (e.g. "fix X, then @codex
# review") is kept (input: issue-comments array).
owner_comments() {
  jq -r --argjson owner "$OWNER_LOGINS" '
    .[] | select([.user.login] | inside($owner))
    | select((.body | ascii_downcase | gsub("[[:space:]]"; "")) != "@codexreview")
    | "OWNER-COMMENT: \((.body | split("\n"))[0])"'
}

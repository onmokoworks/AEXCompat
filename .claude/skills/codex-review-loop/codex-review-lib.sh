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
# Bodies Codex emits when a review did not actually run (rate-limit, auth,
# transient error). Not a finding and not clean.
CODEX_ERROR_RE='Something went wrong|Unknown error|To use Codex here'

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
# never proceed to merge. Such a message can arrive as an issue comment OR an
# inline review comment, so run this over both.
codex_error() {
  jq -r --arg errre "$CODEX_ERROR_RE" --argjson codex "$CODEX_LOGINS" '
    [ .[]
      | select([.user.login] | inside($codex))
      | select((.body // "") | test($errre)) ]
    | if length > 0 then "CODEX-ERROR" else "" end'
}


# Codex inline findings (input: a pull review-comments array). Error/onboarding
# messages can also arrive as inline comments; they are NOT findings (the review
# did not run) and are excluded here — codex_*_error picks them up instead.
codex_findings() {
  jq -r --arg errre "$CODEX_ERROR_RE" --argjson codex "$CODEX_LOGINS" '
    .[] | select([.user.login] | inside($codex))
    | select((.body // "") | test($errre) | not)
    | "FINDING id=\(.id) \(.path):\(.line // .original_line): \((.body | split("\n")[0]))"'
}

# Owner reviews that BLOCK merge, by submission state (input: reviews array).
# CHANGES_REQUESTED always blocks, even with an empty body; a COMMENTED review
# blocks when it carries a body. Bodyless COMMENTED reviews with inline findings
# are caught separately by owner_inline. Used by the live monitor over the
# since-window (new events only); the merge gate uses owner_review_gate.
owner_blocking_reviews() {
  jq -r --argjson owner "$OWNER_LOGINS" '
    .[] | select([.user.login] | inside($owner))
    | select(.state == "CHANGES_REQUESTED" or (.state == "COMMENTED" and ((.body // "") | length) > 0))
    | "OWNER-REVIEW \(.state): \(((.body // "") | split("\n"))[0])"'
}

# Merge gate over the FULL review history: block while ANY owner's change
# request is unresolved. Resolution is PER REVIEWER — only a later APPROVED or
# DISMISSED from the SAME login clears that login's CHANGES_REQUESTED. A
# COMMENTED review is neutral (a reply to an inline comment posts a bodyless
# COMMENTED review, which must not dismiss a real change request), and one
# owner's APPROVED must not clear another owner's blocker (input: reviews
# array). Echoes "BLOCK" or "".
owner_review_gate() {
  jq -r --argjson owner "$OWNER_LOGINS" '
    [ .[] | select([.user.login] | inside($owner)) ]
    | group_by(.user.login)
    | any(
        ( [ .[] | select(.state == "CHANGES_REQUESTED") | .submitted_at ] | max // "" ) as $cr
        | ( [ .[] | select(.state == "APPROVED" or .state == "DISMISSED") | .submitted_at ] | max // "" ) as $clear
        | $cr != "" and ($clear == "" or $cr > $clear)
      )
    | if . then "BLOCK" else "" end'
}

# Timestamp of the newest Codex CLEAN that references $1 (full head SHA), or ""
# (input: issue-comments array). Used to require the LATEST verdict.
codex_clean_ts_for_head() {
  local head="$1"
  [ -n "$head" ] || { echo ""; return 0; }
  jq -r --arg head "$head" --argjson codex "$CODEX_LOGINS" '
    ($head | ascii_downcase) as $h
    | [ .[]
        | select([.user.login] | inside($codex))
        | select(.body | test("Didn.t find any major issues"))
        | select([ .body | ascii_downcase | scan("[0-9a-f]{7,40}") ]
                 | any(. as $s | $h | startswith($s)))
        | .created_at ]
    | if length > 0 then max else "" end'
}

# Timestamp of the newest Codex clean REACTION covering the current head, or ""
# (input: PR-body reactions array from issues/{pr}/reactions; arg $1 = the head
# commit's ISO date). On an auto first-review with no findings, Codex signals
# clean NOT with a text comment but with a "+1" reaction on the PR body (see
# PR #44). That reaction carries no SHA, so bind it to the head by time: it
# counts only when it is at or after the head commit's date, i.e. no commit
# landed after Codex reacted. A later push moves the head commit date past the
# old reaction, which then no longer counts (stale), exactly like a stale
# head-bound text clean. Only the Codex bot's own +1 qualifies, never a human's.
codex_reaction_clean_ts() {
  local headdate="$1"
  [ -n "$headdate" ] || { echo ""; return 0; }
  jq -r --arg headdate "$headdate" --argjson codex "$CODEX_LOGINS" '
    [ .[]
      | select([.user.login] | inside($codex))
      | select(.content == "+1")
      | select(.created_at >= $headdate)
      | .created_at ]
    | if length > 0 then max else "" end'
}

# Timestamp of the newest Codex inline FINDING, or "" (input: review-comments).
# Excludes error/onboarding messages, consistent with codex_findings.
codex_finding_max_ts() {
  jq -r --arg errre "$CODEX_ERROR_RE" --argjson codex "$CODEX_LOGINS" '
    [ .[]
      | select([.user.login] | inside($codex))
      | select((.body // "") | test($errre) | not)
      | .created_at ]
    | if length > 0 then max else "" end'
}

# Timestamp of the newest Codex error/onboarding comment, or "". Works for
# either issue-comments or inline review-comments (both carry .created_at).
codex_error_max_ts() {
  jq -r --arg errre "$CODEX_ERROR_RE" --argjson codex "$CODEX_LOGINS" '
    [ .[]
      | select([.user.login] | inside($codex))
      | select((.body // "") | test($errre))
      | .created_at ]
    | if length > 0 then max else "" end'
}

# Owner inline review comments (input: pull review-comments array).
owner_inline() {
  jq -r --argjson owner "$OWNER_LOGINS" '
    .[] | select([.user.login] | inside($owner))
    | "OWNER-FINDING id=\(.id) \(.path):\(.line // .original_line): \((.body | split("\n")[0]))"'
}

# Merge-gate view of owner activity AT OR AFTER a Codex clean at $1 (the race
# the guard must catch: the owner speaks after clean but before merge). The
# lower bound is INCLUSIVE (>=): GitHub timestamps are second-resolution, so an
# owner comment in the same second as the clean must fail closed and block, not
# slip through. Reply comments (in_reply_to_id set) are INCLUDED: an owner reply
# like "still not fixed" on an existing thread after the clean is real feedback
# and must block. Self-block by this session's own ack replies is prevented by
# the "> = clean" timestamp scope, not by dropping replies: this session posts
# its replies before re-triggering (so before the next clean), and the CLEAN
# branch runs the guard without posting any reply, so any owner reply newer than
# the clean is genuinely the owner's (input: pull review-comments array).
owner_inline_after() {
  jq -r --arg ts "$1" --argjson owner "$OWNER_LOGINS" '
    .[] | select([.user.login] | inside($owner))
    | select(.created_at >= $ts)
    | "OWNER-INLINE id=\(.id) \(.path):\(.line // .original_line): \((.body | split("\n")[0]))"'
}

# Owner blocking REVIEWS at or after a Codex clean at $1 (input: reviews array).
# owner_review_gate blocks unresolved CHANGES_REQUESTED over full history, but a
# repo owner can also submit a non-inline PR review with state COMMENTED and a
# BODY after the clean but before the guard runs; that is a blocker the monitor
# already honors (owner_blocking_reviews) yet neither owner_review_gate nor the
# comment/inline after-checks catch. Same blocking predicate as the monitor
# (CHANGES_REQUESTED, or COMMENTED with a body), scoped inclusively (>=) to the
# clean. Bodyless COMMENTED reviews (posted when this session replies to an
# inline thread) carry no body and are excluded, so they do not self-block.
owner_reviews_after() {
  jq -r --arg ts "$1" --argjson owner "$OWNER_LOGINS" '
    .[] | select([.user.login] | inside($owner))
    | select((.submitted_at // "") >= $ts)
    | select(.state == "CHANGES_REQUESTED" or (.state == "COMMENTED" and ((.body // "") | length) > 0))
    | "OWNER-REVIEW \(.state): \(((.body // "") | split("\n"))[0])"'
}

# Owner top-level comments at or after a Codex clean at $1, excluding a bare
# trigger (input: issue-comments array). Same inclusive (>=) same-second
# fail-closed rule as owner_inline_after. This session posts its summaries
# before re-triggering, so they precede the clean second and are excluded.
owner_comments_after() {
  jq -r --arg ts "$1" --argjson owner "$OWNER_LOGINS" '
    .[] | select([.user.login] | inside($owner))
    | select(.created_at >= $ts)
    | select((.body | ascii_downcase | gsub("[[:space:]]"; "")) != "@codexreview")
    | "OWNER-COMMENT: \((.body | split("\n"))[0])"'
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

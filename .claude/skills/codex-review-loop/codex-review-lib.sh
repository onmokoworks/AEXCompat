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

# Timestamp of the newest Codex clean REACTION plausibly covering the current
# head, or "" (input: PR-body reactions array from issues/{pr}/reactions; arg
# $1 = the head commit's ISO committer date). On an auto first-review with no
# findings, Codex signals clean NOT with a text comment but with a "+1" reaction
# on the PR body (see PR #44). That reaction carries no SHA, so it can only be
# bound to the head by time: it is counted when at or after the head commit
# date. This is a BEST-EFFORT, ADVISORY proxy only, NOT a mergeable verdict:
# committer.date is self-reported, so a cherry-picked or old-dated push could
# make an old reaction appear to cover a head Codex never reviewed. Only the
# monitor uses this, and only to emit a distinct CLEAN-REACTION advisory that
# prompts a re-trigger; codex-merge-guard.sh requires a SHA-bound text clean and
# never merges on a reaction. Only the Codex bot's own +1 qualifies.
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

# Per-owner clearance timestamps (input: reviews array) → a JSON object
# {login: latest APPROVED-or-DISMISSED submitted_at}. An owner who raises a plain
# comment concern and then approves/dismisses WITHOUT a new commit has cleared
# that concern; the owner_*_after checks use this so a resolved older comment on
# the same head stops blocking. Per reviewer, matching owner_review_gate: only
# the SAME owner's later approval clears that owner's comments. Emit with -c.
owner_clearances() {
  jq -c --argjson owner "$OWNER_LOGINS" '
    [ .[]
      | select([.user.login] | inside($owner))
      | select(.state == "APPROVED" or .state == "DISMISSED") ]
    | group_by(.user.login)
    | map({ key: .[0].user.login, value: ([ .[].submitted_at ] | max) })
    | from_entries'
}

# Merge-gate view of owner INLINE feedback on the CURRENT head, bound by SHA
# ASSOCIATION, not timestamps. Args: $1 = current head SHA; $2 = this session's
# login; $3 = clearances JSON. GitHub stamps each inline comment with the commit
# it was made against and does NOT move that stamp to later commits (verified on
# PR #62: onmokoworks's comments keep commit_id at the reviewed SHA even after 20
# pushes), so `.commit_id == head` reliably means "feedback on the current head",
# with no dependence on committer.date (which is self-reported and mis-orders
# push boundaries in both directions). A comment on a superseded commit has a
# different commit_id and drops out. Self-block exemption is NARROW: only this
# session's own ACK replies (in_reply_to_id set AND authored by $2). A comment is
# cleared once its author later approved/dismissed (input: pull review-comments).
owner_inline_on_head() {
  jq -r --arg head "$1" --arg me "$2" --argjson clr "$3" --argjson owner "$OWNER_LOGINS" '
    .[] | select([.user.login] | inside($owner))
    | select( (((.in_reply_to_id // null) != null) and .user.login == $me) | not )
    | select((.commit_id // .original_commit_id // "") == $head)
    | . as $c | select( ($clr[$c.user.login] // "") == "" or $c.created_at > $clr[$c.user.login] )
    | "OWNER-INLINE id=\(.id) \(.path):\(.line // .original_line): \((.body | split("\n")[0]))"'
}

# Owner bodied non-inline REVIEWS on the current head, by SHA association. Args:
# $1 = head SHA; $2 = clearances JSON (input: reviews array). owner_review_gate
# already handles CHANGES_REQUESTED (per-reviewer, full history, persists across
# commits like GitHub), so this covers the gap it misses: a COMMENTED review WITH
# a body submitted against the current head (its .commit_id == head). A bodied
# review on a superseded commit drops out. Bodyless COMMENTED reviews (this
# session's inline-thread replies) carry no body and are excluded. Cleared if the
# reviewer later approved/dismissed.
owner_reviews_on_head() {
  jq -r --arg head "$1" --argjson clr "$2" --argjson owner "$OWNER_LOGINS" '
    .[] | select([.user.login] | inside($owner))
    | select((.commit_id // "") == $head)
    | select(.state == "COMMENTED" and ((.body // "") | length) > 0)
    | . as $r | select( ($clr[$r.user.login] // "") == "" or $r.submitted_at > $clr[$r.user.login] )
    | "OWNER-REVIEW \(.state): \(((.body // "") | split("\n"))[0])"'
}

# NOTE: top-level PR (issue) comments carry NO commit association, so they cannot
# be reliably bound to the current head. They are treated as ADVISORY: the
# monitor surfaces NEW ones (owner_comments over the since-window) for attention,
# but the merge guard does NOT hard-block on them. An owner who wants to hard
# block should request changes (owner_review_gate) or comment inline on the head.

# Owner top-level PR comments, excluding ONLY a bare "@codex review" trigger.
# Real feedback that merely contains the phrase (e.g. "fix X, then @codex
# review") is kept (input: issue-comments array).
owner_comments() {
  jq -r --argjson owner "$OWNER_LOGINS" '
    .[] | select([.user.login] | inside($owner))
    | select((.body | ascii_downcase | gsub("[[:space:]]"; "")) != "@codexreview")
    | "OWNER-COMMENT: \((.body | split("\n"))[0])"'
}

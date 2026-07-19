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
# transient error). Anchored to the START of the body (the observed messages
# open with "Codex Review: Something went wrong ..." — with "Unknown error" in
# a fence below — or "To use Codex here, ..."): a real finding that merely
# QUOTES one of these phrases (this repo's own files contain them) must stay a
# finding, or a stale same-head clean would outlive it in the monitor/guard.
CODEX_ERROR_RE='^[[:space:]]*(Codex Review: (Something went wrong|Unknown error)|To use Codex here)'

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

# RESOLUTION MODEL for owner feedback (inline, bodied reviews, top-level
# comments): NO implicit supersession. Neither a later push (an inline comment's
# .commit_id stays on the old commit, so "commit superseded" says nothing about
# whether the feedback was addressed) nor a later Codex clean resolves owner
# feedback. Feedback blocks until an EXPLICIT signal:
#   - inline: this session ($me) replied in that thread AFTER the owner's last
#     message (the loop's "対応済み" reply), or the author later
#     approved/dismissed;
#   - bodied COMMENTED reviews / top-level comments (no reply threading): a
#     later non-trigger top-level ack comment by $me, or the author's later
#     approval/dismissal. $me's own top-level comments are the ack channel and
#     do not themselves block; $me hard-blocks via a review or a non-reply
#     inline comment, which DO block even from $me.

# Owner INLINE feedback with no later resolution, thread-scoped (input: pull
# review-comments array). Args: $1 = this session's login; $2 = clearances JSON.
# A thread is keyed by its root (.in_reply_to_id // .id). Owner messages exclude
# ONLY $me's replies (the ack channel); a non-reply inline comment from $me is
# genuine feedback. The thread blocks iff its last owner message is newer than
# $me's last reply in that thread and the author has no later approval/dismissal.
# An owner replying again after an ack ("still not fixed") re-blocks.
owner_inline_unresolved() {
  jq -r --arg me "$1" --argjson clr "$2" --argjson owner "$OWNER_LOGINS" '
    group_by(.in_reply_to_id // .id)[]
    | ( [ .[] | select([.user.login] | inside($owner))
          | select( (((.in_reply_to_id // null) != null) and .user.login == $me) | not ) ] ) as $msgs
    | select(($msgs | length) > 0)
    | ( [ .[] | select(.user.login == $me and (.in_reply_to_id // null) != null)
          | .created_at ] | max // "" ) as $ack
    | ($msgs | max_by(.created_at)) as $last
    | select($last.created_at > $ack)
    | $last | select( ($clr[.user.login] // "") == "" or .created_at > $clr[.user.login] )
    | "OWNER-INLINE id=\(.id) \(.path):\(.line // .original_line): \((.body | split("\n")[0]))"'
}

# Timestamp of $me's newest non-trigger top-level ack comment, or "" (input:
# issue-comments array). Bare "@codex review" triggers are not acks.
me_ack_ts() {
  jq -r --arg me "$1" '
    [ .[] | select(.user.login == $me)
      | select((.body | ascii_downcase | gsub("[[:space:]]"; "")) != "@codexreview")
      | .created_at ]
    | if length > 0 then max else "" end'
}

# Owner bodied non-inline REVIEWS with no later resolution (input: reviews
# array). Args: $1 = clearances JSON; $2 = $me ack timestamp (me_ack_ts).
# owner_review_gate already handles CHANGES_REQUESTED; this covers the bodied
# COMMENTED review it misses. Reviews have no reply threading, so resolution is
# a later ack comment (>= blocks a same-second race, fail-closed) or the
# reviewer's later approval/dismissal. Bodyless COMMENTED reviews (this
# session's inline-thread replies) are excluded.
owner_reviews_unresolved() {
  jq -r --argjson clr "$1" --arg ack "$2" --argjson owner "$OWNER_LOGINS" '
    .[] | select([.user.login] | inside($owner))
    | select(.state == "COMMENTED" and ((.body // "") | length) > 0)
    | select(.submitted_at >= $ack)
    | . as $r | select( ($clr[$r.user.login] // "") == "" or $r.submitted_at > $clr[$r.user.login] )
    | "OWNER-REVIEW \(.state): \(((.body // "") | split("\n"))[0])"'
}

# Owner TOP-LEVEL comments with no later resolution (input: issue-comments
# array). Args: $1 = this session's login; $2 = clearances JSON. Excludes bare
# "@codex review" triggers (real feedback containing the phrase is kept) and
# $me's own comments (the ack channel). Blocks a non-$me owner comment with no
# later $me ack (>= blocks a same-second race, fail-closed) and no later
# approval/dismissal by its author — including comments that predate this loop
# invocation entirely: a pre-existing unaddressed owner comment must fail
# closed, not be superseded by a fresh Codex clean.
owner_comments_unresolved() {
  jq -r --arg me "$1" --argjson clr "$2" --argjson owner "$OWNER_LOGINS" '
    def nontrigger: select((.body | ascii_downcase | gsub("[[:space:]]"; "")) != "@codexreview");
    ( [ .[] | nontrigger | select(.user.login == $me) | .created_at ] | max // "" ) as $ack
    | .[] | nontrigger
    | select([.user.login] | inside($owner))
    | select(.user.login != $me)
    | select(.created_at >= $ack)
    | . as $c | select( ($clr[$c.user.login] // "") == "" or $c.created_at > $clr[$c.user.login] )
    | "OWNER-COMMENT id=\(.id): \((.body | split("\n"))[0])"'
}

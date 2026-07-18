#!/usr/bin/env bash
# codex-review-loop monitor. Emits one line and exits on the first TERMINAL
# event: an owner blocker (top priority), a Codex CLEAN bound to the current
# head SHA, a Codex finding batch, or a 1h timeout.
#
#   codex-review-monitor.sh <owner> <repo> <pr> <since_iso8601>
#
# A Codex error/onboarding message ("Something went wrong", "Unknown error",
# "To use Codex") is TRANSIENT: Codex retries internally and posts the real
# verdict seconds/minutes later (observed: errors at 18:59/19:08, clean at
# 19:16). The monitor does NOT stop on it — it keeps waiting for the real
# review; stopping would miss the verdict. A transient API failure is likewise
# retried on the next tick. The 1h timeout is the backstop if no verdict lands.
# Fail-closed lives in codex-merge-guard.sh (it refuses to merge without a
# current head-bound clean), not in stopping this watch.
set -uo pipefail

OWNER="$1"; REPO="$2"; PR="$3"; SINCE="$4"
LIB="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/codex-review-lib.sh"
# shellcheck source=codex-review-lib.sh
. "$LIB"

DEADLINE=$(( $(date +%s) + 3600 ))   # documented 1h hard timeout (finding 5)

# This session's own login, excluded from the owner-feedback-on-head check so
# its acks/summaries/replies (posted under an owner login) do not self-block.
# Fetched once; empty means no exclusion (best effort) if the call fails.
ME=$(gh api user --jq '.login' 2>/dev/null || echo "")

# Fetch a list endpoint across all pages as a single JSON array, or fail.
fetch() {
  local out
  if ! out=$(gh api "repos/$OWNER/$REPO/$1" --paginate --jq '.[]' 2>/dev/null); then
    return 1
  fi
  # Re-collect the streamed objects into one array for the pure predicates.
  jq -s '.' <<<"$out"
}

# Inclusive lower bound: GitHub timestamps are second-resolution, so an event
# in the same second as the trigger would be dropped by strict ">". The trigger
# comment itself is a bare "@codex review", which owner_comments excludes.
since_filter() { jq --arg s "$SINCE" '[ .[] | select((.created_at // .submitted_at) >= $s) ]'; }

while true; do
  if [ "$(date +%s)" -ge "$DEADLINE" ]; then
    echo "TIMEOUT: no terminal result within 1h; check the PR manually"
    exit 0
  fi
  sleep 30

  # Bind CLEAN to the CURRENT head each iteration (finding 1). A transient API
  # failure is retried on the next tick, not treated as a terminal event.
  if ! head=$(gh api "repos/$OWNER/$REPO/pulls/$PR" --jq '.head.sha' 2>/dev/null); then continue; fi
  # Head commit date bounds the reaction-clean (a +1 with no SHA): it counts
  # only if no commit landed after Codex reacted. committer.date reflects when
  # the commit landed on the branch (push/rebase), the right proxy here.
  if ! head_date=$(gh api "repos/$OWNER/$REPO/commits/$head" --jq '.commit.committer.date' 2>/dev/null); then continue; fi
  if ! pr_comments=$(fetch "pulls/$PR/comments"); then continue; fi
  if ! reviews=$(fetch "pulls/$PR/reviews"); then continue; fi
  if ! issue_comments=$(fetch "issues/$PR/comments"); then continue; fi
  if ! pr_reactions=$(fetch "issues/$PR/reactions"); then continue; fi

  # Only the PRINTED finding list is scoped to SINCE (new findings this cycle);
  # the verdict timestamps below use full history.
  new_pr_comments=$(since_filter <<<"$pr_comments")
  # Reactions are scoped to SINCE too: with the always-trigger policy an auto
  # first-review +1 usually predates the explicit @codex review, and an unscoped
  # read would keep returning that stale advisory, exiting CLEAN-REACTION before
  # the in-flight text clean lands — and re-triggering would repeat it. Only a
  # +1 at/after the trigger counts as this cycle's advisory.
  new_reactions=$(since_filter <<<"$pr_reactions")

  # 1. Owner blockers outrank Codex, so surface them as a TERMINAL event (not
  #    just suppress a clean — that would poll to the 1h timeout when the owner
  #    feedback predates SINCE and the since-scoped check misses it). This
  #    mirrors the merge guard's owner gate exactly, so monitor CLEAN <=> guard
  #    accepts: (a) an unresolved CHANGES_REQUESTED (per-reviewer, full history)
  #    and (b) comments / inline / bodied reviews on the CURRENT head (head_date
  #    bound), minus this session's own ack replies, minus ones the owner later
  #    cleared. Emitting on head_date (not SINCE) is what catches owner feedback
  #    that arrived before this trigger.
  clearances=$(owner_clearances <<<"$reviews")
  owner_block=$(
    { [ -n "$(owner_review_gate <<<"$reviews")" ] \
        && echo "OWNER-REVIEW CHANGES_REQUESTED (unresolved; owner must approve/dismiss)"
      owner_inline_after "$head_date" "$ME" "$clearances" <<<"$pr_comments"
      owner_comments_after "$head_date" "$clearances" <<<"$issue_comments"
      owner_reviews_after "$head_date" "$clearances" <<<"$reviews"; } | grep -v '^$' || true
  )
  if [ -n "$owner_block" ]; then echo "$owner_block"; exit 0; fi

  # Codex verdict. A clean is head-bound (finding 1); a FINDING newer than the
  # clean supersedes it (finding 2). A Codex error is NOT a verdict — it is
  # ignored here so a transient error does not stop the watch; the loop keeps
  # waiting for the real review.
  #
  # clean_ts scans the FULL history, so find_ts must too, or the two are
  # asymmetric: re-running on the SAME head after only replying (no new commit)
  # leaves the superseding finding before SINCE, find_ts goes empty, and the
  # stale head-bound clean emits a premature CLEAN that the merge guard (which
  # checks all comments) then refuses — an early-clean/refuse loop. Compare
  # against all findings; only the PRINTED list below is scoped to new comments.
  #
  # Two clean signals with DIFFERENT strength:
  #   - text_clean_ts: a "Didn't find any major issues" comment bound to the
  #     current head SHA. Mergeable — the guard accepts it, so it emits CLEAN.
  #   - react_clean_ts: a +1 reaction on the PR body, the auto first-review's
  #     only clean signal (no text comment; see PR #44). It carries no SHA and
  #     cannot be reliably head-bound (committer.date is self-reported, so a
  #     cherry-picked/old-dated push could forge coverage), so the guard will
  #     NOT merge on it. It is surfaced as a distinct CLEAN-REACTION advisory so
  #     the watch does not run to timeout; the operator re-triggers to obtain a
  #     mergeable SHA-bound clean.
  text_clean_ts=$(codex_clean_ts_for_head "$head" <<<"$issue_comments")
  react_clean_ts=$(codex_reaction_clean_ts "$head_date" <<<"$new_reactions")
  find_ts=$(codex_finding_max_ts <<<"$pr_comments")

  # 2. Findings newer than the newest clean signal (text or reaction) supersede
  #    it. Compare against both so a reaction does not mask a later finding.
  newest_clean=$text_clean_ts
  if [ -n "$react_clean_ts" ] && { [ -z "$newest_clean" ] || [[ "$react_clean_ts" > "$newest_clean" ]]; }; then
    newest_clean=$react_clean_ts
  fi
  if [ -n "$find_ts" ] && { [ -z "$newest_clean" ] || [[ "$find_ts" > "$newest_clean" ]]; }; then
    findings=$(codex_findings <<<"$new_pr_comments" | grep -v '^$' || true)
    if [ -n "$findings" ]; then echo "$findings"; exit 0; fi
  fi

  # (Owner feedback on the current head was already emitted as a terminal event
  # in step 1 and would have exited; reaching here means there is none, so the
  # clean paths below need no further owner check — they match the guard.)

  # 3. Mergeable CLEAN: a SHA-bound text clean, no newer finding. This is exactly
  #    what the guard will accept (owner gate already passed in step 1).
  if [ -n "$text_clean_ts" ] && { [ -z "$find_ts" ] || [[ ! "$find_ts" > "$text_clean_ts" ]]; }; then
    echo "CLEAN: codex clean for head ${head:0:10}"; exit 0
  fi

  # 4. Reaction-only advisory: Codex +1 on the PR body, not SHA-bound. Not
  #    mergeable by the guard; the operator must re-trigger for a text clean.
  if [ -n "$react_clean_ts" ] && { [ -z "$find_ts" ] || [[ ! "$find_ts" > "$react_clean_ts" ]]; }; then
    echo "CLEAN-REACTION: codex +1 on PR body for head ${head:0:10} (no SHA-bound text clean; re-trigger @codex review for a mergeable verdict)"; exit 0
  fi
done

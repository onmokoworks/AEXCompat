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

  new_pr_comments=$(since_filter <<<"$pr_comments")
  new_reviews=$(since_filter <<<"$reviews")
  new_issue_comments=$(since_filter <<<"$issue_comments")

  # 1. Owner blockers, top priority: inline findings, blocking review states
  #    (incl. bodyless CHANGES_REQUESTED), and non-trigger comments.
  owner_hit=$(printf '%s\n%s\n%s' \
    "$(owner_inline <<<"$new_pr_comments")" \
    "$(owner_blocking_reviews <<<"$new_reviews")" \
    "$(owner_comments <<<"$new_issue_comments")" | grep -v '^$' || true)
  if [ -n "$owner_hit" ]; then echo "$owner_hit"; exit 0; fi

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
  # A clean is either a head-bound text comment OR a head-covering +1 reaction
  # on the PR body (the auto first-review signals clean only by reaction, with
  # no text comment; see PR #44). Take the newest of the two.
  text_clean_ts=$(codex_clean_ts_for_head "$head" <<<"$issue_comments")
  react_clean_ts=$(codex_reaction_clean_ts "$head_date" <<<"$pr_reactions")
  clean_ts=$text_clean_ts
  if [ -n "$react_clean_ts" ] && { [ -z "$clean_ts" ] || [[ "$react_clean_ts" > "$clean_ts" ]]; }; then
    clean_ts=$react_clean_ts
  fi
  find_ts=$(codex_finding_max_ts <<<"$pr_comments")

  # 2. Findings newer than any accepted clean supersede it.
  if [ -n "$find_ts" ] && { [ -z "$clean_ts" ] || [[ "$find_ts" > "$clean_ts" ]]; }; then
    findings=$(codex_findings <<<"$new_pr_comments" | grep -v '^$' || true)
    if [ -n "$findings" ]; then echo "$findings"; exit 0; fi
  fi

  # 3. CLEAN when a head-bound clean exists, no finding is newer than it, AND no
  #    owner activity is newer than it. The last check aligns CLEAN with what
  #    codex-merge-guard.sh will accept: without it, a stale head-bound clean
  #    that predates owner feedback (which arrived after that clean but before
  #    SINCE, so owner_hit above does not see it) would emit CLEAN, then the
  #    guard refuses on the same owner-after-clean feedback — an early-clean/
  #    refuse loop. When owner feedback postdates the clean we keep waiting for
  #    the fresh Codex verdict (a new clean will postdate that feedback). Full
  #    history, inclusive (>=), same predicates as the guard.
  if [ -n "$clean_ts" ] && { [ -z "$find_ts" ] || [[ ! "$find_ts" > "$clean_ts" ]]; }; then
    owner_after=$(
      { owner_inline_after "$clean_ts" <<<"$pr_comments"
        owner_comments_after "$clean_ts" <<<"$issue_comments"
        owner_reviews_after "$clean_ts" <<<"$reviews"; } | grep -v '^$' || true
    )
    if [ -z "$owner_after" ]; then
      echo "CLEAN: codex clean for head ${head:0:10}"; exit 0
    fi
  fi
done

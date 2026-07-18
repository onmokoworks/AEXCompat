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
  if ! pr_comments=$(fetch "pulls/$PR/comments"); then continue; fi
  if ! reviews=$(fetch "pulls/$PR/reviews"); then continue; fi
  if ! issue_comments=$(fetch "issues/$PR/comments"); then continue; fi

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
  clean_ts=$(codex_clean_ts_for_head "$head" <<<"$issue_comments")
  find_ts=$(codex_finding_max_ts <<<"$new_pr_comments")

  # 2. Findings newer than any accepted clean supersede it.
  if [ -n "$find_ts" ] && { [ -z "$clean_ts" ] || [[ "$find_ts" > "$clean_ts" ]]; }; then
    findings=$(codex_findings <<<"$new_pr_comments" | grep -v '^$' || true)
    if [ -n "$findings" ]; then echo "$findings"; exit 0; fi
  fi

  # 3. CLEAN when a head-bound clean exists and no finding is newer than it.
  if [ -n "$clean_ts" ] && { [ -z "$find_ts" ] || [[ ! "$find_ts" > "$clean_ts" ]]; }; then
    echo "CLEAN: codex clean for head ${head:0:10}"; exit 0
  fi
done

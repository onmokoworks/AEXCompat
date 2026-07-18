#!/usr/bin/env bash
# codex-review-loop monitor. Emits one line and exits on the first terminal
# event: an owner blocker (top priority), a Codex error (fail-closed, review did
# not run), a Codex CLEAN bound to the current head SHA, a Codex finding batch,
# or a 1h timeout. Run under the Monitor tool (persistent: true).
#
#   codex-review-monitor.sh <owner> <repo> <pr> <since_iso8601>
#
# Fail-closed: an API call that fails is reported as API-ERROR and stops the
# loop (it is never treated as "no new events"), so the caller cannot merge on
# an unverified state.
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

  # Bind CLEAN to the CURRENT head each iteration (findings 1); fail closed if
  # the PR cannot be read.
  if ! head=$(gh api "repos/$OWNER/$REPO/pulls/$PR" --jq '.head.sha' 2>/dev/null); then
    echo "API-ERROR: cannot read PR head; stopping (fail-closed)"; exit 0
  fi

  if ! pr_comments=$(fetch "pulls/$PR/comments"); then echo "API-ERROR: pulls/comments"; exit 0; fi
  if ! reviews=$(fetch "pulls/$PR/reviews"); then echo "API-ERROR: pulls/reviews"; exit 0; fi
  if ! issue_comments=$(fetch "issues/$PR/comments"); then echo "API-ERROR: issues/comments"; exit 0; fi

  new_pr_comments=$(since_filter <<<"$pr_comments")
  new_reviews=$(since_filter <<<"$reviews")
  new_issue_comments=$(since_filter <<<"$issue_comments")

  # 1. Owner blockers, top priority: inline findings, blocking review states
  #    (incl. bodyless CHANGES_REQUESTED, finding 4), and non-trigger comments.
  owner_hit=$(printf '%s\n%s\n%s' \
    "$(owner_inline <<<"$new_pr_comments")" \
    "$(owner_blocking_reviews <<<"$new_reviews")" \
    "$(owner_comments <<<"$new_issue_comments")" | grep -v '^$' || true)
  if [ -n "$owner_hit" ]; then echo "$owner_hit"; exit 0; fi

  # Codex signals: the LATEST verdict wins. Compare timestamps so a newer
  # finding/error supersedes an older head-bound clean (and vice versa).
  clean_ts=$(codex_clean_ts_for_head "$head" <<<"$issue_comments")
  # A Codex error can arrive as an issue comment OR an inline review comment;
  # take the newest across both so it is never misread as a finding/clean.
  err_issue_ts=$(codex_error_max_ts <<<"$new_issue_comments")
  err_inline_ts=$(codex_error_max_ts <<<"$new_pr_comments")
  err_ts=$err_issue_ts
  [ -z "$err_ts" ] || { [ -n "$err_inline_ts" ] && [[ "$err_inline_ts" > "$err_ts" ]] && err_ts=$err_inline_ts; }
  [ -n "$err_ts" ] || err_ts=$err_inline_ts
  find_ts=$(codex_finding_max_ts <<<"$new_pr_comments")

  # 2. A Codex error newer than any accepted clean => review did not run;
  #    fail-closed, never merge (finding 3, latest-verdict).
  if [ -n "$err_ts" ] && { [ -z "$clean_ts" ] || [[ "$err_ts" > "$clean_ts" ]]; }; then
    echo "CODEX-ERROR: review did not run; re-trigger @codex review"; exit 0
  fi

  # 3. Codex findings newer than any accepted clean supersede it.
  if [ -n "$find_ts" ] && { [ -z "$clean_ts" ] || [[ "$find_ts" > "$clean_ts" ]]; }; then
    findings=$(codex_findings <<<"$new_pr_comments" | grep -v '^$' || true)
    if [ -n "$findings" ]; then echo "$findings"; exit 0; fi
  fi

  # 4. CLEAN only when a head-bound clean exists and is the latest verdict
  #    (no newer finding or error). Bound to the current head SHA (finding 1).
  if [ -n "$clean_ts" ] \
     && { [ -z "$find_ts" ] || [[ ! "$find_ts" > "$clean_ts" ]]; } \
     && { [ -z "$err_ts" ] || [[ ! "$err_ts" > "$clean_ts" ]]; }; then
    echo "CLEAN: codex clean for head ${head:0:10}"; exit 0
  fi
done

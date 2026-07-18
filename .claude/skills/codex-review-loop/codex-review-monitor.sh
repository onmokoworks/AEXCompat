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

since_filter() { jq --arg s "$SINCE" '[ .[] | select((.created_at // .submitted_at) > $s) ]'; }

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

  # 2. Codex error => review did not run; fail-closed, do not merge (finding 3).
  if [ -n "$(codex_error <<<"$new_issue_comments")" ]; then
    echo "CODEX-ERROR: review did not run; re-trigger @codex review"; exit 0
  fi

  # 3. Codex CLEAN, only if it references the current head SHA (finding 1).
  if [ -n "$(codex_clean_for_head "$head" <<<"$issue_comments")" ]; then
    echo "CLEAN: codex clean for head ${head:0:10}"; exit 0
  fi

  # 4. Codex findings.
  findings=$(codex_findings <<<"$new_pr_comments" | grep -v '^$' || true)
  if [ -n "$findings" ]; then echo "$findings"; exit 0; fi
done

#!/usr/bin/env bash
# Fail-closed pre-merge gate + atomic merge for the codex-review-loop.
#
#   codex-merge-guard.sh <owner> <repo> <pr>
#
# Refuses to merge unless, for the CURRENT head SHA:
#   - no owner blocker is pending (inline finding, CHANGES_REQUESTED / bodied
#     COMMENTED review, or non-trigger top-level comment), and
#   - a Codex "Didn't find any major issues" comment references that head SHA.
# Then merges with --match-head-commit so a head that advanced between the check
# and the merge fails the merge instead of merging an unreviewed SHA (finding 2).
set -uo pipefail

OWNER="$1"; REPO="$2"; PR="$3"
LIB="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/codex-review-lib.sh"
# shellcheck source=codex-review-lib.sh
. "$LIB"

fetch() {
  local out
  out=$(gh api "repos/$OWNER/$REPO/$1" --paginate --jq '.[]') || return 1
  jq -s '.' <<<"$out"
}

head=$(gh api "repos/$OWNER/$REPO/pulls/$PR" --jq '.head.sha') || {
  echo "REFUSE: cannot read PR head"; exit 1; }

pr_comments=$(fetch "pulls/$PR/comments") || { echo "REFUSE: pulls/comments fetch failed"; exit 1; }
reviews=$(fetch "pulls/$PR/reviews") || { echo "REFUSE: pulls/reviews fetch failed"; exit 1; }
issue_comments=$(fetch "issues/$PR/comments") || { echo "REFUSE: issues/comments fetch failed"; exit 1; }

owner_hit=$(printf '%s\n%s\n%s' \
  "$(owner_inline <<<"$pr_comments")" \
  "$(owner_blocking_reviews <<<"$reviews")" \
  "$(owner_comments <<<"$issue_comments")" | grep -v '^$' || true)
if [ -n "$owner_hit" ]; then
  echo "REFUSE: unresolved owner items:"; echo "$owner_hit"; exit 1
fi

if [ -z "$(codex_clean_for_head "$head" <<<"$issue_comments")" ]; then
  echo "REFUSE: no Codex clean for current head ${head:0:10}"; exit 1
fi

# Atomic: fails if head moved since the checks above.
gh pr merge "$PR" --repo "$OWNER/$REPO" --merge --delete-branch --match-head-commit "$head"

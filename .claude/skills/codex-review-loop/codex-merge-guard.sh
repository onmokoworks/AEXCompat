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

# Owner gate by LATEST review state, not history: a CHANGES_REQUESTED that the
# owner later dismisses/approves must not block forever (finding: resolved
# history). GitHub itself gates merges this way.
if [ -n "$(owner_review_gate <<<"$reviews")" ]; then
  echo "REFUSE: owner's latest review is CHANGES_REQUESTED"; exit 1
fi

# Codex gate: a head-bound clean must exist (finding 1), and no Codex FINDING
# may be newer than it (finding 2). A Codex error is not a verdict and does not
# invalidate a clean on the same head — but note the clean is head-bound, so if
# head moved the clean is already stale and this refuses for lack of a clean.
clean_ts=$(codex_clean_ts_for_head "$head" <<<"$issue_comments")
if [ -z "$clean_ts" ]; then
  echo "REFUSE: no Codex clean for current head ${head:0:10}"; exit 1
fi
find_ts=$(codex_finding_max_ts <<<"$pr_comments")
if [ -n "$find_ts" ] && [[ "$find_ts" > "$clean_ts" ]]; then
  echo "REFUSE: newer Codex findings after the clean"; exit 1
fi

# Owner comment race: owner_review_gate only sees review STATES. An owner can
# raise a concern as a top-level PR comment or a fresh inline review comment
# WITHOUT a CHANGES_REQUESTED review, after Codex went clean but before the
# merge. Refuse when any such owner activity is newer than the clean. Scoped to
# "> clean" so already-addressed history does not block forever, and inline
# replies (an ack thread) are excluded so this session's own replies-as-owner
# do not self-block.
owner_after=$(
  { owner_inline_after "$clean_ts" <<<"$pr_comments"
    owner_comments_after "$clean_ts" <<<"$issue_comments"
    owner_reviews_after "$clean_ts" <<<"$reviews"; } | grep -v '^$' || true
)
if [ -n "$owner_after" ]; then
  echo "REFUSE: owner raised comments after the Codex clean; address them first:"
  echo "$owner_after"
  exit 1
fi

# Atomic: fails if head moved since the checks above.
gh pr merge "$PR" --repo "$OWNER/$REPO" --merge --delete-branch --match-head-commit "$head"

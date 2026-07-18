#!/usr/bin/env bash
# Fail-closed pre-merge gate + atomic merge for the codex-review-loop.
#
#   codex-merge-guard.sh <owner> <repo> <pr>
#
# Refuses to merge unless, for the CURRENT head SHA:
#   - no owner blocker is pending (inline finding, CHANGES_REQUESTED / bodied
#     COMMENTED review, or non-trigger top-level comment), and
#   - Codex went clean for that head via a "Didn't find any major issues"
#     comment referencing that head SHA (a SHA-bound clean).
# A +1 reaction on the PR body (the auto first-review's clean signal) is NOT
# accepted here: it carries no SHA and cannot be reliably head-bound (a
# cherry-picked/old-dated push could forge coverage), so it must not gate a
# merge. When only a reaction exists, re-trigger @codex review to obtain a
# SHA-bound text clean. Then merges with --match-head-commit so a head that
# advanced between the check and the merge fails instead of merging an
# unreviewed SHA (finding 2).
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
# This session's own login, so its inline ACK replies (posted under an owner
# login) do not self-block; a genuine owner reply from any login still blocks.
ME=$(gh api user --jq '.login') || { echo "REFUSE: cannot read authenticated login"; exit 1; }

pr_comments=$(fetch "pulls/$PR/comments") || { echo "REFUSE: pulls/comments fetch failed"; exit 1; }
reviews=$(fetch "pulls/$PR/reviews") || { echo "REFUSE: pulls/reviews fetch failed"; exit 1; }
issue_comments=$(fetch "issues/$PR/comments") || { echo "REFUSE: issues/comments fetch failed"; exit 1; }

# Owner gate by LATEST review state, not history: a CHANGES_REQUESTED that the
# owner later dismisses/approves must not block forever (finding: resolved
# history). GitHub itself gates merges this way.
if [ -n "$(owner_review_gate <<<"$reviews")" ]; then
  echo "REFUSE: owner's latest review is CHANGES_REQUESTED"; exit 1
fi

# Codex gate: a SHA-bound text clean must exist (finding 1), and no Codex
# FINDING may be newer than it (finding 2). A Codex error is not a verdict and
# does not invalidate a clean on the same head — but note the clean is
# head-bound, so if head moved the clean is already stale and this refuses for
# lack of a clean. A +1 reaction is deliberately not accepted here (see header).
clean_ts=$(codex_clean_ts_for_head "$head" <<<"$issue_comments")
if [ -z "$clean_ts" ]; then
  echo "REFUSE: no SHA-bound Codex clean for current head ${head:0:10} (a +1 reaction alone is not mergeable; re-trigger @codex review)"; exit 1
fi
find_ts=$(codex_finding_max_ts <<<"$pr_comments")
if [ -n "$find_ts" ] && [[ "$find_ts" > "$clean_ts" ]]; then
  echo "REFUSE: newer Codex findings after the clean"; exit 1
fi

# Unresolved owner feedback, beyond the CHANGES_REQUESTED that
# owner_review_gate covers. No implicit supersession: neither a later push nor
# a later Codex clean resolves owner feedback (see the resolution model in
# codex-review-lib.sh). Inline threads block until this session replied after
# the owner's last message in the thread; bodied COMMENTED reviews and
# top-level comments (no reply threading) block until a later non-trigger
# top-level ack comment by this session; each is also cleared by its author's
# later approval/dismissal. This covers feedback that predates this loop
# invocation entirely — a pre-existing unaddressed owner comment fails closed.
clearances=$(owner_clearances <<<"$reviews")
ack_ts=$(me_ack_ts "$ME" <<<"$issue_comments")
owner_after=$(
  { owner_inline_unresolved "$ME" "$clearances" <<<"$pr_comments"
    owner_reviews_unresolved "$clearances" "$ack_ts" <<<"$reviews"
    owner_comments_unresolved "$ME" "$clearances" <<<"$issue_comments"; } | grep -v '^$' || true
)
if [ -n "$owner_after" ]; then
  echo "REFUSE: owner raised feedback on the current head; address it first:"
  echo "$owner_after"
  exit 1
fi

# Atomic: fails if head moved since the checks above.
gh pr merge "$PR" --repo "$OWNER/$REPO" --merge --delete-branch --match-head-commit "$head"

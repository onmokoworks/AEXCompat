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

fetch_review_threads() {
  local out query comment_query encoded thread id resolved comments result='[]'
  query='query($owner:String!,$repo:String!,$pr:Int!,$endCursor:String){repository(owner:$owner,name:$repo){pullRequest(number:$pr){reviewThreads(first:100,after:$endCursor){nodes{id isResolved}pageInfo{hasNextPage endCursor}}}}}'
  out=$(gh api graphql --paginate -F owner="$OWNER" -F repo="$REPO" -F pr="$PR" -f query="$query") || return 1
  comment_query='query($id:ID!,$endCursor:String){node(id:$id){... on PullRequestReviewThread{comments(first:100,after:$endCursor){nodes{databaseId author{login} body createdAt path line originalLine replyTo{databaseId}}pageInfo{hasNextPage endCursor}}}}}'
  for encoded in $(jq -r -s '[.[].data.repository.pullRequest.reviewThreads.nodes[]] | .[] | @base64' <<<"$out"); do
    thread=$(printf '%s' "$encoded" | base64 --decode) || return 1
    id=$(jq -r '.id' <<<"$thread"); resolved=$(jq -r '.isResolved' <<<"$thread")
    comments=$(gh api graphql --paginate -F id="$id" -f query="$comment_query") || return 1
    comments=$(jq -s '[.[].data.node.comments.nodes[] | {id:.databaseId,user:{login:.author.login},body,created_at:.createdAt,path,line,original_line:.originalLine,in_reply_to_id:(.replyTo.databaseId // null)}]' <<<"$comments") || return 1
    result=$(jq -c --argjson resolved "$resolved" --argjson comments "$comments" '. + [{isResolved:$resolved,comments:$comments}]' <<<"$result") || return 1
  done
  printf '%s\n' "$result"
}

# Client-side snapshots cannot atomically exclude an inline owner comment that
# lands between the final fetch and merge. GitHub's required conversation
# resolution rule is evaluated by the server in the merge transaction, so it is
# required WHERE THE PLAN OFFERS IT. Three outcomes:
#   0 = proven enabled (strongest path)
#   1 = the feature is reachable but not enabled, or the lookup failed for an
#       unknown reason — REFUSE (an available-but-unconfigured rule is an owner
#       choice; an unknown failure fails closed)
#   2 = the plan provably does not offer branch protection/rulesets (GitHub
#       Free private repo: protection 404 AND the rules API answers with its
#       explicit upgrade message) — fall back to the FINAL OWNER SNAPSHOT +
#       --match-head-commit below, accepting the documented sub-second residual
#       race as the best available guarantee on this plan. Demanding a feature
#       the plan does not sell would make the guard permanently unable to merge
#       (observed on this repo), which just pushes operators to bypass it.
server_thread_gate_state() {
  local base protection rules_out
  base=$(gh api "repos/$OWNER/$REPO/pulls/$PR" --jq '.base.ref') || return 1
  protection=$(gh api "repos/$OWNER/$REPO/branches/$base/protection" 2>/dev/null) || protection='null'
  if jq -e '.required_conversation_resolution.enabled == true' <<<"$protection" >/dev/null; then
    return 0
  fi
  # Rulesets expose the setting as a pull_request rule with
  # parameters.required_review_thread_resolution (per the REST docs for
  # "Get rules for a branch"), not as a standalone rule type; accept both.
  if rules_out=$(gh api "repos/$OWNER/$REPO/rules/branches/$base" 2>/dev/null); then
    jq -e 'any(.[]; .type == "required_review_thread_resolution"
                 or (.type == "pull_request"
                     and ((.parameters.required_review_thread_resolution // false) == true)))' \
      <<<"$rules_out" >/dev/null && return 0
    return 1
  fi
  rules_out=$(gh api "repos/$OWNER/$REPO/rules/branches/$base" 2>&1) && return 1
  if grep -q "Upgrade to GitHub Pro or make this repository public" <<<"$rules_out"; then
    return 2
  fi
  return 1
}

head=$(gh api "repos/$OWNER/$REPO/pulls/$PR" --jq '.head.sha') || {
  echo "REFUSE: cannot read PR head"; exit 1; }
# This session's own login, so its inline ACK replies (posted under an owner
# login) can be identified; GraphQL thread state remains authoritative inline.
ME=$(gh api user --jq '.login') || { echo "REFUSE: cannot read authenticated login"; exit 1; }

pr_comments=$(fetch "pulls/$PR/comments") || { echo "REFUSE: pulls/comments fetch failed"; exit 1; }
review_threads=$(fetch_review_threads) || { echo "REFUSE: reviewThreads fetch failed"; exit 1; }
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
if [ -n "$(codex_finding_supersedes_clean "$find_ts" "$clean_ts")" ]; then
  echo "REFUSE: Codex findings at or after the clean"; exit 1
fi

# Unresolved owner feedback, beyond the CHANGES_REQUESTED that
# owner_review_gate covers. No implicit supersession: neither a later push nor
# a later Codex clean resolves owner feedback (see the resolution model in
# codex-review-lib.sh). Inline threads block until GitHub reports isResolved;
# bodied COMMENTED reviews and
# top-level comments (no reply threading) block until a later non-trigger
# top-level ack comment by this session; each is also cleared by its author's
# later approval. This covers feedback that predates this loop
# invocation entirely — a pre-existing unaddressed owner comment fails closed.
clearances=$(owner_clearances <<<"$reviews")
ack_ts=$(me_ack_ts "$ME" <<<"$issue_comments")
owner_after=$(
  { owner_threads_unresolved "$ME" "$clearances" <<<"$review_threads"
    owner_reviews_unresolved "$clearances" "$ack_ts" <<<"$reviews"
    owner_comments_unresolved "$ME" "$clearances" <<<"$issue_comments"; } | grep -v '^$' || true
)
if [ -n "$owner_after" ]; then
  echo "REFUSE: owner raised feedback on the current head; address it first:"
  echo "$owner_after"
  exit 1
fi

# Prove the static server-side prerequisite before the final dynamic snapshot.
# Looking it up afterward would reopen a window for owner feedback forms that
# the conversation-resolution rule cannot see.
server_thread_gate_state; gate_state=$?
case "$gate_state" in
  0) : ;;
  2) echo "NOTE: branch protection/rulesets are not offered on this plan (private Free repo); falling back to the final owner snapshot + --match-head-commit (documented residual race)" ;;
  *) echo "REFUSE: base branch must enable server-side required conversation resolution (branch protection/ruleset), or the rule lookup failed"; exit 1 ;;
esac

# FINAL OWNER SNAPSHOT: owner feedback does not change the head SHA, so
# --match-head-commit alone cannot close the window between the earlier API
# reads and merge. Re-fetch every owner surface immediately before merge and
# run the complete owner gate again. Also refresh Codex inputs so a finding
# arriving in the same window cannot be hidden by the earlier clean snapshot.
pr_comments=$(fetch "pulls/$PR/comments") || { echo "REFUSE: final pulls/comments fetch failed"; exit 1; }
review_threads=$(fetch_review_threads) || { echo "REFUSE: final reviewThreads fetch failed"; exit 1; }
reviews=$(fetch "pulls/$PR/reviews") || { echo "REFUSE: final pulls/reviews fetch failed"; exit 1; }
issue_comments=$(fetch "issues/$PR/comments") || { echo "REFUSE: final issues/comments fetch failed"; exit 1; }

if [ -n "$(owner_review_gate <<<"$reviews")" ]; then
  echo "REFUSE: final owner review is CHANGES_REQUESTED"; exit 1
fi
clearances=$(owner_clearances <<<"$reviews")
ack_ts=$(me_ack_ts "$ME" <<<"$issue_comments")
owner_after=$(
  { owner_threads_unresolved "$ME" "$clearances" <<<"$review_threads"
    owner_reviews_unresolved "$clearances" "$ack_ts" <<<"$reviews"
    owner_comments_unresolved "$ME" "$clearances" <<<"$issue_comments"; } | grep -v '^$' || true
)
if [ -n "$owner_after" ]; then
  echo "REFUSE: owner feedback arrived before merge; address it first:"
  echo "$owner_after"
  exit 1
fi

clean_ts=$(codex_clean_ts_for_head "$head" <<<"$issue_comments")
find_ts=$(codex_finding_max_ts <<<"$pr_comments")
if [ -z "$clean_ts" ] || [ -n "$(codex_finding_supersedes_clean "$find_ts" "$clean_ts")" ]; then
  echo "REFUSE: final Codex snapshot is not clean for current head ${head:0:10}"; exit 1
fi

# Atomic head check closes the remaining commit race after the final snapshot;
# server-side conversation resolution independently closes the review race.
gh pr merge "$PR" --repo "$OWNER/$REPO" --merge --delete-branch --match-head-commit "$head"

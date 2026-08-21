# AEXCompat Working Notes

AEXCompat is an actively executing, clean-room compatibility host for Adobe
After Effects Effect AEX plug-ins. The original no-load phase is historical;
reviewed native loading, `EffectMain` selector dispatch, parameter discovery,
and bounded image input/output are now the main implementation path.

## Execution Floor and Safety Rules

- One execution floor, no enforcement tier (decided 2026-08-05 on issue #678;
  the audit behind the decision is `docs/ENFORCEMENT_AUDIT_2026-08-05.md`).
  Every route that loads an AEX gets the same always-on crash containment:
  separate worker process, kill-on-close Job Object with a process-memory
  limit, and a private desktop with modal-dialog sweep (issue #351). No route
  requires an approval receipt, allowlist, or enforced pre-selection hash
  match, so an in-development or unknown AEX can be loaded, dispatched, and
  observed, and a rebuilt plug-in re-runs without re-approval. Failure
  isolation comes from process + Job Object, not from identity pinning;
  hashing the plug-in buys nothing for crash containment.
- A deadline is not part of that floor. Where a wrong answer is worse than a
  slow one, there is none: parameter inspection (discovery) waits indefinitely,
  because a watchdog there contains nothing the Job Object does not already
  contain and instead decides results by wall-clock — a plug-in still mapping a
  sealed closure was reported as "timed out" and that verdict was cached
  (issue #354). Deadlines stay where a caller cannot wait: the interactive
  render session's frame deadline, the selection files' per-plug-in timeouts,
  and the broker's own probe workers in `selftest`. A worker that blocks on a
  modal dialog is a UI-containment problem (issue #351), not a reason to
  reintroduce a discovery deadline.
- Evidence is a recording depth, not an execution tier. Provenance for the
  AE-equivalence/regression corpus (Project Direction 4) comes from recording
  what actually ran — the hash of the actually-loaded plug-in bytes, the
  loaded-module list, the environment — on every run. A recorded identity that
  fails to match at comparison time disqualifies that evidence; it never gates
  launch. The former evidence-tier enforcement (sealed-tree ACL machinery,
  receipt-pinned identity, fail-closed module audit, restricted token) is
  scheduled for removal (#729-#733) and must not be extended.
- Do not bypass output bounds, pixel/output validation, or fail-closed
  suite/handle ownership (reject stale, foreign, exhausted, or double-disposed
  references) to make an AEX appear compatible. These are host-protection
  runtime invariants that keep a malformed plug-in producing diagnostics instead
  of corrupting host state or crashing; they are part of the always-on floor.
- Do not fabricate or hand-edit recorded provenance to make an AEX appear
  compatible or an observation look reproducible; records are written by the
  code path that executed, or not at all.
- Record, never enforce. Hashing the bytes that actually loaded and recording
  that hash in the diagnostic binds observations to what ran and stays
  mandatory. Enforcing a hash as a launch precondition (rejecting dispatch when
  bytes differ from a selected/approved identity) is removed policy-wide. A
  hash mismatch is a state transition, not an error (the #309 pattern): it
  triggers re-discovery, re-verification, or evidence rejection. Stale-reuse
  protection belongs in cache keys (the multifilter `BuildFingerprint` pattern:
  results are keyed by plug-in bytes + host build, so a rebuild invalidates its
  own cache), not in dispatch gates.
- Implementation gap: most of the enforcement this policy removes is gone.
  The worker freshness gate (#729), the module audit (#730), the selection
  file's recorded identity (#732), and the interactive pre-selection hash
  match (#739) are recorded warnings rather than refusals; the restricted
  token, protected DACL, and staged-tree deny ACEs were removed in #731 (the
  worker shares the broker's token, and staging records which bytes ran); the
  compiled-in fixture identities went with #733; and the per-frame output hash
  became an extent cross-check in #690; the `l1` route (load-only probe, the
  last plug-in-loading normal-token route) was deleted outright in #732 —
  nothing executed it and the discovery route plus multifilter discovery cover
  the property it observed. What remains: the `l2`/`render*`/`smart*`/`render_request` CLI
  routes read a selection file that names the plug-in and its dependencies
  (`selftest` launches only the broker's own probe workers and is out of
  scope). Sealed staging still copies and hashes
  the load tree, which #751 is reconsidering. Do not add new enforcement, and
  when touching one of these routes migrate it toward the floor rather than
  extending a gate. The receipt-free routes that already exist
  (`render-video-batch`, `InteractiveRenderSession`, self-hash admission in
  `dispatch_secure_image`) are the floor's reference implementations.
- Keep `imports/` as frozen provenance. Do not redistribute Adobe SDK headers
  or source; the SDK selected by `AFTER_EFFECTS_SDK_ROOT` is an external ABI
  verification and fixture-build input only.
- Do not serialize private absolute paths, plug-in bytes, raw image contents,
  or machine-specific authorization data into shareable reports.
- Compatibility gaps must fail explicitly and become reproducible diagnostics,
  not crashes or fixture-specific silent success.
- When the broker grants the worker access to a filesystem target it validated
  (a dump directory, an output path), pass an already-opened, path-authenticated
  HANDLE (inherited through the existing handle-list transport), not a path
  string the worker re-opens. The worker runs under the same user token as the
  plug-in, so any path the worker re-resolves is a TOCTOU window: the plug-in
  can swap a directory or leaf for a junction between the broker's check and the
  worker's open. Pinning the directory by handle is weaker than never handing
  the worker a path at all. Learned from the issue #18 minidump review: #66's
  broker-created inherited dump handle superseded #67's directory-pin plus
  worker-side re-open.
- The worker is crash containment, not a confidentiality sandbox. Do not claim
  that it prevents user-token filesystem or network access. Recorded provenance
  says which bytes ran, not that the plug-in is safe.
- Prefer machine-portable behavioral self-tests for new compatibility work, and
  update frozen evidence values in `analysis/` only through the
  `tools/refresh-*-evidence.ps1` scripts (`docs/EVIDENCE_POLICY_2026-07-18.md`).
- リポジトリにコミット済みのファイルの内容や構成 (存在/不在) を assert する
  テストは禁止。対象がソースでも `docs/` / `analysis/` の文書でも CI workflow
  でも同じで、理由も同じ: 振る舞いを保つリファクタで壊れ、振る舞いを壊す
  変更で通る (`docs/EVIDENCE_POLICY_2026-07-18.md` §5.3 とその 2026-08-07
  amendment)。ソース側は #691、文書側 17 ファイルは #780 で削除し、#780 の
  棚卸しで「再判定対象」のまま残っていた素の source grep 群と死に import、
  CI workflow 契約 (`test_windows_clean_clone_workflow.py`)、owner 解決の
  `tests/source_owners.py` (issue #127) は PR #885 (2026-08-07 の owner 裁定)
  で全廃した。唯一の例外は `.rc`↔`.cpp` の意味的整合を見る
  `test_probe_pipl_contract.py`。新しい検証は文字列 assert ではなく振る舞い
  (self-test route、contract check、compiled assertion) で書く。
- Image dispatch admits the locally built worker at dispatch time (no frozen
  trust constants; see the section 3 amendment and the 2026-08-05 amendment in
  `docs/EVIDENCE_POLICY_2026-07-18.md`). After any worker rebuild, run the
  broker integration tests before relying on broker dispatch.

## Issue Claim and PR Linking

- Before starting work on a GitHub issue, the working session itself posts a
  claim comment on that issue ("作業をclaimします" plus a one-line scope).
  Do not start on an issue that another session has already claimed unless
  its claim has been explicitly withdrawn.
- When you hit a problem that looks out of scope for the task at hand, always
  search the existing GitHub issues first; if none covers it, file a new issue
  describing the observation before continuing. Do not silently fix it in the
  current PR, and do not drop it unrecorded.
- Follow-up work on an already-closed issue (e.g. a fix-forward after review
  findings on a merged PR) still needs a fresh, claimed issue before starting.
  A closed issue is invisible as an in-progress signal, so skipping this lets
  parallel sessions implement the same fix twice. Learned from PR #50: its
  post-merge review findings were re-implemented as both #67 and #66 because
  no follow-up issue was claimed.
- A PR that implements a claimed issue must carry `Closes #N` in its body.
  Without a corresponding issue, reference related issues with `Refs #N`
  instead; never `Closes` an issue the PR does not actually complete.
- The After Effects installation is an exclusive machine resource. Before any
  capture or aerender run, verify no AfterFX/aerender/aerendercore process is
  running; if one is, another session owns it — wait instead of killing it.
  This is about the local machine only. CI runs on GitHub-hosted runners and
  no longer shares it (#1457), so an open AfterFX session cannot turn a PR
  red any more. It still turns a local `uv run python -m pytest` run red on
  `test_ae_reference_capture_automation`, which is the fail-closed guard
  doing its job rather than a regression
  (`docs/SWEEP_INVESTIGATION_WORKFLOW_2026-08-17.md` §5).
- Do not hold back on implementation, reverse engineering, or AE-oracle
  verification work, and do not ask the user for permission before starting it.
  These are the ordinary work of this project: proceed confidence-first and
  careful, and let owner review happen on the PR. The only thing that needs
  owner sign-off before you act is a change that actually weakens a fail-closed
  host-protection invariant or overrides an explicit CLAUDE.md policy (e.g. the
  #1182 fault→warning reclassification), not the size or risk of an engineering
  unit. For the AE oracle specifically, the only coordination question is
  whether AE is *already* in use — ask/wait only when an
  AfterFX/aerender/aerendercore process is already running (the
  exclusive-resource rule above); otherwise just run the capture. "Remaining
  turns" / "this can't finish in one session" is not a real constraint and is
  never a reason to stop or defer: keep working until the task is done or you
  are genuinely blocked.
- Review happens locally, before the PR exists. Spawn a separate background
  agent of your own to run an adversarial review pass over the working diff,
  fix what it finds, and re-run it on the amended diff. The loop ends when a
  fresh review of the current diff leaves no finding you accepted unaddressed.
  Judge each finding on its merits rather than obeying it; a finding you reject
  goes in the PR body with its reason, since a local loop leaves no other trace
  for the owner to check. Do not open the PR mid-loop, and do not skip a
  re-review because "the fix was small": the regressions this catches are the
  ones introduced while addressing an earlier finding.
- Once the local loop is clean, open the PR. Do not request a bot review on it,
  and do not reinstate a merge guard that gates on a bot verdict: no "@codex
  review" comment, no review-loop skill. What that guard also carried (the
  head-bound merge and the owner re-check below) stays. Nothing on the PR waits
  for a bot verdict; the PR-side gates are CI and owner review.
- Everything on the PR head has to have been through that loop, not just the
  diff you opened the PR with; opening the PR does not end it. A CI fix, an
  answer to owner feedback and further implementation all count, and nothing
  merges while unreviewed changes sit on the head.
- Record where "reviewed" ends: commit what the loop cleared, note that HEAD
  SHA in the PR body next to the rejected findings, and update it every time
  you leave the loop. The first round runs before the PR exists, so its SHA
  goes in the body when you open the PR. A SHA kept only in the session
  context is gone the moment the context is, and the owner cannot check the
  boundary against the head either. The next round reviews
  `git diff <the noted SHA>`, working tree included, rather than one commit at
  a time, since a later commit undoing an earlier fix shows up in neither
  commit's diff alone.
- Run the loop before you move the head yourself (push, force-push). Pulling
  the base branch in takes two passes: once before the operation, and once
  right after on the conflict resolution it produced, which is new code nobody
  has read and does not exist until the operation runs. What the pull
  inherited from the base branch is not yours to review, so the second pass
  looks at the resolution alone, and you re-note the resulting HEAD (the merge
  commit, when you merged) as the reviewed SHA afterwards. Leave it un-updated
  and the inherited work rides along in every later diff.
- Prefer merge over rebase for that pull. A merge commit's combined diff
  (`git show <the merge commit>`) marks with `++` the lines neither parent had,
  which isolates a resolution you wrote; add `--stat` and the isolation is lost.
  A resolution that took one side verbatim (`--theirs` and friends) prints
  nothing at all, because `--cc` drops those hunks, so pair it with
  `git show --remerge-diff <the merge commit>` to see what got discarded. After
  a rebase, `git diff <the noted SHA>` also carries everything inherited from
  the base branch, so note the conflicted paths and read those instead.
- When the head moves without you, as when a suggestion is applied in the
  GitHub UI, fast-forward it into your branch
  (`git pull --ff-only origin <branch>`) and run the loop as soon as you
  notice. `git fetch` alone only moves the remote-tracking ref, so the change
  never reaches the diff you review. If the fast-forward aborts you have local
  commits of your own: take it with `git merge origin/<branch>` and treat it
  like any other base-branch pull, in two passes. Miss the abort and the diff
  looks clean while the change is still not in your tree. Do not proceed to
  merge until it has been through.
- Merge when CI is green on the PR head and no owner comment is unresolved. A
  green run on an older head does not count; re-check after every push, and
  merge with
  `gh pr merge <PR> --merge --match-head-commit <the SHA CI went green on>`
  so a push that lands between the check and the merge call fails the merge
  instead of slipping in unverified. If the merge call refuses, the head
  moved: go back through the loop. After the merge, delete the remote branch
  with `git push origin --delete <branch>` rather than `--delete-branch`: in
  a worktree that flag's post-merge `main` checkout fails with
  `'main' is already used by worktree` and the branch cleanup it promises
  does not complete (the merge itself has already happened). External
  failures (billing/usage limits, runner outages) are not success: report
  them as blocked instead of merging.
- Repo-owner review comments (`onmokoworks`, `naari3`) still outrank
  everything. Never merge while an owner comment on the PR is unresolved, even
  if CI is green. Resolution means the review thread is answered and marked
  resolved; a comment that carries no reply thread (a top-level comment, or a
  bodied COMMENTED review) is resolved by an explicit acknowledgement in a new
  top-level comment. A later push and a later green CI run resolve nothing on
  their own. Owner comments that predate your work block the merge too.
  Re-check for a newer owner comment immediately before merging.

## Canonical Verification

```powershell
[Environment]::SetEnvironmentVariable('AFTER_EFFECTS_SDK_ROOT', 'C:\path\to\AfterEffectsSDK', 'User')
# Reopen PowerShell after changing the user environment.
uv sync --locked
uv run python -m pytest -q
cargo test --manifest-path broker\Cargo.toml --workspace
rustfmt --edition 2024 --check --config skip_children=true @(git ls-files '*.rs')
cargo check --manifest-path bridges\aviutl2-multifilter\Cargo.toml --all-targets --locked
```

Format-check every tracked `.rs`, not `cargo fmt --check`. `cargo fmt` collects
files by following `mod` declarations, so it never reaches anything pulled in
with `include!` — `guest/crates/aex-guest-worker/src/x64.rs` takes 17 files that
way and `bridges/aviutl2-multifilter/src/lib.rs` another 3. `cargo fmt --check`
returned clean on every workspace while 7 of those 20 files were unformatted in
`main`, and touching one of them then failed CI on the pre-existing diffs
(issue #1460). CI runs the same whole-tree check, which takes about 3 seconds.

Some runtime and oracle gates additionally require locally built workers, the
After Effects SDK, approved AEX fixtures, a matching GPU driver, or AE itself.
Run the named build/gate script rather than relying on untracked `target/`
artifacts from a previous checkout.

Build the C++ side with `pwsh -File tools\build-native.ps1`. It resolves the
MSVC developer environment itself, so it works from a plain PowerShell; calling
`cmake --build` directly without vcvars64 fails with `fatal error C1083: Cannot
open include file: 'cstddef'`, which is the missing environment rather than a
source problem. `-Source instruments -Target <name>` builds the other native
tree; `-Target <name>` alone narrows the minihost build to one self-test.

There is one worker executable, `target/minihost-build/aex_worker.exe`, and it
picks its route at run time from a leading `--kind discovery|classic|smart`
pair, which it consumes before its positional argument contract begins. A
render sweep routes each plug-in to the route it needs: SmartFX effects to
`smart`, Classic effects (e.g. every CycoreFXHD effect) to `classic`, parameter
discovery to `discovery`. Missing or unknown `--kind` exits 90 without loading
anything.

It was three executables until issue #1495 — `aex_smart_worker.exe`,
`aex_render_worker.exe`, `aex_l2_worker.exe` — built from three five-line entry
files that differed by one enum value and linked the same
`aex_worker_runtime_core`. Building one of those targets recompiled the shared
object but relinked only that target, so the other two silently stayed on the
previous build and a sweep measured old code on the routes it had not named.
That is why this section used to carry a paragraph telling you to always
rebuild all three; there is now one link step and no way to express a
half-built set.

`rust-toolchain.toml` pins the toolchain, so `cargo fmt --check` means the same
thing on every machine (issue #656). Raising the pin is a deliberate change:
run `cargo fmt --all` across every workspace in that same commit and add the
commit to `.git-blame-ignore-revs`. Run `git config blame.ignoreRevsFile
.git-blame-ignore-revs` once per clone so local `git blame` skips the
formatting-only commits listed there.

## Project Direction

1. Discover and diagnose arbitrary Effect AEX binaries without fixture names.
   The harness no longer recognizes fixtures by a compiled-in hash, and the
   discovery route checks the host's own contract rather than a fixture's flags
   (issue #733); keep it that way.
2. Implement observed missing suites, slots, selectors, and scene semantics as
   general host capabilities.
3. Verify image input/output across Classic, SmartFX, depths, and multiple inputs.
4. Build an After Effects oracle corpus and distinguish host regression evidence
   from Adobe-equivalence evidence.
5. Keep crash containment (process isolation, Job Object), output bounds, and
   fail-closed suite/handle ownership (stale, foreign, exhausted, or
   double-disposed references) always on as host-protection invariants; record
   module identity and loaded bytes as provenance on every run, and enforce
   none of it as a launch precondition (issue #678).

Current status is tracked in `docs/COMPATIBILITY_STATUS_2026-07-16.md`, current
direction in `docs/PROJECT_DIRECTION.md`, and security limitations in
`docs/WINDOWS_NATIVE_HARDENING_PLAN_2026-07-16.md`. Before describing the
worker isolation in security terms, read
`docs/ISOLATION_INVENTORY_2026-08-04.md`: it fixes what is actually
implemented, what is documented plan only (mitigation policies, UI limits,
integrity levels, AppContainer are NOT implemented), and which routes are
sealed versus normal-token. The audit and decision record behind the
single-floor policy is `docs/ENFORCEMENT_AUDIT_2026-08-05.md`. Before running
or citing a render sweep (which folder was swept, how a sweep session and its
worker builds are set up, which local pytest failures are environmental), read
`docs/SWEEP_INVESTIGATION_WORKFLOW_2026-08-17.md`.

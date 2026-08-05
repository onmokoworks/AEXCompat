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
  render session's frame deadline, `l1`'s configured per-plug-in timeout, and
  the broker's own probe workers in `selftest`. A worker that blocks on a modal
  dialog is a UI-containment problem (issue #351), not a reason to reintroduce a
  discovery deadline.
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
- Implementation gap: the code still contains the enforcement this policy
  removes. Approval receipts still gate the `l1`/`l2`/`render*`/`smart*`/
  `render_request` CLI routes (#732), and interactive image dispatch still
  enforces a per-session pre-selection hash match (`ApprovedImageArtifact`,
  #739). The worker freshness gate (#729) and the module audit (#730) were
  demoted to recorded warnings, and the restricted token, protected DACL, and
  staged-tree deny ACEs were removed in #731 (staging now records which bytes
  ran; the worker shares the broker's token). Until the rest lands,
  do not treat the existing enforcement as policy, do not add new enforcement,
  and when touching one of these routes migrate it toward the floor rather
  than extending the gate. `l1` is the last plug-in-loading normal-token route
  and is settled by #732. `selftest` launches only the broker's own probe
  workers and loads no plug-in, so it is out of scope. The receipt-free routes
  that already exist (`render-video-batch`, `InteractiveRenderSession`,
  self-hash admission in `dispatch_secure_image`) are the floor's reference
  implementations.
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
- Pure source-text grep tests (ソースを read_text して文字列 assert するだけの
  テスト) は #691 で全廃した。新規追加も禁止のまま
  (`docs/EVIDENCE_POLICY_2026-07-18.md` §5.3)。`tests/source_owners.py`
  (issue #127) は、凍結 evidence とソースを突き合わせる残存テストのために
  残っている。TU 抽出で実装が移動したら owner を `source_owners.py` に追記
  する運用は変わらない。例外として残したのは `.rc`↔`.cpp` の意味的整合
  (`test_probe_pipl_contract.py`) と CI workflow 契約
  (`test_windows_clean_clone_workflow.py`) の 2 ファイル。
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
- Every PR goes through the Codex review loop before merging. Post an
  "@codex review" comment explicitly at PR creation AND after every new
  commit. Opening a PR does fire an automatic first review, but when it finds
  nothing that review signals clean only with a non-mergeable +1 reaction on
  the PR body (no SHA-bound text comment; see PR #44), which the merge guard
  will not accept. An explicit "@codex review" yields a SHA-bound "Didn't find
  any major issues" text clean, so triggering from the start avoids waiting on
  a reaction and re-triggering later. Address findings and repeat until Codex
  replies that text clean for the latest commit, then merge; never merge
  without it (the `codex-review-loop` skill automates this loop).
- Repo-owner review comments (`onmokoworks`, `naari3`) outrank Codex and are
  handled first: the owner catches issues Codex misses. Never merge while an
  owner review comment on the PR is unresolved, even if Codex is clean —
  address and reply first. Before every merge, re-check the PR for a newer
  owner review or comment, since an owner review and a Codex "clean" can land
  seconds apart.

## Canonical Verification

```powershell
[Environment]::SetEnvironmentVariable('AFTER_EFFECTS_SDK_ROOT', 'C:\path\to\AfterEffectsSDK', 'User')
# Reopen PowerShell after changing the user environment.
uv sync --locked
uv run python -m pytest -q
cargo test --manifest-path broker\Cargo.toml --workspace
cargo fmt --manifest-path broker\Cargo.toml --all --check
cargo check --manifest-path bridges\aviutl2\Cargo.toml --all-targets --locked
cargo check --manifest-path bridges\aviutl2-multifilter\Cargo.toml --all-targets --locked
```

Some runtime and oracle gates additionally require locally built workers, the
After Effects SDK, approved AEX fixtures, a matching GPU driver, or AE itself.
Run the named build/gate script rather than relying on untracked `target/`
artifacts from a previous checkout.

## Project Direction

1. Discover and diagnose arbitrary Effect AEX binaries without fixture names.
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
single-floor policy is `docs/ENFORCEMENT_AUDIT_2026-08-05.md`.

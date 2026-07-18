# AEXCompat Working Notes

AEXCompat is an actively executing, clean-room compatibility host for Adobe
After Effects Effect AEX plug-ins. The original no-load phase is historical;
reviewed native loading, `EffectMain` selector dispatch, parameter discovery,
and bounded image input/output are now the main implementation path.

## Current Safety Rules

- Native AEX execution must go through the Rust broker and its authenticated,
  sealed load tree, restricted worker, Job Object, timeout, and module audit.
- Do not bypass identity checks, approval receipts, dependency manifests, or
  output bounds to make an AEX appear compatible.
- Keep `imports/` as frozen provenance. Do not redistribute Adobe SDK headers
  or source; the SDK selected by `AFTER_EFFECTS_SDK_ROOT` is an external ABI
  verification and fixture-build input only.
- Do not serialize private absolute paths, plug-in bytes, raw image contents,
  or machine-specific authorization data into shareable reports.
- Compatibility gaps must fail explicitly and become reproducible diagnostics,
  not crashes or fixture-specific silent success.
- The worker is crash containment and integrity hardening, not a confidentiality
  sandbox. Do not claim that it prevents user-token filesystem or network access.
- Prefer machine-portable behavioral self-tests for new compatibility work, and
  update frozen evidence values in `analysis/` only through the
  `tools/refresh-*-evidence.ps1` scripts (`docs/EVIDENCE_POLICY_2026-07-18.md`).
- Image dispatch admits the locally built worker at dispatch time (no frozen
  trust constants; see the section 3 amendment in
  `docs/EVIDENCE_POLICY_2026-07-18.md`). Receipt-pinned worker identity, the
  worker's own plug-in hash check, and plug-in/dependency admission stay
  fail-closed; after any worker rebuild, run the broker integration tests
  before relying on broker dispatch.

## Issue Claim and PR Linking

- Before starting work on a GitHub issue, the working session itself posts a
  claim comment on that issue ("作業をclaimします" plus a one-line scope).
  Do not start on an issue that another session has already claimed unless
  its claim has been explicitly withdrawn.
- A PR that implements a claimed issue must carry `Closes #N` in its body.
  Without a corresponding issue, reference related issues with `Refs #N`
  instead; never `Closes` an issue the PR does not actually complete.
- The After Effects installation is an exclusive machine resource. Before any
  capture or aerender run, verify no AfterFX/aerender/aerendercore process is
  running; if one is, another session owns it — wait instead of killing it.
- Every PR goes through the Codex review loop before merging: comment
  "@codex review", wait for the response, address findings and re-trigger
  until Codex replies "Didn't find any major issues", then merge. Do not
  merge a PR that has not received that reply for its latest commit (the
  `codex-review-loop` skill automates this loop).

## Canonical Verification

```powershell
[Environment]::SetEnvironmentVariable('AFTER_EFFECTS_SDK_ROOT', 'C:\path\to\AfterEffectsSDK', 'User')
# Reopen PowerShell after changing the user environment.
python -m pip install -r requirements-dev.txt
python -m pytest -q
cargo test --manifest-path broker\Cargo.toml --workspace
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
5. Preserve fail-closed ownership, bounds, module identity, and process isolation.

Current status is tracked in `docs/COMPATIBILITY_STATUS_2026-07-16.md`, current
direction in `docs/PROJECT_DIRECTION.md`, and security limitations in
`docs/WINDOWS_NATIVE_HARDENING_PLAN_2026-07-16.md`.

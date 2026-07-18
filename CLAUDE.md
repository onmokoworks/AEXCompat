# AEXCompat Working Notes

AEXCompat is an actively executing, clean-room compatibility host for Adobe
After Effects Effect AEX plug-ins. The original no-load phase is historical;
reviewed native loading, `EffectMain` selector dispatch, parameter discovery,
and bounded image input/output are now the main implementation path.

## Execution Tiers and Safety Rules

- Two execution tiers. The default tier is a crash-contained dev/observation
  worker: separate process, kill-on-close Job Object, and timeout. This is the
  floor for running any AEX and needs no approval receipt, allowlist, or enforced
  pre-selection hash match. It exists so an in-development or unknown AEX can be
  loaded, dispatched, and observed (Project Direction 1/3) without the evidence
  apparatus, and so a rebuilt plug-in re-runs without re-approval. Failure
  isolation comes from process + Job Object + timeout, not from identity pinning;
  hashing the plug-in buys nothing for crash containment.
- The evidence tier adds the authenticated sealed load tree, receipt-pinned
  identity, dependency manifest, and module audit. Treat these as provenance
  for a trustworthy AE-equivalence/regression corpus (Project Direction 4), not
  as a security boundary against untrusted binaries. Require this tier only when
  producing evidence that will be compared, committed, or trusted later; do not
  force it onto interactive observation or reverse-engineering.
- Do not bypass output bounds or pixel/output validation to make an AEX
  appear compatible. Containing malformed AEX output is part of the always-on
  crash-containment floor, not an evidence-tier concern.
- In the evidence tier, do not bypass identity checks, approval receipts, or
  dependency manifests to make an AEX appear compatible.
- Record versus enforce are separate decisions. Hashing the bytes that actually
  loaded and recording that hash in the diagnostic is cheap provenance and stays
  fine in the default tier; it binds observations to what ran. Enforcing a hash
  as a load precondition (rejecting launch when the bytes differ from a
  selected/approved identity, as the shipped `ApprovedImageArtifact.expected_sha256`
  does) is the evidence-tier gate, and is what the default tier drops so a
  post-selection rebuild is not treated as a mismatch.
- Implementation gap: production dispatch has no normal-token fallback; every
  path goes through the sealed/restricted launch. The identity source differs by
  path. L2 and the deterministic render/smart profile flows require schema-v2
  approval receipts. Interactive image dispatch (`image_render.rs` via
  `dispatch_secure_image`) instead uses per-session approval
  (`ApprovedImageArtifact` from `selection.sha256`), which enforces a pre-selection
  hash match and already admits the locally built worker at dispatch time
  (`docs/EVIDENCE_POLICY_2026-07-18.md` §3), so it is not a schema-v2 receipt.
  Either way the crash-containment-only default tier is not yet the shipped
  default; restoring it is tracked work (#36). Do not describe the light path as
  already available in the shipped host.
- Keep `imports/` as frozen provenance. Do not redistribute Adobe SDK headers
  or source; the SDK selected by `AFTER_EFFECTS_SDK_ROOT` is an external ABI
  verification and fixture-build input only.
- Do not serialize private absolute paths, plug-in bytes, raw image contents,
  or machine-specific authorization data into shareable reports.
- Compatibility gaps must fail explicitly and become reproducible diagnostics,
  not crashes or fixture-specific silent success.
- The worker is crash containment, not a confidentiality sandbox. Do not claim
  that it prevents user-token filesystem or network access. The evidence-tier
  integrity machinery guarantees which bytes ran, not that the plug-in is safe.
- Prefer machine-portable behavioral self-tests for new compatibility work, and
  update frozen evidence values in `analysis/` only through the
  `tools/refresh-*-evidence.ps1` scripts (`docs/EVIDENCE_POLICY_2026-07-18.md`).
- Image dispatch admits the locally built worker at dispatch time (no frozen
  trust constants; see the section 3 amendment in
  `docs/EVIDENCE_POLICY_2026-07-18.md`). In the evidence tier, receipt-pinned
  worker identity, the worker's own plug-in hash check, and plug-in/dependency
  admission stay fail-closed; after any worker rebuild, run the broker
  integration tests before relying on broker dispatch.

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
5. Keep crash containment (process isolation, Job Object, timeout) and output
   bounds always on; treat fail-closed ownership, module identity, and receipt
   pinning as the evidence tier rather than a universal requirement.

Current status is tracked in `docs/COMPATIBILITY_STATUS_2026-07-16.md`, current
direction in `docs/PROJECT_DIRECTION.md`, and security limitations in
`docs/WINDOWS_NATIVE_HARDENING_PLAN_2026-07-16.md`.

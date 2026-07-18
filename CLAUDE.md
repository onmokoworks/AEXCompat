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
  or source; the SDK at `C:\Program Files\Adobe\AfterEffectsSDK` is an external
  ABI verification and fixture-build input only.
- Do not serialize private absolute paths, plug-in bytes, raw image contents,
  or machine-specific authorization data into shareable reports.
- Compatibility gaps must fail explicitly and become reproducible diagnostics,
  not crashes or fixture-specific silent success.
- The worker is crash containment and integrity hardening, not a confidentiality
  sandbox. Do not claim that it prevents user-token filesystem or network access.

## Canonical Verification

```powershell
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

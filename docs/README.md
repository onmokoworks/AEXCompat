# Documentation Index

This directory keeps project-level design material out of the root README so the
repository has a short entry point and a durable place for longer plans.

## Design Documents

- `PROJECT_DESIGN_2026-07-03.md`
  - Claude/Fable architecture and implementation plan for AEXCompat.
  - Defines the staged target: static lab, validation harness, minimal
    compatibility host, and strict native-loading gate.

## Related Sources

- `../analysis/AEX_COMPAT_LAB_PLAN_2026-06-05.md`
  - Historical execution log and no-load chain plan.
- `../imports/aviutlas-rust-contracts/`
  - Frozen import of AviUtlas AEX/AEPX/OFX contracts and fixtures.
- `../contracts/PROVENANCE.md`
  - Promoted contract mapping from imported sources to canonical lab paths.

# Documentation Index

This directory keeps project-level design material out of the root README so the
repository has a short entry point and a durable place for longer plans.

## Design Documents

- `PROJECT_DESIGN_2026-07-03.md`
  - Claude/Fable architecture and implementation plan for AEXCompat.
  - Defines the staged target: static lab, validation harness, minimal
    compatibility host, and strict native-loading gate.
- `IMPLEMENTATION_ROADMAP_2026-07-06.md`
  - Task-level roadmap derived from the design document, written at a
    granularity that an implementation agent can execute directly.
  - Phases: A (no-load Python), B (native build without execution),
    H (human-only blockers), D (gate-locked native execution), plus the
    machine-checkable safety-gate checklist (G-1 to G-8).
- `SAFETY_GATE_STATUS_2026-07-13.md`
  - Current gate evidence after the owner authorized native execution. It
    records the satisfied controls and the remaining compatibility boundaries.
- `HUMAN_GATE_HANDOFF_2026-07-13.md`
  - Historical H-1/H-2/H-3 questions from before authorization. It is retained
    for audit history and is not current execution policy.
- `HOST_CORE_BOUNDARY_2026-07-13.md`
  - Separates generic host policy from fixture profiles and defines the evidence
    required before general AEX support can be claimed.
- `DESCRIPTOR_MANIFEST_PROMOTION_2026-07-13.md`
  - Documents no-native regeneration and comparison of reviewed parameter
    manifests from isolated L2 observations.
- `MASKOFFSET_SECOND_FIXTURE_2026-07-13.md`
  - Records the fixed identity, isolated L1/L2 evidence, descriptor promotion,
    and remaining render work for the second owner-authored AEX fixture.

## Related Sources

- `../analysis/AEX_COMPAT_LAB_PLAN_2026-06-05.md`
  - Historical execution log and no-load chain plan.
- `../imports/aviutlas-rust-contracts/`
  - Frozen import of AviUtlas AEX/AEPX/OFX contracts and fixtures.
- `../contracts/PROVENANCE.md`
  - Promoted contract mapping from imported sources to canonical lab paths.

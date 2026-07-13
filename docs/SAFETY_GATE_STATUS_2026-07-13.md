# Safety Gate Status (2026-07-13)

This records staged gate openings, not blanket native execution approval. L1
and L2 have authoritative evidence and explicit receipts. Rendering and every
later stage remain forbidden until separately approved and re-audited.

## Current Result

`gate_state: extended_classic_render_executed_smartfx_pending`

| Gate | Status | Evidence and remaining work |
| --- | --- | --- |
| G-1 fixture approval | Satisfied | The owner confirms self-authorship, project use, and native loading. `ScatterMap.aex` is fixed at 201216 bytes and SHA-256 `223FF5EC542DD74374C727F16AA6C068073D1C2D7A5CABF20512CB289F0716EB`. Test artifacts remain non-evidence. |
| G-2 loader approval receipt | Satisfied for L2 | Receipts `scattermap-l1-20260713-001` and `scattermap-l2-20260713-001` bind direct owner approvals to the fixed hash, staged selector scope, and expiry at 2026-08-12 23:59:59 +09:00. |
| G-3 dependency review | Satisfied | Static x64 PE import review found only Windows/API-set and release VC runtime dependencies, all locally available, with zero candidate blockers. |
| G-4 process isolation | Satisfied | B-1 broker selftest passes normal, timeout, crash, hang-kill, and sentinel-noninheritance scenarios under a kill-on-close Job Object. |
| G-5 broker-owned path policy | Satisfied for L2 | The request carries only fixed id `scattermap`; separate L1/L2 broker paths resolve exactly one local-only allowlist entry and enforce stage, size, receipt, expiry text, timeout, and create-new output containment. Arbitrary plug-in paths remain forbidden. |
| G-6 crash isolation | Satisfied for L2 | B-1 fault injection passes. L1 negative controls and initial L2 access violations were contained and classified; broker survival and create-new crash reports were verified. |
| G-7 redaction and create-new output | Satisfied | Broker output is bounded and path-redacted; selftest output uses create-new semantics. Python and Rust negative tests are green. |
| G-8 cleanroom and licensing decisions | Satisfied | The owner selected public-document cleanroom. Adobe terms were reviewed and the external SDK root `C:\Program Files\Adobe\AfterEffectsSDK` was confirmed. SDK material remains outside Git and outside `minihost/`. |

## Human Work Required

1. H-1 is complete: the approved self-authored fixture is identified by SHA-256 and byte size, and native loading is explicitly permitted.
2. H-2 is complete: the local SDK root and applicable project terms boundary are recorded. The SDK remains outside Git.
3. H-3 is complete: public-document cleanroom is recorded and remains enforced.
4. G-2 and G-3 are complete through L2.
5. L1/L2 implementation and re-audit are complete. A separate render approval and receipt are required before image execution.

L1, L2, and one explicitly approved classic default render case were executed
successfully through the isolated broker. Initialization, descriptor values,
deterministic output, buffer guards, and independent pixel hash parity are
observed. Extended classic render and connected map cases now have oracle
parity. SmartFX/GPU and H-4 AE reference trace capture remain pending under the
continuous authorization and existing isolation controls.

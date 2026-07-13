# Safety Gate Status (2026-07-13)

This records staged gate openings under the owner's continuous execution
authorization. Each stage still requires fixed-fixture controls and re-audit.

## Current Result

`gate_state: target_ae_argb8_full_channel_matrix_verified`

| Gate | Status | Evidence and remaining work |
| --- | --- | --- |
| G-1 fixture approval | Satisfied | The owner confirms self-authorship, project use, and native loading. `ScatterMap.aex` is fixed at 201216 bytes and SHA-256 `223FF5EC542DD74374C727F16AA6C068073D1C2D7A5CABF20512CB289F0716EB`. Test artifacts remain non-evidence. |
| G-2 loader approval receipt | Satisfied for L2 | Receipts `scattermap-l1-20260713-001` and `scattermap-l2-20260713-001` bind direct owner approvals to the fixed hash, staged selector scope, and expiry at 2026-08-12 23:59:59 +09:00. |
| G-3 dependency review | Satisfied | Static x64 PE import review found only Windows/API-set and release VC runtime dependencies, all locally available, with zero candidate blockers. |
| G-4 process isolation | Satisfied | B-1 broker selftest passes normal, timeout, crash, hang-kill, and sentinel-noninheritance scenarios under a kill-on-close Job Object. |
| G-5 broker-owned path policy | Satisfied through SmartFX | Fixed broker commands resolve exactly one local-only allowlist entry and enforce stage, size, receipt, expiry, timeout, and create-new output containment. Arbitrary plug-in paths remain forbidden. |
| G-6 crash isolation | Satisfied through SmartFX | B-1 fault injection passes. SmartFX ran twice in fresh Job Object-isolated workers and the broker survived. |
| G-7 redaction and create-new output | Satisfied | Broker output is bounded and path-redacted; selftest output uses create-new semantics. Python and Rust negative tests are green. |
| G-8 cleanroom and licensing decisions | Satisfied | The owner selected public-document cleanroom. Adobe terms were reviewed and the external SDK root `C:\Program Files\Adobe\AfterEffectsSDK` was confirmed. SDK material remains outside Git and outside `minihost/`. |

## Human Work Required

1. H-1 is complete: the approved self-authored fixture is identified by SHA-256 and byte size, and native loading is explicitly permitted.
2. H-2 is complete: the local SDK root and applicable project terms boundary are recorded. The SDK remains outside Git.
3. H-3 is complete: public-document cleanroom is recorded and remains enforced.
4. G-2 and G-3 are complete through L2.
5. L1/L2 and the extended classic and SmartFX ARGB8 matrices are complete.

L1, L2, and one explicitly approved classic default render case were executed
successfully through the isolated broker. Initialization, descriptor values,
deterministic output, buffer guards, and independent pixel hash parity are
observed. Extended classic render and connected map cases now have oracle
parity. SmartFX PreRender/Render now also has two-run deterministic oracle
parity across the ARGB8 matrix. 16-bpc and 32-bpc CPU cases reproduce the
fixture's declared-but-byte-oriented writes under a harness sentinel; actual AE
evidence shows the unwritten tail is not deterministic. GPU lifecycle
negotiation correctly falls back because PreRender does not opt in to GPU pixel
execution. A missing mandatory input propagates error 4 without output writes
or a process crash. A target-specific malformed frame contract crashes only its
disposable worker in two repeated runs while the broker survives and records
evidence. H-4 default reference capture is satisfied by an actual AE 25.2
trace: the fixed fixture was discovered, added to a temporary layer, and
rendered from a fixed 16x12 input. PNG RGBA was normalized to PF ARGB8 and
matched the independent oracle exactly with zero byte or pixel differences.
See `AE_REFERENCE_TRACE_2026-07-13.md`.
The production-host matrix additionally matches for amount 0 and 500,
horizontal and vertical directions, seed 10000, mix 0, a connected 5x3 map
resampled to 11x7, and an inverted 11x7 map. Repeat Edge remains the only
production-host parameter gap because AE does not enumerate that property.
The cause is verified: its raw checkbox definition has current false but
default true, while Adobe's checkbox contract initializes both fields equally.
The cleanroom L2 report now retains this mismatch explicitly.
An AE 25.2 AEPX roundtrip also preserves the non-default seed 10000,
reconstructs the effect after close/reopen, and renders exact
oracle-equivalent pixels.
SmartFX PreRender checkout also receives a valid time basis; a nonzero
`42/2/24` context was observed exactly while preserving deterministic output.
Production AE also rendered byte-identical oracle-exact frames at time 0 and
1/24 second.
At composition resolution factor `[2,2]`, AE supplied an 8x6 world and the
fixture output matched an oracle built from AE's exact downsampled input.
A variable-alpha production-host case also matched the arbitrary-source oracle
exactly, proving four-channel ARGB movement through transparent pixels.
Its 37.5% mixed case also matches after the observed AE round-to-nearest output
premultiplication transform.
The ARGB8 matrix also covers valid parameter boundaries amount 500, seed 10000,
and mix 0% in both classic and SmartFX paths with broker-enforced oracle parity.

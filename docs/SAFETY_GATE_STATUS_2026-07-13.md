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

MaskOffset's bounded request-v4 host context now has deterministic two-run native
parity for uniform expansion, separate X/Y expansion, corner rounding,
feathering, feather-plus-invert, and a combined transformed color-fill case.
The same regression pass reverified legacy v3, bounded v4, ScatterMap SmartFX,
and intentional mask-suite Access Violation isolation. This expands verified
render behavior without opening arbitrary native paths or fault modes.

AEGP mask ownership is additionally verified: native MaskOffset runs balance
mask, stream-reference, and stream-value acquisition/disposal exactly. Fixed
profile-gated tests reject duplicate mask disposal and stream disposal with a
live value, then prove valid cleanup restores a leak-free state. Existing count
error, Access Violation, multi-mask, and ScatterMap regressions remain green.

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
parity across the ARGB8 matrix. L2 also enforces the target's conditional
selector policy: GLOBAL_SETUP advertises neither `SEND_UPDATE_PARAMS_UI` nor
`SUPPORTS_QUERY_DYNAMIC_FLAGS`, so the host omits both selectors rather than
inventing unsupported callbacks. 16-bpc and 32-bpc CPU cases reproduce the
fixture's declared-but-byte-oriented writes under a harness sentinel; actual AE
evidence shows the unwritten tail is not deterministic. GPU lifecycle
negotiation correctly falls back because PreRender does not opt in to GPU pixel
execution. The advertised threaded-render contract is also verified with two
simultaneous renders against one loaded module in each of two fresh isolated
workers; all four outputs are guarded and oracle-exact. A missing mandatory
input propagates error 4 without output writes
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
Its partial output request contract is also verified: `[3,2,11,8]` reaches
both source and map checkouts, and both returned result rectangles are clipped
to that request while the isolated render remains oracle-exact.
The classic path also remains full-world oracle-exact when `extent_hint` is
`[3,2,11,8]`, proving this target ignores that optimization hint without
writing outside the output world.
Production AE also rendered byte-identical oracle-exact frames at time 0 and
1/24 second.
At composition resolution factor `[2,2]`, AE supplied an 8x6 world and the
fixture output matched an oracle built from AE's exact downsampled input.
A variable-alpha production-host case also matched the arbitrary-source oracle
exactly, proving four-channel ARGB movement through transparent pixels.
Its 37.5% mixed case also matches after the observed AE round-to-nearest output
premultiplication transform.
Production AE 25.2 additionally matches the arbitrary-source oracle exactly at
13x9, covering odd-width row transitions and the final pixel with zero byte
differences.
The ARGB8 matrix also covers valid parameter boundaries amount 500, seed 10000,
and mix 0% in both classic and SmartFX paths with broker-enforced oracle parity.
Production AE additionally rejects ten values immediately outside the exposed
Amount, Direction, Seed, Mix, and Invert ranges without clamping or changing
the stored property, establishing the pre-dispatch validation contract.
A reusable descriptor-driven value gate now enforces that contract before any
native dispatch is permitted. It fails closed for unknown parameters, malformed
ranges, unsupported types, non-finite values, and fractional integer/choice
values; generalized caller-supplied render requests are still intentionally
closed until this decision is wired into the fixed broker boundary.
The sentinel noninheritance probe now verifies the synthetic Event object's
operation rather than only its numeric handle. This avoids false positives when
Windows reuses the parent's handle number for an unrelated child pipe; the
strengthened isolation scenario passed 20 consecutive runs.
The Rust broker now repeats the production-observed parameter validation from
broker-owned descriptors. Strict requests may contain only values for five
known fields under a dedicated local root; callers cannot supply ranges or
plug-in paths. Valid requests currently stop at dispatch permission, while
invalid requests return exit 3 and prove that no native process was started.
Accepted values can now proceed through a separate parameterized classic ARGB8
route. The fixed-hash worker revalidates all five values before AEX loading,
runs twice under the existing Job Object boundary, echoes the bound PF values,
and must match a dynamic independent Rust oracle. A non-table valid endpoint
combination passed both isolated runs; the same route rejected Direction 4
without starting a native process.
An arbitrary 33.333333333% Mix case also passed twice after conformance probing
established that the target casts Mix to f32 before percentage normalization;
both cleanroom oracles now preserve that observable operation order.
The same arbitrary request now passes twice through SmartFX PreRender/Render
under its separate allowlist and receipt, with valid rectangles, intact guards,
and the identical dynamic oracle hash. SmartFX range rejection also proves no
native process starts for invalid values.

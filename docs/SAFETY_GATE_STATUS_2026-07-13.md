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

SPBasicSuite now maintains bounded, thread-safe name/version reference counts.
Unknown and unacquired releases fail without mutating state. Native evidence
records MaskOffset's single module-lifetime `PF Handle Suite@2` reference while
all AEGP suites balance; the broker rejects over-release and inconsistent
counters rather than requiring an observationally false zero count. The new
post-setdown rejection mode is fixed and profile-gated.

PF Handle allocation and lock ownership are now independently bounded and
reported. Normal MaskOffset runs finish with one create/dispose, six
lock/unlock pairs, zero live handles, and zero invalid operations. A fixed
profile-gated case rejects resize while locked and then proves cleanup restores
balanced state. ScatterMap, suite errors, Access Violation isolation, and all
existing render oracles remain green.
Handle allocation is capped at 1024 records and 64 MiB aggregate, including
resize accounting, and successful native runs end with zero live bytes.

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

PF World Suite v2 scratch allocations are now ownership tracked independently
from borrowed render worlds. Only ARGB32/64/128 are accepted, all size
arithmetic is checked, and the complete pool is capped at 64 worlds and
256 MiB. Fixed `world_double_dispose` and `world_allocation_limit` gates each
ran twice after a successful real MaskOffset render. They observed exactly one
rejected operation per run, left zero live worlds/bytes, preserved worker guard
bytes, and produced schema-valid reports. ScatterMap L2, classic render, and
SmartFX plus MaskOffset translated-mask regression remained passing afterward.

PF Pixel Format Suite v2 declarations are bounded to the three implemented CPU
formats and to the SDK-prescribed Global Setup phase. The fixed
`pixel_format_registry` gate ran twice after a successful native MaskOffset
render and proved ordered add, duplicate idempotence, clear, invalid-format
rejection, and out-of-phase rejection. Each run ended with an empty registry,
intact render guards, and a schema-valid report. ScatterMap ARGB64 and ARGB128
render regressions and the MaskOffset two-mask regression remained passing.

AEGP Mask Outline Suite v5 mutation is bounded to 64 vertices and 64 feather
points per mask, validates every index and numeric field, and preserves the
SDK's closed-mask terminal-vertex rule. The fixed `outline_mutation` gate ran
twice after successful native rendering and recorded eight valid state changes,
one rejected invalid feather, complete state restoration, and balanced handle
lifetimes. Its report is schema-valid. Fresh translated-mask and second-mask
native regressions retained their independent oracle hashes.

AEGP Layer Mask Suite v7 now bounds each scene to eight stable-address mask
records and tracks Create, Duplicate, Delete, and Dispose ownership explicitly.
Deleted masks become non-visible tombstones until disposal. The fixed
`mask_attribute_ownership` gate ran twice after successful native MaskOffset
rendering, exercised eleven valid attribute/ownership mutations, rejected one
invalid mode, restored the source scene, and ended with balanced MaskRefs. A
fresh two-mask regression retained count 2 and its independent oracle hash.

AEGP Stream Suite v11 now exposes all 23 callback slots without null function
pointers. Mask-outline streams use independently owned, stable stream refs;
duplicate refs preserve the underlying unique stream ID, and multiple checked-
out values are tracked per ref. Metadata reports mask type, interpolation,
variation, time-varying state, units, and flags. Layer/effect streams and
Memory-Suite-backed strings fail explicitly rather than returning fabricated
objects, while the mask-outline SetStreamValue path is rejected as required.
The fixed `stream_metadata_ownership` gate ran twice after successful native
MaskOffset rendering under the Job Object boundary. Each run recorded nine
metadata queries, one duplicate ref, two intentional rejections, intact guards,
and fully balanced mask/stream/value lifetimes.

AEGP Effect Suite v2/v3/v4 effect application is bounded to eight hosted
instances and sixteen generation-tagged leases. Apply rejects invalid plug-in
owners, layers, installed keys, exhausted capacity, stale references, and
foreign references without publishing a partial instance. Dispose invalidates
the lease before reuse, and parameter streams retain their parent instance
identity. The fixed `aegp_apply_effect` self-test passes in the L2, Classic
render, and SmartFX workers; the refreshed real-AEX render and SmartFX safety
gates remain passing.

AEGP Keyframe Suite v5 now exposes all 22 callback slots and bounds each mask
outline stream to 64 time-sorted keyframes and 256 concurrent checked values.
Keyframe values are independently
cloned on checkout, deletion is rejected while a value is live, and batch-add
transactions support explicit commit or cancellation. Flags, interpolation,
labels, rational times, and whole-outline values are preserved. HOLD sampling
returns the preceding outline; LINEAR sampling interpolates compatible vertex
and feather topology into an owned temporary value. Spatial tangents and
temporal ease reject mask-outline streams instead of fabricating dimensions.
The fixed `keyframe_ownership` gate passed twice after native MaskOffset render,
covering twelve mutations, two intentional type/ownership rejections, HOLD and
LINEAR sampling, transaction rollback/commit, intact guards, and zero leaked
MaskRef, StreamRef, StreamValue, or transaction handles.

AEGP Dynamic Stream Suite v4 now exposes all 26 callback slots over a bounded
property tree: Layer root, indexed Mask Parade, named Mask Atom, and Outline,
Feather, Opacity, and Expansion leaves. Index and match-name traversal share
the same StreamRef ownership as the regular Stream Suite. Stable logical mask
ordering permits add, delete, duplicate, and reorder without moving live host
objects. Dynamic flags, names, parent refs, modified state, and match names are
tracked; unsupported dimension separation fails explicitly. Opacity, Feather,
and Expansion now use typed OneD/TwoD StreamValue storage with units and bounds.
The fixed `dynamic_stream_tree` gate passed twice after native MaskOffset
rendering with twenty traversal queries, eight mutations, one intentional
flag-policy rejection, restored scene state, intact guards, and balanced refs.

AEGP Memory Suite v1 now implements all eight callbacks with a mutex-protected
registry capped at 256 handles and 16 MiB. Handles support CLEAR allocation,
nested locks, size/stats queries, unlocked resize, reporting state, and strict
free ownership. Stream names and expressions now return owned null-terminated
UTF-16 memory handles; per-leaf expression text and enabled state are retained.
The fixed `aegp_memory_strings` gate passed twice after native MaskOffset render,
created and freed three handles per run, rejected one resize while nested-
locked, roundtripped `Mask Path` and `time*2`, preserved guards, and ended with
zero live AEGP memory handles or bytes.

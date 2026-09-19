# Windows render sweep performance ledger (2026-09-20)

This is machine-local evidence for the `local-psoft-item11` performance cycle.
It records both accepted behavior and rejected experiments so a later cycle
does not infer safety from the fastest number alone. After Effects, AfterFX,
and aerender were not started.

## Fixed conditions

- Source HEAD before this note: `e9010d005` (`Accelerate SmartFX corpus rendering`).
- Shipping path: `bridges/aviutl2-multifilter/examples/render_sweep.rs`.
- Image: 256x144 ARGB8, time 0, one frame, generated secondary layer where
  declared, one render job.
- PSOFT corpus: 19 installed AEX files below the shipping scan root.
- Accepted baseline worker SHA-256:
  `f9494e5163cb3fd1e993617cc648b70fb817c14cf928141c90e6511b6fd36602`.
- Baseline report:
  `%TEMP%/aexcompat-psoft-known-worker-control-20260920.json`.

## Accepted measurements

The resident-session parameter-update behavioral test completed three real
frames in one session in 0.20 s. The manual 1920x1080 latency comparison used
12 parameter changes:

| path | measured latency |
|---|---:|
| one-shot frame, median | 183.8 ms |
| resident session open | 69.8 ms |
| resident frame, median | 47.5 ms |
| worker portion of resident frame, median | 43.0 ms |

The PSOFT clustered baseline rendered 19/19 effects. Its total was 3,398 ms:
598 ms discovery and 2,800 ms render. The earlier accepted clustered run was
3,316 ms total (610 ms discovery, 2,706 ms render). Both had identical bucket
and pixel SHA results for all 19 effects.

The safe non-vendor milestone report remains:
`%TEMP%/aexcompat-render-clustered-final-nonvendor-nodistort.json`.
It records 74 entries in 24,812 ms: 70 rendered, three AEGP/non-image entries,
and one explicit `DepthONNX.aex` frame error `-6`. Its worker fingerprint is the
accepted baseline hash above. Sapphire, Maxon/Red Giant/Trapcode, Boris FX, and
DistortChroma were excluded from execution, not from the larger inventory.

## Clean-build discriminator

Two PSOFT effects (`P_Texture.aex` and `P_BlurCelLayer.aex`) exited 22 after an
incremental native rebuild, while the accepted worker rendered both. A fresh
Ninja Release directory from the same source and MSVC 19.50 rendered both in
focused shipping sweeps. Fresh worker SHA-256 was
`936953504a33d92461fe52fcd642e6c0f488255fb049eccc2cccce5df7fe5edf`.

The failure did not reproduce in a fresh build and is therefore consistent
with an incremental-build-directory-dependent problem, not evidence of a
source or VS 2026 compatibility regression. The exact cause, including whether
a stale object was responsible, remains unidentified. Native performance
experiments must use a fresh build directory; copying an older known-good
worker back after each experiment remains the safe shipping rollback.

## Rejected module-audit cache

Profiling attributed about 50 ms to each record-only pre-unload audit and about
51 ms to each post-load audit in a PSOFT cluster swap. Caching canonical paths
for every loaded HMODULE produced the attractive result below:

| experiment | total | discovery | render | pixel/bucket differences |
|---|---:|---:|---:|---:|
| accepted baseline | 3,398 ms | 598 ms | 2,800 ms | 0 |
| all-module cache | 2,693 ms | 559 ms | 2,134 ms | 0 |

The all-module result is **rejected**. WorkerSession retains AEX images, but a
plug-in may load and unload arbitrary dependency DLLs. Windows can reuse a base
address/HMODULE and loader path, so a cached canonical or reparse result can
describe an earlier module lifetime. Record-only provenance would become
inaccurate even though enforced audit bypassed the cache.

Restricting the cache to the worker and explicitly retained AEX images removed
that lifetime ambiguity, but also removed the speedup: 3,683 ms total, 661 ms
discovery, 3,022 ms render (19/19 rendered). That implementation and its test
were fully reverted; the worktree returned to the accepted product code and
the shipping worker returned to the accepted `f9494e...` binary.

## Next safe performance boundary

A later optimization must avoid treating HMODULE plus path as a module-lifetime
identity. Plausible bounded directions are an unload-aware loader notification
index, or a snapshot-difference design with an independently verified lifetime
identity. Merely caching canonical paths or reparse classifications for every
observed module is not acceptable. Before another native optimization, retain
the validation ladder used here: focused behavioral self-test, clean Release
worker build, the two PSOFT regressions, one 19-effect PSOFT milestone, and
bucket/pixel SHA comparison.

## Full shipping-scan inventory

The shipping `scan_for_diagnostics` path found 984 AEX files after configured
ignore processing. An inventory-only pass hashed every candidate without
loading an AEX or starting `aex_worker`, After Effects, or aerender:

| classification | count |
|---|---:|
| external blocked (Sapphire) | 292 |
| external blocked (Trapcode) | 3 |
| unexecuted | 689 |
| total | 984 |

The pass took 17,937 ms. Every row has a 64-character canonical-path identity,
file SHA-256 and size, scan-root-relative identity, final stage, execution and
failure classification, and the boundary-verified CLI/worker build fingerprint.
The requested render conditions are recorded once as report-wide conditions.
Bucket counts sum to the record count. The report is
`%TEMP%/aexcompat-inventory-all-2026-09-20.json`; absolute scan paths remain
omitted. `--blocked-path` retains matching candidates in the denominator and
labels them instead of silently excluding them.

## Current 984-record reconciliation and clean-build recovery

After the OLM semantic cycles, the current shipping scan still found exactly
984 AEX files across the two default roots. The load-free inventory report is
`%TEMP%/aexcompat-inventory-fresh-worker-current-20260920.json`. It records 570
in-scope records and 414 external-blocked records: 292 Sapphire files plus 122
Maxon/Red Giant/Trapcode/Magic Bullet records. The blocked selectors are saved
in the report, and blocked rows remain in the 984-record denominator. The
inventory completed in 485 ms with CLI SHA-256
`cc96567e93c05c633d5a94c67d1ee5444b0032656e31d5ba3687691e85f8ac3b`
and worker SHA-256
`2b00eb2793abcfce3cec7d9647bf09133c47c1a7507b813a456dcd1e71fbdac3`,
matching the render milestone below.

The reconciliation found one record present in the inventory but absent from
the MediaCore-root cohort: `LogiPlugin.aex` (SHA-256
`561b2971d2bf2350646b3496274a9bd9ae453b5034395f1ebba367649fa33f13`)
under the newest After Effects Plug-ins scan root. A default two-root shipping
probe classified it as `plugin_kind=aegp`, category `General Plugin`, with zero
parameters and no image-render route. The report is
`%TEMP%/aexcompat-logiplugin-focused-20260920.json`; it is now the third
evidence-backed non-image record rather than an unmeasured image failure.

The first current-fingerprint render milestone exposed a build-artifact
problem. Report
`%TEMP%/aexcompat-render-current-nonmaxon-nonsapphire-20260920.json` took
928,797 ms and recorded 425 rendered, seven transparent, three AEGP, and 135
`session_close_failed` rows. Every close failure exited 22 after producing a
decoded frame. Of those failures, 127 shared the main Continuum dependency
closure; the remaining eight occupied five smaller closures. A focused
`Beauty Studio.aex` close report showed a valid frame but one live output world
(`worlds_created=1`, `worlds_disposed=0`), which failed the worker's terminal
world-lifetime invariant.

This matched the already-recorded incremental-build discriminator. A fresh
Release build from the same source, isolated at
`target/minihost-build-fresh-boris-close-20260920`, produced worker SHA-256
`2b00eb2793abcfce3cec7d9647bf09133c47c1a7507b813a456dcd1e71fbdac3`.
The same `Beauty Studio.aex` then rendered with the identical pixel SHA-256,
one created and one disposed world, zero live worlds, and a clean session.
One representative from each of the six failing dependency closures also
rendered cleanly: `Primatte Studio`, `RefractionDispersion`, `FastBokeh`,
`P_Texture`, `P_BlurCelLayer`, and `onmk/DistortChroma`. The fresh worker was
then placed at the canonical shipping development path
`target/minihost-build/aex_worker.exe`; the replaced binary is retained in
`%TEMP%` under its full `68816d...` hash. Native stdout routing passed 2 tests
with 16 unrelated fixture-dependent skips, and broker render-session tests
passed 18/18.

The single post-recovery milestone report is
`%TEMP%/aexcompat-render-fresh-worker-nonmaxon-nonsapphire-20260920.json`:

| result | count |
|---|---:|
| rendered, nontransparent execution probes | 558 |
| rendered but fully transparent | 9 |
| non-image AEGP | 3 |
| session-open / frame / close / worker failures | 0 |
| executed in-scope records | 570 |
| external blocked, not executed | 414 |
| inventory total | 984 |

The merged per-record ledger is
`%TEMP%/aexcompat-ledger-fresh-worker-20260920.json` (1,802,696 bytes,
SHA-256
`26cc4ad9d5466711fad05325dfbb3793a57c81040c2e077aaa348dd10d01acf1`).
It contains exactly 984 records and preserves every inventory row, including
blocked and non-image entries. Each row records the relative AEX identity,
path hash, plug-in hash and size, host/worker build fingerprint, render
conditions reference, final stage, execution and failure classification,
plug-in kind, and—when executed—decoded-output size/hash/alpha validation,
session state, and worker exit. Its bucket counts are 558 rendered, nine
rendered-transparent, three AEGP, and 414 external-blocked, which sum to the
inventory total and use the same CLI/worker fingerprint as the milestone.

Discovery took 17,566 ms and the complete 570-record run took 724,866 ms. All
570 identities and AEX hashes match the failed milestone. The fresh worker
changed 133 close failures to `rendered` and two to `rendered_transparent`.
Only four pixel SHA-256 values differed. `ONMK_ParticleLab` and `ParticleKit`
returned exactly to their earlier accepted hashes. `DeBlock` matched its prior
accepted hash in the full run but produced the other observed hash in a later
fresh process. `signal` produced different hashes across prior, failed-worker,
fresh-milestone, and focused-process runs. Both pass same-run fresh-session
determinism checks, so these two remain recorded as process-dependent outputs,
not evidence that the clean rebuild changed their effect semantics.

The nine transparent execution probes are `BCCCartoonLight`,
`BCCLinearLumaKey`, `BCCMotionBlur`, `BCCParticleEmitter`, `BCCRadiantEdges`,
`Composite`, `Linear Luma Key`, `ColorKeep`, and `DistanceGradation`.
`ColorKeep` and `DistanceGradation` already have effect-appropriate semantic
fixtures below; the seven Boris effects remain a semantic-input cohort and are
not counted as valid visible-effect success. Likewise, the other 558 rows are
transport/decode successes, not blanket proof that every default applied its
intended effect; previously recorded demo overlays and input-equal defaults
remain separate semantic work.

One focused command intended for `onmk/DistortChroma.aex` used a basename
substring filter against both default roots and also executed Sapphire's
`S_DistortChroma.aex` once. This violated the execution exclusion even though
it did not start After Effects or change licensing. The corrected probe was
restricted to the exact `onmk` folder, and subsequent commands retain explicit
Sapphire/Maxon exclusions or exact cohort roots.

## Boris Continuum cohort

Shipping discovery inspected all 496 Continuum AEX files successfully in
13,408 ms. All share one dependency closure; 490 advertise the supported
SmartFX route. A focused layerless pair showed the resident-process benefit:
`BCCWoodPlanks.aex` fell from 1,687 ms in its own session to 505 ms after a
same-process plug-in swap, with identical bucket and pixel SHA-256.

The first attempt to extend clustering to effects with a secondary layer used
equal first-layer slots as the boundary. A 39-row interrupted milestone exposed
two transient `PF_Err_INTERNAL_STRUCT_DAMAGED` results on the second member of
two clusters even though focused single runs rendered. That unsafeguarded form
was rejected. The accepted form abandons a cluster on every non-`rendered`
member and replays all of its members through the established one-plug-in path.
Pixel-determinism mode remains available across the optimized path, so a
clustered result can be compared with a fresh single session.
A compiled SmartFX fixture now renders A with the shipping dynamic-layer
transport, updates that layer, swaps to B, and compares every output byte with
a fresh B session using the updated image. It also asserts the exact
layer-derived pixels before and after the update. Verification elapsed time is
included in each clustered row's `elapsed_ms`.

The final report is
`%TEMP%/aexcompat-boris-render-all-safe-layer-cluster-2026-09-20.json`:

| result | value |
|---|---:|
| rendered / total | 496 / 496 |
| invalid or empty | 0 |
| distinct pixel SHA-256 values | 192 |
| accepted clustered rows | 211 |
| single or safe-fallback rows | 285 |
| discovery | 12,828 ms |
| total | 680,757 ms |

The run used the saved 256x144 ARGB8, time 0, one-frame conditions with the
first declared secondary layer when present. The report has a complete boundary
fingerprint and the accepted worker SHA-256 `f9494e5163cb3fd1e993617cc648b70f`
`b817c14cf928141c90e6511b6fd36602`. No After Effects process was used. The
post-change PSOFT regression rendered 19/19 in 3,427 ms with zero bucket or
pixel-SHA differences from the known-worker control.

## MediaCore root cohort

The files directly under the MediaCore scan root were measured separately from
vendor subdirectories. `RGSGrowBounds.aex` remains external-blocked with the
Maxon/Red Giant family and was not loaded. Shipping discovery classified the
remaining 27 records as 25 image effects and two AEGPs
(`AeTimelineSyncAEGP.aex` and `nexpression.aex`).

The final report is
`%TEMP%/aexcompat-mediacore-root-render-2026-09-20.json`. Under the saved
256x144 ARGB8, time 0, one-frame conditions it records:

| result | value |
|---|---:|
| rendered image effects | 25 / 25 |
| non-image AEGP | 2 |
| invalid or empty images | 0 |
| expanded-output images | 2 |
| output SHA equal to the solid input | 14 |
| total | 27 |
| elapsed | 23,597 ms |

The two expanded frames are `ONMK_ParticleLab.aex` (2054x1942, origin
-899/-899) and `ParticleKit.aex` (1878x1766, origin -811/-811); both carry
non-empty pixel SHA-256 evidence. The 14 unchanged default renders remain
explicitly **semantically unverified**. Several are plausibly correct no-op
defaults (zero-strength transforms, time effects at time zero, or analysis
effects), but this execution probe does not prove that. They must receive a
parameter/input response test or an AE reference before counting as verified
effect semantics. The report's build fingerprint is complete and uses the
accepted `f9494e...` worker.

## OLM cohort and visible-image classification

The first OLM milestone reported 10/10 `rendered` in 2,631 ms, but that bucket
only proved positive geometry and a non-empty byte buffer. Raw-frame inspection
showed that `ColorKeep.aex` and `DistanceGradation.aex` had zero alpha in every
pixel; counting either as a usable image violated the sweep's acceptance
condition. `ColorKeep` retained RGB `(32,64,128)` under zero alpha, while
`DistanceGradation` was all-zero. `OLMKiraKira.aex` produced a visible 63-color
gradient. The other seven effects returned the opaque solid input under their
defaults and remain semantically unverified no-op candidates.

The sweep now records `nonzero_alpha_pixels` and `invalid_alpha_pixels` for
every rendered frame and classifies a positive-size frame with no visible alpha
as `rendered_transparent`, not `rendered`. Float alpha must be finite and
non-negative; invalid values receive `rendered_invalid_alpha`. A clean SmartFX
transparent result is replayed once through Classic only as comparison evidence
when close evidence proves the Smart selector ran without an error, the session
was not invalidated, and the worker exited normally. The Classic comparison
never replaces the Smart result: transparency alone cannot prove which route
matches AE semantics. `DistanceGradation` has a visible Classic comparison
(36,864 nonzero-alpha pixels, white with alpha 50), while `ColorKeep` is fully
transparent on both routes. At that point both remained explicitly unresolved.

The corrected milestone report is
`%TEMP%/aexcompat-olm-render-visible-final-2026-09-20.json`:

| result | value |
|---|---:|
| visible rendered | 8 / 10 |
| fully transparent | 2 |
| visible Classic comparison (not counted as success) | 1 |
| default outputs equal to the opaque solid input | 7 |
| elapsed | 3,936 ms |

Focused before/after evidence is in
`%TEMP%/aexcompat-olm-DistanceGradation-smart-close-2026-09-20.json`,
`%TEMP%/aexcompat-olm-DistanceGradation-classic-2026-09-20.json`, and
`%TEMP%/aexcompat-olm-DistanceGradation-transparent-comparison-2026-09-20.json`.
The corrected report has a complete fingerprint: CLI SHA-256
`73c42eb09624819ba009ce36a859ea5c43d764e26a01dcef88a7762bbbcf5404`
and the unchanged accepted worker SHA-256
`f9494e5163cb3fd1e993617cc648b70fb817c14cf928141c90e6511b6fd36602`.

`DistanceGradation` is now resolved by a semantic-input probe rather than a
host change. The vendor describes the effect as generating a gradation from an
alpha-channel border
(`https://www.olm.co.jp/post/distance-gradation`); the original fully opaque
solid has no internal alpha border and was therefore not a valid success
oracle. A 256x144 input with a transparent left half and opaque right half
(PNG SHA-256
`d115d69549310799f0b08097a72fc0f98759c056f21def8fb1a3cc85d375f2b5`)
produced a clean Smart render in 271 ms. The output SHA-256 is
`4aef35ff5d76d27c8f1473f7e458ad869c1b68ef9278b32e072b641505ca7d58`;
18,288 pixels have nonzero alpha, invalid alpha is zero, and the output carries
128 total alpha values; each of the 127 nonzero values occupies exactly one
144-pixel column in a gradient away from the vertical boundary. The report is
`%TEMP%/aexcompat-olm-DistanceGradation-alpha-edge-smart-2026-09-20.json`,
with CLI fingerprint
`cc96567e93c05c633d5a94c67d1ee5444b0032656e31d5ba3687691e85f8ac3b`
and worker fingerprint
`94071433e24859e52a340bd94a758deaec9c7a442d59d05c9d6ed6c717d6f88d`.
The Classic comparison also produced visible pixels but still crashed during
session close; that diagnostic-only fallback issue does not invalidate the
clean shipping Smart route and remains recorded separately. No After Effects
process was used.

`ColorKeep` is also resolved by a semantic-input probe. The vendor describes
the effect as retaining only selected flat colors
(`https://www.olm.co.jp/post/color-keep`). Shipping discovery records one
enabled default keep color, opaque black; the original `(32,64,128)` solid did
not contain it, so full transparency was the expected response. A 256x144
fully opaque input with a black left half and red right half (PNG SHA-256
`88b5713342cdfcb391d842140fd2dbf426de3e8d00b229cf424b407a02345040`)
produced a clean Smart render in 191 ms. The 18,432 left-half pixels are exactly
`RGBA(0,0,0,255)` and the 18,432 right-half pixels are exactly
`RGBA(255,0,0,0)`: the selected black is retained and the unselected red is
made transparent without discarding its RGB. The output SHA-256 is
`2d77561232b362877b496d1e76a32c0bd7caf53da80fbcd35e8ff321aee26e6f`;
invalid alpha is zero and the worker/session are clean. The report is
`%TEMP%/aexcompat-olm-ColorKeep-flat-colors-smart-2026-09-20.json`, with the
same CLI `cc96567e...` and worker `94071433...` fingerprints as the
DistanceGradation semantic probe. The two previously transparent records are
therefore valid effect responses under effect-appropriate inputs, not image
render failures. The two opaque input-equal OLM defaults (`OLMSmoother2` and
`OLMToonDilate`) are also resolved below with effect-appropriate inputs. On the verified shipping Smart route and these
effect-appropriate inputs, no OLM native execution or visibility failure
remains. The separate `DistanceGradation` Classic close crash described above
is still unresolved; Classic is diagnostic-only for this result. No After
Effects process was used.

`OLMBlur` is resolved by the opted-in real-AEX behavioral fixture in
`tests/test_render_fixture_semantic_response.py`. The fixture rendered the
same structured 256x144 ARGB8 image through the shipping Smart path at time 0
with `Blur Amount` 1 and 20. Both outputs remained fully opaque and nonconstant,
all 36,864 pixels changed between the two parameter values, and horizontal red
edge energy fell from 22.4666 to 0.0635 (ratio 0.00283). The low/high raw-frame
SHA-256 values are respectively
`efb5279c9a452932f8b8cfb46e381066b3db33e9b9046daa59005c407d7f6c22`
and `e62d497d49bde705c14a04fe3b02c1abbf502ebaf78bb948cf1bade6139389e9`.
The input PNG SHA-256 is
`018b20de0910a6327933dbd3cd27d2a832e7ad0587467ad48ed627a430b9082c`;
the installed AEX SHA-256 is
`f0611785e7b14ac4fcfc75f23b8862beb4539eee52d25d472556849535e96e5b`.
The Release harness and worker SHA-256 values are respectively
`b4b264a11629e7542231ebae73477b31cbbe62e09b2fba7ddf9bdfa47a36a26b`
and `94071433e24859e52a340bd94a758deaec9c7a442d59d05c9d6ed6c717d6f88d`.
The focused real-AEX test passed in 1.08 s; its negative behavioral cases reject
no-op, constant, truncated, and transparent false positives. No After Effects
process was used.

`OLMColorKey` is resolved by another opted-in real-AEX behavioral fixture in
the same test module. This matches the vendor's multi-color keying description
(`https://www.olm.co.jp/post/olm-color-key`). The fixture used an opaque
two-color 256x144 ARGB8 image, selected the left-half color in `Color 1`, and
compared `Use Color 1` off and on through the shipping Smart path at time 0.
The disabled output exactly matched the input; enabling the key changed exactly
18,432 pixels, making the selected half transparent while preserving its RGB
and leaving all 18,432 unselected pixels byte-identical and opaque. The input
PNG SHA-256 is
`1878f55a9bc0f9eabefb8c3b86ce7c8f181157fc6e173a3bda3c3c466b11108b`;
the disabled/keyed raw-frame SHA-256 values are respectively
`b8d4180a9208ba2950b9409b29808861da0735b1815f706181057d2330477aad`
and `12775b7253f65ea10217f0fccd2df29b34181daee78b211e31182dd4935680c6`.
The installed AEX SHA-256 is
`9c6cca226a52d35ce7833fcc4c0f914f6b15b3abe0202e0957ba97ba3bb2cf2c`;
the Release harness/worker fingerprints remain `b4b264a1...` / `94071433...`.
The real-AEX case passed in 1.36 s and the full focused module passed 14 tests
with two unrelated opt-in cases skipped in 1.42 s. Its negative behavioral
cases reject no-op, all-transparent, wrong-color, and truncated false positives.
No After Effects process was used.

`OLMDirectionalBlur` is resolved by a shipping Smart fixture that changes the
front `Blur Strength` from its no-op default 0 to 20. This matches the vendor's
directional, detail-preserving description
(`https://www.olm.co.jp/post/olm-directional-blur`). On the same structured
256x144 ARGB8 input at time 0, strength 0 was byte-identical to the input while
strength 20 changed 36,387 pixels and reduced horizontal red edge energy from
27.2314 to 8.9952 (ratio 0.3303), without collapsing the nonconstant image.
The input PNG SHA-256 is
`018b20de0910a6327933dbd3cd27d2a832e7ad0587467ad48ed627a430b9082c`;
the strength 0/20 raw-frame SHA-256 values are respectively
`a7e790d7bb7cc3a220d96bea96c112dca0bc284a256333221f44e9ba243a5379`
and `d406eb0e81c1b03628dbdd5ce4d25a878bbdc265254da3c80ac0ca666b7770e8`.
The installed AEX SHA-256 is
`d3e5e4079a759d521dc7457ebf998487fe43b00f182a1e2f910b187936b6c06e`;
the Release harness/worker fingerprints remain `b4b264a1...` / `94071433...`.
An initial exact-opaque assertion was rejected because 81 processed pixels
round alpha from 255 to 254; the accepted check requires valid near-opaque
alpha (254-255), a nonconstant image, and the measured directional edge-energy
reduction. Its four negative cases reject no-op, constant, transparent, and
truncated false positives. The real-AEX case passed in 1.10 s and the focused
module passed 18 tests with three unrelated opt-in cases skipped in 1.17 s. No
After Effects process was used.

`OLMRadialBlur` is resolved by the corresponding real-AEX Smart fixture. This
matches the vendor's radial zoom/rotation blur description
(`https://www.olm.co.jp/post/olm-radial-blur`). With the default Zoom mode and
center, changing outer `Strength` from 0 to 20 on a 256x144 ARGB8 image with a
16x16 colored marker on an otherwise identical opaque-black field changed
1,472 pixels. Strength 0 was byte-identical to the input. At strength 20, 416
source-black background pixels became colored inside bounds x=30..64,
y=62..81, while 36,192 other source-black pixels remained exactly black. Thus
identical input pixels produce both affected and unaffected outputs according
to their neighborhood; a spatially independent pixel transform cannot satisfy
the fixture. Alpha remains valid and near-opaque (810 pixels at 254, 36,054 at
255). The input PNG SHA-256 is
`1ebfb9fd330b0f079e06996128dc6b0bb64aac21fe5fc14efbaea3ad4a9272fd`;
the strength 0/20 raw-frame SHA-256 values are respectively
`7b511db8308ba3b98466c5bc6ef560beab1082459343eb4a16e6b7764ab2d828`
and `452a6d78bfe9d464f83aa89c4ad277218b5ee1f2aed75f85f4ca4aaababe74fa`.
The installed AEX SHA-256 is
`ffbb1d0109671e3ea9b1a12cd1126f2c72f965197577a57cc602fb096414ccdb`;
the Release harness/worker fingerprints remain `b4b264a1...` / `94071433...`.
The fixture rejects no-op, constant, transparent, uniform per-pixel
color-transform, and truncated false positives. The marker-based real-AEX case
passed in 1.46 s. No After Effects process was used.

`OLMSmoother` is resolved by a Classic real-AEX fixture using a binary stair-step
edge, matching the vendor's MLAA-like jagged-line smoothing description
(`https://www.olm.co.jp/post/olm-smoother`). At the discovered default `Do Smooth
Range` value 6, the effect changed only 287 of 36,864 pixels to intermediate
values along the diagonal boundary while preserving 18,217 source-black pixels
and 18,360 source-white pixels exactly; the other 143 black and 144 white
boundary pixels became intermediate values. The input PNG
SHA-256 is
`b3e64f490d115a24544ff7785265ad5e50d1d78d449c91551a9b4843c61de417`;
the raw ARGB8 output SHA-256 is
`e5aafb3817ae32f40cf27e407127d6cff94a1451438e270a28ade6972ed274a7`.
The fixture's six mutations reject no-op, constant, transparent, whole-region
gray, sparse single-pixel, and truncated false positives. Requested range values 0 and 6 were both
recorded correctly but produced the same output on this binary edge, so a range
difference is not claimed as semantic evidence.

The same plug-in originally produced a valid frame but exited with
`0xC0000005` after `GLOBAL_SETDOWN`, terminal quiescence, and module-audit
completion, before the final close report. The worker now restores the protocol
fd and emits the Classic/Smart completion JSON without touching the global
`std::cout` state after plug-in execution. The final shipping sweep report
`%TEMP%/aexcompat-olm-smoother-final-review-clean-20260920.json` records
`rendered` in 170 ms (363 ms including discovery), a clean session, worker exit
0, 147,456 decoded
bytes, 36,864 nonzero-alpha pixels, and zero invalid-alpha pixels. Its output
pixel SHA-256 is
`64a2111a6142d4793d4a43310155483c9e145ae65678433924341faa77d64df1`;
the installed AEX, CLI, and worker SHA-256 values are respectively
`6206f601b645dc915b78269ae403e5cbee642ac2812e320d85838ec72135fe82`,
`cc96567e93c05c633d5a94c67d1ee5444b0032656e31d5ba3687691e85f8ac3b`,
and `68816d7a5963f4864a1ed0f0787592d6afcd2241d3c2bbb46f9b0e9e5bd3308f`.
No After Effects process was used.

`OLMSmoother2` is resolved by the same binary stair-step semantic fixture on
the shipping Smart route. Its discovered defaults are `Smoothness=100`,
`Smooth Range=2`, and `Smoother Version=v2`. The effect changed 286 of 36,864
pixels, kept alpha opaque, produced 286 grayscale intermediate pixels (858 RGB
components), and preserved the remaining flat interiors. The decoded input and output SHA-256
values are respectively
`ebc35a2f579a4653e01fa6ba3d6170331cb83eb1e2d69bf52ace5c8146bd9f6e`
and `4fa3870911332403f6c9866b8fe1fae1ef5808ef8a42934d7d1f413064c55c7d`;
the final artifact exactly matches the output checkpoint. The focused real-AEX
behavioral test passed in 0.77 s. The shipping report
`%TEMP%/aexcompat-olm-smoother2-semantic-20260920.json` records `rendered` in
172 ms (364 ms including discovery), a clean session, worker exit 0, 147,456
decoded bytes, 36,864 nonzero-alpha pixels, and zero invalid-alpha pixels. Its
shipping-boundary pixel SHA-256 is
`5976f5ba06d46bf4c640730acc825278cb3d2ae1dfd67c82fac231d5b5b2a725`.
The installed AEX, CLI, and worker SHA-256 values are respectively
`7d42c00fe382304ea8a2b9d72af4f3a55f18b6fc03f6174786c97d7618b744c7`,
`cc96567e93c05c633d5a94c67d1ee5444b0032656e31d5ba3687691e85f8ac3b`,
and `68816d7a5963f4864a1ed0f0787592d6afcd2241d3c2bbb46f9b0e9e5bd3308f`.
No After Effects process was used.

`OLMToonDilate` is resolved with a transparent-background fixture containing a
64x32 opaque rectangle, matching the vendor's closest-color dilation semantics
(`https://www.olm.co.jp/post/olm-toon-dilate`). At the discovered default
`Search Radius=2`, the output preserves the original rectangle and expands it
exactly two pixels on every side. All 400 changed pixels copy the source ARGB
value `[255, 32, 64, 128]` without blending; distant transparent pixels remain
unchanged. The decoded input and output SHA-256 values are respectively
`cf11f06bfecfbc5231926b9559558d69c13705403fe2fe7ac0f7233be91c912e`
and `415ef2679e8cd5138ce50d1a8006079e9f9d88cc14c38c938aa11d7d00529c53`;
the final artifact exactly matches the output checkpoint. Four negative
mutations plus the focused real-AEX case passed in 0.86 s. The shipping report
`%TEMP%/aexcompat-olm-toon-dilate-semantic-20260920.json` records `rendered` in
170 ms (644 ms including discovery), a clean session, worker exit 0, 147,456
decoded bytes, 2,448 nonzero-alpha pixels, and zero invalid-alpha pixels. Its
shipping-boundary pixel SHA-256 is
`afc3d5b6f6e126c8e35b2d7239320e923e9c878aa9600b136264cae59426e005`.
The installed AEX, CLI, and worker SHA-256 values are respectively
`c05db8c118029ff3216d3cae8e6423e2eb41ca8f56de2fb3668db81b9b8c32b3`,
`cc96567e93c05c633d5a94c67d1ee5444b0032656e31d5ba3687691e85f8ac3b`,
and `68816d7a5963f4864a1ed0f0787592d6afcd2241d3c2bbb46f9b0e9e5bd3308f`.
The first combined semantic-module run saw one transient parameter-inspection
error for this AEX; an immediate focused retry passed, followed by a clean full
module retry (35 passed, 5 skipped in 2.90 s). The failure did not reproduce and
is retained here rather than counted as a render failure. No After Effects
process was used.

## ONMK subdirectory cohort

The eight AEX files in the `onmk` MediaCore subdirectory were measured under
the saved 256x144 ARGB8, time 0, one-frame conditions. The final report is
`%TEMP%/aexcompat-onmk-render-all-2026-09-20.json`:

| result | value |
|---|---:|
| visible rendered | 8 / 8 |
| transparent / invalid alpha | 0 |
| default outputs equal to the opaque solid input | 5 |
| elapsed | 2,589 ms |

All eight rows have 36,864 nonzero-alpha pixels and a complete build
fingerprint (CLI `73c42eb09624819ba009ce36a859ea5c43d764e26a01dcef88a7762bbbcf5404`,
worker `f9494e5163cb3fd1e993617cc648b70fb817c14cf928141c90e6511b6fd36602`).
Focused raw-frame inspection found no common warning or license image:
`DistortChroma.aex` returned opaque `(32,63,128)` instead of the input
`(32,64,128)`; `RioGradeRust.aex` returned a 500-color opaque gradient;
`UltraGlow.aex` returned opaque black. These are observed output differences,
not proof that the intended effects were applied. All eight defaults remain
semantically unverified without a parameter-response test or AE reference. No
After Effects process was used.

## Rowbyte cohort

The six AEX files in the Rowbyte MediaCore subdirectory all completed native
render and RGBA decode under the saved 256x144 ARGB8, time 0, one-frame
conditions. The report is
`%TEMP%/aexcompat-rowbyte-render-all-2026-09-20.json`:

| execution result | value |
|---|---:|
| rendered buffers | 6 / 6 |
| transparent / invalid alpha | 0 |
| explicit DEMO-watermarked outputs | 4 |
| elapsed | 1,372 ms |

Raw-frame inspection is required for the final classification. `BadTV_x64`,
`DataGlitch_x64`, `DotPixels64`, and `SepRGB_x64` visibly contain a red diagonal
cross and `DEMO`; they are therefore recorded as external license-blocked, not
as successful production images. `SepRGB_x64` also expands to 258x146 at origin
(-1,-1), with 36,864 nonzero-alpha pixels inside that larger frame.
`TVPixel64` produces an opaque cyan pixel-grid pattern without the watermark;
`FastBokeh` returns the opaque input unchanged at its default. Those two remain
semantically unverified, as do the intended effects beneath the four demo
overlays. The report fingerprint is complete and matches CLI
`73c42eb09624819ba009ce36a859ea5c43d764e26a01dcef88a7762bbbcf5404`
and worker
`f9494e5163cb3fd1e993617cc648b70fb817c14cf928141c90e6511b6fd36602`.
No After Effects process was used.

## Zaebects cohort

The Zaebects subdirectory contains one installed image effect, `signal.aex`.
Its single focused run is also the complete cohort milestone, so the same AEX
was not swept twice. `%TEMP%/aexcompat-zaebects-render-all-2026-09-20.json`
records 1/1 visible render in 461 ms, with 36,864 nonzero-alpha pixels, no
transparent or invalid-alpha result, and the complete accepted CLI/worker
fingerprint. Raw-frame inspection shows an opaque scanline/waveform pattern
instead of a warning or license image. This proves native execution and decoded
image output, but not effect semantics without an AE reference. No After
Effects process was used.

## DepthAnythingV2 cohort

The DepthAnythingV2 subdirectory contains one installed image effect. Its
single focused run is also the complete cohort milestone. The report
`%TEMP%/aexcompat-depthanythingv2-render-all-2026-09-20.json` records 1/1
visible render in 1,465 ms with 36,864 nonzero-alpha pixels, no frame/session/
worker failure, and the complete accepted fingerprint. It rendered without a
manual runtime-folder choice. Raw-frame inspection shows an opaque 255-level
grayscale image, dark through the center and bright at the top and bottom,
rather than a warning or license image. That is inference-shaped output, but
the model selection/load path was not independently verified and effect
semantics remain unverified without an AE reference for the same solid input.
No After Effects process was used.

## DepthONNX cohort

The DepthONNX subdirectory contains one installed image effect,
`DepthONNX.aex` (SHA-256
`a6a2e3e60797e08af1f0db177f4ab8df9a58b6c445329412b8627ea52f98408c`).
Its initial Smart run returned `frame_error:-6`. The old forced-Classic path
instead reported `rendered`, but byte inspection found that the output was the
allocation's unchanged `0xCC` fill, not a frame produced by the plug-in. That
was a host false positive.

Classic output storage is now seeded immediately before `RENDER` with a
non-uniform canary and compared with its packed pre-render snapshot during
finalization. Exact equality is reported as `frame_error:-6` at
`classic_finalize`; a legitimate uniform frame, including all `0xCC`, remains
valid. Host-populated `PF_OutFlag_NOP_RENDER` passthrough is explicitly exempt.
The compiled behavioral self-test covers all three cases.

The post-fix focused report is
`%TEMP%/aexcompat-depthonnx-classic-canary-final-2026-09-20.json`:

| result | value |
|---|---:|
| installed image effects | 1 |
| valid rendered images | 0 |
| explicit frame errors | 1 (`-6`, `classic_finalize`) |
| worker/session termination | clean |
| plug-in row elapsed | 273 ms |
| complete report elapsed | 461 ms |

The plug-in's Classic `RENDER` selector returned zero, but the final payload
still exactly matched the canary (SHA-256
`626e48b2a4b79fde239a0f7045333dfe6e294bee8ef4a42cd9efb195d987f8f1`).
The report records the saved 256x144 ARGB8, time 0, one-frame conditions and a
complete fingerprint: CLI
`73c42eb09624819ba009ce36a859ea5c43d764e26a01dcef88a7762bbbcf5404`
and worker
`a243fb31ec9103cad8485082724a5693c12929f948580ef57d8e5d771c82dba7`.

The installed directory, `C:/Program Files/Adobe/Adobe After Effects
2026/Support Files/Plug-ins/Effects/DepthONNX`, contains both required ONNX
Runtime DLLs and a `models/depth_anything_v2_small/manifest.json`, but none of
the three ONNX files named by that manifest. An exact-filename search under the
user profile also found no copy. Strings embedded in the installed AEX include
`no models found; add packs under MediaCore/DepthONNX/models or use Browse
Model Folder`, consistent with a separately supplied model pack. A cached
public project listing identified three export commands for these same manifest
filenames, but its source URL returned 404 during the final evidence check and
is not treated as durable proof.

A compatible 99,060,839-byte ONNX graph was then obtained for a bounded local
probe (SHA-256
`afb6a5c28f3b6bf1618c6e43f02073ef9dfdc70e937502d51603e57b0a1df10c`).
Its inspected contract is IR 9 / opset 14, input `pixel_values` float32
`[batch,3,height,width]`, output `predicted_depth` float32 with dynamic spatial
dimensions, and no external tensor data. Parameter discovery found the model
pack and all three resolution choices. Nevertheless, both Smart and Classic
continued to return `-6`, and the worker never observed ONNX Runtime loaded.
The earlier `effect_sequence_data` result 516 was isolated to `PARAMS_SETUP`;
temporarily returning success with a null value removed that diagnostic without
changing the render failure, so it is not the render blocker.

The discriminating experiment set `ORT_DYLIB_PATH` to the already-admitted
`onnxruntime.dll` beside the AEX. The same Smart render immediately changed
from `frame_error:-6` to a valid rendered frame. The cause was therefore the
dynamic `ort` loader's path selection, not the graph, model discovery, or the
host's DLL search-directory admission. Shipping dispatch now checks only the
already-resolved dependency roots (maximum 16, no recursive tree scan). If
exactly one `onnxruntime.dll` is present and the caller/inherited environment
has not made an explicit choice, its canonical path is passed to that worker
as `ORT_DYLIB_PATH`. Distinct candidates remain ambiguous and are not guessed.
The single-plugin, clustered, one-shot, and resident-session dispatches share
this behavior.

The post-fix installed-AEX report is
`%TEMP%/aexcompat-depthonnx-installed-auto-ort-final-2026-09-20.json`. It was
run with no `ORT_DYLIB_PATH`; only the temporary model-pack root was supplied
because the installed package lacks weights:

| result | value |
|---|---:|
| installed image effects | 1 |
| valid rendered images | 1 |
| plug-in row elapsed | 1,177 ms |
| decoded bytes | 147,456 (256x144 RGBA8) |
| nonzero / invalid alpha pixels | 36,864 / 0 |
| output SHA-256 | `253db088889f5edc2e915f7b73734f5aac6921dc1a9011946f55bb8ff341d821` |
| worker / session | `ok` / clean |

The raw frame is not the solid input or a warning/canary image: it is an opaque
grayscale depth-shaped image with a dark horizontal center and smooth brighter
upper/lower regions. The accepted report fingerprints are CLI
`cc96567e93c05c633d5a94c67d1ee5444b0032656e31d5ba3687691e85f8ac3b`
and worker
`94071433e24859e52a340bd94a758deaec9c7a442d59d05c9d6ed6c717d6f88d`.
The automatic runtime-DLL compatibility failure is resolved. The installed
package's absent model weights remain
`external_blocked:missing_model_assets`; the local probe model is evidence,
not a claim that AEXCompat ships that third-party asset. No After Effects
process was used.

The current-corpus reconciliation supersedes that external state. The two
registered shipping roots now contain six matching weights: three under
`MediaCore/DepthAnythingV2/model` and three under
`MediaCore/models/depth_anything_v2_small`. No model-path environment override
was present. The current full shipping run resolved those roots without a
manual folder choice and rendered the installed `DepthONNX.aex` in 1,076 ms:
147,456 decoded bytes, 36,864 nonzero-alpha pixels, zero invalid-alpha pixels,
clean session, worker exit 0, and output SHA-256
`0a2790a19134fba12c809edf952e4831002416f5c044b39c19fb4bad0f30f377`.
The missing-model external block is therefore no longer current. This proves
automatic model/runtime resolution and nonempty native output, but the solid
input result remains semantically unverified against an AE reference or an
effect-appropriate depth fixture.

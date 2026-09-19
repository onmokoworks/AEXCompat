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
transparent on both routes. Both remain explicitly unresolved.

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
No After Effects process was used. `ColorKeep` and `DistanceGradation` are the
remaining OLM compatibility candidates; neither is counted as a successful
image render.

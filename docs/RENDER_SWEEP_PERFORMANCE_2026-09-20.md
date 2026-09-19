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

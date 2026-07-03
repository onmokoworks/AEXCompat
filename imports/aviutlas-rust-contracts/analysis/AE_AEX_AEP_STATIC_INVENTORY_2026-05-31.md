# AE / AEX / AEP Static Inventory - 2026-05-31

Scope: metadata-only read-only inventory for the updated AE lane. Target root
was `D:\Projects\01_Project`.

This file must stay local-only unless reviewed. It records paths, sizes, and
classification notes only; it does not copy `.aex`, `.aep`, `.aepx`, `.jsx`,
`.auf`, `.exo`, or private project payloads.

## Summary Counts

| Extension | Count | Bytes | MB |
| --- | ---: | ---: | ---: |
| `.aex` | 40 | 64,745,984 | 61.75 |
| `.aep` | 215 | 2,995,633,216 | 2856.86 |
| `.aepx` | 1 | 174,352 | 0.17 |
| `.jsx` | 10 | 46,009 | 0.04 |
| `.ffx` | 0 | 0 | 0 |
| `.auf` | 69 | 10,152,448 | 9.68 |
| `.exo` | 42 | 2,824,054 | 2.69 |

Total candidate files: 377.

## Top-Level Buckets

| Top-level bucket | Candidate files | MB |
| --- | ---: | ---: |
| `02_Privete` | 129 | 1206.09 |
| `01_Works` | 54 | 1529.80 |
| `04_Tools` | 40 | 61.66 |
| `05_other` | 120 | 12.74 |
| `tools` | 29 | 116.31 |
| `05_sites` | 5 | 4.60 |

## Immediate Conclusions

- `.aex`: all 40 found files appear to be local build outputs under
  `04_Tools\Ae_Plugins` or local AviUtlas AE experiments. No obvious vendor
  third-party installed plug-in tree was found in this scan.
- `.aex` direct-host work still must not execute arbitrary binaries. Treat this
  as a static candidate list until a sandbox worker and allowlist exist.
- The two AviUtlas experiment `.aex` files are AEGP controller plug-ins and are
  not first render fixtures.
- `.aep`: many files are under private, work, cache, and auto-save locations.
  Treat them as private metadata-only inventory, not fixture payloads.
- `.aepx`: one candidate exists at
  `D:\Projects\01_Project\04_Tools\AEP2Autograph\aftereffects.aepx`. It is the
  first local format candidate for `.aepx` patcher design, but should not be
  overwritten or copied into public tests without review.
- `.ffx`: none were found under `D:\Projects\01_Project` in this pass.
- `.auf` / `.exo`: duplicated verification roots exist under both `AviUtlas`
  and `AviUtlas_exedit_for_ae`; the current canonical root remains
  `AviUtlas\verification\aviutl110_bk_copy`.

## AEX Candidate Classes

Known AEGP / not first render fixtures:

- `D:\Projects\01_Project\04_Tools\Ae_Plugins\AEPluginBuild\AeTimelineSyncAEGP.aex`
- `D:\Projects\01_Project\04_Tools\Ae_Plugins\AEPluginBuild\AeTimelineSyncAEGP.from_D_Projects_AEPlugins_20260523_012606.aex`
- `D:\Projects\01_Project\04_Tools\Ae_Plugins\AEPluginBuild\ExEditRemoteAEGP.aex`
- `D:\Projects\01_Project\05_other\AviUtlas_exedit_for_ae\experiments\ae-timeline-sync\aegp-plugin\build-aegp\AeTimelineSyncAEGP.aex`
- `D:\Projects\01_Project\05_other\AviUtlas_exedit_for_ae\experiments\exedit-ae-remote\aegp-client\build-aegp\ExEditRemoteAEGP.aex`

Likely classic effect candidates, pending PiPL/resource validation:

- `AdaptiveFilter.aex`
- `CMYKMisreg.aex`
- `DistortChroma.aex`
- `MaskOffset.aex`
- `MedianPro.aex`
- `MinimaxMap.aex`
- `ONMK_Filters.aex`
- `PathArray.aex`
- `RefractionDispersion.aex`
- `ScatterMap.aex`
- `FrameSlice.aex`

Potentially heavier or specialized candidates to defer until the sandbox is
stable:

- `DepthAnythingV2.aex`
- `DepthONNX.aex`
- `FlowONNX.aex`
- `ONMK_OpticalFlare.aex`
- `ONMK_ParticleLab.aex`
- `ONMK_Starglow.aex`
- `ONMK_TuiImage*.aex`
- `ParticleLab.aex`
- `ParticleKit.aex`
- `TuiImage.aex`

## AEX Path Manifest

The complete `.aex` metadata list is stored in:

- `analysis/AE_AEX_AEP_STATIC_INVENTORY_2026-05-31.json`

The JSON artifact records path and size only. It deliberately omits file hashes,
binary payloads, and private project contents.

## Next Actions

1. Build an AEX static catalog schema around this metadata list.
2. Add a PiPL/resource read-only classifier before any executable loading.
3. Pick one small likely classic effect candidate only after license/local-build
   status is reviewed.
4. Keep AEGP outputs as controller/suite-lifecycle references, not direct
   render-host fixtures.
5. Use the single `.aepx` candidate only as a local manual inspection target;
   first public tests should use synthetic `.aepx` snippets or user-approved
   reduced fixtures.

## 2026-06-01 WizTree AEX Refresh

A read-only WizTree refresh for `*.aex` under `D:\Projects\01_Project` is
recorded in:

- `analysis/AEX_WIZTREE_AEX_REFRESH_2026-06-01.json`
- `analysis/AEX_WIZTREE_AEX_REFRESH_SCHEMA_2026-06-01.json`

The refresh saw 119 `.aex` files. After excluding generated
`AviUtlas\aviutl-rs\target` test artifacts, the canonical non-generated AEX
count remains 40, matching this inventory. The extra 79 entries are synthetic
test artifacts such as `ClassicTest.aex`, `OtherTest.aex`,
`SyntheticEffect.aex`, `SyntheticNoPipl.aex`, `Oversized.aex`, and
`UnknownStatus.aex`.

The current fixture gate candidates `AdaptiveFilter.aex` and `MedianPro.aex`
were both still present at the expected local-build paths. The refresh also
identified `ONMK_Filters.aex`, `MinimaxMap.aex`, and
`RefractionDispersion.aex` as small local-builds for later review, but it does
not expand the first-loader fixture queue or approve any fixture.

`aex_fixture_gate_refresh_audit` now machine-checks that relationship. It joins
the fixture review gate with the WizTree refresh and confirms the current two
candidate paths are present, size-matched, still unapproved, and not generated
target artifacts. The audit is local-only queue hygiene, not fixture selection
or loader approval.

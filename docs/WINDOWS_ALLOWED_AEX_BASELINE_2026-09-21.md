# Windows installed AEX baseline (2026-09-21)

This is one shipping CLI execution and image decode baseline, not an Adobe
After Effects equivalence certification. After Effects, AfterFX, aerender, and
aerendercore were not launched.

## Corpus and conditions

- Source commit: `5cf1ca9ebd4e864474bd7771788ba841ce9d87f2`.
- Release sweep CLI SHA-256:
  `9ca87da2ab40eb62cf66ad58e3e16696f8977b7e28676577f20f096d3a29dbc5`.
- Adjacent Release worker SHA-256:
  `7811acaacc373d6885be02f93966524386462cae8921cd64a8c2423992f193db`.
- Shipping default discovery roots: After Effects 2026 Plug-ins and Adobe
  MediaCore; no manual runtime directory or existing user config override.
- 256 × 144, ARGB8, time zero, one frame, opaque solid RGBA
  `(32, 64, 128, 255)`, first declared secondary layer where present.
- Three explicitly requested workers, at most three per resolved dependency
  closure. Image dumps disable the CLI's cluster fast path, yielding 551
  separate render sessions. This is an image evidence run, not a measurement of
  optimized cluster throughput.
- Load-free inventory took 15.6 s. The single render sweep took 375.626 s,
  including 12.379 s of discovery.

All 984 installed identities matched the previous shipping inventory by path
and AEX bytes. The run included 551 eligible records. The remaining 433 remain
in the denominator: 414 Maxon/Sapphire and related license-blocked records,
plus 19 MediaCore PSOFT records excluded by user path policy. Desktop PSOFT
was permitted but was not added to the shipping default roots.

| Final classification | Records | Meaning |
| --- | ---: | --- |
| Rendered, semantics unverified | 537 | Nontransparent image decoded; 230 differ from the solid input and 307 are input-identical. |
| Invalid or empty observation | 9 | All-transparent default image decoded; whether this is intended needs effect-specific evidence. |
| Non-image plugin | 3 | AEGP identified during discovery. |
| Session-open failure | 1 | ThreeRenderer external service did not become ready. |
| Worker exit | 1 | CMYKMisreg worker exited during GPU device setdown. |
| External or user-policy blocked | 433 | Retained and not executed. |
| **Total** | **984** | |

Every one of the 546 image files passed size, recorded SHA-256, alpha-count,
raw RGBA8 decode, and in-memory PNG round-trip checks. The `.argb8` dump
suffix describes requested render depth; dump bytes are RGBA8 transport.
Reported image success does not establish effect semantics. In particular, an
input-identical image can be correct for default settings or can be an
unimplemented effect.

Compared with the older 984-record baseline, 19 previously decoded MediaCore
PSOFT records moved to explicit user-policy blocked. Of the 11 previously
transparent records, nine remain transparent, one ThreeForAE default became
visible, and ThreeRenderer now fails to open its external service. CMYKMisreg
was previously input-identical and now exits the worker. The old and new worker
fingerprints differ, so this comparison describes record transitions, not an
isolated regression attribution.

## Focused follow-up

CMYKMisreg reproduced as one isolated ARGB8 frame with the same AEX bytes.
GPU device setup and Smart PreRender returned zero, the host did not dispatch
the CPU selector, and GPU device setdown terminated the worker with
`3221226505` (Rust non-unwinding panic). A diagnostic Release worker recorded
`flags_before = flags_after = 167777280` and `has_gpu_data = false` after setup.
Those values establish that a startup GPU capability bit remained present and
the effect returned no device-owned pointer. They do not, by themselves, prove
the effect's intended per-device GPU choice. The diagnostic records state only;
it does not implement a compatibility fix.

The ThreeRenderer failure happened before its worker or AEX launched. A
different effect using the same registered external service rendered later in
the same sweep. Startup readiness remains an unresolved separate cohort.

Detailed per-record manifests, decoded images, focused receipts, and the
offline reconciliation script remain local under `target/cycle29-baseline/`
and `target/cmyk-exp29-auto-diagnostic/`. They contain machine-specific paths
or raw vendor output and are intentionally not committed. The older historical
ledger was not overwritten.

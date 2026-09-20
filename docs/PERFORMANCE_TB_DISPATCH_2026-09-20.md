# Indirect TB dispatch: leaf cache-hit path

Issue: GitLab #15. Baseline: `8a151c16e5dc25773c8938fe8341032f89cc4393`.
This records the scoped optimization and its local validation. The broader
threefold resident-render objective remains unmet.

## Change

The x86 indirect-branch helper previously saved host callee-saved registers
even on a jump-cache hit, because its miss path called the TB hash-table lookup.
The hit check is now shared in `tb_lookup__explicit_state_cached`; a noinline
slow helper with the same arguments handles misses. This lets the compiler
tail-call the slow helper without a stack frame on the hit path.

The observed arm64 Release assembly removes three `stp` and three `ldp`
instructions plus frame setup from the hit path. The miss ends in a tail
branch to `lookup_tb_ptr_slow`. This assembly property is compiler/target
dependent, not a promise for every host architecture. Misses recheck the
jump cache before the existing table lookup; the measurements below cover
both warm resident frames and cold per-plugin launches.

PC, CS base, translation flags, trace state, cluster, and invalid-TB checks
remain shared with the ordinary lookup. Cache storage and TB lifetime are
unchanged; no additional translated-code pointers are retained. Worker process
isolation, output validation, and cleanup are not changed.

## Validation

- Native arm64 Release worker builds succeeded before/after the C change.
- Focused Release tests: 20 passed, 2 manual benchmarks ignored, 0 failed.
- Worker Release tests: 670 passed, 2 manual benchmarks ignored, 0 failed.
- Full Release guest workspace: 734 passed, 5 manual/environment-dependent
  tests ignored, 0 failed, including Apple OpenCL and Metal-related tests.
- Final worker Release build passed; its distinct identity is recorded below.
- Behavioral coverage includes warm indirect calls/returns, colliding target
  hashes, nonzero 16-bit CS base, a two-page instruction after explicit
  invalidation, remapped/unmapped targets, execution permissions after explicit
  invalidation, code-hook stops, and cold/warm instruction-count stops.
- Independent local review of the complete code/test diff: no outstanding
  findings. CI is not a completion gate per owner instruction.
- Windows/native-x64 host, Adobe SDK conformance, and the full Python suite
  were not rerun in this low-space macOS session. Validation concentrated on
  the changed Unicorn path, the full guest Rust workspace and actual AEX runs;
  the evidence is not a cross-platform compatibility certification.

```sh
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path guest/Cargo.toml -p aex-unicorn-buffer -- --test-threads=1
CARGO_BUILD_JOBS=1 cargo test --release --locked --manifest-path guest/Cargo.toml --workspace -- --test-threads=1
CARGO_BUILD_JOBS=1 cargo build --release --locked --manifest-path guest/Cargo.toml -p aex-guest-worker
```

Two initial assertions also failed on the unmodified baseline: removing EXEC
from a warm callee did not stop execution, and adding a code hook did not
instrument an existing warm TB. These observations are recorded separately in
[GitLab #16](https://gitlab.com/onmokoworks/aexcompat/-/work_items/16), unclaimed
and unresolved. Tests here exercise the existing explicit invalidation APIs;
they do not establish automatic invalidation or fix #16. After hook changes,
caller/callee caches are explicitly cleared; the instruction-count test then
repeats eight times without further cache flushes.

## Small resident screening measurements

Apple M1 Pro, native arm64 Release workers, 256x144 ARGB8, default parameters,
jobs=1. Each plugin used one ABBA sequence (two sessions per worker), 100 frames
per session, excluding the first ten frames from timing medians. No build/test
ran concurrently. Values below are the median of the two session medians of
`effect_render`, excluding startup/admission and host pixel I/O.

| Plugin | Before | After | Observed change |
|---|---:|---:|---:|
| S_PrismLens | 5.33025 ms | 5.21650 ms | -2.13% |
| S_Blur | 4.11150 ms | 4.05725 ms | -1.32% |
| ColorKeep | 3.08500 ms | 3.02400 ms | -1.98% |

All 1,200 frame records matched output SHA by plugin/frame index. All twelve
sessions passed generation, format/size, checksum, guard, backend, cleanup and
exit-status checks. A preceding S_Blur-only four-session screening run observed
4.24175 -> 4.13975 ms (-2.40%); it is kept separately, not pooled into the table.

## Longer resident comparison

The same before-worker and the rebuilt after-worker then ran two ABBA sequences
per plugin, 1,000 frames per session, with ten warmup frames excluded from each
session median. This is
four independent worker sessions per side/plugin, 24 sessions and 24,000
frames in total. No build or other benchmark ran concurrently.

| Plugin | Before | After | Observed change |
|---|---:|---:|---:|
| S_PrismLens | 5.41875 ms | 5.29850 ms | -2.22% |
| S_Blur | 4.20225 ms | 4.03750 ms | -3.92% |
| ColorKeep | 3.11125 ms | 3.03300 ms | -2.52% |

Values are again medians of session effect-render medians, not total session
wall time. Individual session medians ranged 5.355-5.506 ms before and
5.197-5.345 ms after for PrismLens, 4.148-4.308 ms before and 4.023-4.0725 ms
after for Blur, and 3.060-3.393 ms before and 3.010-3.118 ms after for ColorKeep.
The ColorKeep ranges overlap; the observed median improvement is not a claim
that every individual session is faster. These modest gains are far from a
threefold rendering improvement and do not prove the same gain for every AEX.

All 24,000 output SHA records matched by plugin/frame index, all 24 sessions
passed cleanup/exit validation, and runner exit status was zero with empty
stderr. The minimum sampled free disk space was 5,187,477,504 bytes. The
resident harness checks its four-GiB free-space budget and 480-second budget
cooperatively between frames/sessions, raising an ordinary exception to use
the existing worker cleanup path. These are not hard memory/disk/time caps.

Worker SHA-256:

- Before: `167fb83e109beeb844d94d71090de26d95a2cc74254aeb904fe545b5bbe75e6d`
- After, screening: `a6dca0966225f55471cdfa1a746c2f8bde52f10b47c6041cb59e9627835a4bc6`
- After, final build / longer resident comparison: `8fcb367ead59b2ba25bcfb0e76bd4a8d34f1c1d83ad3b6d3dd157da858146aaa`

The final rebuild followed WIP commit `62f26df23782`. The worker build script
embeds the current Git revision; screening's after binary contains revision
`8a151c16e5dc`, while the final binary contains `62f26df23782`. The production
source files did not change between these builds, but they are distinct
binaries and must not share an identity record. The longer measurements record
the final binary's actual SHA, not the screening binary's SHA. Its disassembly
also retains the leaf hit path and tail-call miss handling.

Raw per-frame identities/timings, baseline failing-test log, disassembly, and
the resident harness are retained in the local corpus evidence directory
`runs/issue15-tb-dispatch-screening-20260920/evidence/`. The harness validates
pixel SHA, protocol/metadata and cleanup; Python assertions were enabled.
The three-plugin screening run used a 60-second whole-run deadline and
completed with exit 0; stderr was empty. The longer run used the cooperative
budget described above. Benchmark workers did not remain running afterward.

## Complete Sapphire sweep

All 292 plugins rendered successfully with jobs=1. The strict comparison to
the retained post-#14 baseline report checked unique plugin identities,
input/library/asset manifests, return codes, diagnostic JSON without duplicate
keys, pixel metadata, raw-pixel SHA, guards, cleanup and both sets of actual
PNG files. There were zero mismatches. Existing RLM -102 diagnostics occurred
for all 292 plugins on both sides; this is output/behavior parity for this
corpus, not a claim that licensing diagnostics were resolved.

| Per-plugin wall-time statistic | Post-#14 baseline | After #15 |
|---|---:|---:|
| Sum | 475.3629 s | 474.0763 s |
| Median | 1.52465 s | 1.51640 s |
| p90 | 1.79779 s | 1.84132 s |
| p95 | 2.343345 s | 2.462675 s |
| p99 | 3.199387 s | 3.138379 s |

The sum is only 0.27% lower, while p90/p95 are higher. The baseline and new
sweeps were not interleaved; this does not establish an overall sweep speedup.
The before sweep used worker SHA
`7e8aff2e81e74a4675dca060f099a591610d39ae1ccd2422cbdaa31abc8ae9e8`,
the retained #14 build, not the separately rebuilt resident before-worker.
The after sweep used the final `8fcb367e...` worker listed above.

Reports are retained as `runs/issue14-heap-search-j1-20260920/report.json` and
`runs/issue15-tb-dispatch-j1-20260920/report.json`. The full new command,
including manifest hashing, reporting and sampled resource monitoring, took
486.50 seconds. Its process group was empty at completion; the minimum
sampled free disk space was 5,071,458,304 bytes and maximum group RSS was
921,232 KiB. The temporary external runner stops its own process group on a
resource-budget failure and treats unverified cleanup as failure. Synthetic
checks covered normal exit, a parent exiting while its TERM-ignoring child
remains, and failed process-membership queries. This runner does not change
the product's execution floor.

## Resource limitations and remaining goal

The host had only approximately 5.3 GiB free and a 30 GiB swap allocation.
The initial small measurements were followed by the longer comparison above.
Resource checks found stable allocation
and usable current memory headroom despite the large existing swap allocation,
so further validation proceeded serially with sampled resource budgets.

The workspace test run took 258.54 seconds including compilation, observed at
least 5,293,760,512 free disk bytes and at most 1,018,896 KiB process-group RSS.
The final Release build took 63.15 seconds, observed at least 5,226,016,768 free
bytes, and exited with its process group empty. These are sampled observations,
not guaranteed hard resource limits. No reboot or host-wide process termination
was performed.

The observed resident gains are modest. The goal of reducing general render
time to roughly one third, and substantially shortening the complete sweep,
is not achieved by this change. Measurements on a less resource-constrained
host can further quantify the gain; this report does not claim a universal
percentage improvement across AEX, frame sizes, parameters or host platforms.

All builds reused one target directory. Sources, plugin inputs, presets, and
existing evidence were not removed to reclaim space.

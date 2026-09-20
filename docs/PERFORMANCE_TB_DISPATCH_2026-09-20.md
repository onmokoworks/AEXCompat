# Indirect TB dispatch: leaf cache-hit path (in progress)

Issue: GitLab #15. Baseline: `8a151c16e5dc25773c8938fe8341032f89cc4393`.
This is a reviewed intermediate implementation, not a completed performance
acceptance report. The threefold resident-render objective remains unmet.

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
jump cache before the existing table lookup; their cost still needs broader
measurement.

PC, CS base, translation flags, trace state, cluster, and invalid-TB checks
remain shared with the ordinary lookup. Cache storage and TB lifetime are
unchanged; no additional translated-code pointers are retained. Worker process
isolation, output validation, and cleanup are not changed.

## Validation so far

- Native arm64 Release worker builds succeeded before/after the C change.
- Focused Release tests: 20 passed, 2 manual benchmarks ignored, 0 failed.
- Behavioral coverage includes warm indirect calls/returns, colliding target
  hashes, nonzero 16-bit CS base, a two-page instruction after explicit
  invalidation, remapped/unmapped targets, execution permissions after explicit
  invalidation, code-hook stops, and cold/warm instruction-count stops.
- Independent local review of the complete code/test diff: no outstanding
  findings. CI is not a completion gate per owner instruction.

```sh
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path guest/Cargo.toml -p aex-unicorn-buffer -- --test-threads=1
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

Worker SHA-256:

- Before: `167fb83e109beeb844d94d71090de26d95a2cc74254aeb904fe545b5bbe75e6d`
- After: `a6dca0966225f55471cdfa1a746c2f8bde52f10b47c6041cb59e9627835a4bc6`

Raw per-frame identities/timings, baseline failing-test log, disassembly, and
the resident harness are retained in the local corpus evidence directory
`runs/issue15-tb-dispatch-screening-20260920/evidence/`. The harness validates
pixel SHA, protocol/metadata and cleanup; Python assertions were enabled.
The three-plugin run used a 60-second whole-run deadline and completed with
exit 0; stderr was empty. Benchmark workers did not remain running afterward.

## Still required before acceptance

The host had only approximately 5.3 GiB free and a 30 GiB swap allocation.
These small measurements are screening evidence, not a reliable estimate of
general speedup under stable resource conditions. Their modest differences
need repeated longer measurements after resource recovery.

- Final Release workspace tests and worker validation.
- Longer jobs=1 interleaved resident measurements with output/cleanup parity.
- Complete 292-plugin Sapphire sweep and strict baseline comparison.
- Final evidence review, owner/head checks, and merge; #15 remains open.

No broad sweep or new build directory was created for this screening. Sources,
plugin inputs, presets, and existing evidence were not removed to reclaim space.

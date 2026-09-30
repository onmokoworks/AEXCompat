# CRT heap search: cached free extents

Issue: GitLab #14. Baseline: `d0c95beebdc380a9483b73ef174292b65bd929bf`.

`CrtHeap::first_fit_aligned` previously searched the allocation tree twice even
when its cursor was still inside a gap found by the preceding allocation.
The per-range hint now retains the end of that proven-free gap. A fitting
allocation can advance the cursor without another allocation-tree lookup.
Ownership remains in the existing `BTreeMap`; byte/count budgets, allocation
kinds, containing-address checks, and the worker boundary are unchanged.

Every successful insert or realloc invalidates overlapping cached gaps across
all query ranges, including native/direct insertions. Free and moved realloc
rewind affected cursors and discard their gap proofs. Failed searches preserve
the saved cursor. Selecting an address does not commit ownership.

Mixed-alignment tests also exposed an existing traversal defect: after rounding
past a predecessor's end, the iterator started at the rounded address and could
skip a live block beginning between the two positions. The slow path now keeps
the original iterator start and advances monotonically. An explicit regression
and an independent full ordered-walk oracle cover this case.

## Local validation

- Release build of `aex-guest-worker`: passed.
- 18 focused heap tests: passed, including 10,000 deterministic mixed-size,
  mixed-alignment, overlapping-range operations checked against the uncached
  ordered-walk oracle and cache non-overlap assertions.
- Release guest workspace tests, `--test-threads=1`: 730 passed, 5 ignored
  manual/environment-dependent tests, 0 failed.
- Independent local review found the failed-search cursor mutation and the
  alignment traversal defect. Both were fixed and the full code diff was
  re-reviewed without remaining findings.
- CI is not a completion gate for this work, per owner instruction.

Commands (from the isolated checkout):

```sh
cargo test --release --manifest-path guest/Cargo.toml -p aex-guest-worker crt_heap::tests -- --test-threads=1
cargo build --release --manifest-path guest/Cargo.toml -p aex-guest-worker
cargo test --release --manifest-path guest/Cargo.toml --workspace -- --test-threads=1
```

## Measurements

Apple M1 Pro, native arm64 Release worker, 256x144 ARGB8 input, default
parameters, one active render worker. The same private Sapphire DLL/asset
manifests were supplied to both workers. No parallel build/test ran during the
render measurements.

Worker SHA-256:

- Before: `950ed6b816b899201f52a634693e4c788f70aa2fa63906146621a565e54b8ce8`
- After: `7e8aff2e81e74a4675dca060f099a591610d39ae1ccd2422cbdaa31abc8ae9e8`

Six S_PrismLens one-shot runs per worker, interleaved in ABBA order, with a
distinct PNG path for every execution:

| Median | Before | After | Change |
|---|---:|---:|---:|
| Wall time, including setup | 4.284815 s | 4.141271 s | -3.35% |
| User CPU | 4.207808 s | 4.110115 s | -2.32% |

All twelve runs matched PNG and raw-pixel SHA, with successful cleanup and
intact guards. These total one-shot timings do not isolate setup from rendering
and do not establish threefold rendering throughput.

S_PrismLens resident sessions used ABBA order twice, four sessions per worker,
100 frames per session, and excluded the first ten frames from timing medians.
The median of session effect-render medians was 5.439 ms before and 5.475 ms
after (+0.66%). Individual session medians ranged from 5.292 to 5.676 ms before
and 5.332 to 5.954 ms after. This does **not** establish a resident-render
speedup. Setup including the admission probe had a median of 4.182144 s before
and 4.128492 s after (-1.28%). All 800 frame outputs matched by frame index;
guards, generation, output format/size, checksums, and session cleanup passed.

The full Sapphire jobs=1 sweep completed 292/292 renders. Against the retained
same-base report `issue13-protected-read-final-j1-20260920`:

| Metric | Before | After |
|---|---:|---:|
| Sum of plugin elapsed times | 473.9276 s | 475.3629 s |
| Median | 1.514300 s | 1.524650 s |
| p90 | 1.812030 s | 1.797790 s |
| p95 | 2.324480 s | 2.343345 s |
| p99 | 3.309327 s | 3.199387 s |

The +0.30% total-time difference does **not** establish a sweep speedup. The
before sweep was retained from the preceding issue, not a contemporaneous
interleaved corpus run; its same-source worker SHA was
`90edff7cd8c0c6ca80bca07a85dd2c3b5b001fe4e0ac3771eefed8cc559ab1b5`.
There were zero differences in status, return code, plugin SHA, PNG SHA and
dimensions, raw-pixel SHA, pixel format/size, guards, or cleanup. Strict JSON
loading rejected duplicate keys and checked original stdout against the stored
diagnostic; every generated PNG was hashed again. Existing RLM license warnings
were present in both reports and are not reclassified as new failures.

An isolated heap microbenchmark (100 batches of 8,192 allocations, free every
other allocation, refill those holes, then free all) used ABBA order twice.
Median runtime decreased from 0.307243 s to 0.243070 s (-20.89%), with identical
selected-address checksum in all eight runs. This measures heap metadata work,
not complete rendering throughput.

The accepted benefit is less heap-search work and lower observed S_PrismLens
first-execution latency, plus correct mixed-alignment traversal. The broader
resident-render and full-sweep performance goals remain open. Windows native
carrier execution and AE-oracle comparison were not exercised by this Rust-only
change; direct-insertion behavior is covered by the heap tests.

Private evidence is retained under the run `issue14-heap-search-j1-20260920`:
`report.json` contains the full sweep, and `evidence/` contains final paired
one-shot/resident records, comparison metrics, workspace test log, and benchmark
harnesses. These records include worker/plugin/input/manifest identities; no
private absolute paths or proprietary plugin bytes are committed here.

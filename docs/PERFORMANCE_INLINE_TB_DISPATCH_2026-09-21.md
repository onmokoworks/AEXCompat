# Remaining warm indirect TB dispatch overhead

Issue: GitLab #19. Baseline production source:
`689feb3904616ede77443f24b9961e55ae846891`.
This is an investigation/acceptance plan, not a speedup claim.

## Baseline observations

Fresh jobs=1 resident profiles used the retained production-baseline worker
SHA-256 `d5182665dc995f89b7bff775305867fe46cf427701b5cbdc38fc7f0b32acaac5`.
It embeds revision `5b4a56c9`, but was built with the exact baseline production
header and no production guest diff against the baseline above; #17's test
changes did not enter the worker. Neither of #17's retired optimizations is
present in this worker.

For each of S_Blur and S_PrismLens, fifty warmup frames preceded 2,000 profile
frames, with a five-second native sampler. Both runs validated 2,050 frames,
closed with exit zero, and verified empty worker process groups. No build or
other guest run overlapped. Sampling timings are not benchmark timings.

The collapsed exclusive symbol samples for `helper_lookup_tb_ptr_fast` were
633 for Blur and 658 for PrismLens, compared with 255/150 for
`helper_uc_tracecode`. Counts are observations, not percentages of render wall
time; sleeping timer-thread samples are not useful optimization targets.
The existing #15 change keeps a leaf C helper on the hit path. The generated
x86 indirect-branch path still calls it for each lookup.

Private evidence is retained under
`runs/issue19-tb-inline-20260921/evidence/`, including per-frame records,
actual identities, cleanup, sampler output and resource observations. The
existing profiling runner was loaded with its worker path explicitly replaced
by the saved baseline above and TMPDIR directed into the persistent evidence
directory. It did not run the runner's obsolete default /tmp worker path.

## Candidate and constraints

Investigate generating the existing jump-cache hit check as TCG operations,
using the existing C lookup as the miss path. Expected touched production
area: `qemu/target/i386/translate.c`, with shared definitions only if required.
The existing `misc_helper.c` fallback and `exec/tb-lookup.h` key/lifecycle
semantics remain authoritative.

Preserve PC, CS base, CPU flags, trace state, cluster, compile flags and
CF_INVALID checks. No independently retained code-pointer cache, removed
restoration check, plugin special case, or relaxation of process/bounds/cleanup
rules. Unsupported host configurations retain their existing path. TCG local
temporary lifetimes and global-register synchronization across conditional
branches and `goto_ptr` need explicit inspection: eliminating a C helper must
not accidentally remove state stores it previously forced.

## Required evidence before acceptance

- Behavioral tests in `guest/crates/aex-unicorn-buffer/src/lib.rs`: cold/warm
  indirect calls and returns, colliding jump-cache keys, nonzero CS base,
  remapped/unmapped targets, explicit invalidation, code-hook stops and
  instruction-count stops. Add mode/trace/key-transition cases as needed;
  do not use source-substring assertions.
- Native Release worker build and guest workspace tests, jobs=1 and serial
  test execution. Inspect emitted code for the intended hit/miss paths.
- Actual-AEX jobs=1 ABBA timing on representative Sapphire and ColorKeep,
  recording actual worker/plugin/input identities and checking every frame's
  pixels, metadata, guards, generation and cleanup. No concurrent profiling
  or compilation during timing.
- Complete 292-plugin Sapphire sweep with strict output/cleanup comparison.
  A runner exiting zero while recording a failed plugin is not a passing run.
- Independent local review of all unreviewed changes before push/MR; CI is
  outside the completion gate per owner instruction.

```sh
CARGO_BUILD_JOBS=1 CMAKE_BUILD_PARALLEL_LEVEL=1 CARGO_INCREMENTAL=0 \
  cargo test --manifest-path guest/Cargo.toml --release --locked -p aex-unicorn-buffer -- --test-threads=1
CARGO_BUILD_JOBS=1 CMAKE_BUILD_PARALLEL_LEVEL=1 CARGO_INCREMENTAL=0 \
  cargo test --manifest-path guest/Cargo.toml --release --locked --workspace -- --test-threads=1
CARGO_BUILD_JOBS=1 CMAKE_BUILD_PARALLEL_LEVEL=1 CARGO_INCREMENTAL=0 \
  cargo build --manifest-path guest/Cargo.toml --release --locked --bin aex-guest-worker
```

The broader threefold rendering goal is unmet. #16's automatic-invalidation
and #18's single-hook restoration defects are separate unclaimed issues, not
silently fixed here. #17 was withdrawn with negative evidence preserved; no
failed candidate was merged.

## First inline candidate

The x86-64 target on a 64-bit TCG host now emits the existing jump-cache hash
and complete key comparison directly. Other build configurations retain the
helper path. A null entry or any key mismatch calls the unchanged helper.
PC and TB pointer use local temporaries across conditional basic-block ends;
the remaining key mismatches are aggregated before a single conditional exit.
The existing TCG BB_END/BB_EXIT rules synchronize architectural globals before
the generated jump, without relying on a helper call to do so.

Initial Release focused tests: 21 passed, 2 existing ignored, including a new
32/64-bit indirect-call test for lazy carry flags, memory and stack state.
The bounded build/test process exited zero with an empty process group;
minimum sampled free disk was 38,111,870,976 bytes. This is correctness evidence,
not performance acceptance.

Release worker SHA-256:
`35c9c18abb81a65c861c7951df04c3aabfd8dbfe3fbae997d389c4e62cd71dec`.
It embeds baseline revision `689feb39` plus the uncommitted translator candidate
(file SHA-256 `d4b8521a402df57818506321e5fabb36a2e76ea83e3cf00b3dda90d5e3b8f831`).
Jobs=1 ABBA repeated twice per plugin, 1,000 frames per session with ten warmup
frames excluded from timing, completed 24 sessions / 24,000 validated frames.
The runner exited zero after comparing every frame's pixel hash across sessions,
identities, metadata, guard state, cleanup and bounded diagnostic excerpts.
All 24 recorded process groups were separately checked absent after completion.
No compiler or profiler overlapped this measurement.

Median of the four warm-session medians per binary (microseconds):

| Plugin | Before | Inline candidate | Time reduction |
| --- | ---: | ---: | ---: |
| S_PrismLens | 5892.50 | 5456.75 | 7.39% |
| S_Blur | 4527.25 | 4158.25 | 8.15% |
| ColorKeep | 3164.25 | 2750.50 | 13.08% |

Evidence: `inline-resident-abba.jsonl` and `.stderr` in the private evidence
directory above; the latter is empty. These are representative resident results,
not a threefold speedup or proof of full-corpus compatibility. Full workspace
tests and the 292-plugin sweep were pending at that checkpoint.

## Follow-up regression and profile validation

The Release workspace suite completed with 735 passed, zero failed and five
existing ignored tests. The bounded command exited zero in 236.93 seconds,
verified its process group empty, and observed a minimum 37,146,169,344 free
bytes. Worker SHA-256 remained unchanged after this test build.

Fresh Blur and PrismLens profiles each validated 2,050 frames (50 warmup plus
2,000 sampled-run frames), closed successfully and verified empty worker
process groups. The collapsed exclusive samples for `helper_lookup_tb_ptr_fast`
fell from 633 to 54 for Blur and from 658 to 50 for PrismLens. These counts
corroborate removal of helper work on cache hits; they are neither a precise
call-count ratio nor an overall-render speedup percentage. Profiling was run
separately from all timing and compilation.

Private evidence: `inline-workspace-tests.log`,
`inline-blur-profile-summary.json` and `inline-prism-profile-summary.json`, with
per-frame results and sampler files in the directories identified by those
summaries.

## Full Sapphire sweep

The jobs=1 candidate sweep completed 292/292 plugins successfully. The strict
comparison against the retained baseline sweep found zero mismatches in plugin
identity, raw pixels, PNG bytes, extent/format, guards, render status and cleanup.
The bounded command exited zero, verified its process group empty and observed
at least 37,359,362,048 free bytes. Both runs retained the corpus's known RLM
license diagnostic; success here means compatibility with that recorded corpus,
not a claim of licensed After Effects equivalence.

The candidate whole-command elapsed time was 514.71 seconds. Sum of individual
plugin times was 500.9427 seconds versus baseline 499.5659 seconds. This
non-interleaved cold-start comparison establishes no sweep speedup. The measured
benefit remains resident rendering; startup/translation and cold sweep cost
still require future optimization under a separately claimed issue.

Evidence: `runs/issue19-inline-j1-20260921/report.json` and its PNGs compared
against `runs/issue17-before-j1-20260921/report.json`, plus `inline-sweep.log` in
this issue's evidence directory. Actual candidate worker identity matches the
resident benchmark above.

## Generated-code inspection

A bounded LLDB attempt did not advance beyond launching the focused test within
60 seconds; its process group was terminated and verified empty. This did not
produce disassembly and is not counted as a passing debugger test.

As an alternative, a private standalone C fixture linked the same built Unicorn
archive without modifying production source. It executed eight indirect calls,
checked the accumulated register result, copied the caller TB bytes before
closing its engine, and exited zero with an empty process group. Capstone 5.0.7
decoded the resulting 368-byte AArch64 block. At block offset 0x88, a null cache
entry branches to the miss path; offset 0xfc branches there on a key mismatch.
The hit path loads `tc.ptr` and executes `br x20` at 0x108. Only the miss path
performs the lookup helper call (`blr x30` at 0x124) followed by `br x0`.
Architectural stack/PC stores precede lookup. This fixture uses a constant
target, so the hash arithmetic is constant-folded; dynamic-target behavior is
covered separately by the warm/colliding-target tests.

Private evidence includes `dump_inline_tb.c`, its executable, and
`inline-tb-bytes.log`; the unsuccessful debugger attempt is retained in
`inline-generated-code-lldb.log`. These diagnostic artifacts are not used as
timing evidence. The full threefold-render and faster-cold-sweep objectives
remain unmet despite accepting this incremental resident optimization.

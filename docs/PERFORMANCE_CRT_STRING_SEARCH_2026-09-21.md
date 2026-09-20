# Bounded CRT string search

Issue: GitLab #20. Baseline: merged #19, commit
`30d5cdfea6d6f1c25acf057add675d061cc03ec6`.
The retained baseline worker SHA-256 is
`35c9c18abb81a65c861c7951df04c3aabfd8dbfe3fbae997d389c4e62cd71dec`.

The post-#19 PrismLens profile has 157 collapsed exclusive samples in
`scan_crt_stdio_c_string`. Baseline AArch64 disassembly confirms a scalar
LDRB/CBZ loop for its NUL search, not a vectorized search. Sampling counts do not
prove the attainable wall-time benefit. Of those exclusive samples, 156 occur
at function offsets including +356/+364, matching the scalar loop's load and
increment instructions in the retained binary. This supports targeting the
search rather than changing guest-memory validation.

Candidate: use the existing `memchr` dependency on the already protected,
page-bounded host slice. Keep the 256-byte buffer, access checks, limits,
overflow/null errors and output accumulation unchanged. No guest pointer is
exposed and no protection checks are removed.

Acceptance before merge:

- Behavioral tests for each NUL position across 256-byte chunk boundaries and
  varied alignments; length-only and collected-output modes; absent terminator,
  zero/exact limits, page boundary and unreadable-page failures.
- Release worker and serial workspace tests with one build job.
- Jobs=1 resident ABBA against the retained baseline: every frame's pixels,
  metadata, guards, actual binary identities and cleanup must match.
- If representative rendering benefits, strict 292-plugin Sapphire sweep and
  independent local review of every change before push/MR. CI is waived by owner.
- Reject or revise if actual render improvement is not demonstrated; a faster
  synthetic search alone is insufficient. The original 1/3 goal is unchanged.

Validation commands use `CARGO_BUILD_JOBS=1`, `CMAKE_BUILD_PARALLEL_LEVEL=1`,
`CARGO_INCREMENTAL=0`, `--release --locked` and `--test-threads=1`:
`cargo test --manifest-path guest/Cargo.toml -p aex-guest-worker bounded_crt_string`,
then the existing strlen cases and `cargo test --workspace` against that manifest.
Build `--bin aex-guest-worker` for real-AEX comparison. No timing run overlaps
compilation or sampling.

## Initial evidence (not final acceptance)

The two new behavioral tests and four existing strlen tests passed in Release.
The Release worker built successfully. Each bounded process group exited zero
and was verified empty. The candidate worker SHA-256 is
`5ff4f7089113e4e5ffe07dc3655d67fe401f36bea4e4ef90a167b84277a835ae`.

Jobs=1 ABBA twice, 1,000 frames per session (ten warmup frames excluded), completed
24 sessions / 24,000 frames. All pixel sequences, metadata, guards, identities,
bounded diagnostic excerpts and cleanup compared successfully. All recorded
worker process groups were also checked absent after completion.

Median of four session medians per binary, microseconds (session range):

| Plugin | Baseline #19 | Candidate | Time change |
| --- | ---: | ---: | ---: |
| PrismLens | 5500.00 (5479.5–5556.0) | 5292.75 (5242.0–5423.5) | -3.77% |
| Blur | 4170.25 (4123.5–4252.5) | 4191.75 (4151.0–4217.0) | +0.52% |
| ColorKeep | 2754.00 (2740.0–2822.5) | 2752.00 (2748.0–2784.0) | -0.07% |

PrismLens shows a benefit in this run; Blur and ColorKeep ranges overlap and
do not establish improvement. Repeat with 2,000 frames per session before
acceptance. No general all-effects speedup or threefold completion is claimed.
Private evidence: `runs/issue20-crt-string-search-20260921/evidence/`, including
`focused-build.log`, `strlen-tests.log`, `worker-build.log`, and
`resident-abba.jsonl` / `.stderr`. Full-workspace and sweep validation remain
pending. Production and tests have an independent source review with no
actionable findings; new evidence documentation must be reviewed before push.

## Longer confirmation

ABBA twice was repeated with 2,000 frames per session, giving another 24 sessions
and 48,000 frames. The runner exited zero after all frame/identity/cleanup
comparisons; all recorded worker process groups were separately checked absent.
Binary identities remained the same as above. No builds or sampling overlapped.

| Plugin | Baseline #19 | Candidate | Time change |
| --- | ---: | ---: | ---: |
| PrismLens | 5506.00 (5389.5–5560.0) | 5274.50 (5234.0–5331.5) | -4.20% |
| Blur | 4193.00 (4152.0–4279.5) | 4181.00 (4173.5–4194.0) | -0.29% |
| ColorKeep | 2751.50 (2749.0–2785.0) | 2782.00 (2750.0–2840.0) | +1.11% |

PrismLens improvement repeats with non-overlapping session-median ranges in
both runs. The other effects do not show a repeatable benefit: their ranges
overlap, and differences change direction between runs. This does not prove
zero regression for every workload. Proceed to full regression/sweep validation
as a generic bounded-string-search improvement, not an all-effects speedup.
Evidence: `resident-abba-confirmation.jsonl` / `.stderr` beside the first run.
The original one-third render-time goal remains unmet.

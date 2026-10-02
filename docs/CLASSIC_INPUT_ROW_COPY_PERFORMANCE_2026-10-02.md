# Classic primary-input row-copy performance

Issue #1731. Baseline main `077b9b066`, Windows native MSVC Release,
Ryzen 9 5900X (12 cores / 24 logical processors). This is one candidate for
the additional three-improvement Goal. Count it only after reviewed merge and
postmerge verification. No After Effects timing or Adobe-equivalence claim.

## Boundary and experiment

`render::build_argb_input` creates the same logical packed ARGB snapshot but
copies its completed rows into the guarded primary world rather than invoking
a runtime-sized `memcpy` for every pixel. At 1920x1080, 2,073,600 tiny copies
become 1,080 row copies. The only product change is this copy boundary.

Conversion, logical snapshot lifetime, active bytes, padding, allocation,
input protection/writability, output-coverage/extent validation, provenance,
deadlines and containment are unchanged. There is no plug-in-name branch,
approximation, cached guest-mutable world, new persistent buffer or parallelism.
SmartFX's separate input construction is not changed or advertised as faster.

Hypothesis: the dynamic tiny copies contribute measurable common host cost.
Prediction: lower worker nonselector and frame wall time, same output and
diagnostics, unchanged selector within ordinary variation. Reject on byte,
padding, cleanup or normal-output mismatch, or on wall savings inside noise.
The existing aggregate timings cannot establish the cost of this boundary.
Supported results proceed to behavioral/regression tests and the review/CI
gates; rejected results remove the candidate/temporary telemetry and release
the claim. The earlier output-shuffle attempt #1729 was withdrawn at this
noise gate and contributes neither code nor an improvement count here.

## Qualified workload and reproduction

- Installed OLMSmoother SHA256
  `6206f601b645dc915b78269ae403e5cbee642ac2812e320d85838ec72135fe82`.
- Same 1920x1080 opaque black/white slanted-line input as the qualified #1720
  workload. Input PNG SHA256
  `b5222b49371731c795abb11d5d0338423a2a0ba59092e7c9bf1022f2e648126e`.
- Classic ARGB8, explicitly supplied discovered defaults: Use Color Key=0,
  opaque white Color Key, Smooth Range=6. time_step=1, time_scale=30.
- Thirty-two identical inputs per resident session. Frame zero is separately
  retained as process-cold (not OS/driver/vendor-cache cold); all 31 warm
  frames contribute to each run median. No outlier is removed.
- Three fresh-session pairs in order AB/BA/AB, repeated in order BA/AB/BA.
  Before every worker swap's real run, the resident sampling-probe broker
  integration test passes. AEX, input and broker hashes are verified before
  and after each series; both workers share the same Release configuration.
- Baseline worker SHA256
  `5b36ab468c649293094e666caac09cecf704de96331f8071fca34a9d89cbe827`;
  candidate `c90223b7b96143255eea7c7f8e5df1ce1bca0cd14aeae2fc62f921a5b45d5881`.
  Measurement broker SHA256
  `bf21ca9c41a1fdaeec5fa6049495147b7c93426fc2ff8145cc6a8601864c5c93`.
  Its sole temporary change serializes existing FramePerformance after the
  timed render, equally for both workers; removed from the final product.

Build using `tools/build-native.ps1` and the broker Release build; generate the
input using the polygon specification in
`OLMSMOOTHER_NATIVE_PERFORMANCE_2026-10-01.md`, discover/pass parameters, and
run `broker render-video-batch` with fresh request/report/output paths. Local
ignored `target/issue1731/` contains worker snapshots, scripts, identity and
parameter records, raw reports, all timings and decoded images. No private
paths, images or AEX bytes are committed.

## Measurements

Times below are ms, median over the run's 31 warm frames. Nonselector is
computed per frame as worker-render minus selector, then medianed. Frame wall
includes transport and validation; it is not PNG encoding or CLI startup.

| Series/run | Worker | Frame wall | Nonselector | Selector |
| --- | --- | ---: | ---: | ---: |
| Initial 1 | A | 120.4242 | 48.1325 | 66.1178 |
| Initial 2 | B | 115.0626 | 42.6894 | 66.1047 |
| Initial 3 | B | 115.5903 | 42.9423 | 66.3733 |
| Initial 4 | A | 121.8613 | 48.5749 | 66.5728 |
| Initial 5 | A | 121.7265 | 48.9301 | 66.5482 |
| Initial 6 | B | 114.7787 | 42.3676 | 65.8648 |
| Repetition 1 | B | 116.0084 | 42.9821 | 66.7140 |
| Repetition 2 | A | 121.9889 | 48.8485 | 66.3710 |
| Repetition 3 | A | 121.8233 | 48.3688 | 66.7365 |
| Repetition 4 | B | 114.9147 | 42.4268 | 66.1372 |
| Repetition 5 | B | 116.9525 | 42.8886 | 67.0239 |
| Repetition 6 | A | 122.2270 | 49.0189 | 66.5638 |

Initial aggregate frame 121.7265 -> 115.0626 ms (**5.47% shorter**),
nonselector 48.5749 -> 42.6894 ms (**12.12% shorter**). Repetition frame
121.9889 -> 116.0084 ms (**4.90% shorter**), nonselector 48.8485 -> 42.8886 ms
(**12.20% shorter**). All six paired frame savings are 5.27–6.95 ms; the
slowest B run median is below the fastest A in each series. Selector medians
change -0.67% and +0.23%, respectively; finalization is essentially unchanged.

Within-run median absolute deviations are 0.87–2.91 ms. Exploratory circular
seven-frame-block bootstrap intervals (4,000 resamples, deterministic seed;
not a causality proof) for paired savings are [4.00,8.28], [4.00,7.97],
[4.81,9.33], [3.86,7.89], [5.46,8.41], [3.11,6.81] ms, all improvement-side.
The acceptance rests on interleaving, repetition, identical outputs and the
consistent changed-host/unchanged-selector measurements, not bootstrap alone.

All frame-zero timings are retained: A wall 165.9–246.1 ms, B 159.4–288.2 ms.
Cold behavior is noisy and **no cold-start speedup is claimed**. Whole-command
run medians, separately, are 4.7311 -> 4.5747 s (3.31%) and
4.7288 -> 4.6303 s (2.08%). These include startup, decoding, encoding and close,
and are not substituted for the roughly 5% warm-frame result.

## Correctness, memory and verification

Every one of 384 comparison PNGs is independently decoded and rehashed as
RGBA. All have SHA256
`d3d11e2e8aadfb2646d263e671e5cea3a4425ded344390bb80b5e4e259b80159` and the same
154,271 changed RGBA bytes from the two-color input: actual smoothing, not a
passthrough or error image. All twelve session final reports are semantically
identical (only loader module-array order is canonicalized), including
unwritten count zero, guard bytes intact, render/setup/setdown success,
32 worlds created/disposed, and balanced handle/world/suite lifetimes.

The existing memory advisory reports stable plateau with
`post_frame_retention_candidate` in both variants, tail live commit
85.83–86.27 MB (decimal). This is a time-advancing session with retained state,
not proof of an AEX leak. The copy change does not eliminate or hide that
warning and is not claimed as a memory-management fix.

Compiled behavioral input tests independently construct expected logical and
strided bytes at all depths, odd/wide rows, single-pixel external dimensions,
multiple rows, padding, alignment offsets and legacy gradient. They compare
the full allocation, source and guards and exercise malformed-input refusal.
A representative wrong-row mutation makes the compiled test exit 1 with
world/padding failures; restoring the row offset makes it pass. These are
behavioral tests, not source-string assertions.

Native Release build and header dependencies 207/207 pass. Rust workspace
and all five Release broker containment scenarios pass. All 52 focused Python tests
cover Classic/Smart, depths, input bytes, resizing, output coverage and the
resident broker loop. The full Python suite and AE oracle are not run locally;
broader supported-platform coverage is delegated to CI, not presumed passed.
The focused run uses `PYTHONUTF8=1` and `--run-built-artifact-tests` across
`test_partial_output_coverage.py`, `test_output_coverage_native.py`,
`test_render_lifecycle_frame_setup_extent.py`, `test_render_session_worker.py`,
`test_smart_session_worker.py` and `test_render_video_batch_cli.py`.
An earlier rerun after worker snapshot swaps refused a stale geometry fixture
before dispatch (22 failures). Clean-rebuilding that fixture resolved the
freshness failures; no timestamp manipulation or check relaxation was used.

Second real AEX regression: OLMRadialBlur, its advertised SmartFX route,
ARGB8 1080p, discovered defaults except both Strength controls=120. Eight
actual radial-blur outputs match between workers (more than two colors and
different from input), RGBA SHA256
`1863621d6de9943bba66454d1e3a2751bbfa8a00e99477af17560a68f3f76fc5`, clean
sessions and identical final diagnostics. This is a regression check, not a
SmartFX speedup claim. Earlier Classic OLMBlur identity output and Classic
OLMRadialBlur untouched-output rejection are retained but **not normal-effect
performance evidence**; no validation was bypassed to accept them.

Independent local review, reviewed-head CI/owner gates, exact-head merge and
postmerge results must be recorded on the implementing PR/Issue before this
candidate counts toward the Goal.

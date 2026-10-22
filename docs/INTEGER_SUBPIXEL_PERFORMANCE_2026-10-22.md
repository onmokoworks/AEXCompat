# Exact integer subpixel sampling performance

Issue #1787. Baseline main `b7c6f6d79531e2d1e34ecaaee0cb6d31c8f4923d`,
Windows x64 MSVC 19.50 Release. This is a candidate for the additional
three-improvement Goal; credit requires reviewed merge and postmerge verification.
There is no After Effects timing or Adobe pixel-equivalence claim.

## Change and equivalence

PF subpixel sampling at integer depths uses exact 64-bit weighted arithmetic
instead of the generic floating-point interpolation/rounding path. With 16.16
coordinates, each weight has denominator 2^32; even a raw uint16 value of 65535
produces a numerator below 2^48. The original double products and partial sums
are exact. Adding 2^31 before division reproduces positive half-up rounding.
ARGB16 still clamps at 32768, including inputs above that white point.

Negative coordinates retain floor semantics. Out-of-bounds neighbors are not
read. Channel-by-channel reads and writes preserve shifted source/destination
alias behavior. The float path, argument/world validation, diagnostics/history,
ownership, allocations, cleanup, coverage, provenance and containment remain
unchanged. No plugin-name branch, approximation, cache or threading is added.

Hypothesis: millions of host subpixel calls make exact arithmetic specialization
large enough to improve whole-frame wall time. The alternative is that unchanged
world validation, diagnostic atomics/history locking or the guest body dominate.
Reject if normal output/diagnostics change or if the fixed whole-frame comparison
fails. A faster isolated kernel alone does not qualify.

## Qualification and fixed comparison

- Installed Adobe Displacement (AE 2025 distribution), SHA256
  `5cd0816d7e6a1943871358fc4781868d65adbe3d70708e18eaefa6a8a50f460e`.
- 1920x1080 opaque slanted black/white input from the #1720 workload; PNG SHA256
  `b5222b49371731c795abb11d5d0338423a2a0ba59092e7c9bf1022f2e648126e`.
  The same image is explicitly bound as layer slot 1; other parameters use the
  plugin defaults. SmartCPU ARGB8, times 0..3, step 1, scale 30.
- One four-frame qualification with a temporary typed-call counter recorded
  4,147,200 valid subpixel8 calls per frame, including 1,479,682 fractional and
  4,127,653 interior samples. Each decoded result differs from input in 779,970
  bytes. All guards, callback results and cleanup were normal. The temporary
  counters were removed before comparison; these timings are not speed evidence.
- Eight balanced randomized four-run blocks, seed 1787, each run a fresh
  resident session of four frames. Orders: BAAB, ABBA, BAAB, BAAB, ABBA, ABBA,
  BAAB, ABBA. Frame zero is separate; all three remaining frames form the run's
  warm median. No excluded/replacement run or favorable-result extension.
- Two Smoother control blocks, ABBA and BAAB, same four-frame session length.
  Existing #1731 input/defaults/Classic workload is unchanged.
- Primary acceptance was fixed before execution: exact randomization over all
  70 balanced block orientations, one-sided p <= .05, positive savings in both
  four-block halves, every output and final diagnostic matching. Each control
  block must not regress beyond max(5 ms, 5% of its baseline).
- Before every comparison session, the broker integration test passed: one test,
  zero skips/failures/errors, verified from JUnit. The runner uses the existing
  owned Job-based capture helper for bounded child cleanup and restores both
  runtime binaries in `finally`. No other build/render/AE process was seen in
  the pre-run check; this is not continuous whole-machine contention monitoring.

## Results

Values are means over session-level measurements; warm values first take the
median of frames 1..3. The frame wall clock starts at input-slot writing and ends
after output verification/memory sampling. It excludes process startup, input
PNG decoding and output encoding, which are included in the command measurement.

| Measurement | Baseline A | Candidate B |
| --- | ---: | ---: |
| Warm frame wall | 306.918 ms | 214.563 ms |
| First frame wall | 339.069 ms | 245.794 ms |
| Warm selector | 223.170 ms | 130.815 ms |
| Four-frame command | 1599.226 ms | 1208.389 ms |

Warm frame wall is **30.091% shorter** (1.4304x throughput for this workload).
Block savings are 89.617, 90.484, 95.165, 89.669, 93.979, 101.217, 88.397 and
90.313 ms. Exact p=1/70; both halves improve. Selector savings account for the
observed wall difference, rather than moving work outside the frame measurement.
First-frame numbers describe a fresh worker, not an OS/filesystem-cold cache.

All 128 Displacement PNGs decode to
`a72d698b48df172eaa3f0b08df4f6d20bdfeac03404394478ead4423b80199cf`.
All 32 Smoother control PNGs match the established #1731 output. Full final
diagnostics match per workload after only module-array ordering normalization.
Control warm-wall changes are +0.580 and -1.872 ms, within the fixed gate.
An independent reviewer decoded all 160 PNGs and recomputed the raw-report
statistics, integration results, identities and final-diagnostic comparisons.

Memory is not improved: final-frame live commit averages approximately 60.61 MB
in both arms, with approximately 8.3 MB/frame growth. Four samples do not establish
a leak or a steady-state plateau. Existing memory advisories remain unchanged.

## Build, tests and retained evidence

Fresh baseline: `tools/build-native.ps1 -BuildDir target/issue1787/native-build
-Target aex_worker`; broker: `cargo build --release --manifest-path
broker/Cargo.toml -p aexcompat-broker --bin broker`. Candidate uses the same native
build directory, adding target `worker_pf_sampling_wiring_selftest`. The native
dependency checker passed (141 baseline, 146 after the selftest targets).

- Baseline worker SHA256
  `a315bbf319832f5ba118178f66e8304e9df99aa046bd937cb07672e0e84fd609`.
- Candidate worker SHA256
  `abae4c3243e0cc8c0f2525e520b37bdc4e89f62387ccbe7469a3aa04ec44082c`.
- Common measurement broker SHA256
  `ee057e8d0d2612d27f704b5865042a1bd22eb52e52dd457e3510274f9e4c01f9`.
  The sole temporary source delta was adding `"performance": outcome.performance`
  to the successful frame JSON in `run_video_batch`, after rendering. It records
  existing timings and was removed from shipping source before this comparison.
- Shipping broker SHA256
  `60034f18c6f178706968ad25f433dcc7c79cf46e45626f064f016f4e3f42f376`.

The compiled behavioral selftest compares 3,200 cases to the original double
algorithm: signed/extreme coordinates, ties, randomized values, raw16 super-white,
1x1/odd/padded/unaligned worlds and shifted aliasing, comparing entire buffers.
Removing the half-denominator rounding bias failed this test; restoration passed.
The candidate passed 34 focused pytest cases including the SDK sampling probe's
8/16/32F numerical oracle and broker integration. Missing local catalog-probe and
raw-input prerequisites were built/generated before rerunning their tests; the
initial missing-artifact failures are not product successes.
The full Rust workspace test rerun passed after building the missing catalog
probe prerequisite; built-in ignored tests remain outside that verification.

Local ignored `target/issue1787/` retains the contract, runner, worker/broker
snapshots, qualification, requests, every raw report, JUnit record and image.
No proprietary binary, image or private machine path is committed. Source/build
qualification also uses the actual build tool history; the cache by itself is
not a historical source manifest. Real-AEX speed evidence is limited to this
one 1080p static-input ARGB8 workload. Integer16 correctness is covered by numeric
tests, not real-AEX timing. Float sampling has no speedup claim. Sustained long
sessions, other machines and AE equivalence are unverified.

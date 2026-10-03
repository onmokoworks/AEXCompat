# Resident worker memory advisories (#1722)

## Scope and operation

Ordinary Classic/SmartFX resident frames now feed a bounded monitor after a
validated frame reply. It retains at most 32 scalar metadata samples and four
baseline integers, never image buffers. Classification uses fixed-size stack
values and a warning bitmask: no report JSON or geometry set is built on an
unchanged ordinary frame. JSON is built for explicit access/close or a tracing
state transition.

Live process commit, historical process
peak, aggregate job peak and the actual configured Job per-process cap stay
separate. A cap is read from the launched process, not guessed from job peak.
No memory limit, launch admission, isolation, cleanup or render verdict changes.

`RenderSession::memory_advisory()` exposes the current structured observation;
the normal session close (including video-batch close) carries `memory_advisory`.
Tracing emits warnings only when the active reason set changes; a transition
to no active warning is informational. Missing telemetry is unavailable, not
zero and not proof that pressure recovered. Tracing requires the caller's
subscriber; the structured report remains available without one.

Reasons distinguish:

- `sustained_live_growth_candidate`: latest eight live observations, signed
  endpoint slope at least 128 KiB/frame, at least six nondecreasing steps and
  later-half versus earlier-half lower-median increase at least 512 KiB.
- `process_limit_near`: live process commit / actual process cap >= 0.8. Job
  peak is not used as its numerator or denominator.
- `post_frame_retention_candidate`: stable latest tail, with a lower-median
  increase from samples 5–8 >= max(1 MiB, 10% of baseline).
- `rapid_live_growth_headroom_candidate`: a provisional warning before the
  normal 12-observation trend window. The latest four observations must have
  live values, the same nonzero process cap and geometry, and consecutive
  frame indices. All three increments must be at least max(4 MiB, cap / 64).
  Using the smallest increment, remaining headroom would be consumed within
  the number of observations still needed for the normal trend window, if
  growth continued. An isolated cold spike, a flat or falling step, or a
  changed/missing observation does not satisfy this rule.
- `stable_plateau`: latest-tail range <= max(256 KiB, 2% of its lower median),
  when the sustained-growth rule is not satisfied.
- `temporary_peak_or_recovered_commit`: historical process peak exceeds the
  latest tail's highest live commit by the plateau tolerance. Observation only.

Trend requires at least 12 completed observations and all eight tail live
values. Signed decline and variable commit remain distinct. No outliers are
removed. Numeric policies, observation counts, geometry, times, slopes, ratio,
baseline and retained delta are published with each report.

`early_growth` publishes the provisional status, smallest observed increment,
conditional `projected_frames_to_limit`, and remaining observations until the
regular trend window. The regular `trend_status` stays unavailable for a
short run. The warning clears as soon as the four-observation predicate stops
holding, including when initialization or a bounded cache plateaus. At sample
12 it gives way to the regular trend classifier; near-limit pressure remains
independent. A repeated cold-cache initialization can temporarily satisfy the
provisional rule, so this projection neither predicts continued growth nor
establishes a leak, future render failure, or which allocator owns the memory.

These are whole-worker candidates, including cluster-session member changes,
not attribution to an AEX leak. The worker returns after render/FRAME_SETDOWN
and frame-local auxiliary-channel reclamation, but retains session buffers,
frame caches, plug-in sequence/global state and allocator caches. A frame reply
is not a full-session release checkpoint. Validated close proves the existing
cleanup contract and process exit; historical peaks do not represent live
memory after exit. This change does not attempt cache eviction or leak repair.

## Bounded diagnostic CLI

```
aexcompat-harness --headless --render-memory-diagnostics <AEX> classic argb8 repeat
aexcompat-harness --headless --render-memory-diagnostics <AEX> classic argb8 advance-then-repeat
```

Smart and other depths use the existing validated path/depth arguments.
Optional final `<slot> <value>` changes a discovered numeric parameter.
Each request uses two fresh workers, 640×360 and 1920×1080, 32 frames each:
64 samples maximum. `repeat` keeps time zero; `advance-then-repeat` uses times
0–15 followed by 16 repeats of time 15. A normalized slanted bilevel pattern
exercises Smoother instead of a passthrough gradient. Reports contain hashes
and changed-byte counts, not private paths or pixels. Cleanup, admitted plug-in
identity and before/after plug-in/host/worker identity are checked by the
existing diagnostic wrapper. A failed render/close remains unavailable.

## Experiment contract and observations

Hypothesis: four endpoint observations can mistake bounded initialization for
continuing growth; extending an ordinary session distinguishes ongoing growth,
retained memory and repeated-time plateau. Prediction: advancing time may
populate a host frame cache, while repeated-time control stops growing. Reject
if both profiles show indistinguishable ongoing growth or outputs/cleanup fail.
If rejected, replan at the existing worker frame-lifecycle boundary; do not add
another diagnostic layer or expand to unrelated AEX fixes.

Actual native OLMSmoother, Classic ARGB8, discovered defaults, two independent
64-frame runs per profile, 2026-10-01. All 256 frames rendered, all eight fresh
sessions validated cleanly. Every output hash was constant within its session;
25,260 RGBA bytes changed at 640×360 and 75,888 at 1920×1080, so these were
non-passthrough normal renders. Plug-in SHA-256:
`6206f601b645dc915b78269ae403e5cbee642ac2812e320d85838ec72135fe82`.

| Profile | Resolution | Final live bytes (run 1 / run 2) | Tail slope bytes/frame | Final state |
| --- | --- | --- | --- | --- |
| advance/repeat | 640×360 | 19,292,160 / 19,259,392 | 1,755.43 / -132,242.29 | variable / declining; no active warning |
| advance/repeat | 1920×1080 | 86,052,864 / 86,224,896 | 1,170.29 / 0 | stable; retention candidate |
| repeat | 640×360 | 5,423,104 / 5,435,392 | 0 / 2,340.57 | stable; no warning |
| repeat | 1920×1080 | 27,598,848 / 27,742,208 | -25,161.14 / 17,554.29 | stable; no warning |

1080p retention delta was 16,908,288 / 17,129,472 bytes above post-warmup
baseline. Both repeated-time controls produced zero warning-state changes.
Advancing 1080p produced 1 / 3 changes and ended in the same retention state.
Small-resolution variation was not relabeled stable to make the experiment
pass. Time-dependent host caches are a plausible confounder, not proven AEX
ownership; this is actionable investigation evidence, not leak diagnosis.

## Verification boundaries

Rust behavioral tests cover four-frame insufficiency against the existing
production summary, sustained growth, retained plateau, recovered peak,
pressure, missing/short/zero-budget observations, signed decline, bounded
1,000-frame history and non-spamming state changes. Short-series tests also
cover the recorded four-frame rapid increase, cold spikes/noise, provisional
warning entry/clear, missing or changed budget/geometry/frame indices,
smallest-increment projection and its inclusive integer headroom boundary.
Native public fixture
modes add bounded retention and temporary committed-memory recovery. Native
tests check actual sample-derived slopes/baselines/ratios, output identity and
close; allocator noise is allowed to remain variable/declining rather than
requiring every real process to satisfy a fixed plateau range. Actual-AEX
warning and non-warning evidence above is complementary to these tests.

No AE measurements, cross-machine performance claim or memory-leak repair is
claimed. This closes the advisory cycle, not all future low-level work.

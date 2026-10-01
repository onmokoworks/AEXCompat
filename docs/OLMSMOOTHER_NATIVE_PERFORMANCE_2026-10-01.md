# Windows native OLMSmoother: output-coverage inspection

Issue #1720. Baseline main: `3ddce2f41de2a0dc4212aff76c9bc77b330a07b8`.
This is Windows native Classic ARGB8, not the Mac Unicorn workload in #1559.
No After Effects timing or Adobe-equivalence claim is made.

## Change and experiment

The initial 1080p normalized-XY/XOR diagnostic measured warm frame wall
143.0 ms, selector dispatch 75.5 ms, and worker nonselector work 60.8 ms.
This established a substantial shared host cost. That input was unchanged
by the effect, so it was not used as normal-effect acceptance evidence.

Hypothesis: coverage inspection generates every sentinel byte even after a
different byte has already proved a pixel was written. Stop at the first
difference without changing the all-byte-equality predicate. Prediction:
lower worker nonselector time, unchanged selector time and output, unchanged
unwritten counts/bounds/runs. Reject if normal output cannot be demonstrated,
diagnostics differ semantically, or the improvement does not repeat.

The sole product change short-circuits that comparison. It does not skip a
pixel, sample the image, change the seed, cache sentinel buffers, alter
ownership or limits, or weaken any output-coverage rejection. All bytes of
a matching pixel are still compared. Partial writes that exactly reproduce
the sentinel remain indistinguishable, as documented before this change.

## Qualified workload and reproduction

- Ryzen 9 5900X, 12 cores / 24 logical processors, Windows, MSVC Release.
- Installed OLMSmoother SHA-256:
  `6206f601b645dc915b78269ae403e5cbee642ac2812e320d85838ec72135fe82`.
- 1920x1080 opaque white RGBA image. For `y` in `range(20, 1000, 80)`, draw
  black filled polygons `[(0,y),(1900,y+61),(1900,y+85),(0,y+24)]` with Pillow
  ImageDraw. Input PNG SHA-256:
  `b5222b49371731c795abb11d5d0338423a2a0ba59092e7c9bf1022f2e648126e`.
- Discovered default parameters: Use Color Key=0, Color Key=opaque white,
  Do Smooth Range=6. Classic, ARGB8, time step=1, time scale=30.
- The real effect changes 154,271 RGBA bytes and produces 93 colors from
  the two-color input. Normal smoothing is demonstrated, not just success
  JSON or a passthrough image.
- `broker render-video-batch` renders twelve identical inputs in one resident
  worker per run. Frame zero is cold; all eleven warm frames contribute to
  each run's median. Order is A/B, B/A, A/B, three fresh-session pairs.
  No outlier is removed. A second twelve-frame repetition is retained below.
- Before each worker swap's real-AEX run, the resident sampling-probe broker
  integration test passes. Existing `FramePerformance` measurements are
  temporarily serialized in the batch report; this one-line diagnostic is
  removed from the final product diff. It runs after the measured render.
- Baseline worker SHA-256:
  `6c7734e9bc35f7fb7164de283cd1cc8b2a60a479f9c3513949f10496c0ddcc7a`.
  Candidate worker SHA-256:
  `5df4140551443d5f8265d36e88c8fe3c3d38983a34a9efff82e42c2627cb61f3`.
  Both have the same build configuration; only the reviewed coverage change
  differs in product source. The baseline tree precedes that change.
- The fingerprint-recorded repetition used one unchanged diagnostic broker
  SHA-256 `3648e253aaaf8f3ed0121e88765966bfb1fca38ee7c1ee76f0c599845e58e06f`.
  Broker, input and AEX hashes are verified before and after the series.

Build with `tools/build-native.ps1` and the broker Release build. Discover
parameters with the headless harness, pass the returned defaults to batch,
and use fresh request/report/output paths for each run. Local ignored
`target/issue1720/` retains the scripts, binaries, images, raw reports,
identities and rejected/early measurements; no private paths or AEX bytes
are committed here.

## Measurements

Times are ms. Wall is the resident render-frame wall including transport and
validation, not PNG encoding or whole-command startup. Nonselector is computed
per frame as worker-render minus selector time, then medianed (not a difference
of independently aggregated medians).

| Run | Worker | Warm wall | Worker nonselector | Selector |
| --- | --- | ---: | ---: | ---: |
| 1 | A | 132.9889 | 60.4127 | 66.5981 |
| 2 | B | 122.3902 | 49.7718 | 66.8824 |
| 3 | B | 121.6415 | 48.5604 | 67.4364 |
| 4 | A | 134.4817 | 61.4092 | 67.8467 |
| 5 | A | 134.5927 | 61.8142 | 67.1314 |
| 6 | B | 121.2707 | 48.7207 | 66.5199 |

Run-median aggregate: wall 134.4817 -> 121.6415 ms (**9.55% shorter**);
shared worker nonselector 61.4092 -> 48.7207 ms (**20.66% shorter**).
Selector time changes only 0.37%. All three paired nonselector improvements
exceed 17%; the slowest B is faster than the fastest A in this series.

Fingerprint-recorded repetition (same order):

| Run | Worker | Warm wall | Worker nonselector | Selector |
| --- | --- | ---: | ---: | ---: |
| 1 | A | 177.0709 | 87.3629 | 82.9258 |
| 2 | B | 123.3845 | 48.2172 | 66.7399 |
| 3 | B | 122.3484 | 49.2755 | 66.7903 |
| 4 | A | 133.2937 | 60.8464 | 66.6915 |
| 5 | A | 143.9773 | 62.6613 | 66.1251 |
| 6 | B | 121.8962 | 49.3284 | 67.2618 |

Aggregate nonselector reduction is 21.36%. Its wall aggregate is 15.02%,
but A includes a noisy run and that figure is not advertised as the effect's
general speedup. The earlier four-frame series likewise included large
outliers in both variants: wall medians 134.7692 -> 123.0775 ms (8.68%),
nonselector 61.4361 -> 49.3701 ms (19.64%). The reproducible acceptance is
the roughly 20% shared-host-stage improvement; overall-frame benefit is
conservatively around 9%, not a guaranteed >=10% end-to-end improvement.

All comparison frames succeed, are nonempty/decodeable, and have identical
RGBA SHA-256 `d3d11e2e8aadfb2646d263e671e5cea3a4425ded344390bb80b5e4e259b80159`.
Sessions close cleanly, workers exit normally, and final diagnostics are
semantically identical (module-list ordering can differ).

## Verification and boundaries

- Compiled coverage self-test mutates each byte at every supported depth,
  including the last byte and finite float exponent bytes; restored sentinels
  remain unwritten. Existing cropped/empty/solid/partial region cases remain.
- Native Release build and header dependency verification: 207 translation
  units have recorded dependencies. Coverage, Classic execution/runtime and
  suite-report self-tests pass.
- Python UTF-8 built-artifact coverage/batch tests: 24 pass, including Classic
  and Smart, ARGB8/16/32F and partial-output failures.
- After the final worker swap, fixture freshness checks rejected the older
  geometry AEX (22 failures before dispatch). A clean fixture rebuild restored
  all 24 passes; no validation was bypassed and no product fix was needed.
- Rust broker workspace tests and the Release broker isolation self-test pass.
  Optional GPU/manual latency tests remain ignored by their existing gates;
  the full Python suite and an AE comparison were not run locally.
- Second installed real AEX OLMBlur: both workers render four frames cleanly,
  same final diagnostics and same RGBA output hash
  `f20b3cdaa96eefc94a676728d1008d8b898e2cd20a495624126877a3a2abd7cf`.

The multi-time resident workload's live commit can grow with retained frame
state in both variants. This observation is not a leak diagnosis and is not
changed here; bounded retention/plateau/limit advisory work is the separate
memory cycle. This change allocates no new buffer or persistent state.

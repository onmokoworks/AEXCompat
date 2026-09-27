# Advisory render performance diagnostics

`aexcompat-harness --headless --render-performance-diagnostics <aex> classic|smart argb8|argb16|argb32f [slot value]`
uses the ordinary isolated resident render worker. The optional slot/value
pair replaces one discovered parameter default for the entire family. The
command does not save images or launch After Effects. It prints a shareable
JSON report. An incomplete family prints `status: unavailable` with a reason
and exits 2; invalid inputs or failed preflight exit 1. Neither a timing nor
a memory reason changes render success, worker admission, or the established
per-frame deadline.

The fixed ladder is 320×180, 640×360, 1280×720, 1920×1080, each in a fresh
worker with one cold and three same-session warm frames at time 0. All frames
receive the same normalized XY/XOR RGBA8 pattern and one parameter set. The
run requires nonempty rendered pixels and a clean session close at every
resolution. A report has at most 64 samples; an invalid or duplicate
resolution and an oversized family are refused before launch.

`identity` records plug-in file SHA-256 at start/end, host executable and
worker executable SHA-256, parameter JSON SHA-256, depth, render path, time,
and input pattern. The file hashes are observations, not pre-launch gates; an
identity change during the run makes the measurement unavailable. The report
contains no file paths, image bytes, or authorization material. The existing
worker close report supplies the admission-file SHA observed by that worker
and the normal render validation. The full close report is not copied into
this shareable performance document.

The `samples` array carries the raw phase durations and memory readings.
All durations are monotonic nanoseconds. Broker and worker clocks are
independent; only durations, not absolute timestamps, may be compared.
`worker_setup_ns` covers pre-render copying and session preparation after
message validation; `worker_render_ns` surrounds `render_frame`; the
`render_selector_ns` subset sums audited render-selector calls, including
host audit overhead rather than pure plug-in CPU; `worker_finalize_ns`
covers output processing before the response. Broker input-slot write and
output validation/copy are separately measured. Process `PagefileUsage` is
the frame-end *live* committed memory; its peak and Job peak are recorded
separately. A peak alone cannot establish a leak. If a phase cannot be
measured it is null in a raw sample and has an unavailable reason in the
summary, never a fabricated zero.

Per-resolution warm phase summaries are medians of three frames. Cold frame
wall and session-open durations are separate. No outliers are removed.
`warm_ms_per_megapixel` normalizes the warm render-selector median by input
pixel count; adjacent ratios divide dispatch-time growth by pixel-count
growth. The report declares its sample counts, aggregation, outlier policy,
and classification thresholds. Timer resolution is `unavailable` with reason
`not_calibrated`; no precision finer than the platform clock is promised.
Each fresh worker reports the SHA-256 it computed while admitting its AEX,
before `LoadLibraryExW`. The family requires all four worker-observed values
to match the file digest bracket around the run and records them separately.
Incomplete families retain the worker-admission hashes observed before the
failure, including a hash that caused an identity mismatch.
This records worker admission provenance, not a hash of relocated in-memory
image pages; it is never a launch precondition added by this diagnostic.

`superlinear_candidate` requires a normalized dispatch ratio ≥2.25 from the
smallest to largest of the three highest resolutions. This two-step span is
less sensitive to one noisy adjacent measurement; all adjacent ratios remain
visible in the report. `memory_growth_candidate` requires a monotonically rising
frame-end live commit slope of at least 128 KiB/frame in at least two
resolutions. These are advisory candidates, not bug verdicts: legitimate
effects may be superlinear, and allocator/runtime behavior can retain memory
without a leak. `warm_worker_nonselector`, `warm_nonselector_wall`, and the
broker/IPC/scheduling estimate are deliberately named as remainder terms,
not attributed to the plug-in.
The signed endpoint live-commit slope is reported even when it is negative or
intermediate frames decrease; a separate monotonicity field controls whether
that slope can contribute to the growth candidate.

The public `pf_performance_probe` AEX has a mode parameter: 0 linear pixels,
1 fixed work plus linear pixels, 2 bounded superlinear work, 3 a released
temporary allocation, 4 retained but bounded per-frame commit. The built
artifact test compares their ratios and live-memory trends rather than
absolute elapsed times. Mode 4 retains at most 16 × 256 KiB per worker; all
memory is reclaimed when its isolated worker exits.

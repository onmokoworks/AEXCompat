# Independent typed requests in a resident worker

`aexcompat-harness --headless --render-request-batch <manifest.json>` renders
one finite group of independent effect instances. Existing single-request
commands remain the default and retain their behavior. This command does not
start a daemon, GUI, AE, or a worker shared with another command.

```json
{
  "schema": "aexcompat.independent_case_batch",
  "schema_version": 1,
  "plugin": "D:/originals/OLMBlur.aex",
  "route": "smart16",
  "isolation": "fresh_sequence",
  "frame_deadline_ms": 120000,
  "cases": [
    {
      "id": "case-A",
      "input": "D:/run/input.png",
      "request": "D:/run/request.json",
      "output": "D:/run/result.png"
    }
  ]
}
```

The manifest is at most 1 MiB, has 1–128 cases, and uses absolute paths.
Case IDs are unique, nonempty, at most 240 UTF-8 bytes, and contain no control
characters. Output parents must exist; output names and deep sidecar names
must be unique and must not already exist. Unknown fields, duplicate JSON
keys, duplicate IDs/destinations, oversized requests, and malformed typed
assignments are rejected. The frame deadline is required and is 1–600000 ms.
Parameter discovery retains its existing no-deadline policy.

Routes are `smart8`, `smart16`, `smart32`, `classic8`, `classic16`, and
`classic32`, all CPU. SmartFX and deep capabilities must be advertised by the
plug-in. Classic requests for SmartFX-declaring effects are rejected before
opening the render worker. There is no automatic depth conversion, Classic fallback, or worker
restart in this command. Choose the route appropriate to the effect; the OLM
qualification uses SmartFX for nine effects and Classic for OLMSmoother.

Every case uses the existing typed request parser, range validation, layer
path resolution relative to its request file, host-context encoder, and
render-settings preprocessing. Omitted parameters mean untouched defaults,
including defaults outside the legal explicit-edit range (#1824). They are
not converted into explicit assignments or clamped.

One discovery precedes preflight of the whole list. Before opening the render
worker, all cases must agree on dimensions, route/depth, timing step/scale/
duration, static spatial/mask/camera/environment context, render settings,
alpha coverage slots, dependencies, and secondary-layer content. Primary
pixels, current time, and explicit parameter edits may vary. This first
version rejects auxiliary-channel manifests; it does not silently omit them.
GPU, audio, UI actions, animation, closure swaps and native-input fixtures are
outside this interface. Partition incompatible cases into separate commands.

## Instance lifetime and failure behavior

The module and GLOBAL_SETUP/global data stay loaded once. PARAMS_SETUP's
declarations and owned arbitrary default source handles also belong to that
loaded effect. Each case restores declaration metadata, initializes fresh
parameter definitions through the existing constructors, copies arbitrary
values through ARB_COPY, and runs a fresh SEQUENCE_SETUP/render/SEQUENCE_SETDOWN
lifetime. Instance arbitrary values are disposed through their existing
ownership-aware cleanup. The declaration defaults are disposed at final close.

This refines the proposal's possible PARAMS_SETUP-per-case implementation:
KiraKira reuses an owned declaration default handle, so disposing that handle
and calling PARAMS_SETUP again produces a stale source. Keeping declarations
and copying instance values matches the existing ownership model. The native
test fixture reproduces this behavior without proprietary binaries.

Per-case boundaries clear parameter edits, checkout definitions, keyframe/UI
instance state, PF effect state and primary temporal history. They compare
live handle identities and suite lease identities/reference counts against the
global baseline, check world counts/bytes and reject new invalid handle,
arbitrary or world operations. Independent ownership baselines also cover
AEGP memory (including lock depth), PF path checkouts and preparations,
AEGP worlds, render options and receipts, GPU allocations/worlds/exclusive
access, extended-inter allocations, color-profile references, Drawbot/progress
objects, masks/streams/values/keyframe transactions, compute-cache receipts
and computations, asynchronous layer requests, audio checkouts and legacy
sampling sessions. SceneRuntimeState effect handles/leases, transform and
legacy effect streams and their value leases, and selection collections also
have ownership baselines. Audio certification reads actual outstanding handles
after automatic cleanup. Auxiliary chunks are reclaimed before certification.
Module-owned compute-cache values and host facade/tombstone metadata remain
resident; they are not outstanding instance checkouts. Existing
pixel bounds, guard, output coverage and shared output-validation gates apply.

A recoverable render error is reported for its case after successful cleanup;
later cases still run. Sequence failure, captured selector fault, deadline,
worker death, malformed protocol, guard corruption or cleanup failure
invalidates the worker. Remaining cases are `unattempted`. No silent
per-process retry replaces the failed observation. Independent SmartFX cases
also disable the ordinary session's error-14 GPU and error-512/516 Premiere
retries: the first error and actual attempt count remain in the report. A failure returns a nonzero
CLI exit code, including a batch that successfully continues after one failed
case.

DLL statics, TLS, global caches and allocator history remain shared. This is
fresh effect-instance state, not process isolation. Corpus byte equivalence
does not establish equivalence for every plug-in, nor native AE conformance.

## Reports and transport

The worker advertises `render_case:1` in response to the v1 `capabilities`
message. A negotiated session accepts v2 `render_case` with explicit
`parameters`, `frame_index`, and rational `current_time`. It returns v1
`case_done` after cleanup, with `case_cleanup` and the existing native render
report as `case_report`. It cannot mix ordinary frames or closure swaps with
independent cases. An old worker is rejected before case dispatch.

Normal control frames retain the 64 KiB limit. Negotiation permits at most
4 MiB worker replies for the existing diagnostic report (KiraKira exceeds
64 KiB); input messages and shared-section bounds remain unchanged. Pixels
still use the existing copy-through mapping and grow handshake.

Each result preserves the single-image report schema and adds its ID/index,
outcome, requested route, cleanup, actual dispatch depth, worker PID/SHA-256,
session generation, wall and per-stage timings. Deep results use the same
native channel ordering and `.rgba16le` / `.rgba32f-le` sidecars; PNG is a
preview at deep depths. No file is published for a render-error case.
The actually admitted plug-in hash comes from the final worker report.

Case render fields are current-case observations. Resource counters and suite
timelines explicitly have `lifecycle_telemetry_scope` of
`process_cumulative_global_setup_pending_close`; a nonzero live global count
in a case report is not falsely described as a case leak. Return-message and
fault attribution reset at each case. Final `session_close` contains the
process-wide missing-suite and unsupported-call evidence; those two diagnostic
collections in each case report contain only that case, including repeated
failures in separate cases. Final `session_close` also contains the
process-wide lifecycle and module evidence. `evidence_valid` becomes true
only after final global close validates; failed close revokes provisional
case success. Artifacts already written remain available for diagnosis.

## Reproduction

Build the native Release worker/fixtures and the Release harness in this
checkout, then run the behavioral tests:

```text
cargo test -p aexcompat-broker --test independent_cases_native -- --test-threads=1
cargo test -p aexcompat-harness --test independent_cases_cli
```

Run these from `broker/` after building this checkout's worker and fixture.
Both targets are registered in CI's native Rust test partition.
Harness unit tests also cover manifest bounds and strict JSON parsing.

`tools/measure_independent_cases.py --originals <external-AEX-directory>
--output <new-run-directory>` generates deterministic 97×61 images and six
cases (A/B/A/edited-C/B/A), measures three repetitions in alternating order,
checks PNG and native deep bytes against the single-request commands, and
verifies the resident PID exited. Originals and raw reports stay outside
committed evidence. The generated measurement summary records executable and
plug-in identities, output hashes, all timings, and exact-match counts.
Numbers describe AEXCompat on that machine and workload, not native AE.

Recorded qualification: [all ten originals, 29 routes/depths](evidence/independent-case-batch-2026-11-12.json),
[downsample 1/2](evidence/independent-case-downsample-2-2026-11-12.json), and
[downsample 1/4](evidence/independent-case-downsample-4-2026-11-12.json).
The main matrix has 522 exact artifact comparisons; the supplemental OLMBlur
matrix has 36. The records retain every completed sample and identify the
worker/harness bytes. Wall-clock timings include workload variation on a
machine without CPU affinity or machine-wide isolation.

# Rust Host Core Staged Migration

## Scope

Issue #616 is the first bounded gate for moving reusable AE-compatible host
state from the native worker to Rust without a full rewrite. It does not change
the C++ worker, resident protocol (#98), active C++ scene work (#26 / PR #571),
or #614/wgpu/GPU work.

The existing process boundary remains unchanged: the Rust broker launches and
supervises the isolated native worker. Later phases may place a Rust host-core
library behind a thin C++ adapter, but no Adobe SDK-shaped pointer enters the
Rust domain model in phase 0.

## Boundary ownership

| Boundary | Owner | Required containment |
|---|---|---|
| Adobe SDK structs, suite tables, calling convention | Thin C++ adapter | Compile-time SDK/public-observation layout checks |
| Native plug-in invocation | Thin C++ adapter | Windows SEH filter and existing minidump telemetry |
| C adapter to Rust value ABI | C++ adapter plus Rust boundary module | Size/version validation before use |
| Rust callback body | Rust host core | `catch_unwind`; panic becomes a stable error and fault report |
| Opaque object identity | Rust host core after migration | Owner, kind, slot, generation, live-state validation |
| Session mutation | Rust host core after migration | Origin-thread check and explicit lifecycle transition |
| Published diagnostics | Rust host core | Value-only report: no pointer, host handle, absolute path, pixels, or plug-in bytes |

SEH and Rust panic containment are deliberately separate. Rust cannot make an
arbitrary access violation safe, and C++ must not attempt to catch a Rust panic.
The adapter first enters the existing SEH-protected native frame; any call into
Rust is wrapped by the Rust panic boundary before control returns to C++.

## Phase 0 contract

The Phase 0 contract now lives in `broker/crates/host-core/src` after the
Phase 2 ownership extraction. `aexcompat_broker::host_core` compatibility
re-exports preserve the original module paths. The contract owns:

- versioned `#[repr(C)]` value-only call context, status, and opaque token;
- stable error codes for argument, state, thread, handle, panic, and SEH faults;
- generation-aware opaque handle storage with owner and kind checks;
- an origin-thread session state machine;
- a structured value-only report contract.

The matching public adapter declarations live in
`broker/crates/broker/include/aexcompat_host_core_abi.h`. Rust and C++ each
compile independent size, alignment, offset, and stable-error-code checks.
The header has no Adobe type and no pointer field.

The `#[repr(C)]` structures are an AEXCompat adapter ABI, not reconstructed
Adobe SDK layouts. Existing observed Adobe layout constants in
`guest/crates/aex-abi` remain byte-offset evidence only. SDK-shaped callback
tables and raw pointers stay in C++.

## Phase 1 adapter gate (Issue #619)

Phase 1 adds `broker/crates/host-core-ffi`, a dedicated Rust `cdylib` with six
versioned session lifecycle entry points. The entry points accept only the
phase 0 value ABI, keep session objects behind registry-bound opaque tokens,
validate ABI version/size, owner, token, origin thread, state, and stale
generation, and project every completed call into a fixed 72-byte report
snapshot. Rust panic containment runs before an `extern "C"` return; a panic
is never converted to success.

`rust_host_core_ffi_dual_run_selftest` is a thin C++ dynamic loader. It resolves
the six exports at runtime, invokes them inside a native Windows SEH frame, and
runs the same normalized lifecycle and error trace through an independent C++
session oracle. The comparison covers null/version/size rejection, malformed
and stale handles, owner and logical/actual thread mismatches, invalid
transitions, callback counters, disposal, status/report correlation, and
monotonic report identity. Opaque token bits and report IDs are intentionally
checked as identities, not for byte-for-byte equality with the C++ oracle.

This gate does not pass Adobe SDK pointers into Rust, and production worker
routing remains unchanged. It is a pre-routing compatibility proof rather than
an actual AE launch requirement. The resident protocol (#98), active scene
model (#26 / PR #571), and #614/wgpu/GPU scope remain untouched.

## Phase 2 core ownership gate (Issue #621)

Phase 2 moves the value boundary, stable errors, opaque handle registry,
fixed report projection, and session state machine into the
dependency-minimal `aexcompat-host-core` library crate. The FFI `cdylib` now
depends directly on that crate instead of compiling through
`aexcompat-broker` and its image, Windows orchestration, tracing, hashing, and
worker dependency graph.

The broker retains its original public module paths through a compatibility
re-export of the exact core modules, so this extraction does not create a
second type universe or change callers. ABI symbols, enum values, layouts,
panic behavior, opaque token ownership, session transitions, and report
snapshots remain the Phase 1 contract. The existing C++ DLL dual-run is the
runtime equivalence gate.

Parameter transport and approved-artifact/descriptor policy remain in the
broker. Scene/world/parameter semantic migration still waits for the
unresolved #26 / PR #571 correctness work, and resident integration remains
outside #98. Production worker routing remains unchanged.

## Phase 3 native fault-boundary gate (Issue #623)

Phase 3 extracts the DLL loading and native call boundary from the Phase 1
self-test into the reusable C++ adapter
`aexcompat_host_core_adapter.hpp`. The adapter owns one DLL lifetime and a
complete version-1 function table. It resolves the input to an absolute path,
loads dependencies only from that DLL directory and System32, and rejects a
module unless all six session exports are present. It is intentionally limited
to Windows compilers with MSVC-compatible structured exception handling.

Every Rust entry call is made inside the adapter's SEH frame. If a native fault
occurs, a create output token is cleared and the adapter publishes a stable
`SEH_FAULT` status plus a boundary-phase fault report. A report ID zero
explicitly means the fault occurred before Rust could allocate a report; the
status and snapshot IDs remain correlated. Rust panics continue to be handled
separately by `contain_panic` inside the DLL.

Normal adapter methods copy the call context by value and keep output storage
inside the SEH frame. The raw-pointer invocation helpers remain available only
for ABI conformance tests, with the same validity precondition as the C ABI.

The native dual-run now consumes this adapter rather than maintaining its own
loader and SEH helpers. It keeps missing-export rejection in the adapter's
complete-table validation and raises a synthetic SEH exception through the
invocation path to verify token cleanup and normalized status/report fields.
The original independent session oracle comparison remains unchanged.

No Adobe SDK-shaped value enters this adapter. Production worker routing
remains unchanged, scene/world/parameter semantics still wait for #26 / PR
#571, resident integration remains outside #98, and #614/wgpu/GPU remains
untouched.

## Phase 4 pre-cast ABI identity gate (Issue #626)

Phase 4 adds one fixed, pointer-free ABI descriptor to identify the exact
value layouts already shared by the Rust DLL and C++ adapter. Its fields are
limited to a magic value, ABI version, descriptor size, size/alignment pairs
for `AexHostCallContext`, `AexHostCallStatus`, `AexHostOpaqueHandle`, and
`AexHostReportSnapshot`, plus the single session-lifecycle-v1 capability bit.
The descriptor is exported as the data symbol
`aex_host_core_abi_descriptor_v1`; it does not introduce a function table,
SDK pointer, callback, or broader feature schema.

`AdapterV1::Load` now resolves that data symbol first, copies its value inside
an SEH-protected helper, and requires an exact descriptor match before it
resolves or casts any of the six function exports. A missing symbol is
distinguished from an incompatible value, and either result unloads the DLL
without publishing a callable adapter. The native dual-run loads the real
Release Rust DLL through the compatible path, rejects a real DLL with no
descriptor, and exercises every descriptor mismatch through the same
compatibility predicate used by the loader. Missing-function classification
remains covered independently after descriptor validation.

This gate does not negotiate report meaning, alter scene/session state, or
route a production worker. Issue #26 / PR #571, #98, and #614/wgpu/GPU remain
outside its write scope.

## Phase 5 scene identity/generation gate (Issue #628)

After PR #571 landed, Phase 5 freezes only its pointer-free scene identity as a
shared Rust/C value: `project_id`, `object_id`, `generation`, and an integer
object kind, plus three required-zero reserved bytes. The 24-byte layout, the
ten live #571 kind values, and its zero `none` sentinel are asserted
independently by Rust, the public C header, and a native test compiled with the
actual C++ scene model. The C ABI keeps kind as `uint8_t`; Rust validates the
integer before using it and never constructs an enum from an unknown
discriminant.

A dedicated 32-byte identity descriptor contains only magic, ABI version,
descriptor size, identity size/alignment, and the matcher-v1 capability.
`AdapterV1::Load` copies and exactly validates this descriptor inside its SEH
frame before resolving or casting the identity matcher export. The Rust export
is separately wrapped by `contain_panic`.

The matcher receives a C++-owned current identity and a caller-held candidate.
It validates canonical nonzero IDs/generation and required-zero reserved bytes,
then classifies foreign project, wrong kind, different object, and stale
generation using the existing stable host error codes. A standalone native
Release test runs the Rust matcher beside an independent C++ identity oracle
using identities created and invalidated by the real #571
`scene_model::Registry`. The focused Phase 5 Python gate invokes
`tools/test-rust-host-core-scene-identity.ps1`, which builds the Release Rust
DLL and this one native self-test with MSVC `/W4 /WX`; it does not modify or
depend on the shared minihost CMake definition.

This phase does not move `ObjectSnapshot`, stream/keyframe/parameter values,
registry mutation, transaction, or snapshot ownership. Production worker
routing remains unchanged, including every resident and one-shot/session path
owned by Issue #98. The gate does not require After Effects and does not
compare pixels.

## Phase 6 scene object owner-edge gate (Issue #630)

Phase 6 projects exactly one additional value from the C++ `ObjectSnapshot`:
the object identity and its owner identity. The resulting 48-byte
`AexHostSceneOwnerRelation` contains two Phase 5 identities and no pointer,
name, Adobe SDK type, `related_item`, `parent_layer`, stream, keyframe,
parameter, or world value. The all-zero owner sentinel is valid only for a
project root. Every non-project edge requires a canonical owner in the same
project and rejects self-ownership.

The Rust matcher compares a C++-owned current edge with a caller-held
candidate. It reuses the Phase 5 identity classifier for the object and owner,
maps a substituted owner object to `WRONG_OWNER`, and preserves
`STALE_HANDLE` for an invalidated owner generation and `WRONG_KIND` for an
unknown integer kind. The C++ registry remains authoritative for ownership,
generation propagation, and mutation.

A dedicated owner-edge descriptor is copied inside the native SEH frame and
must match the 48-byte layout and matcher capability before the function is
resolved or cast. The Rust export has its own panic boundary. A standalone
MSVC `/W4 /WX` self-test builds only the Phase 6 dual-run and the existing
C++ scene registry, then compares Rust with an independent C++ oracle. It
covers the project-root sentinel, exact child ownership, C++ owner-generation
invalidation, foreign and substituted owners, wrong/unknown kind, malformed
values, self-ownership, cross-project ownership, nulls, and synthetic SEH.

This phase does not move registry ownership or mutation and does not change
`related_item`, `parent_layer`, names, stream/keyframe/parameter/world values,
reports, or sessions. Production worker routing remains unchanged, including
every Issue #98 path. It does not require After Effects and does not compare
pixels.

## Later phases

1. Define the next bounded value-only scene/world/parameter payload beyond
   the Phase 6 owner edge and run it beside the C++ owner. Compare only the
   normalized fields owned by that slice; do not require pixel identity when
   the phase does not render.
2. Add an explicitly opt-in worker dual-run call site only after its ownership
   and resident-session dependencies no longer overlap #26 or #98. Any
   mismatch must fail that migration gate without changing production output.
3. Move selected world/parameter state owners only after layout, ownership,
   callback, cleanup, and thread tests pass for that slice.
4. Keep SDK ABI tables and SEH frames in C++ unless a specific, reviewed
   bindgen layout is proven against the same compiled observation on every
   supported target.

Each phase requires one Issue and one PR, focused Rust tests, relevant native
Release self-tests when native code is touched, source-contract tests,
and independent review with no unresolved P1/P2. For this migration task,
GitHub Actions are explicitly disabled/non-gating by user policy; the merge
gate is the focused Release build/tests, latest-head independent review,
resolved threads, and CLEAN/MERGEABLE GitHub state.

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

## Later phases

1. After #26 / PR #571 lands, define value-only scene/world/parameter snapshots
   and run the Rust model beside the C++ owner. Compare normalized state,
   lifecycle counters, error class, and report fields; do not require pixel
   identity when the phase does not render.
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

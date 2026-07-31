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

`broker/crates/broker/src/host_core` now owns:

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

## Later phases

1. After #26 / PR #571 lands, define value-only scene/world/parameter snapshots
   and run the Rust model beside the C++ owner. Compare normalized state,
   lifecycle counters, error class, and report fields; do not require pixel
   identity when the phase does not render.
2. Move session/report/error decision logic behind the adapter while C++ still
   owns SDK allocation and callbacks. Dual-run mismatches fail the migration
   gate without changing production output.
3. Move selected world/parameter state owners only after layout, ownership,
   callback, cleanup, and thread tests pass for that slice.
4. Keep SDK ABI tables and SEH frames in C++ unless a specific, reviewed
   bindgen layout is proven against the same compiled observation on every
   supported target.

Each phase requires one Issue and one PR, focused Rust tests, relevant native
Release self-tests when native code is touched, source-contract tests,
independent review with no unresolved P1/P2, and green CI before merge.

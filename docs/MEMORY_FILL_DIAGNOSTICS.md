# Request memory-fill diagnostics

The headless typed image-request renderer accepts an optional request object:

```json
{"diagnostics":{"memory_fill":{
  "pattern_hex":"3f3f3f3f",
  "world_padding":true,
  "fresh_output_pixels":true,
  "parameter_unused_storage":true,
  "fresh_nonclear_handles":true,
  "fresh_nonclear_suite_allocations":true
}}}
```

This is an explicit experiment on host-owned uninitialized storage. With the
object absent, allocation initialization, world strides, output sentinels and
report fields retain their existing behavior. There is no environment toggle.
The synchronous request scope does not affect parameter discovery or unrelated
threads. This contract applies to CPU Classic/SmartFX typed image requests;
audio, resident batches and GPU allocations are not qualified by this change.

`pattern_hex` is required and contains 1–64 bytes (2–128 hex digits, case
insensitive). Each omitted scope defaults to false. Unknown/duplicate diagnostic
fields, null objects, non-boolean flags and invalid patterns are rejected.

## Fill boundaries

| Scope | Eligible storage | Preserved storage / limits |
| --- | --- | --- |
| `world_padding` | Primary, secondary, output and PF-created world row tails | Active input/secondary pixels and descriptors; add 16 tail bytes when the requested stride was packed. Existing explicit padding is retained. Clear PF worlds remain zero. |
| `fresh_output_pixels` | Fresh host output before rendering | Position-dependent coverage sentinel XORed with repeating pattern; write-coverage detection remains enabled. |
| `parameter_unused_storage` | Tail of the measured Windows x64 parameter union | Entire active variant, including padding, pointer fields and custom payload handles; defaults and assignments remain intact. |
| `fresh_nonclear_handles` | Fresh PF handle payload and newly grown logical bytes | Handle metadata, locks, retained prefix and ownership validation. Shrink does not fill; regrowth fills only the newly exposed suffix. |
| `fresh_nonclear_suite_allocations` | AEGP memory handles and PF world payloads | Clear flags and ExtendedSuite calloc semantics are respected. AEGP resize retains the prefix and clear policy. GPU allocator callbacks report unsupported. |

The parameter mask preserves the complete measured variant: slider/float slider
88 bytes, checkbox/popup 16 bytes, arbitrary 32 bytes, inside the 120-byte union
at offset 56. Layer variants have no eligible tail. Other variants report
`unsupported` instead of guessing their ABI. Arbitrary-data payload contents
are never poisoned after construction. If a plug-in constructs such a payload
using a fresh non-clear PF handle, only that initial allocation is eligible.

All fills occur after the host's existing initialization and before exposure to
the plug-in, so computing the before hash does not read indeterminate C++
storage. A fill never modifies freed storage, a retained resize prefix, plug-in
stack, plug-in-owned heap, or bytes outside a known host allocation.

The output XOR is a clarification of the original ubdetect proposal: replacing
the spatial sentinel with a flat zero/float pattern would mistake legitimate
black/transparent pixels for unwritten output. `coverage_xor` records this
distinction explicitly; other scopes use `repeat`. A zero pattern retains the
original output sentinel. Pattern indexing is allocation-relative for repeat,
and active-row-relative for output XOR.

## Evidence and failure handling

The final `memory_fill` report includes all five scopes, their `selected` flag,
and `applied`, `not_present`, or `unsupported`. A partially supported scope is
`unsupported` even when some eligible ranges were applied; counts retain both
observations. Clear-only allocations are `not_present` for non-clear fill.

Events record logical allocation ID, owner/callback, selector, size, strided
half-open range (`offset`, `length`, `count`, `stride`), its protected complement,
clear flag, pattern, fill mode, and before/after FNV-1a-64 hashes of the allocation
and protected bytes. These hashes are diagnostic checksums, not cryptographic
identities. `host_bounds_checked` means the fill's range was checked; the normal
render guard/coverage fields separately report post-render integrity. No pointer
addresses or raw payload bytes are logged. Resize retains the allocation ID;
IDs are local to one worker, not stable identities between processes.

An inherited broker-opened file handle receives flushed JSONL events in
`target/memory-fill/`. The worker never reopens a filesystem path. The files are
retained local evidence, including after failure or timeout; the request owns
the handle and the existing private desktop/Job containment remains in place.
They can be removed explicitly after collecting the experiment.

The event budget is 2,048 records plus one truncation marker. Overflow or a
journal write failure makes final scope status `unsupported`. This avoids
unbounded diagnostic memory/disk growth; it does not make omitted events
successful observations. `memory_fill_partial` retains the relative journal
name, captured stderr events and truncation state when final JSON is missing.
It always has `complete:false`; absence in a partial log never proves
`not_present`. The journal is an allocation/fill log, not a matched free-lifetime
trace, and a killed worker cannot provide final guard verification.

## Verification

`--kind classic --self-test-memory-fill` (also available on discovery/smart)
exercises the production allocators, masks, defaults, output coverage and
Classic guard checker. Two patterns change intentional reads independently in
each scope while preserving initialized inputs/parameters, zero flags, resize
prefixes, sentinels and balanced ownership. An injected padding overrun is
detected at 8/16/32 bpc. The Python journal test checks normal, failing and
terminated owned processes without loading an AEX.

The external original campaign uses one baseline and two repetitions each of
`00000000` and `3f3f3f3f`, all scopes enabled, fresh contained workers. It compares
decoded pixel words, not PNG compression, and verifies protected-input hashes,
successful journal writes, no dropped records and owned-process cleanup.

The initialized controls are OLMBlur Smart8, OLMDirectionalBlur Smart16 and
OLMRadialBlur Smart32. DistanceGradation Classic8, DirectionalBlur Smart8 and
RadialBlur Smart32 exercise the three previously investigated undefined-read
cases. Results and executable identities accompany the local review evidence.
Same-pattern instability in the latter cases is not proof of host-fill
causality. Their stack reads and heap-table underreads outside the host-owned
allocation remain beyond this facility's reach; this does not replace the
existing debugger-based ubdetect experiment or establish AE equivalence.

## Concurrent resident-request work

The parallel resident-batch implementation must reject this diagnostic object
or explicitly isolate/reset its configuration, counters, allocation IDs,
selector context, journal and diagnostic world layout for each case. This
implementation deliberately uses the existing one-request worker lifetime.
Before combining the changes, review shared harness request validation,
worker argument parsing, render-session transport and CMake integration.

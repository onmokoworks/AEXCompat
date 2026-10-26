# Resident Smart Render watches

Set `AEXCOMPAT_RESIDENT_TRACE` to a JSON output path and
`AEXCOMPAT_RESIDENT_WATCHES` to a JSON array file. Only SMART_RENDER uses these
settings. Geometry, cropped checkout, extended arenas and 4K options are unchanged.
Do not set `AEXCOMPAT_SMART_TRACE` at the same time (explicit configuration error).
The existing SMART_TRACE interface remains available.

```json
[
  {"id":"plane", "function_rva":45392, "register":"rdx", "size":2361600, "occurrence":1},
  {"id":"context", "instruction_rva":23578, "register":"rcx", "dereference_offset":16912, "size":2361600},
  {"id":"chain", "function_rva":45392, "register":"stack5", "dereference_offsets":[8,56], "size":132710400}
]
```

Numbers are unsigned decimal JSON integers. Supply exactly one RVA selector.
`instruction_rva` means a direct call instruction, not an arbitrary instruction;
unsupported sites appear in `trace_configuration.unhookable_watches`.
Registers: rcx, rdx, r8, r9, rax, stack5 through stack8 (Win64 argument slots).
`dereference_offset` preserves the C53b single-pointer read. Alternatively,
`dereference_offsets` follows up to 16 successive pointer reads:
`base = read_u64(base + offset)` at each step. Use one spelling only.
Unmapped reads or address overflow produce an unmapped snapshot. Both before and
after use the pointer captured at entry, even if the callee changes its arguments.

Use `occurrence:1` for first hit; omit it for all hits, or select another positive
occurrence. IDs must be nonempty and unique; 1–64 watches are accepted. Unknown
fields, invalid registers, ambiguous RVA selectors and invalid limits fail with
an input diagnostic, not a panic.

Each watch allows 1–268435456 bytes. This accommodates a complete 3840×2160 RGBA
float plane (132710400 bytes). A trace reserves at most 256 MiB of logical watch
bytes and 256 witness pairs, including calls still pending. After reservation is
exhausted, selected hits increment `dropped_memory_witnesses` and the trace reports
witness-budget truncation. This bounds retained before/after hex payloads to 1 GiB.
The changed-range summary stops at 4096 ranges and sets
`changed_ranges_truncated`; captured hex/raw bytes are still complete.

JSON preserves the existing snapshot status, hashes and hex fields. Every captured
snapshot also writes `<trace-path>.<witness-index:03>.before.bin` or `.after.bin`,
where indices refer to `memory_witnesses` array order. IDs never become paths.
Use a fresh output path per render; a repeated path overwrites matching files and
may leave old higher-index raw files. Only files corresponding to the current JSON
are authoritative. Raw export errors fail explicitly. Trace files contain private
image data: keep them local; share input/output/capture hashes and counts instead.

Tracing is read-only. Bind captures to matching untraced complete output and input
hashes. No tracing environment variables means no watch configuration or dumps.

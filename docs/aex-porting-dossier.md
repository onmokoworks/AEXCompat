# AEX execution porting dossier

The Unicorn worker can record an opt-in, module-relative execution dossier for
an unchanged Windows x64 AEX. Normal rendering does not install these hooks.

Trace setup behavior:

```sh
aex-guest-worker trace-selector plugin.aex PARAMS_SETUP > params-trace.json
```

Trace the effect body with a real image and optional generic parameter values:

```sh
aex-guest-worker render-trace-png \
  plugin.aex input.png output.png Amount=25 > render-trace.json
```

`render-trace-png` performs normal setup first, then records the classic
`RENDER` or `SMART_RENDER` selector. Its JSON contains the normal render report
under the top level and one dossier per render-lifecycle selector under
`execution_traces`. Smart Render normally records `SEQUENCE_SETUP`,
`FRAME_SETUP`, `SMART_PRE_RENDER`, `SMART_RENDER`, `FRAME_SETDOWN`, and
`SEQUENCE_SETDOWN`.

## Reading the dossier

- `image_sha256`, `preferred_image_base`, and `entry_export` identify the exact
  binary and make every RVA reproducible in a disassembler.
- `events` preserves first-observed order. Repeated visits to the same call
  site are folded into `observed_count`; a pixel loop therefore remains
  readable without losing its execution count.
- `function_rva`, `pc_rva`, `target_rva`, `call_kind`, and
  `instruction_bytes` locate and characterize each observed call.
- `arguments` captures the Win64 register arguments `rcx`, `rdx`, `r8`, and
  `r9`. Pointer-like values are classified as `image`, `guest_data`,
  `guest_stack`, or `host_stub` and include a stable relative offset.
- `functions` groups observed callees, imports, and host callbacks by function
  entry RVA and includes the first 16 entry bytes for matching.
- `state_changes` shows the host-visible ABI fields changed by the selector.
- `timeline` is a compact, depth-indented view of the same event data.

The dossier describes the executed path, not every possible path in the AEX.
Use multiple representative images and parameter sets when porting code with
substantial branches.

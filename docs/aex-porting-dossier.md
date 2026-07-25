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
  plugin.aex input.png output.png \
  --watch function=0xcce0,arg=rcx,size=16,when=entry+return \
  --watch rva=0x350b,register=r9,size=64 \
  --watch-output-pixel 92,841 \
  Amount=25 > render-trace.json
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
- `events` preserves first-observed order. Repeated visits retain bounded
  first/last/distinct exemplars, numeric ranges, and explicit dropped counts.
- `function_rva`, `pc_rva`, `target_rva`, `call_kind`, and
  `instruction_bytes` locate direct, indirect, and runtime jump targets.
- `arguments` captures the Win64 register arguments `rcx`, `rdx`, `r8`, and
  `r9`; `xmm_arguments` and `stack_arguments` retain floating-point and
  fifth-through-eighth arguments. `call_id` connects each call to its return,
  including `rax` and `xmm0`.
- `memory_witnesses` records bounded entry/return bytes, typed interpretations,
  SHA-256, pointer chains, changed ranges, and explicit invalid-read status.
  Output-pixel watches also include format, coordinate, and row offset.
- `basic_blocks` and `branch_edges` describe the path actually taken.
- `modules`, `worker_build_identity`, `trace_configuration`, input PNG SHA, and
  parameter values make a run reproducible. Imported DLLs are identified as
  emulated import-stub modules; they are not silently reported as loaded DLLs.
- `truncation` states the exhausted budget and dropped observation count.
- `functions` groups observed callees, imports, and host callbacks by function
  entry RVA and includes the first 16 entry bytes for matching.
- `state_changes` shows the host-visible ABI fields changed by the selector.
- `timeline` is a compact, depth-indented view of the same event data.

The dossier describes the executed path, not every possible path in the AEX.
Use multiple representative images and parameter sets when porting code with
substantial branches.

Compare two render dossiers without dumping their entire JSON:

```sh
python3 tools/diff_aex_dossiers.py \
  before.json after.json -o dossier-diff.json
```

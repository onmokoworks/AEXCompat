# Known-function observation (Issue #34)

A lightweight path to observe **known functions** (functions whose module-relative
RVA and rough signature are already understood) inside an existing Effect `.aex`
while the worker renders it, using Frida to read a few arguments / return values
and record them on the existing trace timeline.

This is a **reverse-engineering / observation tool, not evidence generation.**
Read the boundary section before using or extending it.

## Boundary: observation is not evidence

Observation output carries no integrity or provenance. It must never be promoted
into the After Effects equivalence corpus. The boundary is expressed structurally,
not just by convention:

- Every observation event uses `host_kind = "native_observation"` and the
  `event_kind = "known_function_invoke"` payload (see
  `contracts/trace/host_trace_event.schema.json`).
- No `conformance_rules.json` rule targets `known_function_invoke` or any
  `known_function.*` field. `tests/test_trace_contracts.py`
  (`test_native_observation_stays_outside_conformance`) enforces that the
  observation kind can never become a conformance comparison target.
- This sits below the receipt / sealed-load-tree tier. It corresponds to the
  crash-containment default tier, and per
  `docs/EVIDENCE_POLICY_2026-07-18.md` it produces diagnostics, not evidence.

## Redaction

The trace validator (`tools/trace_contract_validator.py`) is the enforcing
authority. For `known_function_invoke` it allows only:

- `symbol`, `module_label`: filename-stem strings (no `\ / :`).
- `module_rva`: `^0x[0-9a-f]+$` **and** numerically bounded below 4 GiB
  (`MODULE_RVA_LIMIT`), so it is **module-relative**. Uppercase absolute addresses
  fail the shape check; a lowercased ASLR address such as `0x7ffabc001c40` passes
  the shape but is rejected by the magnitude bound (module images are far under
  4 GiB, 64-bit code addresses are far above it). Compute the RVA as
  `address - module.base` before it ever leaves the process.
- `phase`: `enter` | `leave`.
- `return_value`: a numeric scalar (e.g. a `PF_Err`), never a pointer.
- `fields[]`: `{name, value}` where `value` is a number or boolean only. Field
  `name` may not contain an absolute path.

The global forbidden set (`raw_payload`, `binary_payload`, `pixels`, `pointer`)
and the absolute-path regex still apply. Because field values are constrained to
numbers/booleans, raw pointer values and pixel bytes cannot be logged even by
mistake — the Frida script reduces every read to an interpreted scalar before
`send()`, and the Python formatter rejects anything else (fail-closed).

## Timeout relationship

The worker render timeout is 30 s; exceeding it kills the whole Job. Hooks must
never stall execution. The Frida script only calls `send()`, which is
asynchronous and does not block the hooked thread, so instrumentation does not
push the render toward the timeout. The launcher additionally bounds its own wait
(`--timeout-seconds`, default 30) and kills the spawned worker afterward, so the
observation run preserves the crash-containment intent even though it does not run
under the broker's Job Object (see the PID decision below).

## Hook spec format

A hook set declares the known functions to observe. Schema:
`contracts/observation/known_function_hook_set.schema.json`. Example:
`contracts/observation/examples/example_hook_set.json`.

```json
{
  "schema_version": 1,
  "spec_kind": "known_function_hook_set",
  "module_label": "gamma-classic",
  "hooks": [
    {
      "symbol": "apply_gamma",
      "module_rva": "0x1c40",
      "arg_structs": [
        {"index": 0, "struct": "in"},
        {"index": 1, "struct": "out"}
      ],
      "scalar_args": [
        {"index": 2, "name": "mode", "as": "int", "phase": "enter"}
      ],
      "reads": [
        {"name": "in.width",  "as": "int", "phase": "enter"},
        {"name": "out.width", "as": "int", "phase": "leave"}
      ],
      "return_as": "int"
    }
  ]
}
```

### Passing the signature

Frida does not need the function *typed*; on Windows x64 there is a single calling
convention (`win64`), so ABI is fixed and not declared. A hook conveys three
things instead:

1. **`arg_structs`** — which integer argument slot is a pointer to which struct
   namespace. `{"index": 0, "struct": "in"}` means `args[0]` points to a
   `PF_InData`, so any `reads` entry named `in.*` is read relative to `args[0]`.
2. **`reads`** — which offset-map fields to expand and how to interpret them
   (`as`: `int` | `uint` | `float` | `bool`). The read's prefix (before the first
   `.`) selects the arg base from `arg_structs`. Interpretation width must match
   the field size in the offset map, or resolution fails loudly.
3. **`scalar_args`** — a genuine value argument passed directly in an integer
   register slot (e.g. an `int` mode). Read straight from `args[index]`.

**Float caveat.** On x64, `float`/`double` arguments are passed in XMM registers,
**not** the integer `args[index]` slots. A `scalar_args` entry declared `float`
is rejected during resolution with a hint to read the value through a struct field
instead. Float *fields* inside a struct (read via a struct pointer + offset) are
fine.

`phase` defaults to `enter`. Use `leave` for outputs (e.g. `out.*`) so they are
read after the call returns. `return_as` (`int` | `uint`) reads the scalar return
in `onLeave`.

## Offset map generation

Field offsets/sizes come from the ABI layout probe, so no SDK headers are carried
into the Frida side. Build and run:

```powershell
# Build the probe against the SDK selected by AFTER_EFFECTS_SDK_ROOT.
tools/build-pf-suite-abi-probe.ps1   # (or the abi-layout-probe build in tools/)
# Run it; it prints a JSON offset map on stdout.
target/<abi-layout-probe>.exe > my-offset-map.json
```

The relevant part of the emitted JSON is the `fields` table:

```json
{ "fields": { "in.width": {"offset": 260, "size": 4}, ... } }
```

`tools/known_function_observation.py::load_offset_map` accepts either the whole
probe output or just the `fields` object. A minimal synthetic example (illustrative
offsets, not ABI-accurate) is
`contracts/observation/examples/example_offset_map.json`.

## Resolution and formatting (machine-portable core)

`tools/known_function_observation.py`:

- `resolve_spec(spec, offset_map)` → a `known_function_read_plan`: per hook, the
  `enter_reads` / `leave_reads` (each with `arg_index`, `offset`, `size`,
  `interpret` or a register source) and an optional `return` interpretation.
  Every gap (missing offset field, unmapped struct prefix, size/interpret
  mismatch, float scalar arg) raises `ResolutionError`.
- `build_event(message, ...)` → a validated `known_function_invoke` event from a
  Frida `send` payload (fail-closed on any redaction/shape violation).

Both are unit tested without Frida (`tests/test_known_function_observation.py`).

## Frida script (thin)

`tools/frida/known_function_probe.js`. It receives the resolved plan
(`recv('plan', ...)`), attaches `Interceptor` at `base + module_rva_int`, reads
exactly the scalars the plan names, and forwards them with `send()`. It writes no
files, forwards no raw bytes/pointers, and never blocks. Frida 17.x is assumed
(`Process.findModuleByName().base`).

Because the launcher spawns the worker **suspended**, the plug-in DLL is not
mapped yet — it is `LoadLibrary`'d later, during the render. So the script cannot
attach the hooks before resume. Instead it arms a loader watch (`LoadLibrary*`
`onLeave`) synchronously and attaches the moment the module appears, then emits a
`ready` control message meaning "safe to resume". If the module is already loaded
it attaches immediately and `ready` carries `installed: true`. The known functions
are render-path functions invoked well after load, so attaching in the loader's
`onLeave` never misses them.

## Launcher and the PID-resolution decision

`tools/observe-known-functions.ps1` → `tools/observe_known_functions.py`.

**Decision: spawn the worker's own receipt-free render CLI under Frida.** The
worker (`aex_render_worker.exe`) already exposes a standalone render contract
(`--render-image <aex> <aex_sha256> v5| <input> <output> 16 12 0 1 1 1` and the
`16`/`32`/`-layer`/`--render-request` variants — see
`tools/run-aex-render-gate.ps1`). Frida spawns that argv suspended, the launcher
injects the read plan, **waits for the `ready` acknowledgement** (loader watch
armed / hooks attached) before resuming, then resumes. Frida owns the PID, so the
loader watch is armed before any code runs and every plug-in invocation is
captured deterministically. If the worker does not exit within
`--timeout-seconds`, the launcher kills it and finalises the session as
**incomplete** (`trace_complete: false`, no `session_end`, CLI exit 3) rather than
writing a truncated trace that looks complete.

Trade-offs versus the alternatives the issue listed:

- **(chosen) Frida spawn of the worker render CLI.** Highest completeness
  (pre-execution hooking), and it touches **no broker core path**. Cost: this run
  does not execute under the broker's Job Object / restricted token / sealed load
  tree. That is acceptable because (a) this is an explicitly non-evidence RE path,
  (b) the worker still runs its own plug-in hash and admission checks internally,
  and (c) the launcher applies its own bounded wait + kill to keep crash
  containment. Not a confidentiality sandbox — same caveat as the default tier.
- **(rejected) Enumerate `aex_render_worker.exe` by process name and attach.**
  Racy at both ends: it attaches after some render code has already executed
  (missing early calls) and can mis-target a concurrent worker. The broker also
  creates the worker `CREATE_SUSPENDED` → assigns the Job → `ResumeThread` →
  blocks synchronously, so there is no clean, non-racy window to catch by name.
- **(rejected) Broker-core PID handoff with a resume gate.** Emitting the PID in
  the `CREATE_SUSPENDED` window and holding `ResumeThread` until Frida attaches
  would give the highest fidelity to the *real broker-isolated* worker. But it
  modifies the security-critical launch path
  (`broker/crates/broker/src/windows_process.rs::run_isolated_impl`) and adds a
  resume-gate hang risk if the observer never attaches. Too invasive to land for
  an observation tool; left as a documented future option if evidence-tier
  observation is ever required (and it would require re-running the broker
  integration tests after any worker rebuild).

### Running it (gated: needs a worker build, a real `.aex`, an offset map, Frida)

```powershell
# 1. Build the worker and generate an offset map (see above).
# 2. Reverse the target .aex (e.g. in Ghidra) to get the module-relative RVAs,
#    and write a hook set JSON per the schema.
# 3. Run the observation. RenderArgs is the worker render verb + args.
tools/observe-known-functions.ps1 `
  -Spec my-hook-set.json `
  -OffsetMap my-offset-map.json `
  -Out target/known-function-observation.jsonl `
  -ModuleFile Gamma.aex `
  -PluginLabel gamma-classic `
  -RenderArgs @('--render-image','<aex>','<aex_sha256>','v5|','<input>','<output>','16','12','0','1','1','1')
```

Frida is an **observation-only** dependency. It is intentionally not in
`requirements-dev.txt`, so `python -m pytest` and `cargo test` (the canonical,
machine-portable verification) never require it. The launcher imports `frida`
lazily and fails with a clear message if it is absent.

The worker render does not use After Effects, so `AfterFX` / `aerender` /
`aerendercore` are neither required nor contended by this path.

## Files

| Path | Role |
| --- | --- |
| `contracts/trace/host_trace_event.schema.json` | `known_function_invoke` + `native_observation` |
| `contracts/observation/known_function_hook_set.schema.json` | hook set schema |
| `contracts/observation/examples/` | example hook set + offset map |
| `contracts/trace/examples/native_observation_session.jsonl` | example observation trace |
| `tools/trace_contract_validator.py` | redaction / shape enforcement |
| `tools/known_function_observation.py` | resolver + event formatter (no Frida) |
| `tools/frida/known_function_probe.js` | thin observer |
| `tools/observe_known_functions.py` | Frida launcher (lazy import) |
| `tools/observe-known-functions.ps1` | gated wrapper |
| `tests/test_known_function_observation.py`, `tests/test_observe_known_functions.py`, `tests/test_trace_contracts.py` | machine-portable tests |

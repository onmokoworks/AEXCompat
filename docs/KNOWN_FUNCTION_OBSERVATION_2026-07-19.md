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
- `return_value`: a finite numeric scalar (e.g. a `PF_Err`). Non-finite values
  (`nan`/`inf`) are rejected.
- `fields[]`: `{name, value}` where `value` is a finite number or boolean only.
  Field `name` may not contain an absolute path.

The global forbidden set (`raw_payload`, `binary_payload`, `pixels`, `pointer`)
and the absolute-path regex still apply. The Frida script reduces every read to
an interpreted scalar before `send()`, and the Python formatter rejects
non-scalar or non-finite values (fail-closed).

Redaction has a boundary worth stating plainly: a **numeric type alone does not
prove a value is not a pointer**. A 64-bit integer read (`uint` width 8, or a
`readU64` struct field) can carry a pointer-magnitude value, and the validator
cannot tell that apart from a legitimately large integer. The defence is at the
ABI layer, not the value layer: the hook schema has **no pointer role** (a
`scalar_args` entry can only be `int`/`uint`/`bool` with an explicit `width`, and
struct reads are typed), so a correctly authored spec never reads a pointer. It
is the spec author's responsibility not to point a read at a pointer/handle
field. So the accurate claim is "the tooling never *intentionally* reads a
pointer, and rejects non-finite and non-scalar values" — not "raw pointers are
structurally impossible for any input".

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
        {"index": 0, "struct": "in", "extent": 400},
        {"index": 1, "struct": "out", "extent": 200}
      ],
      "scalar_args": [
        {"index": 2, "name": "mode", "as": "int", "width": 4, "phase": "enter"}
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

1. **`arg_structs`** - which integer argument slot is a pointer to which struct
   namespace, and the struct's byte **`extent`**. `{"index": 0, "struct": "in",
   "extent": 400}` means `args[0]` points to a `PF_InData` of 400 bytes, so any
   `reads` entry named `in.*` is read relative to `args[0]` and **must lie within
   `[0, extent)`**. The extent comes from the abi-layout-probe `*_size` fields
   (e.g. `pf_in_data_size`). A read whose `offset + size` exceeds the extent, or a
   negative offset, is rejected at resolution and re-checked at runtime, so a bad
   offset can never walk outside the struct.
2. **`reads`** - which offset-map fields to expand and how to interpret them
   (`as`: `int` | `uint` | `float` | `bool`). The read's prefix (before the first
   `.`) selects the arg base from `arg_structs`. Interpretation width must match
   the field size in the offset map, or resolution fails loudly.
3. **`scalar_args`** - a genuine value argument passed directly in an integer
   register slot (e.g. an `int` mode). Requires an explicit **`width`** (4 or 8
   bytes) and `as` (`int`/`uint`/`bool`). **Pointers are not representable** - a
   scalar arg cannot be a pointer role - so a spec cannot declare "read this
   pointer register as a value". `index` is bounded (0..32).

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
files, forwards no raw bytes/pointers, and never blocks. Frida 17.x is assumed.

Runtime safety, layered on the resolver's static validation:

- **Module identity.** The target is located by its **full canonical path**
  (`Process.enumerateModules()` matched case-insensitively against the expected
  path the launcher passes), not by basename, so a same-named dependency DLL or a
  swapped module is not hooked. The expected path is the plug-in the worker loads;
  the worker's own hash/admission check on that same file completes the identity
  chain.
- **Executable-range check.** Before `Interceptor.attach`, each target is verified
  to lie inside the module image (`rva < module.size`, overflow-safe) **and** in an
  executable range (`Process.findRangeByAddress().protection` contains `x`). All
  targets are validated first, then attached, so one out-of-range hook fails the
  whole install closed rather than attaching a partial set.
- **Read safety.** Every read null-checks its pointer and re-checks the struct
  extent at runtime. A read failure is reported as a `read_error` control message
  and the field is omitted - it is never thrown out of the Frida callback (which
  could destabilise the worker) and never fabricated.

Because the launcher spawns the worker **suspended**, the plug-in DLL is not
mapped yet - it is `LoadLibrary`'d later, during the render. So the script cannot
attach the hooks before resume. Instead it arms a loader watch (`LoadLibrary*`
`onLeave`) synchronously and attaches the moment the identity-matched module
appears, then emits a `ready` control message meaning "safe to resume". If the
module is already loaded it attaches immediately and `ready` carries
`installed: true`. The known functions are render-path functions invoked well
after load, so attaching in the loader's `onLeave` never misses them.

## Launcher and the PID-resolution decision

`tools/observe-known-functions.ps1` → `tools/observe_known_functions.py`.

**Decision: spawn the worker's own receipt-free render CLI under Frida.** The
worker (`aex_render_worker.exe`) already exposes a standalone render contract
(`--render-image <aex> <aex_sha256> v5| <input> <output> 16 12 0 1 1 1` and the
`16`/`32`/`-layer`/`--render-request` variants - see
`tools/run-aex-render-gate.ps1`). Frida spawns that argv suspended, the launcher
injects the read plan, **waits for the `ready` acknowledgement** (loader watch
armed / hooks attached) before resuming, then resumes. Frida owns the PID, so the
loader watch is armed before any code runs and every plug-in invocation is
captured deterministically.

Completeness gating: a trace is finalised **complete** only if the worker exited
*and* the expected hooks actually attached (`installed_hook_count` equals the
plan's hook count). If the module never loads (wrong `module_path`, or a loader
path we do not watch) the hooks never install and the empty trace is finalised
**incomplete** (`trace_complete: false`, no `session_end`, CLI exit 3) instead of
being reported as a successful empty observation. The same incomplete finalisation
applies on timeout.

Isolation: the spawned worker is assigned to a **Windows Job Object** with
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and a process-memory cap
(`_JobIsolation`). Closing the job handle at the end terminates the whole process
tree, including any descendant the plug-in spawns - the `device.kill(pid)` alone
would only reach the target PID. This gives the observation path the same
crash-containment floor as the broker default tier. It is **not** a
confidentiality sandbox and does not apply a restricted token (Frida spawn cannot
launch under one); the evidence tier's `secure_launch` remains the path for
restricted-token isolation.

Output safety: the trace is written under a fixed allowed root
(`target/known-function-observation/`), canonicalised, with any symlink/reparse
point on the path rejected, via a temp file + atomic `os.replace`. A `-Out` race
or symlink cannot redirect the write elsewhere.

Trade-offs versus the alternatives the issue listed:

- **(chosen) Frida spawn of the worker render CLI.** Highest completeness
  (pre-execution hooking), and it touches **no broker core path**. It runs under a
  Job Object (kill-on-close + memory cap) so descendants are contained, but not
  under the broker's restricted token / sealed load tree. That is acceptable
  because (a) this is an explicitly non-evidence RE path, (b) the worker still runs
  its own plug-in hash and admission checks internally, and (c) the Job Object plus
  the launcher's bounded wait keep the crash-containment floor. Not a
  confidentiality sandbox - same caveat as the default tier.
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
  -Out target/known-function-observation/gamma.jsonl `
  -ModulePath C:\path\to\Gamma.aex `
  -PluginLabel gamma-classic `
  -RenderArgs @('--render-image','<aex>','<aex_sha256>','v5|','<input>','<output>','16','12','0','1','1','1')
```

`-ModulePath` is the canonical path of the plug-in the worker loads; hooks bind
to that exact module. `-Out` must resolve under `target/known-function-observation/`.

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

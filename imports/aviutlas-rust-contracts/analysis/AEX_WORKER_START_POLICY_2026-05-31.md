# AEX Worker Start Policy v0 - 2026-05-31

Purpose: freeze the minimum policy that must exist before any implementation
starts loading allowlisted `.aex` files in a worker process.

This policy follows:

- `analysis/AEX_IMAGE_PROBE_TOOL_SPEC_2026-05-31.md`
- `analysis/AEX_IMAGE_PROBE_DEVELOPMENT_HANDOFF_2026-05-31.md`
- `analysis/AEX_STATIC_CLASSIFIER_RUNBOOK_2026-05-31.md`
- `analysis/EXTERNAL_EFFECT_CAPABILITY_SCHEMA_2026-05-31.md`

## Core Boundary

The broker may eventually spawn a worker process. The broker must never load a
`.aex` module, call `LoadLibrary`, use `libloading`, or route through the
existing AviUtl `PluginManager`.

Current contract tests forbid process execution in the broker because the first
slice is contract-only. When the worker-launch slice begins, replace that guard
with a narrower rule:

- `std::process::Command` or a Windows process API is allowed only in a
  dedicated worker-launch function/module;
- that launcher must pass an explicit worker executable path;
- no shell (`cmd.exe`, PowerShell, `wscript`, file association, or PATH search)
  may be used;
- the broker source must still contain no `.aex` `LoadLibrary`, `libloading`,
  or `PluginManager` path.

## Worker Executable Trust

V0 worker launch requires:

- explicit worker executable path from the installed tool or test build output;
- worker protocol version handshake before any `.aex` load;
- worker reports its own executable path and protocol version;
- broker rejects unknown protocol versions;
- broker rejects worker paths that are not files;
- broker records only path metadata, not binary payloads.

Do not spawn a user-supplied arbitrary executable as an AEX worker.

## Windows Sandbox Minimum

Before the worker can load a `.aex`, implement or explicitly defer each item:

| Area | V0 policy |
| --- | --- |
| Process lifetime | Worker is short-lived per request or per bounded batch. |
| Timeout | Broker enforces launch, setup, render, and teardown timeouts. |
| Process tree | Broker kills the worker and descendants on timeout where possible. |
| Job object | Prefer Windows Job Object with kill-on-job-close before real loading. |
| Handle inheritance | Disable handle inheritance by default. |
| Environment | Use a minimal sanitized environment. |
| PATH/DLL search | Do not rely on ambient PATH. Use explicit worker path and controlled working directory. |
| Working directory | Use a per-request generated directory under `target/aex-image-probe/` or an explicit generated root. |
| Temp/output roots | Confine generated raw buffers, logs, and output images to the generated root. |
| Stdout/stderr | Bound captured output and redact local/private payloads. |
| Network | No network is required; do not add network access in v0. |

The sandbox profile name is a requested/declared policy only. The reportable
gate is `sandbox_preflight.status`, backed by per-primitive measured checks
such as job object attachment, kill-on-close behavior, handle inheritance,
environment sanitization, controlled working directory, bounded stdio, and
network-disabled/no-network-required state.

Passing sandbox preflight is necessary but not sufficient for native `.aex`
loading. It does not approve the loader, does not enable `loader_enabled`, and
does not permit `real_aex_load_enabled=true` until a separate loader slice is
explicitly opened and approved.

If a sandbox item is not implemented in the first worker-launch slice, the
status must stay `worker_protocol_error` or `unsupported_plugin_class`; do not
silently downgrade to unsafe loading.

## Allowlist Identity Gate

Path-text matching is sufficient only for the contract-only broker. Real worker
loading requires a stronger gate:

1. Normalize the `.aex` path to an absolute canonical local path.
2. Require the file to exist and end with `.aex`.
3. Check size against a bounded maximum.
4. Record modified time.
5. Require inventory/classifier evidence for `classic-effect-candidate` or
   `classic-effect`.
6. Require publication/license status to be `local-only` or reviewed
   `public-candidate`; fail closed for `unknown`.
7. Require allowed operations to include the requested operation.
8. Require fixture status such as `local-build-candidate` for initial worker
   tests.

No content hashes are taken by default. A local-only digest may be introduced
only as an explicit opt-in for a reviewed fixture, and it must not be published
without approval.

## Status Vocabulary

Use the schema/broker status names as canonical v0:

- `catalog_ok`
- `ok`
- `allowlist_denied`
- `invalid_request`
- `unsupported_plugin_class`
- `unsupported_selector`
- `unsupported_suite`
- `timeout`
- `plugin_exception`
- `worker_crash`
- `worker_protocol_error`
- `internal_error`

Do not introduce parallel `load_failed`, `setup_failed`, `render_ok`, or
`crash` status names in the worker. Instead report the stage separately:

- `handshake`
- `load`
- `global_setup`
- `params_setup`
- `sequence_setup`
- `render`
- `sequence_teardown`
- `global_teardown`

Examples:

- worker exits before handshake: `worker_protocol_error`, stage `handshake`;
- module load failure: `plugin_exception`, stage `load`;
- missing selector: `unsupported_selector`, relevant stage;
- missing suite: `unsupported_suite`, relevant suite name;
- process timeout: `timeout`, relevant stage;
- process crash: `worker_crash`, stage if known.

## Path And Data Policy

V0 broker/worker data movement:

- input PNG path is user-supplied but broker validates existence, dimensions,
  and byte limits before worker launch;
- broker decodes PNG to raw RGBA8 or writes a bounded temp input buffer under
  the generated root before the worker consumes it;
- worker should not read arbitrary input paths directly in v0;
- `params` stays inline JSON and bounded; `params_path` is deferred;
- `output_png` must be explicit, non-existing, and inside the generated root or
  another explicit generated output root;
- failed worker reports must keep `output_png` null;
- report JSON must not include binary payloads, image bytes, private XML, or
  local project contents.

`describe` does not have to be a separate user command before `render_png`.
However, `render_png` must perform the same class/capability checks internally
before any render selector is called.

## V0 Host API Policy

Allowed initial target:

- local-build or explicitly reviewed classic CPU effect candidates;
- one input RGBA8 frame;
- one output RGBA8 frame;
- frame index/time zero first.

Allowed selector/suite surface:

- global setup;
- parameter setup;
- sequence setup;
- classic render;
- sequence teardown;
- global teardown;
- memory/handle allocation shims required by those selectors;
- progress/abort callback that can request cancellation.

Denied/deferred:

- AEGP;
- AEIO;
- SmartFX-only render path;
- GPU;
- custom UI/dialogs;
- audio;
- layer checkout/project/camera APIs;
- arbitrary file/network APIs;
- effect parameter animation beyond static v0 params.

SmartFX-capable plug-ins may only run if static/describe evidence shows a
legacy/classic render fallback. Otherwise they remain metadata-only.

On setup/render failure, the worker should attempt best-effort teardown for any
stage already entered, but timeout/crash isolation takes priority over cleanup.

## First Worker-Launch Handoff

The next implementation slice should be a worker-launch stub, not real
`EffectMain` hosting:

1. add an explicit worker executable path;
2. spawn without a shell;
3. perform protocol/version handshake;
4. enforce timeout and bounded stdout/stderr;
5. return `worker_protocol_error`, `timeout`, or `worker_crash` in structured
   reports;
6. keep `.aex` loading disabled until sandbox and allowlist identity gates are
   implemented.

Only after that slice is measured should a separate loader slice call into an
allowlisted classic-effect `.aex`.

Status update: the worker-launch stub and generated-root transport preflight are
now measured in `aviutl-rs/examples/aex_image_probe.rs` and
`aviutl-rs/tests/aex_image_probe_contract.rs`. The next policy gate is
worker-side revalidation plus a controlled raw-buffer protocol. Real
`EffectMain` / `.aex` loading remains outside this measured slice.

Status update: worker-side raw-buffer manifest validation is now measured in
`aviutl-rs/examples/aex_effect_worker_stub.rs`. The worker validates only
generated-root raw RGBA metadata and still receives no `.aex` path. The next
policy gate is sandbox/loader approval before any native effect entrypoint call.

Status update: sandbox/loader approval gating is now measured in
`aviutl-rs/examples/aex_image_probe.rs` and
`aviutl-rs/tests/aex_image_probe_contract.rs`. A request may now express
`loader_intent.request_real_aex_load=true`, but that flag is not approval. The
broker requires matching local-only approval metadata, a declared sandbox
profile policy, and required worker revalidation metadata, then still reports
`worker_protocol_error` with `loader_approval.approved=false`,
`loader_enabled=false`, and `real_aex_load_enabled=false`. Missing approval,
missing/unknown sandbox profile, absent worker-side revalidation, and workers
that claim enabled loading are all fail-closed contract cases.

Status update: reportable sandbox preflight is now measured next to loader
approval. `sandbox_preflight` records explicit worker path launch, no-shell
execution, sanitized environment, generated-root working directory, bounded
stdio reporting, no-network-required state, and Windows Job Object
kill-on-close assignment when available. This is runtime evidence only: even a
passed preflight leaves `loader_approval.approved=false`,
`loader_enabled=false`, `real_aex_load_enabled=false`, and `output_png=null`
while the current no-load contract remains active.

Status update: worker-side allowlist identity revalidation is now a measured
contract slice. The broker writes a separate `worker-identity-*.json` manifest
after broker-side identity preflight and passes it as `--identity-manifest`.
The pixel transport manifest remains free of `.aex` identity fields. The worker
validates schema/protocol, freshness, generated-root manifest location,
canonical local `.aex` file metadata, class/publication/license/classifier
status, loader-gate status, sandbox profile metadata, and metadata-only
evidence mode before reporting
`worker_revalidation.status = "passed"`.

Passing worker identity revalidation proves only that the worker independently
matched the broker-approved local fixture identity. It does not approve, enable,
or perform native `.aex` loading. Current reports still keep
`loader_approval.approved=false`, `loader_enabled=false`,
`real_aex_load_enabled=false`, and `output_png=null`.

Current policy gate: the Windows broker path uses an explicit handle-list
launcher for the reviewed worker process. Only the stdin/stdout/stderr handles
needed for the bounded handshake are inherited; the sentinel handle is excluded
and must report `sentinel_not_inherited`. Real native effect entrypoint /
`.aex` loading remains outside this slice until a separate loader slice is
explicitly opened.

Status update: worker-side sandbox attestation is now measured as an additive
runtime contract. The worker handshake reports its observed current directory,
generated-root confinement, absence of PATH and ambient command-processor
environment variables after broker `env_clear`, env count, null stdin
contract, platform, and worker executable name. The broker cross-checks the
attested current directory against the generated sandbox workdir and exposes a
`worker_sandbox_attestation` check in `sandbox_preflight`.

This improves evidence for environment/cwd/stdin claims but still does not
complete the sandbox story.

Status update: handle inheritance is now measured with a Windows-only
inheritable sentinel handle. The broker creates the sentinel before worker
spawn and passes its numeric value only as a probe argument. The worker calls
`GetHandleInformation` on that value:

- `sentinel_not_inherited` -> `handle_inheritance=measured_pass`;
- `sentinel_inherited`, `sentinel_unverified`, `sentinel_missing`, or
  sentinel creation failure -> `handle_inheritance=measured_fail`;
- non-Windows runs report `not_applicable`.

On Windows, the broker now launches the worker with an explicit inherited
handle list so the sentinel is measured as not inherited in the targeted
contract. A passing sentinel check is only prerequisite evidence; it is not a
full handle-table enumeration. Real native effect entrypoint / `.aex` loading
remains outside this slice.

Status update: descendant cleanup is now measured for the synthetic timeout
worker path. On Windows the broker creates the worker suspended, assigns the
process to the Job Object, and only then resumes it. A targeted contract uses a
worker mode that spawns a child process, forces a handshake timeout, closes the
job, and verifies the descendant PID exits. This proves the current
worker-launch race is closed for the measured stub path. It is still not a
production proof for arbitrary native `.aex` loader behavior, external helper
processes launched by plug-ins, or render-time crash/timeout cleanup after real
effect entrypoints are enabled.

Status update: `aviutl-rs/examples/aex_loader_preflight.rs` is now the
metadata-only gate immediately before any separate native loader slice. It reads
the fixture review gate and readiness loader gate, requires exactly one selected
and approved fixture plus a single open render candidate, and still performs no
`.aex` loading. The current local report is `blocked_no_selected_fixture`
because `selected_fixture` remains null and all loader approval flags remain
false.

Status update: worker failure stdout/stderr report previews are now measured as
bounded and sanitized output surfaces. Timeout, worker-crash, and malformed
handshake reports use a dedicated worker-log preview path that caps each stream
at 2048 characters, redacts local absolute path-like tokens, normalizes unsafe
control characters, and emits an explicit truncation marker for overlong output.
The successful handshake JSON parse path remains separate and is not sanitized
before parsing. This is report hygiene only; it does not approve loader access,
enable `.aex` loading, or produce render pixels.

Status update: `aviutl-rs/examples/aex_loader_implementation_manifest.rs` now
adds a no-load review-packet checker after loader preflight. It can report that
the evidence is ready for a separate loader implementation review, but the
manifest itself keeps `native_load_performed=false`,
`broker_may_load_plugin=false`, `loader_may_load_plugin=false`,
`ofx_may_route_to_loader=false`, and
`implementation_gate.native_loader_calls_allowed=false`. It consumes only JSON
metadata from the loader-preflight receipt and readiness capability drafts.

Status update: the worker-boundary policy gate now has a no-load loader ticket.
When real-load intent reaches the worker boundary, the broker writes
`worker-loader-ticket-*.json` under the generated root and passes it as
`--loader-ticket`. The worker validates schema, freshness, generated-root
placement, no-load booleans, selected loader-entry identity, required runtime
evidence, `planned_not_run` native stages, and denied host surfaces before
returning `loader_ticket.status=accepted_no_load`. This is still not loader
approval: the worker reports `aex_loading=disabled`, and broker reports keep
`approved=false`, `loader_enabled=false`, `real_aex_load_enabled=false`, and
`output_png=null`.

Status update: the next host-planning gate is now explicit but still no-load.
`aviutl-rs/examples/aex_native_stage_plan.rs` consumes a ready loader
implementation manifest plus an accepted worker loader ticket, checks that they
agree on effect id and normalized path, and emits the planned PF selector
sequence and host-structure surface for a future classic-effect loader slice.
Every selector is `planned_not_run`; `PF_InData`, `PF_OutData`,
`PF_ParamDef[]`, and source/destination `PF_LayerDef` are only
`declared_not_allocated` / `not_mutated`. The plan keeps
`native_load_performed=false`, `selectors_executed=false`,
`render_performed=false`, `worker_may_load_plugin=false`,
`broker_may_load_plugin=false`, and `ofx_may_route_to_loader=false`.

Status update: `aviutl-rs/examples/aex_loader_approval_receipt.rs` is now the
explicit human-receipt validator after the sanitized loader-slice review
packet. It binds an operator-filled receipt to the exact review packet by
metadata checksum and can report
`approved_for_loader_implementation_review_no_load`, but that approval is only
permission to open a separate loader implementation/code-review slice. The
validator still keeps `native_load_performed=false`, `loader_enabled=false`,
`real_aex_load_enabled=false`, `worker_may_load_plugin=false`,
`render_performed=false`, and `ofx_route_allowed=false`, and rejects receipts
that try to approve those runtime effects.
The same validator can emit a `draft_unapproved_template` from a ready
sanitized loader-slice review packet; that template is intentionally invalid
until a human fills the receipt.

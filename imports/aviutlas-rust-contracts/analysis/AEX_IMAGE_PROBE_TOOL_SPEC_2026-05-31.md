# AEX Image Probe Tool Spec - 2026-05-31

Purpose: define the provisional tool the user suggested: a pre-loader image
probe and future worker-loader readiness path for testing an allowlisted After
Effects `.aex` effect against a still image. This is the fastest useful proof
point for broker-mediated, out-of-process `.aex` hosting before UI integration.

This is a specification and handoff artifact. It does not authorize loading
arbitrary `.aex` binaries in-process.

## Product Shape

Working name: `aex-image-probe`.

The tool should answer one concrete question:

> Can this allowlisted `.aex` describe its params and render one RGBA image
> frame inside an isolated worker?

It should be useful both as a developer CLI and as the future backend for an
AviUtlas "try this AE effect on a frame" debugging panel.

Companion contract artifacts:

- `analysis/AEX_IMAGE_PROBE_REQUEST_SCHEMA_2026-05-31.json`
- `analysis/AEX_IMAGE_PROBE_ALLOWLIST.example.json`
- `analysis/AEX_WORKER_CAPABILITY_REPORT_SCHEMA_2026-05-31.json`
- `analysis/EXTERNAL_EFFECT_CAPABILITY_SCHEMA_2026-05-31.md`
- `analysis/EXTERNAL_EFFECT_CAPABILITY_SCHEMA_2026-05-31.json`
- `analysis/AEX_STATIC_CLASSIFIER_RUNBOOK_2026-05-31.md`
- `analysis/AEX_WORKER_START_POLICY_2026-05-31.md`
- `analysis/AEX_FIXTURE_REVIEW_GATE_2026-05-31.json`

## Non-Negotiable Safety Boundary

- Never load `.aex` into the AviUtlas GUI process.
- Never load `.aex` into the CLI broker process.
- All executable plug-in interaction happens in a short-lived worker process.
- The first build accepts only self-built or explicitly allowlisted plug-ins.
- The default mode is `catalog` / `dry-run`, which does not execute plug-in
  code.
- The worker must have launch and render timeouts.
- Before any native loader slice, worker launch evidence must include a passed
  sandbox preflight, Job Object kill-on-close assignment, and
  `handle_inheritance_status=sentinel_not_inherited` from the explicit
  inherited-handle-list launcher.
- Before any native loader slice, the single-fixture review gate must select
  exactly one reviewed local-build classic-effect fixture after manual approval.
  The current gate selects none and only recommends review order.
- Crashes, timeouts, missing suites, and unsupported selectors are expected
  capability results, not fatal project states.
- Do not write logs containing binary payloads or private image/project data.

## CLI Contract

Broker command examples:

```powershell
aex-image-probe catalog `
  --inventory analysis\AE_AEX_AEP_STATIC_INVENTORY_2026-05-31.json `
  --out analysis\aex_probe_catalog.local.json
```

```powershell
aex-image-probe describe `
  --plugin D:\Projects\01_Project\04_Tools\Ae_Plugins\AdaptiveFilterRust\rust\target\release\AdaptiveFilter.aex `
  --allowlist analysis\aex_probe_allowlist.local.json `
  --worker target\debug\aex-worker.exe `
  --timeout-ms 3000 `
  --out analysis\aex_probe_describe.local.json
```

```powershell
aex-image-probe apply `
  --plugin D:\Projects\01_Project\04_Tools\Ae_Plugins\AdaptiveFilterRust\rust\target\release\AdaptiveFilter.aex `
  --input tests\fixtures\aex_probe\gradient_rgba8.png `
  --output target\aex-probe\AdaptiveFilter.gradient.png `
  --params tests\fixtures\aex_probe\adaptive_filter_params.json `
  --allowlist analysis\aex_probe_allowlist.local.json `
  --worker target\debug\aex-worker.exe `
  --timeout-ms 5000 `
  --report target\aex-probe\AdaptiveFilter.gradient.report.json
```

CLI note: `apply` is a user-facing alias for schema operation `render_png`.

## Modes

### `catalog`

No code execution.

Inputs:

- static inventory JSON;
- optional path filters.

Outputs:

- path;
- size;
- inferred class;
- known local-build source;
- whether candidate is allowed for worker testing;
- reason if blocked.

### `describe`

Current slices must not load the plug-in. A future explicitly approved loader
slice may allow the worker process to load the plug-in and perform the minimum
setup sequence.

Allowed only for allowlisted plug-ins.

Future approved-loader steps:

1. Start worker.
2. Load module only after loader approval, worker identity revalidation, and
   sandbox preflight are accepted.
3. Resolve effect entry point from PiPL/resource/export metadata.
4. Call global setup and parameter setup only.
5. Return parameter descriptors and required capability flags.

No rendering in this mode.

### `apply` / `render_png`

Reserved future mode for rendering one frame from one input image. In the
current implementation, real `.aex` rendering remains disabled; the broker can
only validate request/allowlist/transport gates and launch a no-load worker
stub that fails closed before any native render.

In request JSON, this operation is named `render_png`.

Allowed only after `describe` succeeds or with an explicit `--force-describe`
flag that still runs the same setup inside the worker.

Initial image contract:

- input: PNG decoded by broker or worker into RGBA8;
- output: PNG RGBA8;
- no audio;
- no timeline;
- no layer checkout;
- one synthetic frame at time zero;
- fixed frame size from the input image;
- optional checker/gradient fixture generated by the broker for smoke tests.

## Worker Request Schema

The broker sends a JSON request containing metadata and file paths only. Pixel
payloads should be passed as image files or shared memory later, not embedded in
JSON.

The machine-readable request contract is
`analysis/AEX_IMAGE_PROBE_REQUEST_SCHEMA_2026-05-31.json`.

```json
{
  "schema_version": 1,
  "operation": "render_png",
  "plugin_path": "D:/.../AdaptiveFilter.aex",
  "plugin_allowlist_id": "adaptive-filter-local",
  "input_image": "tests/fixtures/aex_probe/gradient_rgba8.png",
  "output_image": "target/aex-probe/AdaptiveFilter.gradient.png",
  "params_path": "tests/fixtures/aex_probe/adaptive_filter_params.json",
  "pixel_format": "rgba8",
  "frame": {
    "time_seconds": 0.0,
    "frame_index": 0,
    "width": 128,
    "height": 128
  },
  "limits": {
    "timeout_ms": 5000,
    "max_width": 4096,
    "max_height": 4096,
    "max_bytes": 67108864
  }
}
```

## Report Schema

```json
{
  "schema_version": 1,
  "status": "worker_protocol_error",
  "plugin_path": "D:/.../AdaptiveFilter.aex",
  "plugin_class": "classic-effect",
  "entrypoint": "aex_worker_stub_handshake",
  "selectors": [],
  "identity_preflight": {"status": "allowed"},
  "loader_approval": {
    "approved": false,
    "loader_enabled": false,
    "real_aex_load_enabled": false
  },
  "worker_identity_revalidation": {"status": "passed"},
  "sandbox_preflight": {"status": "passed", "profile": "windows-job-object-v0"},
  "output_png": null,
  "warnings": ["worker handshake ok; real .aex loading remains disabled"],
  "unsupported": ["worker-launch slice stops before native entrypoint"],
  "crash": null,
  "elapsed_ms": 42
}
```

Allowed `status` values:

- `catalog_ok`
- `ok`
- `allowlist_denied`
- `unsupported_plugin_class`
- `invalid_request`
- `worker_protocol_error`
- `worker_crash`
- `unsupported_selector`
- `unsupported_suite`
- `timeout`
- `plugin_exception`
- `internal_error`

The shared capability report contract is
`analysis/AEX_WORKER_CAPABILITY_REPORT_SCHEMA_2026-05-31.json`.

## Minimal Classic Effect Host Sequence

Initial sequence:

1. `PF_Cmd_GLOBAL_SETUP`
2. `PF_Cmd_PARAMS_SETUP`
3. `PF_Cmd_SEQUENCE_SETUP`
4. `PF_Cmd_FRAME_SETUP`
5. `PF_Cmd_RENDER`
6. `PF_Cmd_FRAME_SETDOWN`
7. `PF_Cmd_SEQUENCE_SETDOWN`

Defer:

- SmartFX `SMART_PRE_RENDER` / `SMART_RENDER`;
- GPU;
- custom UI;
- audio;
- arbitrary data persistence;
- layer checkout;
- project/camera/light access;
- AEGP suites;
- AEIO.

## First Candidate Plug-ins

Static source inspection suggests these are reasonable first local-build
candidate families after license review:

- `AdaptiveFilter`: CPU based; `SmartRenderGpu` falls back to CPU; has legacy
  render path; first in the review queue, not approved for loading.
- `MedianPro`: CPU based; `SmartRenderGpu` falls back to CPU; focused still
  image filter semantics; second in the review queue.
- `PathArray`: likely more geometry/UI dependent, useful later but not first
  image-probe target.

Implementation context already present in `aviutl-rs`:

- `serde_json` is already used for bridge request/response shapes.
- `image` is already available with PNG support.
- `plugin::types::FrameBuffer` and `plugin::bridge::SharedFrameLayout` provide
  useful frame-transport patterns.
- Do not wire AEX through the existing plug-in manager or any in-process
  `libloading` path. The broker must spawn a separate AEX worker.

Known non-first candidates:

- `AeTimelineSyncAEGP` and `ExEditRemoteAEGP`: AEGP controller plug-ins.
- ONNX/depth/flow/TuiImage/particle/optical flare families: likely heavier or
  external-dependency-heavy; defer until worker isolation and logs are stable.

## Fixture Strategy

Synthetic input images only at first:

- `gradient_rgba8.png`
- `checker_rgba8.png`
- `solid_alpha_rgba8.png`

`aex_probe_fixture_images` now generates this set under
`aviutl-rs\target\aex-probe-fixtures` and writes a local-only manifest guarded
by `analysis/AEX_PROBE_SYNTHETIC_IMAGE_FIXTURES_SCHEMA_2026-06-01.json`.

```powershell
cargo run --example aex_probe_fixture_images --no-default-features -- `
  --out target\aex-probe-fixtures `
  --size 128 `
  --manifest target\aex-probe-fixtures\manifest.local.json
```

The manifest is input-fixture evidence only. It pins
`native_load_performed=false`, `render_performed=false`, `aex_loaded=false`,
`worker_started=false`, `broker_invoked=false`, `ofx_route_invoked=false`,
`ae_invoked=false`, and `private_payload_copied=false`.

The broker now also has an explicit `identity_transport` request operation for
these generated fixtures. It decodes one PNG, validates dimensions and decoded
byte limits, and writes a create-new RGBA8 PNG under
`target\aex-image-probe`. This operation does not require `plugin_path`,
`allowlist`, `worker_exe`, `loader_preflight`, or `loader_intent`; it never
loads or renders `.aex`, and `status=ok` is only pixel-transport evidence.

The first test can validate report shape and file creation without claiming
visual correctness. Pixel correctness requires trusted reference behavior from
After Effects or a known self-built filter with a deterministic expected output.

## Development Chat Prompt

Current implementation status: broker, worker handshake, raw RGBA transport,
identity preflight, worker manifest validation, and fail-closed
sandbox/loader approval reporting now exist. Real `.aex` loading remains
disabled. Worker-side allowlist identity revalidation now also exists as a
separate measured contract. Passing revalidation proves only local identity
agreement between broker and worker; it does not enable native loading.
Reportable sandbox preflight now also measures the Windows Job Object
kill-on-close attempt, generated-root current directory, no-shell launch,
sanitized environment, bounded stdio reporting, and no-network-required state.
Even when `sandbox_preflight.status` is `passed`, the current contract keeps
`loader_approval.approved=false`, `loader_enabled=false`,
`real_aex_load_enabled=false`, and `output_png=null`. The next implementation
slice should improve the isolation evidence or worker capability report, not
call the native effect entrypoint.

The fixture review gate is now also contract-tested. It records
`AdaptiveFilter` as the first candidate to review, keeps `selected_fixture`
null, and forbids parallel first-loader fixtures. It is not permission to load
or render a real `.aex`.

The static classifier also has an opt-in no-execution evidence path:
`--inspect-pe` for PE headers/export/resource type names plus PiPL
resource-entry metadata, and
`--inspect-adjacent-source` for adjacent `build.rs` PiPL declarations. A
stricter `--inspect-pipl-payload` opt-in mode now performs a bounded
semantic-only PiPL content scan: it matches only already-expected display name,
category, match name, and entrypoint strings and emits no raw bytes or decoded
layouts. These fields may support fixture review, but they are not loader
approval and do not change `loader_approval.approved=false`.

The readiness planner now accepts `--fixture-gate
analysis/AEX_FIXTURE_REVIEW_GATE_2026-05-31.json`. When supplied, describe-only
draft allowlist entries, capability drafts, and the closed loader gate are
filtered to the review queue. This keeps broader static classifier findings
from widening the first loader queue.

For static-classifier catalog input, readiness also requires
`pipl_content_scan.status=semantic_matches` and `contents_emitted=false` before
drafting a describe entry. Any missing, partial, truncated, oversized,
no-resource, or content-emitting PiPL scan result remains blocked. This does
not enable loader approval, worker execution, rendering, or OFX routing.

`aex-loader-preflight` is now the last no-execution gate before a future native
loader slice. It requires exactly one selected fixture, fixture-gate approval,
an open readiness loader gate, `render_png` operation approval, worker
revalidation, sandbox preflight, worker attestation, Job Object evidence, and
`sentinel_not_inherited-with-explicit-handle-list`. Passing it still performs no
native loading; failing it keeps the loader slice closed.

When `aex_probe_readiness` is run with `--fixture-gate`, it also writes
`loader-preflight.local.json`. This makes the fail-closed loader-preflight
status visible in the same generated readiness bundle as the draft allowlist,
capabilities, and closed loader gate.

The same fixture-gated readiness bundle now writes `fixture-review.local.json`.
That file is the manual queue packet for choosing a future first-loader fixture;
it is not a selected fixture, loader approval, or render approval.

The OFX side now has a read-only readiness planner:

```powershell
cargo run --example ofx_aex_facade_readiness --no-default-features -- `
  --contract ..\analysis\OFX_AEX_FACADE_CONTRACT_2026-05-31.json `
  --fixture-gate ..\analysis\AEX_FIXTURE_REVIEW_GATE_2026-05-31.json `
  --loader-gate target\aex-probe-readiness-pe-source-gated\loader-gate.local.json `
  --capability-dir target\aex-probe-readiness-pe-source-gated\capabilities `
  --out target\aex-ofx-facade-readiness\ofx-facade.local.json
```

The current report is `deferred_contract_only` with no OFX host/adapter/broker
permission to load `.aex`. This is a contract report only, not an OFX adapter.

The planner also accepts `--native-stage-plan
target\aex-native-stage-plan\native-stage-plan.local.json`. When supplied, the
OFX readiness report emits `native_stage_plan_summary` with only derived
no-load facts: stage-plan status, runtime-evidence readiness, optional
cleanroom-boundary readiness, selector/render/OFX-route blocked booleans, stage
counts, and a forbidden-token contamination flag. A passing native stage plan
does not expose OFX operations or route AviUtlas through OFX; it only proves the
OFX readiness report is looking at the same AEX no-load review packet.

## 2026-06-01 Loader Preflight Receipt Update

The broker request contract now includes `loader_preflight`, a path to the
`aex_loader_preflight` report. It is required only when
`loader_intent.request_real_aex_load=true`.

The broker validates the receipt before worker identity revalidation:

- `status=preflight_passed_no_load`;
- no native load already occurred;
- broker permission to load remains false;
- selected candidate path matches the request plugin path and allowlist entry
  path;
- selected loader-gate effect id matches the allowlist id;
- `selected_loader_entry.effect_id` also matches the allowlist id;
- `selected_loader_entry.plugin_path` matches the request path, allowlist path,
  and selected candidate path;
- `selected_loader_entry.normalized_plugin_path` is consistent with those paths
  and `selected_loader_entry.path_match_status=matched_normalized_path`;
- selected loader-entry readiness fields remain open:
  `pre_loader_status=approved-local-only`,
  `loader_approval_status=approved-local-only`,
  `allowlist_operation_status=render_png`,
  `handle_inheritance_required=sentinel_not_inherited-with-explicit-handle-list`,
  `worker_identity_revalidation_required=passed`,
  `worker_attestation_required=passed`, `sandbox_preflight_required=passed`,
  `job_object_required=assigned-with-kill-on-close`, and `entry_ready=true`;
- top-level selected fixture and nested fixture-gate selection match the
  selected candidate id;
- receipt fixture-gate approval flags are open;
- fixture and loader gate summaries report nonzero candidate/entry counts;
- loader gate is open for exactly one candidate;
- required preflight checks are all passed.

`aex_loader_preflight` can also carry the read-only fixture refresh audit via
`--fixture-refresh-audit
target\aex-fixture-gate-refresh-audit\fixture-gate-refresh.local.json`. When
that optional input is supplied, the report emits
`fixture_refresh_audit_summary` and requires
`fixture_gate_refresh_audit_ready_no_load` before treating that queue-hygiene
evidence as usable. The summary must stay no-load:
`native_load_performed=false`, `render_performed=false`,
`fixture_selected=false`, `loader_enabled=false`,
`wiztree_canonical_non_generated_count=40`, and
`wiztree_generated_target_artifact_count=79`. A failed or contaminated refresh
audit blocks the evidence without echoing private output or payload fields.

This is still not real `.aex` execution. A passing receipt lets the disabled
worker-revalidation path prove identity consistency, then the broker still
fails closed with loader approval false and no output PNG.

## 2026-06-01 Loader Implementation Manifest

`aex_loader_implementation_manifest` is the next no-load checker after a
passing loader-preflight receipt. It reads the receipt plus matching static
readiness capability drafts and emits
`target\aex-loader-implementation\loader-implementation.local.json`.
It can also read `--readiness target\aex-probe-readiness\readiness.local.json`
to verify that the selected effect/path is still a draft-allowlisted readiness
entry with `pipl_content_scan.status=semantic_matches` and
`pipl_content_scan_ready=true`.

The manifest can report
`ready_for_separate_loader_implementation_review_no_load`, but that status is
only permission to review a separate loader implementation slice. It still
freezes:

- `native_load_performed=false`;
- `broker_may_load_plugin=false`;
- `loader_may_load_plugin=false`;
- `ofx_may_route_to_loader=false`;
- `implementation_gate.native_loader_calls_allowed=false`;
- `implementation_gate.broker_may_load_aex=false`;
- `implementation_gate.ofx_facade_may_route_to_loader=false`.

The checker requires the selected fixture, selected candidate, selected loader
entry, and matching capability draft to agree on effect id and normalized path.
The capability draft must remain static metadata only:
`evidence_mode=static-classifier-metadata-only`, `load_status=not_loaded`,
`broker_may_load_plugin=false`, selectors `not_run`,
`aex_worker.supported=false`, and `ofx_facade.supported=false`.
When readiness evidence is supplied, `readiness_pipl_semantic_gate` is also
required to pass. Missing, mismatched, blocked, partial, or non-semantic PiPL
readiness evidence reports `blocked_readiness_evidence` and keeps loader review
closed.

If the preflight receipt includes `fixture_refresh_audit_summary.provided=true`,
the manifest preserves that summary under
`preflight_summary.fixture_refresh_audit_summary` and requires the preflight's
own `fixture_gate_refresh_audit_ready_no_load` check to have passed. The
propagated summary must keep `status=fixture_gate_refresh_ready_no_load`,
`native_load_performed=false`, `render_performed=false`,
`fixture_selected=false`, `loader_enabled=false`,
`wiztree_canonical_non_generated_count=40`,
`wiztree_generated_target_artifact_count=79`,
`generated_target_artifacts_excluded=true`,
`candidates_present_in_refresh=true`,
`input_contains_forbidden_tokens=false`, and `blocked_reason_count=0`. This is
metadata-only queue hygiene; it is not fixture selection or loader approval.

## 2026-06-01 Worker Loader Ticket

`aex_image_probe` now writes a generated-root `worker-loader-ticket-*.json`
when a real-load intent has passed broker-side loader-preflight receipt
validation and worker identity revalidation is being requested. The ticket is
passed to the worker as `--loader-ticket`.

The worker validates the ticket independently and can report
`loader_ticket.status=accepted_no_load`, but the ticket still freezes:

- `native_load_performed=false`;
- `broker_may_load_plugin=false`;
- `worker_may_load_plugin=false`;
- all planned native stages as `planned_not_run`;
- denied surfaces including AEGP, AEIO, SmartFX-only, GPU, custom UI, audio,
  layer checkout, and file/network APIs.

The worker rejects stale, oversized, off-generated-root, malformed,
execution-claiming, mismatched, or runtime-evidence-incomplete tickets before
any native entrypoint could be reached. This is the worker-visible bridge for
future loader authorization metadata, not loader approval itself.

## 2026-06-01 Native Stage Plan

`aex_native_stage_plan` consumes a ready no-load loader implementation manifest
and an accepted worker loader ticket, then emits
`target\aex-native-stage-plan\native-stage-plan.local.json`.

The stage plan maps the coarse worker-ticket stages to the classic-effect PF
selector order a later reviewed loader slice would need:

- module load deferred;
- `PF_Cmd_GLOBAL_SETUP`;
- `PF_Cmd_PARAMS_SETUP`;
- `PF_Cmd_SEQUENCE_SETUP`;
- `PF_Cmd_FRAME_SETUP`;
- `PF_Cmd_RENDER`;
- `PF_Cmd_FRAME_SETDOWN`;
- `PF_Cmd_SEQUENCE_SETDOWN`;
- `PF_Cmd_GLOBAL_SETDOWN`.

It also declares the host-structure surface that must be designed later:
`PF_InData`, `PF_OutData`, `PF_ParamDef[]`, and source/destination
`PF_LayerDef` shapes. All structures remain `declared_not_allocated` and
`not_mutated`.

This remains a no-load planning artifact. It freezes
`native_load_performed=false`, `selectors_executed=false`,
`render_performed=false`, `broker_may_load_plugin=false`,
`worker_may_load_plugin=false`, `ofx_may_route_to_loader=false`, and every
native stage status as `planned_not_run`.

The native stage plan now carries the loader manifest's optional readiness
summary forward as `manifest_readiness_summary`. If the manifest says
readiness evidence was provided, the stage plan requires that evidence to keep
`pipl_content_scan_status=semantic_matches`, `pipl_content_scan_ready=true`,
and `describe` as an allowed readiness operation. If readiness evidence was not
provided, the check remains a compatibility pass and does not authorize loading.

It also carries the optional fixture-refresh queue-hygiene receipt as
`manifest_fixture_refresh_audit_summary`. When the manifest provides that
summary, the stage plan requires
`manifest_fixture_refresh_audit_ready_no_load` and preserves
`status=fixture_gate_refresh_ready_no_load`, no-load/no-render/unselected
booleans, `loader_enabled=false`, the 40 canonical / 79 generated-target AEX
split, generated-target exclusion, candidate presence, and
`input_contains_forbidden_tokens=false`. Missing fixture-refresh evidence
remains compatible; invalid provided evidence blocks without enabling native
load, selector calls, render, or OFX routing.

The stage plan also carries the worker loader ticket's runtime prerequisites as
`ticket_runtime_evidence_summary`. A ticket must preserve
`worker_identity_revalidation_required=passed`,
`worker_attestation_required=passed`, `sandbox_preflight_required=passed`,
`job_object_required=assigned-with-kill-on-close`, and
`handle_inheritance_required=sentinel_not_inherited-with-explicit-handle-list`.
If any of those values are missing or weaker, the stage plan blocks as a worker
ticket problem while keeping all load, selector, render, and OFX permissions
false.

The planner can also accept
`--host-boundary analysis/AEX_HOST_VOCABULARY_BOUNDARY_SCHEMA_2026-06-01.json`.
When provided, it emits an optional `cleanroom_boundary_summary` containing only
schema identity, counts, and no-load policy booleans. It does not serialize the
raw forbidden-token lists from the boundary schema. If the supplied boundary no
longer keeps native loader calls, SDK headers, ABI generators, third-party AE
host crates, third-party PiPL crates, or AviUtl dynamic-loader reuse closed for
AEX, the stage plan blocks while keeping all native load and selector
permissions false.

## 2026-06-01 Cleanroom Vocabulary Boundary

`analysis/AEX_HOST_VOCABULARY_BOUNDARY_SCHEMA_2026-06-01.json` defines the
current no-load AEX source boundary. PF selector names and host-structure names
are allowed only as planning labels; `EffectMain` and `AEEffect` are allowed
only as metadata labels. AEX source files must not introduce direct
`libloading`, `LoadLibrary`, `GetProcAddress`, `bindgen`, `after_effects`,
`pipl`, `repr(C)` PF structs, or callable `EffectMain` extern signatures.

The guard is tested by `aviutl-rs/tests/aex_host_vocabulary_boundary.rs`. It is
not ABI proof and not loader approval; it only catches accidental cleanroom
boundary regressions before a separate reviewed loader slice exists.

## 2026-06-01 No-Load Provenance Audit

`aex_no_load_provenance_audit` now joins the current generated JSON chain:

1. `aex_loader_implementation_manifest`;
2. `aex_native_stage_plan`;
3. `ofx_aex_facade_readiness`.

It emits `target\aex-no-load-provenance-audit\provenance-audit.local.json`.
The audit reports `no_load_provenance_chain_ready` only when:

- the loader manifest is ready and includes semantic readiness evidence;
- the native stage plan remains no-load, no-selector, no-render, no-OFX-route;
- worker runtime evidence and cleanroom boundary evidence are present;
- if fixture-refresh evidence is present, it is preserved from the loader
  manifest into the native stage plan as ready no-load queue hygiene;
- OFX readiness is still `deferred_contract_only`;
- OFX consumed the native-stage summary and still has no host, adapter, broker,
  or AviUtlas-through-OFX route permission;
- no forbidden payload/hash/render-output evidence appears in the inputs.

The audit reads JSON metadata only. It does not approve loading, open `.aex`,
call selectors, render pixels, validate output images, or make OFX the first
AEX route.

## 2026-06-01 WizTree Refresh Guard

`analysis/AEX_WIZTREE_AEX_REFRESH_2026-06-01.json` records a read-only
WizTree `*.aex` refresh under `D:\Projects\01_Project`. The scan saw 119 AEX
files, but 79 are generated `AviUtlas\aviutl-rs\target` test artifacts. After
excluding those generated artifacts, the canonical non-generated AEX count
remains 40 and still matches the static inventory.

This matters for `aex-image-probe`: generated target files such as
`ClassicTest.aex` and `OtherTest.aex` must never be promoted into the
first-loader fixture review queue. The current two fixture-gate candidates,
`AdaptiveFilter.aex` and `MedianPro.aex`, are present but still
`queued-not-approved`.

`aex_fixture_gate_refresh_audit` now joins the fixture review gate with that
WizTree refresh. It emits
`target\aex-fixture-gate-refresh-audit\fixture-gate-refresh.local.json` and
reports `fixture_gate_refresh_ready_no_load` only when:

- the fixture gate is closed and has no selected fixture;
- all fixture approval flags remain false;
- exactly two local-build candidates remain queued and not approved;
- the WizTree refresh still separates 40 canonical non-generated `.aex` files
  from 79 generated target artifacts;
- both fixture-gate candidates are present in the refresh with matching sizes;
- generated target artifacts remain excluded from first-loader fixture review.

This is queue hygiene only. It does not select a fixture, approve a loader,
read an `.aex` binary, ask a worker to describe/render, or validate pixels.

Use this prompt for the separate AviUtlas development chat when ready:

> Implement the first `aex-image-probe` slice without touching existing hot app
> files. Add a broker CLI and a worker skeleton only. The broker must support
> `catalog`, `describe`, and `apply` CLI shapes, with `apply` mapped to request
> operation `render_png`. `describe` / `render_png` may initially return
> `allowlist_denied`, `unsupported_selector`, or `worker_protocol_error` until
> the sandbox/revalidation gate is accepted in a later loader slice. Do not
> load `.aex` in-process.
> Keep all writes
> under new example/tool/test paths. Add JSON schema tests for request/report
> shape and synthetic image fixture generation. Follow
> `analysis/AEX_IMAGE_PROBE_TOOL_SPEC_2026-05-31.md`.

## Next Research Questions

1. Can PiPL/resource metadata be read safely without executing `.aex` code?
2. Which local-build candidate has the simplest parameter set?
3. Can the core filter kernels be tested independently from AE plug-in hosting?
4. Which remaining Windows process restrictions require a lower-level
   `CreateProcessW` suspended-start worker launcher?
5. What minimal worker-side identity revalidation report is enough before a
   reviewed local-build fixture is allowed to load?

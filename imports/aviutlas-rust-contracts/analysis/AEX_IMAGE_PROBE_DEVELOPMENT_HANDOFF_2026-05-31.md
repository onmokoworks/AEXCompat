# AEX Image Probe Development Handoff - 2026-05-31

Purpose: hand the first safe `aex-image-probe` implementation slice to the
separate AviUtlas development chat.

This support thread owns the safety/spec/orchestration context. The development
chat should own code implementation unless the user explicitly redirects the
code slice back here.

## Goal

Create a provisional tool that can eventually apply an allowlisted After
Effects `.aex` effect to a single image and produce an output image plus a
machine-readable report.

The first implementation slice is contract-first. It does not need to load a
real `.aex` yet.

Status note: the contract-only broker already exists:

- `aviutl-rs/examples/aex_image_probe.rs`
- `aviutl-rs/tests/aex_image_probe_contract.rs`
- `aviutl-rs/tests/fixtures/aex_image_probe_request.blocked.json`
- `aviutl-rs/tests/fixtures/aex_image_probe_allowlist.empty.json`

It validates request/allowlist/path/limit contracts and returns blocked
statuses without loading `.aex` or spawning a real worker. The next safe slice
is the worker-launch stub from
`analysis/AEX_WORKER_START_POLICY_2026-05-31.md`, still with `.aex` loading
disabled.

Update: the worker-launch stub slice now exists:

- `aviutl-rs/examples/aex_effect_worker_stub.rs`
- `aviutl-rs/examples/aex_image_probe.rs` with explicit `worker_exe` handshake
  support
- `aviutl-rs/tests/aex_image_probe_contract.rs`

Allowlisted requests now require an explicit absolute reviewed worker
executable path before launch: either the cargo example
`aex_effect_worker_stub` under the crate target tree or a dynamic test stub
under `target/aex-image-probe/test-workers` with an allowed mode, current
process id, and numeric stamp. The broker spawns that path directly, without a
shell or PATH worker discovery, performs only a protocol/version handshake, and
still returns `worker_protocol_error` because `.aex` loading remains disabled.
Malformed handshake, timeout, and crash paths are contract-tested and failed
reports keep `output_png` null.

Update: the generated-root transport and identity preflight slice now exists.
When `render_png` reaches an explicit worker launch, the broker now:

- validates that the `.aex` path exists, canonicalizes, is non-empty, stays
  under the allowlist size cap, and has reviewed publication/license/classifier
  metadata;
- records an `identity_preflight` report with size and modified-time metadata
  but no hashes or binary payloads;
- decodes `input_png`, verifies requested dimensions and decoded RGBA byte
  limits, rejects existing or out-of-root `output_png`, and writes raw RGBA only
  under `target/aex-image-probe`;
- still passes only handshake arguments to the worker and still keeps real
  `.aex` loading disabled.

Update: the worker-side raw-buffer protocol slice now exists. The broker writes
`worker-transport-*.json` next to the raw RGBA buffer and launches the worker
with `--handshake --protocol-version 1 --transport-manifest <path>`. The worker
stub validates only that manifest and raw RGBA file: schema/version,
`pixel_format=rgba8`, nonzero dimensions, `row_stride_bytes`, decoded byte
count, generated-root containment, and raw file length. The transport manifest
contains no `.aex` path and worker-visible status remains canonical
`worker_protocol_error` until a future real render exists.

Update: the sandbox/loader approval gate now exists as a fail-closed contract
slice. `loader_intent.request_real_aex_load=true` can be used by tests to prove
that the future loader route is still blocked. It is not an approval mechanism.
The broker requires matching request/allowlist metadata for local-only approval,
a declared sandbox profile policy, and worker revalidation, then still reports
`worker_protocol_error` with `loader_approval.approved=false`,
`loader_enabled=false`, and `real_aex_load_enabled=false`. Workers that claim
enabled loading, missing approval, unknown sandbox state, or absent
worker-side revalidation are rejected before any native effect entrypoint is
used.

Update: worker-side identity revalidation now exists as a separate measured
contract from loader approval. When loader intent and local-only gate metadata
are present, the broker writes a generated `worker-identity-*.json` manifest and
passes it as `--identity-manifest`; the raw RGBA transport manifest remains
pixel-only and contains no `.aex` identity fields. The worker independently
checks the canonical `.aex` path metadata, allowlist id, class/status gates,
sandbox profile metadata, manifest freshness, and `metadata-only` evidence mode
before reporting `worker_revalidation.status="passed"`. Even when this passes,
the current report keeps `loader_approval.approved=false`,
`loader_enabled=false`, `real_aex_load_enabled=false`, and `output_png=null`.

Update: measured sandbox preflight now exists as runtime evidence, separate
from loader approval. Worker launches attach `sandbox_preflight` with the
requested profile, per-primitive checks, generated-root working directory
state, sanitized environment, bounded stdio reporting, no-network-required
state, and Windows Job Object kill-on-close assignment status when available.
This is only a prerequisite signal: a passed preflight does not enable native
`.aex` loading, does not approve `loader_approval`, and still leaves
`output_png` null under the current no-load contract.

Update: handle inheritance is now measured as part of that preflight with a
Windows-only inheritable sentinel handle. The worker reports whether the
sentinel is visible from the child process. `sentinel_not_inherited` is treated
as `handle_inheritance=measured_pass`; `sentinel_inherited`,
`sentinel_unverified`, `sentinel_missing`, and sentinel creation failure are
hard fails. The current Windows broker path uses an explicit handle-list
launcher, so the measured contract expects `sentinel_not_inherited`; any future
regression where a child observes the sentinel is recorded as a failed
preflight rather than papered over.

Update: a readiness-planning slice now exists before any approved worker
loading. `aviutl-rs/examples/aex_probe_readiness.rs` reads either the static
classifier catalog or the raw inventory and emits local-only draft artifacts
under `target/aex-probe-readiness/`:

- `allowlist.local.draft.json`
- `readiness.local.json`
- `loader-gate.local.json`
- `requests/*.describe.json`
- `capabilities/*.capability.json`
- `analysis/AEX_READINESS_CAPABILITY_DRAFT_SCHEMA_2026-06-01.json`

The planner does not open, hash, load, or execute `.aex` binaries. On the
current local inventory it drafts describe-only allowlist entries for
`AdaptiveFilter` and `MedianPro`; all other candidates remain blocked or
deferred by classifier status.

Update: the readiness planner now also emits a closed loader gate. The gate
lists local-build candidates but keeps `approved=false`,
`loader_enabled=false`, and `real_aex_load_enabled=false`. It requires a later
explicit loader slice, reviewed local fixture approval, worker identity
revalidation, sandbox preflight, worker attestation, Job Object
kill-on-close evidence, and
`handle_inheritance_status=sentinel_not_inherited` with
`handle_inheritance_disabled=true` before any native entrypoint can be
considered. OFX remains deferred as a facade over the same
broker/worker/sandbox gates and cannot bypass the allowlist.

Update: a single-fixture review gate now exists before any loader slice:

- `analysis/AEX_FIXTURE_REVIEW_GATE_2026-05-31.json`

It is metadata-only and records only local path/size evidence for the current
classic-effect candidates. It recommends reviewing `AdaptiveFilter` before
`MedianPro`, but `selected_fixture` remains null and the recommendation is
queue order only, not approval. The gate keeps `approved=false`,
`loader_enabled=false`, `real_aex_load_enabled=false`, and
`render_png_enabled=false`; no `.aex` is opened, hashed, copied, loaded,
described, or rendered by this artifact.

Update: the static classifier now has opt-in read-only evidence collection
before loading:

- `--inspect-pe` reads PE machine, export names, top-level resource type names,
  and bounded PiPL resource-entry metadata only.
- `--inspect-adjacent-source` reads adjacent `build.rs` PiPL declarations only.

The combined local run observed `AdaptiveFilter` and `MedianPro` as x86_64
AEEffect candidates with `PIPL` resources, `EffectMain` exports,
`Filter` category, `ONMK_*` match names, and SmartFX declarations that remain
deferred. This improves fixture review evidence but still does not approve a
real `.aex` load, describe, or render. PiPL resource-entry metadata is limited
to id/name, language, data size, and codepage with `contents_read=false`;
PiPL contents are not decoded in this slice.

Update: the classifier also has `--inspect-pipl-payload`, gated behind
`--inspect-pe`. This is still not a general PiPL parser. It scans bounded PiPL
resource contents only for already-expected semantic strings from adjacent
source/static metadata, records `pipl_content_scan`, and emits no raw bytes,
unknown properties, hashes, decoded layouts, parameter schemas, image paths, or
worker paths. The current local run produced
`catalog.pe-source-pipl.local.json` with 30 `semantic_matches`, 7
`partial_semantic_matches`, and 3 `no_semantic_matches` across the 40 local AEX
inventory rows. Loader approval remains false.

Update: the readiness planner can now consume the fixture review gate:

```powershell
cargo run --example aex_probe_readiness --no-default-features -- `
  --input target\aex-static-classifier\catalog.pe-source-pipl.local.json `
  --out target\aex-probe-readiness-pe-source-gated `
  --fixture-gate ..\analysis\AEX_FIXTURE_REVIEW_GATE_2026-05-31.json
```

This filters the describe-only draft allowlist and closed loader gate to the
review queue in `AEX_FIXTURE_REVIEW_GATE_2026-05-31.json`. In the current local
run it produced two draft entries, `AdaptiveFilter` and `MedianPro`, with
`selected_fixture=null`, `approved=false`, `loader_enabled=false`, and
`real_aex_load_enabled=false`.

Update: when the input is a static-classifier catalog, readiness now requires
`pipl_content_scan.status=semantic_matches` and `contents_emitted=false` before
a candidate can become a describe draft. Missing, partial, truncated, oversized,
no-resource, or content-emitting scan evidence is reported as
`blocked_or_deferred`. This is only a planning gate and still performs no
`.aex` open/hash/load/describe/render.

Update: an explicit loader preflight now exists:

- `aviutl-rs/examples/aex_loader_preflight.rs`
- `aviutl-rs/tests/aex_loader_preflight_contract.rs`

It reads only the fixture review gate and readiness `loader-gate.local.json`.
It does not open, hash, load, execute, describe, or render `.aex`. With the
current gate it reports `blocked_no_selected_fixture`,
`preflight_passed=false`, `native_load_performed=false`, and
`broker_may_load_plugin=false`. This is the final fail-closed JSON check before
any separate loader implementation slice is allowed to start.

Update: fixture-gated readiness now writes `loader-preflight.local.json` beside
`allowlist.local.draft.json`, `readiness.local.json`, `loader-gate.local.json`,
requests, and capability drafts. The generated preflight is still metadata-only
and currently reports `blocked_no_selected_fixture`.

Update: fixture-gated readiness now also writes `fixture-review.local.json`.
This is a metadata-only manual review packet for the current first-loader
queue. It records the queue order, selected fixture state, required manual
decisions, and forbidden actions without opening, hashing, copying, loading, or
rendering `.aex`.

Update: an OFX facade readiness planner now exists:

- `aviutl-rs/examples/ofx_aex_facade_readiness.rs`
- `aviutl-rs/tests/ofx_aex_facade_readiness_contract.rs`

It reads the OFX facade contract, fixture gate, loader gate, and generated
capability drafts, then emits `ofx-facade.local.json`. The current local report
is `deferred_contract_only`, with `ofx_host_may_load_aex=false`,
`ofx_adapter_may_load_aex=false`, `broker_may_load_aex=false`, and
`render_png_exposure=blocked_loader_gate_closed`. This is not an OFX SDK,
adapter, host process, or AEX loader.

It can now optionally consume the native stage plan report and emit
`native_stage_plan_summary`. That summary carries only derived no-load facts:
ready no-load stage-plan status, worker runtime-evidence readiness,
cleanroom-boundary readiness when present, selector/render/OFX-route blocked
flags, and stage counts. Even when this summary is ready, OFX remains a
deferred facade and does not gain describe/render operations or permission to
point at the broker.

## Required Safety Properties

- The broker process must not load `.aex`.
- The broker must not call `LoadLibrary`, `libloading`, or the existing AviUtl
  `PluginManager` for `.aex`.
- Only a separate worker process may load `.aex`, and only after allowlist
  validation.
- The first worker may be a stub that returns
  `allowlist_denied`, `unsupported_selector`, or `worker_protocol_error`.
- No local `.aex`, `.aep`, `.aepx`, `.ffx`, `.auf`, `.exo`, or image payload is
  copied into public tests.
- All output files should go under `target/` or another generated path.
- No Adobe SDK, OFX SDK, `after-effects`, or `pipl` dependency should be added
  in the first contract slice.

## Input Specs

Use these support-thread artifacts:

- `analysis/AEX_IMAGE_PROBE_TOOL_SPEC_2026-05-31.md`
- `analysis/AEX_IMAGE_PROBE_REQUEST_SCHEMA_2026-05-31.json`
- `analysis/AEX_IMAGE_PROBE_ALLOWLIST.example.json`
- `analysis/AEX_WORKER_CAPABILITY_REPORT_SCHEMA_2026-05-31.json`
- `analysis/AE_AEX_AEP_STATIC_INVENTORY_2026-05-31.json`
- `analysis/OFX_AEX_BRIDGE_STRATEGY_2026-05-31.md`
- `analysis/CURRENT_RISK_REGISTER_2026-05-31.md`
- `analysis/AEX_WORKER_START_POLICY_2026-05-31.md`
- `analysis/AEX_FIXTURE_REVIEW_GATE_2026-05-31.json`

## Allowed Write Set

Preferred new files:

- `aviutl-rs/examples/aex_image_probe.rs`
- `aviutl-rs/tests/aex_image_probe_contract.rs`
- `aviutl-rs/tests/fixtures/aex_image_probe_request.blocked.json`
- `aviutl-rs/tests/fixtures/aex_image_probe_allowlist.empty.json`

Optional new files if a worker stub is included:

- `aviutl-rs/examples/aex_effect_worker_stub.rs`
- `aviutl-rs/tests/fixtures/aex_image_probe_worker_response.unsupported.json`

Do not edit without explicit approval:

- `aviutl-rs/src/project/exo.rs`
- `aviutl-rs/src/project/script_lua.rs`
- `aviutl-rs/src/project/exo_keymap.rs`
- `aviutl-rs/src/app.rs`
- `aviutl-rs/src/compat/mod.rs`
- `aviutl-rs/src/plugin/bridge.rs`
- `aviutl-rs/src/plugin/types.rs`
- `aviutl-rs/scripts/gate.ps1`

## Existing Dependencies Available

`aviutl-rs/Cargo.toml` already includes:

- `serde`
- `serde_json`
- `image` with PNG support
- `anyhow`
- `thiserror`

The contract-first slice should not need new dependencies.

Build-surface note:

- Use `--no-default-features` for the first probe checks.
- The crate default features include GUI and native media surfaces, so the probe
  contract should stay minimal until it is intentionally integrated.
- Avoid `PluginManager` and any `.aex` path that resembles the existing
  in-process `libloading`-based AviUtl plug-in loader.

## First Implementation Slice

Implement a broker CLI shape:

```text
cargo run --example aex_image_probe --no-default-features -- --request tests/fixtures/aex_image_probe_request.blocked.json --report target/aex-image-probe/report.json
```

Minimum behavior:

1. Parse request JSON.
2. Validate `schema_version == 1`.
3. Validate operation is one of:
   - `catalog`
   - `describe`
   - `render_png`
4. Load and validate allowlist JSON when operation is `describe` or
   `render_png`.
5. Reject non-allowlisted plug-ins with a structured report.
6. For `render_png`, validate that `input_png` and `output_png` are present, but
   actual image decode may be deferred to the next slice if tests clearly state
   that.
7. Write a report JSON with one of the schema statuses.
8. Never attempt to load `.aex`.

Good first statuses:

- `invalid_request`
- `allowlist_denied`
- `unsupported_plugin_class`
- `worker_protocol_error`

Contract note: `analysis/AEX_IMAGE_PROBE_REQUEST_SCHEMA_2026-05-31.json` is
canonical for v0. If the CLI exposes a user-facing `apply` subcommand, map it to
request operation `render_png`.

## Optional Second Slice

The synthetic input fixture portion of this slice now exists as
`aex_probe_fixture_images`:

```powershell
cargo run --example aex_probe_fixture_images --no-default-features -- `
  --out target\aex-probe-fixtures `
  --size 128 `
  --manifest target\aex-probe-fixtures\manifest.local.json
```

It creates `gradient_rgba8.png`, `checker_rgba8.png`, and
`solid_alpha_rgba8.png` plus a local-only manifest under
`target\aex-probe-fixtures`. The manifest is guarded by
`analysis/AEX_PROBE_SYNTHETIC_IMAGE_FIXTURES_SCHEMA_2026-06-01.json` and keeps
`native_load_performed=false`, `render_performed=false`, `aex_loaded=false`,
`worker_started=false`, `broker_invoked=false`, `ofx_route_invoked=false`,
`ae_invoked=false`, and `private_payload_copied=false`.

The generator now resolves the allowed output root from
`env!("CARGO_MANIFEST_DIR")\target\aex-probe-fixtures`, preflights all PNG and
manifest outputs, writes them with create-new semantics, rolls back newly
created PNGs if the manifest write fails, and rejects existing
symlink/reparse-point ancestors including the `target` parent. Synthetic input
generation therefore cannot be used to overwrite files or escape the generated
fixture root. Output metadata checks fail closed unless the path is simply not
found.

After that fixture generator is available, the broker-side identity transport
slice now exists as explicit request operation `identity_transport`:

1. Decode one generated PNG with `image`.
2. Check width, height, and byte limit.
3. Encode an identity copy to `target/aex-image-probe/*.png` with create-new
   output semantics.
4. Emit report status `ok` only for `operation: "identity_transport"`.

The generated output path check rejects sibling-prefix paths and existing
symlink/reparse-point ancestors under the generated root. Do not label identity
transport as actual `.aex` rendering.

The fixture-to-identity smoke bundle now exists as
`aex_probe_fixture_identity_smoke`:

```powershell
cargo run --example aex_probe_fixture_identity_smoke --no-default-features -- `
  --fixture-manifest target\aex-probe-fixtures\manifest.local.json `
  --out target\aex-image-probe\fixture-identity-smoke `
  --report target\aex-image-probe\fixture-identity-smoke\smoke.local.json
```

It consumes only the synthetic fixture manifest, runs broker
`identity_transport` over each generated PNG, verifies decoded RGBA identity,
and writes a create-new report governed by
`analysis/AEX_PROBE_FIXTURE_IDENTITY_SMOKE_SCHEMA_2026-06-01.json`.
The manifest must contain exactly the generated `gradient`, `checker`, and
`solid_alpha` image entries with their expected file names and patterns, and
the decoded PNG pixels must match those expected synthetic patterns before
transport. Report-visible paths are rejected if they contain
private/payload-looking tokens.
The report has `broker_invoked=true` because the broker identity-transport
contract is exercised, but it keeps `native_load_performed=false`,
`render_performed=false`, `aex_loaded=false`, `worker_started=false`,
`ofx_route_invoked=false`, `ae_invoked=false`, and
`aex_render_correctness_evidence=false`.

## Before Any Real Worker Loading

Use `analysis/AEX_WORKER_START_POLICY_2026-05-31.md`.

The next worker-facing slice should be a worker-launch stub with protocol
handshake, timeout, and bounded logs. It should still keep `.aex` loading
disabled. Update the broker source guard so process spawning is allowed only in
a dedicated worker-launch path while `.aex` loading remains forbidden in the
broker.

## Tests

Recommended targeted tests:

```powershell
cargo test --test aex_image_probe_contract --no-default-features
cargo check --example aex_image_probe --no-default-features
```

Test assertions:

- blocked request returns `allowlist_denied`;
- invalid schema version returns `invalid_request`;
- missing required paths return `invalid_request`;
- broker source does not contain `libloading::Library` or `LoadLibrary` for
  `.aex` handling;
- real-load intent returns a fail-closed `loader_approval` report with
  `approved=false`, `loader_enabled=false`, and `real_aex_load_enabled=false`;
- worker identity revalidation can pass independently while native loading
  remains disabled;
- sandbox preflight reports measured runtime evidence independently while
  native loading remains disabled;
- probe tests run with `--no-default-features`;
- report JSON uses only allowed statuses;
- generated report contains no binary payloads.

## Not In This Slice

- real `.aex` loading;
- PiPL parsing;
- AE SDK bindings;
- `after-effects` / `pipl` crate import;
- OFX SDK import;
- SmartFX/GPU support;
- AEGP/AEIO support;
- AviUtlas UI integration;
- editing hot runtime/plugin bridge files.

## Candidate Future Fixtures

After license/local-build review and only with explicit allowlist:

- `AdaptiveFilter` is first in the review queue, but not selected or approved.
- `MedianPro` is second in the review queue.

Do not use AEGP controller plug-ins as render fixtures:

- `AeTimelineSyncAEGP`
- `ExEditRemoteAEGP`

## Closeout Requirements For Development Chat

Report:

- files changed;
- commands/tests run;
- exact report statuses implemented;
- proof that the broker does not load `.aex`;
- remaining unsupported statuses;
- whether any hot files were touched;
- next worker-sandbox step.

## Development Chat Addendum: Worker Sandbox Attestation

The next AEX slice added worker-side sandbox attestation without enabling real
`.aex` loading. The worker now returns `sandbox_attestation` in successful
handshakes. The broker parses it into `sandbox_preflight.worker_attestation`
and cross-checks observed cwd/env/stdin facts against broker policy:

- worker current directory matches the generated sandbox workdir;
- current directory is under `target/aex-image-probe`;
- PATH and command-processor environment variables are absent after
  `env_clear`;
- stdin contract is `null`;
- env count, platform, and worker executable name are report evidence only.

`sandbox_preflight.checks` now includes
`worker_sandbox_attestation=measured_pass` for the measured happy path. On
Windows, the broker uses an explicit handle-list worker launcher so the
inheritable sentinel is excluded from the child and reports
`handle_inheritance=measured_pass`; if a child ever observes the sentinel, the
same contract fails closed as `measured_fail`.
`environment_sanitized` and `controlled_working_directory` are no longer only
broker-derived in the successful worker path; they also depend on
worker-observed facts.

Still not in scope: a full handle-table enumeration and native `.aex`
loading/rendering. The current probe remains disabled with
`loader_approval.approved=false`,
`loader_enabled=false`, `real_aex_load_enabled=false`, and `output_png=null`.

## Development Chat Addendum: Readiness Capability Drafts

The readiness planner now also emits metadata-only capability drafts under
`capabilities/*.capability.json` for each draft candidate. These files are
derived only from the static inventory/classifier metadata already consumed by
`aex_probe_readiness`.

Each capability draft keeps the loader closed:

- `load_status="not_loaded"`;
- `broker_may_load_plugin=false`;
- `current_supported_operations=[]`;
- `params_status="unknown"` and `params=[]`;
- selector statuses are `not_run`;
- `aex_worker.supported=false`;
- `ofx_facade.supported=false`;
- SmartFX, GPU, AEGP suites, AEIO, audio, layer checkout, and custom UI remain
  unsupported/deferred.

This is planning metadata, not describe/render evidence. It does not add a
worker executable, does not pass `.aex` paths to a worker, and does not read,
hash, load, copy, or execute `.aex` binaries.

The flat capability draft shape is intentionally covered by
`analysis/AEX_READINESS_CAPABILITY_DRAFT_SCHEMA_2026-06-01.json`, not by the
full neutral external-effect capability schema. The draft remains a narrower
readiness artifact with closed-loader fields.

## Development Chat Addendum: Fixture Review And OFX Readiness

The fixture-gated readiness bundle now has two no-load status artifacts:

- `fixture-review.local.json`: manual fixture review packet;
- `loader-preflight.local.json`: final no-load gate before a separate loader
  implementation slice.

The OFX route now has a separate read-only planner,
`ofx_aex_facade_readiness`, that consumes existing readiness artifacts. It keeps
OFX `deferred_contract_only` and proves the no-bypass route before any SDK or
adapter code is introduced. Treat this as planning evidence only; it does not
make `.aex` available through OFX.

## Development Chat Addendum: Descendant Cleanup Proof

The next AEX slice measured timeout descendant cleanup for the worker-launch
stub without enabling real `.aex` loading. On Windows, the broker now starts the
worker with `CREATE_SUSPENDED`, assigns the suspended process to the Job Object,
and resumes it only after assignment succeeds. The worker handle-list launcher
still inherits only the explicit stdio handles and excludes the sentinel.

`aviutl-rs/tests/aex_image_probe_contract.rs` now includes a Windows-only
`worker_timeout_cleans_descendant_process_tree` contract. Its synthetic worker
mode spawns a descendant process, writes the descendant PID under the generated
target root, intentionally exceeds the launch timeout, and then verifies the
descendant exits after the Job Object is closed. The static broker-source guard
also checks for `CREATE_SUSPENDED`, `ResumeThread`, and the
assignment-before-resume ordering.

This is measured evidence for the current stub launch path only. It does not
approve or perform native `.aex` loading, does not prove cleanup for plug-in
spawned helper processes after real selectors are enabled, and does not change
the closed loader state:
`loader_approval.approved=false`,
`loader_enabled=false`, `real_aex_load_enabled=false`, and `output_png=null`.

## Development Chat Addendum: Loader Preflight Hardening

The no-load `aex_loader_preflight` gate now cross-checks JSON consistency before
any future native loader slice can begin. It requires `schema_version=1` for
both fixture and loader gates, unique fixture candidate ids/paths, unique loader
entry ids/paths, selected fixture metadata
`review_status="approved-local-only"`, `fixture_status="local-build-candidate"`,
and `plugin_class="classic-effect-candidate"`, plus exactly one entry-level
approved `render_png` loader candidate that matches the selected fixture.

The hardening closes the case where top-level `open_candidate_count=1` could be
inconsistent with multiple entry-level approved candidates. The preflight still
reads only JSON metadata and still reports `native_load_performed=false` and
`broker_may_load_plugin=false`; a passing preflight only authorizes opening a
separate loader implementation slice.

The emitted report shape is now covered by
`analysis/AEX_LOADER_PREFLIGHT_REPORT_SCHEMA_2026-06-01.json`. The contract test
parses that schema and checks required fields, fixed no-load values, allowed
statuses, required check names, nested fixture/loader reports, required notes,
and forbidden serialized tokens such as hashes, binary payloads, and native
loader entrypoint markers.

The preflight can now optionally consume the generated fixture-refresh audit:
`--fixture-refresh-audit
target\aex-fixture-gate-refresh-audit\fixture-gate-refresh.local.json`. This
adds `fixture_refresh_audit_summary` plus the conditional
`fixture_gate_refresh_audit_ready_no_load` check. The current unselected fixture
gate still blocks as `blocked_no_selected_fixture`, but the report can now carry
machine-checked evidence that the two queued local-build candidates still match
the read-only WizTree refresh and that generated target `.aex` artifacts remain
excluded. Contaminated refresh-audit inputs fail closed without echoing
forbidden output/payload fields.

The OFX facade readiness report has the same dedicated-schema treatment via
`analysis/OFX_AEX_FACADE_READINESS_REPORT_SCHEMA_2026-06-01.json`. Its contract
keeps the route closed by checking no-load booleans, blocked render exposure,
fixture/loader gate summaries, required notes, and forbidden serialized native
loader tokens. This remains planning metadata only, not OFX SDK, adapter, host,
worker describe/render, or `.aex` load evidence.

## Development Chat Addendum: Worker Stdio Report Hygiene

The next no-loader worker slice hardened failure-report stdout/stderr previews.
`aex_image_probe` now uses a dedicated worker-log preview path for timeout,
worker-crash, and malformed-handshake reports. Each preview is bounded to 2048
characters, local absolute path-like tokens are redacted, unsafe control
characters are normalized, and overlong output carries an explicit truncation
marker.

`aviutl-rs/tests/aex_image_probe_contract.rs` now includes
`worker_failure_stdio_previews_are_bounded_and_sanitized`. Its synthetic worker
mode prints private-looking Windows paths plus long stdout/stderr payloads,
then crashes. The contract verifies canonical `worker_crash` /
`stage="handshake"` reporting, no output PNG, no leaked local paths or file
extensions, bounded preview length, and truncation markers.

The successful worker handshake JSON parse path remains separate from report
preview sanitization. This slice improves report safety only; it still does not
load `.aex`, call native effect entrypoints, describe parameters, render
pixels, or open the loader gate.

## Development Chat Addendum: Loader Preflight Receipt

`aex_image_probe` now requires an explicit `loader_preflight` report whenever a
request sets `loader_intent.request_real_aex_load=true`. The broker validates
that receipt before worker identity revalidation can run:

- report status must be `preflight_passed_no_load`;
- `preflight_passed=true`;
- `native_load_performed=false`;
- `broker_may_load_plugin=false`;
- the selected candidate path must match the request `plugin_path`;
- the selected loader-gate effect id must match the allowlist id;
- `selected_loader_entry.effect_id` must also match the allowlist id;
- `selected_loader_entry.plugin_path` and
  `selected_loader_entry.normalized_plugin_path` must be consistent with the
  request path, allowlist path, and selected candidate path;
- `selected_loader_entry.path_match_status` must be
  `matched_normalized_path`, and its readiness/status fields must prove the
  exact selected loader entry is approved for `render_png` with the sandbox,
  job-object, handle-inheritance, worker identity, and attestation prerequisites
  already marked passed;
- the loader gate must be open for exactly one candidate;
- all required loader-preflight checks must be `passed`;
- forbidden serialized tokens for hashes, payloads, native library loading,
  native entrypoints, and rendered pixels remain rejected.

This connects the independent `aex_loader_preflight` gate to the real broker
request path without enabling native loading. Missing, blocked, mismatched, or
payload-bearing receipts fail closed before worker launch. Even with a passing
receipt and worker-side identity revalidation, the current implementation still
reports `approved=false`, `loader_enabled=false`,
`real_aex_load_enabled=false`, and `output_png=null`.

The receipt validation now also keeps fixture identity and allowlist identity
separate but tied together:

- `selected_fixture` must match `selected_candidate.id`;
- `fixture_gate.selected_fixture` must match the same selected candidate id;
- fixture-gate approval booleans must all be open in the receipt;
- fixture and loader gate summaries must report nonzero candidate/entry counts;
- the selected candidate path must match both the request path and allowlist
  entry path;
- `selected_candidate.loader_gate_effect_id` must match the allowlist entry id;
- `selected_loader_entry.effect_id` must match the same allowlist entry id;
- selected loader-entry id, path, normalized path, path-match status, or
  readiness/status mismatches fail closed before worker launch.

This avoids assuming that the human review fixture id and broker allowlist id
are text-identical while still proving the receipt is for the same selected
fixture and same broker-facing effect.

## Development Chat Addendum: Loader Implementation Manifest

`aviutl-rs/examples/aex_loader_implementation_manifest.rs` adds a no-load
review-packet checker for the point after loader preflight. It consumes a
passing `aex_loader_preflight` receipt plus matching
`aex_probe_readiness` capability drafts and emits a local-only manifest whose
best status is `ready_for_separate_loader_implementation_review_no_load`.
It now also accepts optional `--readiness` evidence and, when supplied, requires
the selected effect/path to match a `draft_allowlisted` readiness entry with
`pipl_content_scan.status=semantic_matches` and
`pipl_content_scan_ready=true`.

That ready status still does not authorize native loading. The manifest always
keeps:

- `native_load_performed=false`;
- `broker_may_load_plugin=false`;
- `loader_may_load_plugin=false`;
- `ofx_may_route_to_loader=false`;
- `implementation_gate.native_loader_calls_allowed=false`;
- `implementation_gate.broker_may_load_aex=false`;
- `implementation_gate.ofx_facade_may_route_to_loader=false`.

The checker cross-checks selected fixture identity, selected candidate loader
id, selected loader-entry effect id, raw and normalized paths, and the matching
capability draft. Capability drafts must remain static metadata only:
`load_status=not_loaded`, `broker_may_load_plugin=false`, selectors
`not_run`, `aex_worker.supported=false`, and `ofx_facade.supported=false`.
Supplying readiness evidence adds the `readiness_pipl_semantic_gate`; missing,
mismatched, blocked, partial, or non-semantic PiPL readiness evidence yields
`blocked_readiness_evidence` and keeps implementation review closed.
Execution-claiming, mismatched, or payload/hash-contaminated evidence blocks the
manifest before any implementation review can be opened.

When the preflight receipt includes the optional fixture-refresh audit summary,
the manifest now preserves it at
`preflight_summary.fixture_refresh_audit_summary` and requires
`fixture_gate_refresh_audit_ready_no_load` to have passed in the preflight
checks. If supplied, that summary must still say
`status=fixture_gate_refresh_ready_no_load`,
`native_load_performed=false`, `render_performed=false`,
`fixture_selected=false`, `loader_enabled=false`,
`wiztree_canonical_non_generated_count=40`,
`wiztree_generated_target_artifact_count=79`,
`generated_target_artifacts_excluded=true`,
`candidates_present_in_refresh=true`,
`input_contains_forbidden_tokens=false`, and `blocked_reason_count=0`. This
propagation is queue-hygiene provenance only; it does not select a fixture,
approve a loader, or authorize describe/render.

The report shape is pinned by
`analysis/AEX_LOADER_IMPLEMENTATION_MANIFEST_SCHEMA_2026-06-01.json` and
`aviutl-rs/tests/aex_loader_implementation_manifest_contract.rs`. This manifest
still performs no `.aex` load, describe, render, hash, or binary copy; the
worker-visible ticket below carries a narrower no-load authorization packet to
the worker boundary.

## Development Chat Addendum: Worker Loader Ticket

`aex_image_probe` now creates a generated-root
`worker-loader-ticket-*.json` when `loader_intent.request_real_aex_load=true`
has passed the loader-preflight receipt checks and the broker is about to ask
the worker for identity revalidation. The broker passes that ticket as
`--loader-ticket`.

The worker stub independently validates the ticket and can return
`loader_ticket.status=accepted_no_load`. It still reports
`aex_loading=disabled`, and the broker still returns `worker_protocol_error`
with `loader_approval.approved=false`, `loader_enabled=false`,
`real_aex_load_enabled=false`, and `output_png=null`.

The ticket schema is pinned by
`analysis/AEX_WORKER_LOADER_TICKET_SCHEMA_2026-06-01.json`. Required no-load
invariants are:

- `native_load_performed=false`;
- `broker_may_load_plugin=false`;
- `worker_may_load_plugin=false`;
- selected loader-entry effect id matches the allowlist id;
- selected loader-entry `path_match_status=matched_normalized_path`;
- `allowlist_operation_status=render_png` and `entry_ready=true`;
- runtime prerequisites for worker identity, worker attestation, sandbox
  preflight, Job Object, and handle inheritance are present;
- every planned native stage is `planned_not_run`;
- AEGP, AEIO, SmartFX-only, GPU, custom UI, audio, layer checkout, and
  file/network APIs stay denied.

Malformed, stale, oversized, off-generated-root, mismatched, or
execution-claiming tickets fail closed in the worker before native loading could
be reached.

## Development Chat Addendum: Native Stage Plan

`aviutl-rs/examples/aex_native_stage_plan.rs` is the next no-load artifact after
the loader implementation manifest and worker loader ticket. It consumes those
two JSON artifacts and emits
`target\aex-native-stage-plan\native-stage-plan.local.json`.

The plan converts the worker ticket's coarse stages into a PF selector planning
sequence for a later reviewed classic-effect host slice:

- deferred module load;
- `PF_Cmd_GLOBAL_SETUP`;
- `PF_Cmd_PARAMS_SETUP`;
- `PF_Cmd_SEQUENCE_SETUP`;
- `PF_Cmd_FRAME_SETUP`;
- `PF_Cmd_RENDER`;
- `PF_Cmd_FRAME_SETDOWN`;
- `PF_Cmd_SEQUENCE_SETDOWN`;
- `PF_Cmd_GLOBAL_SETDOWN`.

It also lists the host structures that still need real design:
`PF_InData`, `PF_OutData`, `PF_ParamDef[]`, and source/destination
`PF_LayerDef`. Each is only `declared_not_allocated` and `not_mutated`.

The schema is pinned by
`analysis/AEX_NATIVE_STAGE_PLAN_SCHEMA_2026-06-01.json` and
`aviutl-rs/tests/aex_native_stage_plan_contract.rs`. The plan still freezes
`native_load_performed=false`, `selectors_executed=false`,
`render_performed=false`, `worker_may_load_plugin=false`,
`broker_may_load_plugin=false`, `ofx_may_route_to_loader=false`, and every
stage as `planned_not_run`. It is review input only, not loader approval.

The native stage plan now also carries the loader implementation manifest's
optional readiness evidence as `manifest_readiness_summary`. When the manifest
has `readiness_summary.provided=true`, the stage planner requires the PiPL
readiness gate to remain semantic: `pipl_content_scan_status=semantic_matches`,
`pipl_content_scan_ready=true`, and `describe` present in allowed operations.
When no readiness summary is provided, existing no-load manifest fixtures remain
compatible, but loading is still not authorized.

It now also carries optional fixture-refresh provenance as
`manifest_fixture_refresh_audit_summary`. When that summary is present in the
manifest, the stage plan requires
`manifest_fixture_refresh_audit_ready_no_load` and keeps the same no-load
queue-hygiene facts: `status=fixture_gate_refresh_ready_no_load`,
`native_load_performed=false`, `render_performed=false`,
`fixture_selected=false`, `loader_enabled=false`, 40 canonical non-generated
AEX files, 79 generated target artifacts, generated-target exclusion, candidate
presence, and no forbidden-token contamination. Missing optional evidence is
allowed for legacy no-load manifests; invalid provided evidence blocks the
stage plan.

The native stage plan now also copies the worker loader ticket's runtime
requirements into `ticket_runtime_evidence_summary`. The summary must keep
worker identity revalidation, worker attestation, and sandbox preflight at
`passed`, the Job Object gate at `assigned-with-kill-on-close`, and handle
inheritance at `sentinel_not_inherited-with-explicit-handle-list`. Weaker or
missing runtime evidence blocks the stage plan as a worker-ticket failure
without enabling native load, selector execution, rendering, or OFX routing.

The native stage planner now also accepts the cleanroom boundary schema through
`--host-boundary`. When supplied, the emitted stage plan includes
`cleanroom_boundary_summary` with schema identity, counts, and no-load policy
booleans only. Raw boundary token arrays are not echoed into the plan. A
boundary that reopens native loader calls, SDK headers, ABI generators,
third-party AE host crates, third-party PiPL crates, or AviUtl dynamic-loader
reuse blocks the plan while all native load and selector permissions remain
false.

## Development Chat Addendum: Cleanroom Vocabulary Boundary

`analysis/AEX_HOST_VOCABULARY_BOUNDARY_SCHEMA_2026-06-01.json` now records the
AEX host vocabulary boundary. The guard allows PF selector and host-structure
names only as no-load planning labels, and allows `EffectMain`/`AEEffect` only
as static metadata labels.

`aviutl-rs/tests/aex_host_vocabulary_boundary.rs` scans the AEX planning
examples and OFX-AEX facade example for accidental SDK/native-loader
contamination. It forbids AEX source use of `libloading`, `LoadLibrary`,
`GetProcAddress`, `bindgen`, `after_effects`, `pipl`, `repr(C)` PF structs, and
callable `EffectMain` extern signatures. Existing OS FFI used for worker
isolation evidence remains allowed, but it does not permit `.aex` loading.

This is a regression guard only. It does not define a real ABI and does not
approve native loading, selector calls, parameter discovery, rendering, or OFX
routing.

## Development Chat Addendum: No-Load Provenance Audit

`aviutl-rs/examples/aex_no_load_provenance_audit.rs` now provides the join
check for the generated AEX/OFX evidence chain. It consumes:

- `target\aex-loader-implementation\loader-implementation.local.json`;
- `target\aex-native-stage-plan\native-stage-plan.local.json`;
- `target\aex-ofx-facade-readiness\ofx-facade.local.json`.

It can also consume optional fixture identity smoke evidence from
`target\aex-image-probe\fixture-identity-smoke\smoke.local.json`. That input is
sanitized into `fixture_identity_smoke_summary`; PNG path fields are not
propagated into the audit report. The audit treats `input_png` and `output_png`
as allowed only in the expected smoke entry path fields, scans the rest of the
smoke report for forbidden evidence, and fails closed on extra loader, payload,
hash, or rendered-pixel fields.

The output is
`target\aex-no-load-provenance-audit\provenance-audit.local.json`, pinned by
`analysis/AEX_NO_LOAD_PROVENANCE_AUDIT_SCHEMA_2026-06-01.json` and
`aviutl-rs/tests/aex_no_load_provenance_audit_contract.rs`.

Treat `no_load_provenance_chain_ready` as review evidence only. It proves the
metadata chain stayed closed: no native load, no selector execution, no render,
no OFX route, semantic readiness evidence preserved, runtime/cleanroom evidence
present, optional fixture-refresh queue hygiene preserved when present, optional
fixture identity smoke preserved as broker-only synthetic identity transport
when present, and OFX still deferred. It is not loader approval, is not AEX
render correctness evidence, and must not be used to bypass the separate
reviewed loader slice.

## Development Chat Addendum: Loader Slice Review Packet

`aviutl-rs/examples/aex_loader_slice_review_packet.rs` now creates the sanitized
handoff packet for a separate loader implementation review slice. It consumes:

- `target\aex-loader-implementation\loader-implementation.local.json`;
- `target\aex-no-load-provenance-audit\provenance-audit.local.json`;
- `analysis\AEX_FIXTURE_REVIEW_GATE_2026-05-31.json`.

The output is
`target\aex-loader-slice-review\loader-slice-review.local.json`, pinned by
`analysis/AEX_LOADER_SLICE_REVIEW_SCHEMA_2026-06-01.json` and
`aviutl-rs/tests/aex_loader_slice_review_packet_contract.rs`.

The packet redacts upstream private plug-in paths to booleans, keeps
`loader_slice_approved=false`, `loader_enabled=false`,
`real_aex_load_enabled=false`, `native_loader_calls_allowed=false`,
`broker_may_load_aex=false`, `worker_may_load_plugin=false`,
`render_performed=false`, and `ofx_route_allowed=false`, and requires explicit
user approval, code review, local-build classic-effect fixture review,
cleanroom boundary, license review, worker isolation evidence, OFX deferral,
and generated-target fixture exclusion before any future loader slice can be
opened.

The fixture gate is now an explicit input. A ready packet requires a selected
local-build classic-effect candidate with `review_status=approved-local-only`,
runtime evidence present, and generated-target candidates excluded. The current
checked-in fixture gate remains `review_queue_not_approved`, so local runs
against that gate should produce `blocked_loader_slice_review_packet` until an
explicit manual fixture approval is recorded.

Treat `ready_for_manual_loader_slice_review_no_load` as handoff evidence only.
It is not a loader approval receipt, does not load or describe an `.aex`, and
does not permit worker, broker, render, or OFX execution.

## Development Chat Addendum: Loader Approval Receipt Validation

`aviutl-rs/examples/aex_loader_approval_receipt.rs` now validates an explicit
operator-filled receipt against the sanitized loader-slice review packet. It
consumes:

- `target\aex-loader-slice-review\loader-slice-review.local.json`;
- a local-only `aex-loader-approval-receipt.local.json` supplied by the
  operator.

The output is
`target\aex-loader-approval\approval-validation.local.json`, pinned by
`analysis/AEX_LOADER_APPROVAL_RECEIPT_SCHEMA_2026-06-01.json` and
`aviutl-rs/tests/aex_loader_approval_receipt_contract.rs`.

The same example can also emit
`target\aex-loader-approval\approval-receipt-template.local.json` with
`--draft-template`. That template requires a ready sanitized review packet,
stays `draft_unapproved_template`, keeps all runtime effects false, and must
validate as `invalid_receipt` until a human fills the receipt.

An accepted report is
`approved_for_loader_implementation_review_no_load`: approval to open a
separate loader implementation/code-review slice only. It keeps
`native_load_performed=false`, `loader_enabled=false`,
`real_aex_load_enabled=false`, `native_loader_calls_allowed=false`,
`worker_may_load_plugin=false`, `broker_may_load_aex=false`,
`render_performed=false`, and `ofx_route_allowed=false`.

The validator rejects blocked review packets, checksum mismatches, receipt
effects that try to allow native load, worker plug-in load, render, or OFX
route, and receipts that embed private plug-in paths, `.aex` filenames,
`EffectMain`, `AEEffect`, worker executable paths, image paths, payloads,
hashes, or rendered-pixel evidence.

## Development Chat Addendum: WizTree AEX Refresh

`analysis/AEX_WIZTREE_AEX_REFRESH_2026-06-01.json` is the latest read-only AEX
inventory refresh. It found 119 `.aex` files under `D:\Projects\01_Project`,
but 79 are generated `AviUtlas\aviutl-rs\target` test artifacts. Excluding
those generated files leaves the same 40 canonical non-generated AEX candidates
from the static inventory.

For loader work, do not use generated target artifacts as fixture candidates.
The current first-loader review gate remains intentionally narrow:
`AdaptiveFilter.aex` and `MedianPro.aex` are present and still
`queued-not-approved`. Additional small local builds such as `ONMK_Filters.aex`,
`MinimaxMap.aex`, and `RefractionDispersion.aex` are later-review candidates
only.

`aviutl-rs/examples/aex_fixture_gate_refresh_audit.rs` is the guard to run
after a WizTree refresh and before any loader preflight. It consumes the fixture
review gate and `AEX_WIZTREE_AEX_REFRESH_2026-06-01.json`, then emits
`target\aex-fixture-gate-refresh-audit\fixture-gate-refresh.local.json`.

Treat `fixture_gate_refresh_ready_no_load` as queue hygiene only: the two
candidate paths still match the refresh, generated target artifacts are
excluded, and no fixture has been selected or approved. It is not loader
approval and must not be used to enable `render_png`.

## Development Chat Addendum: Parent Metadata Gate Integration

`aviutl-rs/examples/aex_metadata_gate_integration.rs` now integrates the
parent-visible AEX metadata boundary without opening the loader. It consumes:

- the closed fixture review gate;
- `aex_probe_readiness` top-level readiness metadata;
- the readiness loader gate;
- the synthetic image fixture manifest;
- the fixture identity smoke report;
- `analysis/OFX_AEX_FACADE_CONTRACT_2026-05-31.json`.

The report schema is
`analysis/AEX_METADATA_GATE_INTEGRATION_REPORT_SCHEMA_2026-06-01.json`, and the
contract tests live in
`aviutl-rs/tests/aex_metadata_gate_integration_contract.rs`.

`aex_metadata_gate_ready_no_load` means the parent can consume this evidence as
metadata only: the fixture gate is closed/unselected, readiness is describe-only
planning, the loader gate is not opened, synthetic image fixtures are no-load,
identity smoke is broker identity transport only, and OFX remains
`deferred-contract-only`.

`blocked_aex_metadata_gate` is emitted if the inputs drift toward fixture
approval, loader enablement, real `.aex` load, render claims, AEX render
correctness, AE invocation, private payload copy, or OFX routing.

This integration report intentionally omits plugin paths and fixture paths from
its output. It is not loader approval, fixture approval, AEX describe evidence,
AEX render correctness evidence, or OFX readiness approval.

## Development Chat Addendum: OFX Readiness Consumes Metadata Gate

`aviutl-rs/examples/ofx_aex_facade_readiness.rs` now accepts the parent metadata
gate report through `--aex-metadata-gate-report`. The generated OFX readiness
report publishes a sanitized `aex_metadata_gate_summary` with only status and
boolean no-load facts.

The accepted upstream status is still only `aex_metadata_gate_ready_no_load`.
Even when that report is accepted, OFX stays `deferred_contract_only`:
`ofx_host_may_load_aex=false`, `ofx_adapter_may_load_aex=false`,
`broker_may_load_aex=false`, and
`aviutlas_may_route_through_ofx_to_reach_aex=false`.

The OFX readiness planner blocks as `blocked_contract_mismatch` if the metadata
gate report claims fixture approval, loader opening, native `.aex` load, AEX
render correctness, AE invocation, private payload copy, or OFX routing. Private
paths, `.aex` filenames, payload markers, hashes, and loader symbols are
treated as contamination and are not echoed.

This connection makes the AEX/OFX handoff easier for the parent to audit. It is
still not OFX readiness approval, not a broker route, and not permission to load
or render a real AEX plug-in.

## Development Chat Addendum: OFX Readiness Report Output Boundary

The OFX readiness CLI output is now create-new only. `--out` must point to a
`.json` file under `target/aex-ofx-facade-readiness`; traversal components,
outside target paths, and non-JSON report names are rejected before writing.

Immediately before writing, the canonical parent directory must still resolve
under `target/aex-ofx-facade-readiness`, and the file is opened with
`create_new`. Existing reports are preserved instead of overwritten.

This hardening covers report publication hygiene only. It does not enable OFX
SDK use, OFX host startup, broker calls, worker describe/render, or native
`.aex` loading.

## Development Chat Addendum: Provenance Audit Report Output Boundary

`aviutl-rs/examples/aex_no_load_provenance_audit.rs` now uses the same
create-new report discipline. `--out` must point to a `.json` file under
`target/aex-no-load-provenance-audit`; traversal components, outside target
paths, and non-JSON report names are rejected before writing.

Immediately before writing, the canonical parent directory must still resolve
under `target/aex-no-load-provenance-audit`, and the file is opened with
`create_new`. Existing provenance reports are preserved instead of overwritten.

This is publication hygiene for the no-load audit chain only. It is not loader
approval, not worker startup, not OFX approval, and not permission to load,
describe, or render a real AEX plug-in.

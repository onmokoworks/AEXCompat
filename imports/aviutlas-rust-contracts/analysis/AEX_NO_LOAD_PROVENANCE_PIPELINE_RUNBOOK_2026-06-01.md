# AEX No-Load Provenance Pipeline Runbook

Publication status: local-only design artifact.

This runbook describes the current metadata-only AEX provenance chain from
readiness evidence to OFX facade readiness. It is not loader approval. It does not open, hash, copy, load, execute, describe, or render `.aex` binaries.

## Inputs

- `analysis/AEX_FIXTURE_REVIEW_GATE_2026-05-31.json`
- `analysis/AEX_HOST_VOCABULARY_BOUNDARY_SCHEMA_2026-06-01.json`
- `analysis/OFX_AEX_FACADE_CONTRACT_2026-05-31.json`
- generated `target/aex-fixture-gate-refresh-audit/fixture-gate-refresh.local.json`
- generated `target/aex-probe-readiness-*/readiness.local.json`
- generated `target/aex-probe-readiness-*/loader-preflight.local.json`
- generated `target/aex-probe-readiness-*/loader-gate.local.json`
- generated `target/aex-probe-readiness-*/capabilities/*.capability.json`
- generated `target/aex-image-probe/**/worker-loader-ticket-*.json`
- optional generated `target/aex-image-probe/fixture-identity-smoke/smoke.local.json`

All paths are local-only. Do not publish private absolute paths, binary
payloads, hashes, images, or plug-in contents.

## Optional Pre-Step: Fixture Gate Refresh Audit

```powershell
cargo run --example aex_fixture_gate_refresh_audit --no-default-features -- `
  --fixture-gate ..\analysis\AEX_FIXTURE_REVIEW_GATE_2026-05-31.json `
  --wiztree-refresh ..\analysis\AEX_WIZTREE_AEX_REFRESH_2026-06-01.json `
  --out target\aex-fixture-gate-refresh-audit\fixture-gate-refresh.local.json
```

Expected status:

- `fixture_gate_refresh_ready_no_load`;
- `native_load_performed=false`;
- `render_performed=false`;
- `fixture_selected=false`;
- `loader_enabled=false`;
- `fixture_gate_summary.approval_all_false=true`;
- `wiztree_refresh_summary.canonical_non_generated_count=40`;
- `wiztree_refresh_summary.generated_target_artifact_count=79`;
- `wiztree_refresh_summary.do_not_expand_from_generated_targets=true`.

This audit keeps generated target `.aex` files out of first-loader fixture
review. It is not fixture selection and is not loader approval.

The loader preflight can carry this audit as optional queue-hygiene evidence:

```powershell
cargo run --example aex_loader_preflight --no-default-features -- `
  --fixture-gate ..\analysis\AEX_FIXTURE_REVIEW_GATE_2026-05-31.json `
  --loader-gate target\aex-probe-readiness-pe-source-pipl-gated\loader-gate.local.json `
  --fixture-refresh-audit target\aex-fixture-gate-refresh-audit\fixture-gate-refresh.local.json `
  --out target\aex-loader-preflight\preflight.local.json
```

The current unselected gate may still report `blocked_no_selected_fixture`, but
when the audit is supplied the preflight should include:

- `fixture_refresh_audit_summary.provided=true`;
- `fixture_refresh_audit_summary.status=fixture_gate_refresh_ready_no_load`;
- `fixture_refresh_audit_summary.native_load_performed=false`;
- `fixture_refresh_audit_summary.render_performed=false`;
- `fixture_refresh_audit_summary.fixture_selected=false`;
- `fixture_refresh_audit_summary.loader_enabled=false`;
- `fixture_refresh_audit_summary.wiztree_canonical_non_generated_count=40`;
- `fixture_refresh_audit_summary.wiztree_generated_target_artifact_count=79`;
- `fixture_refresh_audit_summary.candidates_present_in_refresh=true`;
- `fixture_gate_refresh_audit_ready_no_load` passed.

## Step 1: Loader Implementation Manifest

```powershell
cargo run --example aex_loader_implementation_manifest --no-default-features -- `
  --preflight target\aex-probe-readiness-pe-source-pipl-gated\loader-preflight.local.json `
  --capability target\aex-probe-readiness-pe-source-pipl-gated\capabilities\ONMK-AdaptiveFilter.capability.json `
  --readiness target\aex-probe-readiness-pe-source-pipl-gated\readiness.local.json `
  --out target\aex-loader-implementation\loader-implementation.local.json
```

Expected status:

- `ready_for_separate_loader_implementation_review_no_load` only when all
  manifest checks pass;
- `native_load_performed=false`;
- `broker_may_load_plugin=false`;
- `loader_may_load_plugin=false`;
- `ofx_may_route_to_loader=false`;
- when supplied through the preflight,
  `preflight_summary.fixture_refresh_audit_summary.provided=true`;
- when supplied through the preflight,
  `preflight_summary.fixture_refresh_audit_summary.status=fixture_gate_refresh_ready_no_load`;
- when supplied through the preflight,
  `preflight_summary.fixture_refresh_audit_summary.native_load_performed=false`;
- when supplied through the preflight,
  `preflight_summary.fixture_refresh_audit_summary.render_performed=false`;
- when supplied through the preflight,
  `preflight_summary.fixture_refresh_audit_summary.fixture_selected=false`;
- when supplied through the preflight,
  `preflight_summary.fixture_refresh_audit_summary.loader_enabled=false`;
- when supplied through the preflight,
  `preflight_summary.fixture_refresh_audit_summary.wiztree_canonical_non_generated_count=40`;
- when supplied through the preflight,
  `preflight_summary.fixture_refresh_audit_summary.wiztree_generated_target_artifact_count=79`;
- when supplied through the preflight,
  `fixture_gate_refresh_audit_ready_no_load` remains passed in the manifest;
- `readiness_summary.pipl_content_scan_status=semantic_matches` when readiness
  evidence is provided.

## Step 2: Native Stage Plan

```powershell
cargo run --example aex_native_stage_plan --no-default-features -- `
  --manifest target\aex-loader-implementation\loader-implementation.local.json `
  --ticket target\aex-image-probe\...\worker-loader-ticket-*.json `
  --host-boundary ..\analysis\AEX_HOST_VOCABULARY_BOUNDARY_SCHEMA_2026-06-01.json `
  --out target\aex-native-stage-plan\native-stage-plan.local.json
```

Expected status:

- `planned_native_stage_contract_no_load`;
- `native_load_performed=false`;
- `selectors_executed=false`;
- `render_performed=false`;
- `worker_may_load_plugin=false`;
- `ofx_may_route_to_loader=false`;
- `promotion_gate.ofx_facade_may_route_to_loader=false`;
- `ticket_runtime_evidence_summary.worker_identity_revalidation_required=passed`;
- `ticket_runtime_evidence_summary.worker_attestation_required=passed`;
- `ticket_runtime_evidence_summary.sandbox_preflight_required=passed`;
- `ticket_runtime_evidence_summary.job_object_required=assigned-with-kill-on-close`;
- `ticket_runtime_evidence_summary.handle_inheritance_required=sentinel_not_inherited-with-explicit-handle-list`;
- `cleanroom_boundary_summary.native_loader_calls_allowed=false` when the host
  boundary schema is supplied.
- when supplied through the manifest,
  `manifest_fixture_refresh_audit_summary.provided=true`;
- when supplied through the manifest,
  `manifest_fixture_refresh_audit_summary.status=fixture_gate_refresh_ready_no_load`;
- when supplied through the manifest,
  `manifest_fixture_refresh_audit_summary.native_load_performed=false`;
- when supplied through the manifest,
  `manifest_fixture_refresh_audit_summary.render_performed=false`;
- when supplied through the manifest,
  `manifest_fixture_refresh_audit_summary.fixture_selected=false`;
- when supplied through the manifest,
  `manifest_fixture_refresh_audit_summary.loader_enabled=false`;
- when supplied through the manifest,
  `manifest_fixture_refresh_audit_summary.wiztree_canonical_non_generated_count=40`;
- when supplied through the manifest,
  `manifest_fixture_refresh_audit_summary.wiztree_generated_target_artifact_count=79`;
- when supplied through the manifest,
  `manifest_fixture_refresh_audit_ready_no_load` passed.

The PF selector names in this artifact are planning labels only. They are not
ABI definitions and they are not executed.

## Step 3: OFX Facade Readiness

```powershell
cargo run --example ofx_aex_facade_readiness --no-default-features -- `
  --contract ..\analysis\OFX_AEX_FACADE_CONTRACT_2026-05-31.json `
  --fixture-gate ..\analysis\AEX_FIXTURE_REVIEW_GATE_2026-05-31.json `
  --loader-gate target\aex-probe-readiness-pe-source-pipl-gated\loader-gate.local.json `
  --native-stage-plan target\aex-native-stage-plan\native-stage-plan.local.json `
  --capability-dir target\aex-probe-readiness-pe-source-pipl-gated\capabilities `
  --out target\aex-ofx-facade-readiness\ofx-facade.local.json
```

Expected status:

- `deferred_contract_only` while OFX remains closed;
- `ofx_host_may_load_aex=false`;
- `ofx_adapter_may_load_aex=false`;
- `broker_may_load_aex=false`;
- `aviutlas_may_route_through_ofx_to_reach_aex=false`;
- `ofx_facade_review_gate.approved=false`;
- `ofx_facade_review_gate.may_point_to_broker=false`;
- `ofx_facade_review_gate.may_issue_describe=false`;
- `ofx_facade_review_gate.may_issue_render_png=false`;
- `native_stage_plan_summary.provided=true`;
- `native_stage_plan_summary.no_load_stage_plan_ready=true`;
- `native_stage_plan_summary.worker_runtime_evidence_ready=true`;
- `native_stage_plan_summary.selector_execution_blocked=true`;
- `native_stage_plan_summary.render_blocked=true`;
- `native_stage_plan_summary.ofx_route_blocked=true`.

Even when all expected values are present, OFX is still only a deferred facade
readiness report. It must not become the first AEX loader, must not route AviUtlas through OFX to reach AEX, and must not issue describe or render requests before a separate OFX review gate is explicitly approved.

## Step 4: No-Load Provenance Audit

```powershell
cargo run --example aex_no_load_provenance_audit --no-default-features -- `
  --loader-manifest target\aex-loader-implementation\loader-implementation.local.json `
  --native-stage-plan target\aex-native-stage-plan\native-stage-plan.local.json `
  --ofx-readiness target\aex-ofx-facade-readiness\ofx-facade.local.json `
  --fixture-identity-smoke target\aex-image-probe\fixture-identity-smoke\smoke.local.json `
  --out target\aex-no-load-provenance-audit\provenance-audit.local.json
```

The audit report output is create-new only. `--out` must be a `.json` path
under `target\aex-no-load-provenance-audit`, must not contain traversal
components, and must pass canonical parent containment immediately before the
file is created. Existing audit reports are preserved.

Expected status:

- `no_load_provenance_chain_ready` only when the loader manifest, native stage
  plan, and OFX readiness report all preserve their no-load gates;
- `native_load_performed=false`;
- `selectors_executed=false`;
- `render_performed=false`;
- `ofx_route_allowed=false`;
- `evidence_contains_forbidden_tokens=false`;
- `loader_manifest_summary.readiness_provided=true`;
- `native_stage_plan_summary.cleanroom_boundary_no_loader_or_sdk=true`;
- `ofx_readiness_summary.native_stage_plan_summary_provided=true`;
- `ofx_readiness_summary.native_stage_ofx_route_blocked=true`.
- when supplied through the chain,
  `loader_manifest_summary.fixture_refresh_audit_provided=true`;
- when supplied through the chain,
  `native_stage_plan_summary.fixture_refresh_audit_provided=true`;
- when supplied through the chain,
  `loader_manifest_summary.fixture_refresh_audit_status=fixture_gate_refresh_ready_no_load`;
- when supplied through the chain,
  `native_stage_plan_summary.fixture_refresh_audit_status=fixture_gate_refresh_ready_no_load`;
- when supplied through the chain,
  `fixture_refresh_audit_preserved_no_load` passed.
- when supplied as optional evidence,
  `fixture_identity_smoke_summary.provided=true`;
- when supplied as optional evidence,
  `fixture_identity_smoke_summary.status=fixture_identity_smoke_ready_no_load`;
- when supplied as optional evidence,
  `fixture_identity_smoke_summary.transport_operation=identity_transport`;
- when supplied as optional evidence,
  `fixture_identity_smoke_summary.broker_invoked=true`;
- when supplied as optional evidence,
  `fixture_identity_smoke_summary.aex_render_correctness_evidence=false`;
- when supplied as optional evidence,
  `fixture_identity_smoke_summary.expected_synthetic_image_set=true`;
- when supplied as optional evidence,
  `fixture_identity_smoke_summary.all_entries_identity_pixels_match=true`;
- when supplied as optional evidence,
  `fixture_identity_smoke_summary.input_contains_forbidden_tokens=false`;
- when supplied as optional evidence,
  `fixture_identity_smoke_summary.sanitized_summary_contains_forbidden_tokens=false`;
- when supplied as optional evidence,
  `fixture_identity_smoke_ready_no_load` passed.

This audit is a join check only. It reads generated JSON reports and does not
approve a loader, create an OFX adapter, open `.aex`, call selectors, ask a
worker to describe/render, or validate pixels directly. The optional fixture
identity smoke report is reduced to sanitized counters and booleans; PNG path
fields from that report are not propagated.

## Step 5: Loader Slice Review Packet

```powershell
cargo run --example aex_loader_slice_review_packet --no-default-features -- `
  --manifest target\aex-loader-implementation\loader-implementation.local.json `
  --provenance-audit target\aex-no-load-provenance-audit\provenance-audit.local.json `
  --fixture-gate ..\analysis\AEX_FIXTURE_REVIEW_GATE_2026-05-31.json `
  --out target\aex-loader-slice-review\loader-slice-review.local.json
```

Expected status:

- `ready_for_manual_loader_slice_review_no_load` only when the fixture gate,
  loader implementation manifest, and no-load provenance audit are all ready;
- the current checked-in fixture gate is `review_queue_not_approved`, so this
  packet should remain `blocked_loader_slice_review_packet` until explicit
  manual fixture approval is recorded in a local-only gate;
- `native_load_performed=false`;
- `loader_slice_approved=false`;
- `loader_enabled=false`;
- `real_aex_load_enabled=false`;
- `native_loader_calls_allowed=false`;
- `broker_may_load_aex=false`;
- `worker_may_load_plugin=false`;
- `render_performed=false`;
- `ofx_route_allowed=false`;
- `fixture_gate_summary.selected_fixture_present=true` for a ready packet;
- `fixture_gate_summary.approval_approved=true` for a ready packet;
- `fixture_gate_summary.approval_loader_enabled=true` for a ready packet;
- `fixture_gate_summary.approval_real_aex_load_enabled=true` for a ready
  packet;
- `fixture_gate_summary.selected_candidate_review_status=approved-local-only`
  for a ready packet;
- `fixture_gate_summary.generated_target_candidate_count=0`;
- `fixture_gate_summary.input_contains_forbidden_tokens=false`;
- `manifest_summary.selected_plugin_path_redacted=true`;
- `manifest_summary.native_loader_calls_allowed=false`;
- `manifest_summary.broker_may_load_aex=false`;
- `provenance_summary.status=no_load_provenance_chain_ready`;
- `provenance_summary.ofx_readiness_ready=true`;
- `provenance_summary.runtime_and_cleanroom_ready=true`;
- when supplied through provenance,
  `provenance_summary.fixture_identity_smoke_ready=true`;
- when supplied through provenance,
  `fixture_identity_smoke_preserved_no_load` passed;
- `fixture_gate_manual_approval_ready_no_load` passed only after explicit
  fixture approval is recorded;
- `review_requirements.explicit_user_approval_required=true`;
- `review_requirements.code_review_required=true`;
- `review_requirements.local_build_classic_effect_fixture_required=true`;
- `review_requirements.cleanroom_boundary_required=true`;
- `review_requirements.license_review_required=true`;
- `review_requirements.worker_isolation_evidence_required=true`;
- `review_requirements.ofx_facade_review_deferred=true`;
- `review_requirements.generated_target_fixtures_forbidden=true`.

This packet is sanitized handoff evidence only. It does not serialize private
plugin paths from the loader manifest or fixture gate, does not approve a
loader, and does not enable worker, broker, render, or OFX execution.
It does not approve a loader.

## Step 6: Loader Approval Receipt Validation

Optional draft template:

```powershell
cargo run --example aex_loader_approval_receipt --no-default-features -- `
  --draft-template `
  --packet target\aex-loader-slice-review\loader-slice-review.local.json `
  --out target\aex-loader-approval\approval-receipt-template.local.json
```

The template command requires a ready, sanitized loader-slice review packet. It
does not work with the current checked-in blocked packet. The template is
`draft_unapproved_template`, sets `approval_status=draft_unapproved`, leaves
receipt id, approver, and timestamp null, keeps all runtime effects false, and
must validate as `invalid_receipt` until a human fills the required review
acknowledgements and approval effect.

```powershell
cargo run --example aex_loader_approval_receipt --no-default-features -- `
  --packet target\aex-loader-slice-review\loader-slice-review.local.json `
  --receipt path\to\aex-loader-approval-receipt.local.json `
  --out target\aex-loader-approval\approval-validation.local.json
```

Expected status:

- `approved_for_loader_implementation_review_no_load` only when an explicit
  operator-filled receipt binds to the exact sanitized loader-slice review
  packet by `fnv1a64-v1-noncryptographic` checksum;
- a tool-generated template is not approval and must remain invalid before
  human fill-in;
- current checked-in data should not produce an accepted receipt while Step 5
  remains `blocked_loader_slice_review_packet`;
- `approval_accepted=true` means permission to open a separate loader
  implementation review slice only;
- `loader_review_approved=true` for an accepted receipt;
- `native_load_performed=false`;
- `loader_enabled=false`;
- `real_aex_load_enabled=false`;
- `native_loader_calls_allowed=false`;
- `worker_may_load_plugin=false`;
- `broker_may_load_aex=false`;
- `render_performed=false`;
- `ofx_route_allowed=false`;
- `receipt_allows_separate_loader_implementation_review=true`;
- `receipt_allows_native_aex_load=false`;
- `receipt_allows_worker_plugin_load=false`;
- `receipt_allows_render_png=false`;
- `receipt_allows_ofx_route=false`;
- receipt `approval_effect.allow_native_aex_load=true`,
  `allow_worker_plugin_load=true`, `allow_render_png=true`, or
  `allow_ofx_route=true` must fail closed;
- private plug-in paths, `.aex` filenames, `EffectMain`, `AEEffect`,
  `worker_exe`, `input_png`, `output_png`, payload, hash, or rendered-pixel
  fields in the receipt must fail closed without echoing those values.

This validation report is not the loader implementation and is not a runtime
gate opener. It only proves that a human approval receipt was scoped to the
already-sanitized review packet and still kept native loading, worker plug-in
loading, render, and OFX routing disabled.

## Verification Commands

```powershell
cargo test --test aex_native_stage_plan_contract --no-default-features
cargo test --test ofx_aex_facade_readiness_contract --no-default-features
cargo test --test aex_no_load_provenance_audit_contract --no-default-features
cargo test --test aex_loader_slice_review_packet_contract --no-default-features
cargo test --test aex_loader_approval_receipt_contract --no-default-features
cargo test --test aex_fixture_gate_refresh_audit_contract --no-default-features
cargo test --test aex_loader_preflight_contract --no-default-features
cargo test --test aex_probe_contracts --no-default-features
cargo test --test aex_host_vocabulary_boundary --no-default-features
git diff --check
```

## Forbidden Evidence

The reports must not serialize binary payloads, private image outputs, hashes,
or loader-call evidence. If any input contains fields or values such as
`sha256`, `base64`, `output_png`, `input_png`, `rendered_pixels`, native dynamic
loader calls, or callable AEX entrypoint symbols, the relevant report should
fail closed and avoid echoing the forbidden evidence into downstream reports.

The optional `--fixture-identity-smoke` input is the narrow exception for field
names: that source report is expected to contain per-entry `input_png` and
`output_png` synthetic path fields. The provenance audit may read those fields
only to summarize the smoke report, must scan the rest of the smoke input for
forbidden evidence, and must never serialize those PNG path fields downstream.

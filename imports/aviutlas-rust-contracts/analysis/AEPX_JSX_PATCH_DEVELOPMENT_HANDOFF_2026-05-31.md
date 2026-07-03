# AEPX / JSX Patch Development Handoff - 2026-05-31

Purpose: hand a contract-first AE project editing slice to the separate
AviUtlas development chat.

This support thread owns safety, schemas, and orchestration. The development
chat should own implementation unless the user redirects this code slice back
to the support thread.

## Goal

Create the first safe external project-editing probe for After Effects projects.

The first slice proves request validation, output-path safety, generated JSX
boundary checks, and synthetic `.aepx` preservation behavior. It does not write
binary `.aep` files directly and does not run After Effects in automated tests.

Status note: the first JSX transaction contract probe now exists:

- `aviutl-rs/examples/ae_jsx_transaction.rs`
- `aviutl-rs/tests/ae_jsx_transaction_contract.rs`
- `aviutl-rs/tests/fixtures/ae_jsx_transaction.generate.json`

It validates request/report contracts and can generate a JSX file with
create-new semantics. It does not launch AE or write `.aep` / `.aepx` project
files.

Status note: the first AEPX patch contract probe now also exists:

- `aviutl-rs/examples/aepx_patch_probe.rs`
- `aviutl-rs/tests/aepx_patch_contract.rs`
- `aviutl-rs/tests/fixtures/aepx_patch_request.dry_run.json`

It validates strict request/report boundaries for `inspect_metadata`,
`noop_validate`, `dry_run`, and future `apply`. Current `apply` is intentionally
`unsupported_operation` after validation and creates no `.aepx` output. The
probe does not parse XML payloads, does not rewrite projects, and does not
launch After Effects.

Status note: the synthetic AEPX preservation proof artifact now exists:

- `aviutl-rs/examples/aepx_synthetic_preservation_proof.rs`
- `aviutl-rs/tests/aepx_synthetic_preservation_proof_contract.rs`
- `analysis/AEPX_SYNTHETIC_PRESERVATION_PROOF_SCHEMA_2026-06-01.json`

It is separate from `aepx_patch_probe apply`. It accepts only checked-in
synthetic fixtures, supports a narrow exact `xml_id` / old-value guarded
composition rename, writes only a create-new synthetic target, and reports
preservation gates without echoing XML bodies, fixture paths, selectors,
expected values, new values, or sentinels.

Status note: the first AEPX writer strategy decision memo now exists:

- `analysis/AEPX_WRITER_STRATEGY_DECISION_2026-06-01.md`

It keeps production `aepx_patch_probe apply` disabled and recommends hardening
the exact-span synthetic proof before choosing any XML dependency or accepting
real `.aepx` input.

## Input Specs

Use:

- `analysis/AEPX_JSX_PATCH_TOOL_SPEC_2026-05-31.md`
- `analysis/AEPX_PATCH_REQUEST_SCHEMA_2026-05-31.json`
- `analysis/JSX_TRANSACTION_REQUEST_SCHEMA_2026-05-31.json`
- `analysis/AEX_DIRECT_HOST_AND_AEP_EDIT_STRATEGY_2026-05-31.md`
- `analysis/AE_SNAPSHOT_SCHEMA_2026-05-31.md`
- `analysis/CURRENT_RISK_REGISTER_2026-05-31.md`

## Allowed Write Set

Preferred new files only:

- `aviutl-rs/examples/aepx_patch_probe.rs`
- `aviutl-rs/examples/ae_jsx_transaction.rs`
- `aviutl-rs/tests/aepx_patch_contract.rs`
- `aviutl-rs/tests/ae_jsx_transaction_contract.rs`
- `aviutl-rs/tests/fixtures/aepx_patch_request.dry_run.json`
- `aviutl-rs/tests/fixtures/ae_jsx_transaction.generate.json`
- synthetic fixtures under `aviutl-rs/tests/fixtures/`

Do not edit without explicit approval:

- `aviutl-rs/src/project/exo.rs`
- `aviutl-rs/src/project/script_lua.rs`
- `aviutl-rs/src/project/exo_keymap.rs`
- `aviutl-rs/src/app.rs`
- `aviutl-rs/src/compat/mod.rs`
- `aviutl-rs/src/plugin/bridge.rs`
- `aviutl-rs/src/plugin/types.rs`
- `aviutl-rs/scripts/gate.ps1`

## Existing Dependencies

`aviutl-rs` already has:

- `serde`
- `serde_json`
- `anyhow`
- `thiserror`

The first JSX transaction slice should need no new dependency.

The `.aepx` patcher may eventually need a preservation-aware XML library. Do
not add one until its license and whitespace/comment/CDATA behavior are checked.
If no XML dependency is selected yet, keep the first `.aepx` implementation to
request validation plus dry-run/noop reports.

Use `--no-default-features` for focused checks.

## First Implementation Slice

### AEPX Probe

Minimum behavior:

1. Parse request JSON.
2. Validate `schema_version == 1`.
3. Validate operation is one of:
   - `inspect_metadata`
   - `noop_validate`
   - `dry_run`
   - `apply`
4. Reject `apply` without `output_aepx`.
5. Reject source/output equality after path normalization.
6. Reject existing `output_aepx`.
7. Validate operation names and selector fields.
8. Emit a structured report.
9. Do not read or quote local real `.aepx` payloads in tests.

If XML parsing is not implemented in the first slice, return a clear
`unsupported_operation` or `parse_error` only for modes that need XML content.

### JSX Transaction Probe

Minimum behavior:

1. Parse request JSON.
2. Validate `schema_version == 1`.
3. Validate source/output/generated JSX path rules.
4. Reject overwrite and source/output equality.
5. Reject pairwise equality among source project, output project, and generated
   JSX paths.
6. Reject embedded arbitrary JavaScript.
7. Generate JSX containing only allowlisted interpreter code and embedded JSON
   patch data.
8. Scan generated JSX for forbidden API strings.
9. Emit a structured report with `dry_run_ok`, `generated_jsx`,
   `ae_not_run`, `ae_run_ok`, `output_same_as_source`, or
   `forbidden_jsx_api` as applicable.
10. Do not launch After Effects in automated tests.

Forbidden strings to scan:

- `system.callSystem`
- `File.openDialog`
- `Folder.selectDialog`
- `app.executeCommand`
- `BridgeTalk`
- `aerender`
- `afterfx`
- `cmd.exe`
- `powershell`
- `wscript`
- `eval`
- `Function`
- `$.evalFile`
- `File.execute`
- `ExternalObject`
- `Socket`
- `File.open`
- `File.write`
- `File.remove`
- `File.rename`
- `File.copy`
- `alert(`
- bare `app.project.save()`

The scan is not a complete sandbox, but it is a useful contract regression
guard.

## Recommended Commands

```powershell
cargo check --example aepx_patch_probe --no-default-features
cargo check --example ae_jsx_transaction --no-default-features
cargo test --test aepx_patch_contract --no-default-features
cargo test --test ae_jsx_transaction_contract --no-default-features
```

## Contract Tests

AEPX tests:

- dry-run request parses;
- unsupported schema version is `invalid_request`;
- apply without output path is `invalid_request`;
- source/output same path is `invalid_request`;
- existing output path is `output_exists`;
- ambiguous selector is rejected;
- synthetic unknown XML preservation fixture is not normalized if XML patching
  is implemented.
- current v0 fail-closed sentinel: a synthetic unknown XML fixture returns
  `dry_run_ok` / `unsupported_operation` with every preservation field
  `not_written`, does not echo fixture body sentinels, and writes no output.

JSX tests:

- generate request parses;
- output path is required;
- overwrite is rejected;
- same source/output path reports `output_same_as_source`;
- generated JSX path equals source/output path is rejected;
- arbitrary JavaScript payload fields are rejected;
- generated JSX contains no forbidden API strings;
- generated JSX uses explicit `app.project.save(new File(...))` only for the
  requested output path;
- generated JSX supports `rename_comp`, `rename_layer`, guarded
  composition/layer `set_comment`, owner-scoped `set_marker`, and guarded
  layer `replace_text_source` through a fixed interpreter, preflights all
  targets before mutation, and saves exactly once after successful preflight;
- `run_after_effects=true` produces a plan-only status and does not spawn AE;
- `report_private_payloads=true` is rejected;
- `publication_status=unknown` fails closed before writes;
- automated tests do not run AE.

## Not In This Slice

- binary `.aep` parsing or writing;
- AE runtime launcher;
- visual validation;
- project-wide effect parameter editing;
- timeline/keyframe mutation beyond marker/comment v0;
- `.ffx` parsing;
- `.aex` loading;
- Adobe SDK, OFX SDK, `after-effects`, `after-effects-sys`, or `pipl` imports;
- edits to hot AviUtlas runtime files.

## Closeout Requirements

Report back:

- files changed;
- commands/tests run;
- request/report statuses implemented;
- whether generated JSX was scanned for forbidden APIs;
- proof that source projects are never overwritten;
- whether any hot files were touched;
- remaining unsupported operations;
- next safe step.

## Implementation Status - 2026-05-31

- AEPX v0 remains fail-closed for XML writes. `aviutl-rs/tests/fixtures/aepx_preservation_sentinels.aepx`
  contains unknown elements/attributes, XML declaration, comment, CDATA,
  namespace prefix, and unusual whitespace. `aviutl-rs/tests/aepx_patch_contract.rs`
  verifies that `dry_run` does not echo fixture body sentinels and that `apply`
  still returns `unsupported_operation` without creating output. This is not a
  preservation writer proof.
- `analysis/AEPX_XML_PRESERVATION_WRITER_SPIKE_2026-06-01.md` now records a
  separate synthetic-only preservation writer spike. Its standalone test uses
  `aviutl-rs/tests/fixtures/aepx_writer_spike_preservation.aepx` and
  `aviutl-rs/tests/fixtures/aepx_writer_spike_ambiguous.aepx` to prove a narrow
  exact-id composition rename can preserve untouched unknown XML sentinels while
  writing only a create-new synthetic output. It also fails closed for ambiguous
  name selectors, old-value mismatch, and existing outputs. This is still not
  production `apply`, not an XML library choice, and not an AE compatibility
  claim.
- `aviutl-rs/examples/aepx_synthetic_preservation_proof.rs` now promotes that
  spike into a separate local proof artifact with
  `analysis/AEPX_SYNTHETIC_PRESERVATION_PROOF_SCHEMA_2026-06-01.json`. The
  proof is still synthetic-only and exact-id/old-value guarded, writes only a
  create-new generated target, and keeps production AEPX apply disabled.
- `analysis/AEPX_WRITER_STRATEGY_DECISION_2026-06-01.md` records the current
  strategy decision: do not add a production writer or XML dependency yet;
  harden exact-span synthetic proof with byte-diff, path, scanner, replacement
  value, all-or-nothing, report/privacy, license, and manual-AE gates first.
- JSX transaction generation now supports `rename_comp`, `rename_layer`,
  composition/layer `set_comment`, owner-scoped composition/layer
  `set_marker`, and guarded layer `replace_text_source` in
  `aviutl-rs/examples/ae_jsx_transaction.rs`. The generated fixed interpreter
  preflights all requested operations before applying any mutation, rejects
  missing or ambiguous comp/layer targets, requires `expected_old_value` guards
  for mutating operations, and saves once to
  `app.project.save(new File(outputPath))`.
- Automated tests still do not launch After Effects. Real `.aep` mutation
  remains manual smoke only until reviewed local execution is explicitly chosen.

## Addendum: Neutral AE Project Edit IR

The AEP/AEPX route now has a neutral edit-intent contract before widening
either `.aepx` writing or JSX execution:

- `analysis/AE_PROJECT_EDIT_IR_2026-06-01.md`
- `analysis/AE_PROJECT_EDIT_IR_SCHEMA_2026-06-01.json`
- `analysis/AE_PROJECT_EDIT_OPERATOR_RUNBOOK_2026-06-01.md`
- `aviutl-rs/tests/fixtures/ae_roundtrip_expectations.synthetic.json`
- `aviutl-rs/tests/ae_roundtrip_ir_contract.rs`

The IR reserves the shared operation vocabulary
`rename_comp`, `rename_layer`, `set_comment`, `set_marker`,
`replace_text_source`, and `relink_asset_path`, while keeping tool modes such
as `dry_run`, `apply`, `validate`, and `generate_jsx_transaction` out of
`operations[].kind`.

This is not an editor yet. It adds machine-checkable selector, path, privacy,
publication, and runtime boundaries: real request paths must be explicit and
absolute, source/output/generated JSX paths must be pairwise distinct after
Windows-style normalization, synthetic fixtures are the only automated corpus,
`report_private_payloads=true` remains invalid, automated AE launch remains
disabled, and binary `.aep` parsing/writing remains deferred.

The first IR-to-JSX planning adapter now exists:

- `aviutl-rs/examples/ae_project_edit_ir.rs`
- `aviutl-rs/tests/ae_project_edit_ir_adapter_contract.rs`
- `aviutl-rs/tests/fixtures/ae_project_edit_ir.supported_renames.json`

It emits an `ae_jsx_transaction` request only when every IR operation is already
supported by JSX v0 (`rename_comp` / `rename_layer` / `set_comment` /
owner-scoped `set_marker` / guarded `replace_text_source`). Route-neutral
marker targets can lower to the same JSX marker backend only when they carry
route-native owner fields such as `comp_name` and do not carry unresolved
`marker_index`, `stable_id`, or `xml_id`; marker ids/indexes still require a
future resolver before JSX emission. Unsupported operations block the whole
request rather than being partially dropped. The adapter itself does not
generate JSX, launch AE, read/write projects, parse binary `.aep`, or write
`.aepx`.

## Continuation Update - 2026-06-01

Current parent-agent slice tightened the no-write/no-run boundary without
crossing into AviUtl core, AEX, OFX, or binary `.aep` editing:

- `aepx_patch_probe` now rejects supplied `output_aepx` paths for dry-run,
  noop, and metadata modes when the path is not absolute `.aepx`, equals the
  input after Windows-style normalization, or already exists. `apply` remains
  `unsupported_operation` after validation and writes nothing.
- AEPX dry-run validation now checks marker object shape and relink
  old-path guards before reporting planned operations. Reports still omit
  selectors, expected values, new values, and XML body payloads.
- AEPX reports now include an explicit `write_gate`: patch not applied, XML
  writer not implemented, user approval required before write, output write
  not performed, and source overwrite not performed.
- AEPX reports now also include an explicit `io_gate`: XML body read/write not
  performed, XML body and private patch payloads not embedded in reports, no
  external process invoked, and After Effects not invoked. The synthetic
  preservation fixture test checks that the fixture text is unchanged after
  `dry_run` and the unsupported `apply` boundary.
- AEPX reports now include a metadata-only `aepx_patch_report_binding` with
  FNV and `sha256-v1` values. It binds status/path/metadata/gate/count fields
  and excludes elapsed time, XML bodies, selectors, expected/new values, and
  private patch payloads.
- `ae_jsx_transaction` reports now include an explicit execution gate for
  no-AE receipts: not applied, user approval required, After Effects not
  launched, and no project save performed.
- `ae_jsx_transaction` reports now include an explicit artifact gate: generated
  JSX write performed only for `generated_jsx` receipts using create-new
  semantics, output project write by Rust not performed, and source project
  overwrite not performed.
- `ae_jsx_transaction` reports also include source metadata from filesystem
  metadata lookup only: normalized source path, existence, file/type, optional
  byte length, and `project_body_read=false`.
- `ae_jsx_transaction` reports now include a metadata-only
  `jsx_transaction_report_binding` with FNV and `sha256-v1` values. It binds
  report status, paths, source metadata, operation ids/statuses, forbidden scan
  status, artifact gate, and execution gate, while excluding elapsed time,
  request bodies, generated JSX bodies, project payloads, and private patch
  payloads.
- Generated-JSX `ae_jsx_transaction` reports now also carry
  `generated_jsx_artifact_digest` with `sha256-v1`, generated byte count, and
  `generated_jsx_embedded=false`. This lets review packets bind to generated
  artifact metadata without embedding the JSX body.
- Added `ae_jsx_transaction_artifact_verify`, a JSON-only verifier for
  generated JSX transaction artifacts. It reads the generated-JSX transaction
  report plus the local generated JSX bytes, checks path, byte count, and
  `sha256-v1` digest, and emits
  `ae_jsx_transaction_artifact_verification` without launching AE, executing
  JSX, reading project payload bytes, or writing project files.
- `export_ae_jsx` now fails closed if generated JSX contains the high-risk
  forbidden token surface through user-supplied snapshot data. It can also emit
  an optional create-new report sidecar following
  `analysis/AE_JSX_EXPORT_REPORT_SCHEMA_2026-06-01.json`; the sidecar records
  generated-JSX artifact writes and a generated JSX body digest separately from
  AE/project writes, and does not embed the snapshot JSON or generated JSX body.
- `export_ae_jsx` can now emit a create-new metadata-only export manifest with
  `--manifest manifest.json`, following
  `analysis/AE_JSX_EXPORT_MANIFEST_SCHEMA_2026-06-01.json`. The manifest records
  generated JSX/report artifact pointers, no-AE/no-project-write expectations,
  and `approval_status=required_pending` without embedding snapshot or JSX
  bodies. It now carries a generated JSX artifact digest plus metadata-only FNV
  and `sha256-v1` manifest bindings over manifest fields; those bindings cover
  the generated JSX digest field without embedding the JSX body. The manifest is
  review metadata; it is not an approval receipt.
- Added `ae_jsx_export_artifact_verify`, a standalone JSON report preflight for
  export manifests and generated JSX artifacts, following
  `analysis/AE_JSX_EXPORT_ARTIFACT_VERIFY_REPORT_SCHEMA_2026-06-01.json`. It
  recalculates the export manifest binding, hashes the local generated JSX file
  bytes, compares path and byte count, and can optionally compare the export
  report sidecar before approval review. Its `verified` status is local consistency evidence only; it is not an approval receipt and not tool-side
  execution permission.
- Added `ae_jsx_export_approval`, a JSON-only approval validator for standalone
  export manifests following
  `analysis/AE_JSX_EXPORT_APPROVAL_RECEIPT_SCHEMA_2026-06-01.json`. It accepts
  only `ae_jsx_export_artifact_manifest`, rejects
  `ae_project_edit_ir_jsx_request_pair`, and can validate an operator-filled
  `ae_jsx_export_manual_approval_receipt` without launching AE, executing JSX,
  reading project payload bytes, or writing project files. It now recalculates
  the export manifest's metadata-only FNV checksum and `sha256-v1` digest before
  producing a draft template or accepting a receipt, and it now requires a
  matching `ae_jsx_export_artifact_verification` report with
  `verification_status=verified`. The receipt copies the generated JSX artifact
  digest and verified preflight status. This is an operator review record only, not tool-side execution permission.
- Added `ae_jsx_export_smoke_closeout`, the JSON-only manual smoke closeout
  validator for standalone export artifacts, following
  `analysis/AE_JSX_EXPORT_MANUAL_SMOKE_CLOSEOUT_REPORT_SCHEMA_2026-06-01.json`.
  It emits a `not_run` template and validates operator-filled completed,
  failed, or aborted closeouts against the export manifest and approval receipt.
  It copies the generated JSX artifact digest, recalculates the export manifest
  metadata binding, requires the approval receipt to carry a reviewed verified
  artifact preflight prerequisite, and keeps tool-side AE launch, JSX execution,
  project body reads, and project writes false. Its `accepted` status is consistency evidence only, not an AE/runtime oracle.
- `ae_project_edit_approval` accepts only
  `ae_project_edit_ir_jsx_request_pair` as its approval scope and now reports
  that scope explicitly. It rejects `ae_jsx_export_artifact_manifest` before
  producing a template or approving a receipt, so standalone export manifests
  cannot enter the IR request-pair approval route. Standalone export approval is
  handled by `ae_jsx_export_approval`; within the request-pair route,
  standalone export manifests remain review metadata. Approval validation now
  also requires a matching `ae_project_edit_review_packet` report with
  `validation_status=review_ready`; a receipt by itself is rejected. The
  receipt must also copy the packet report's metadata-only
  `review_packet_binding.checksum_hex` and
  `review_packet_binding.cryptographic_digest.digest_hex`.
- Added `ae_project_edit_review_packet`, a JSON-only pre-approval packet
  validator following
  `analysis/AE_PROJECT_EDIT_REVIEW_PACKET_SCHEMA_2026-06-01.json`. It checks the
  request-pair manifest, downstream `dry_run_ok` validation report, generated
  JSX report, optional not-run closeout template, and optional export artifact
  manifest. It now requires metadata-only binding/digest evidence from both JSX
  transaction reports before review can pass. It can also attach an optional
  AEPX `dry_run_ok` report as no-write evidence, requiring that report's
  metadata-only binding and `sha256-v1` digest before review can pass. It emits
  a metadata-only
  `review_packet_binding` with both FNV and `sha256-v1` values for later
  approval/closeout matching. `review_ready` is not approval and does not permit
  tool-side AE launch, JSX execution, project save, source overwrite, binary
  `.aep` write, or AEPX XML apply. Optional export artifact manifests are also
  checked against their own metadata-only checksum/digest, so stale summary
  metadata fails before review. Optional export artifact verification reports can
  also be attached with the export manifest and must match the manifest path,
  byte count, generated JSX digest, manifest binding, and report sidecar fields
  with `verification_status=verified`; this remains supplemental review metadata,
  not approval. It can now also attach an optional
  `aepx_synthetic_preservation_proof` report when the proof is local-fixture-only,
  exact-byte-diff verified, privacy-safe, create-new under
  `target/aepx-synthetic-preservation-proof`, and
  `production_apply_enabled=false`. This is preservation evidence only; it does
  not permit production AEPX apply. It can also attach optional
  `aepx_production_lane_gate_preflight` evidence when the preflight is
  `apply_gate_closed_ready`; this is gate-closed evidence only and does not
  permit production AEPX apply, real `.aepx` input, or binary `.aep` editing.
- The review packet now requires and copies the generated-JSX report's
  `generated_jsx_artifact_digest` metadata. Approval and closeout validation
  reject packets that omit that digest, use malformed digest metadata, or claim
  the generated JSX body is embedded in the report.
- Review packets can now attach optional
  `ae_jsx_transaction_artifact_verification` evidence. If present, review,
  approval, and closeout require `verification_status=verified`, path/byte/
  digest match booleans true, and
  `generated_jsx_artifact_verification_accepted_as_approval=false`; approval and
  closeout receipts must acknowledge the evidence when it is present.
- `ae_project_edit_approval` now carries optional AEPX dry-run evidence from
  the review packet into its validation report. If a review packet includes a
  checked AEPX dry-run report, approval requires
  `prerequisite_reports.aepx_dry_run_reviewed=true`, verifies that the AEPX
  binding/digest fields are present and metadata-only, and still requires
  `aepx_dry_run_accepted_as_apply=false` plus
  `allow_aepx_write_without_preservation_proof=false`. This makes AEPX dry-run
  evidence visible to approval without converting it into XML apply permission.
- `ae_project_edit_approval` also carries optional synthetic AEPX preservation
  proof evidence from the review packet. If present, approval requires
  `prerequisite_reports.aepx_preservation_proof_reviewed=true` and still proves
  `aepx_preservation_proof_accepted_as_apply=false`,
  `aepx_preservation_proof_exact_byte_diff_verified=true`, and
  `aepx_preservation_proof_production_apply_enabled=false`.
- `ae_project_edit_approval` also carries optional AEPX production-lane
  preflight evidence from the review packet. If present, approval requires
  `prerequisite_reports.aepx_production_lane_preflight_reviewed=true` and still
  proves `aepx_production_lane_preflight_accepted_as_apply=false`,
  `aepx_production_lane_preflight_production_apply_enabled=false`, and binary
  `.aep` read/write flags false.
- `ae_project_edit_approval` also carries optional standalone export manifest
  and export artifact verification evidence from the review packet into its
  validation report. If those supplemental artifacts are present, approval
  requires the matching
  `prerequisite_reports.export_artifact_manifest_reviewed=true` and
  `prerequisite_reports.export_artifact_verification_report_reviewed=true`
  acknowledgements as applicable, while still requiring
  `*_accepted_as_approval=false`.
- `ae_manual_smoke_closeout` now checks that same optional standalone export
  evidence chain during closeout validation. If the supplied review packet
  includes export manifest or artifact verification evidence, the supplied
  approval receipt must carry matching reviewed acknowledgements and the review
  packet evidence must still remain supplemental, not approval.
- `ae_manual_smoke_closeout` now also checks the optional synthetic AEPX
  preservation proof chain. If the supplied review packet includes that proof,
  the approval receipt must carry the matching reviewed acknowledgement and the
  proof must still remain non-apply supplemental evidence.
- `ae_manual_smoke_closeout` now also checks the optional AEPX production-lane
  preflight chain. If the supplied review packet includes that preflight, the
  approval receipt must carry the matching reviewed acknowledgement and the
  preflight must still remain gate-closed supplemental evidence.
- Added `aviutl-rs/tests/ae_project_edit_e2e_contract.rs`, a synthetic
  JSON-only chain test from `ae_project_edit_ir` through JSX validate/generate
  reports, `ae_project_edit_review_packet`, `ae_project_edit_approval`, and
  `ae_manual_smoke_closeout`. It also carries standalone export artifact
  verification, synthetic AEPX preservation proof, and production-lane preflight
  evidence as supplemental review evidence. It creates only generated artifacts
  under `target`; it does not launch AE, run JSX in AE, save projects, apply
  production AEPX XML, accept real `.aepx` input, or write binary `.aep`.
- CLI report outputs for AE project edit IR, AEPX patch probe, and JSX
  transaction probe now use create-new semantics and refuse existing report
  files.
- JSX `validate` and `manual_smoke_plan` still perform no writes, but supplied
  `output_project` / `generated_jsx` paths are validated before they can appear
  in an accepted report.
- Added `aepx_synthetic_preservation_proof`, a local-only synthetic proof
  artifact following
  `analysis/AEPX_SYNTHETIC_PRESERVATION_PROOF_SCHEMA_2026-06-01.json`. It reads
  only the allowed checked-in synthetic fixtures, supports only exact
  `xml_id`/old-value guarded composition rename for the ready path, creates new
  outputs only under `target/aepx-synthetic-preservation-proof`, fails closed
  for ambiguous selectors or expected-value mismatch, and does not embed XML
  bodies, fixture paths, selector values, expected values, new values, or
  sentinels in reports.
- Added `analysis/AEPX_WRITER_STRATEGY_DECISION_2026-06-01.md`. It integrates
  three read-only sidecar reviews plus current public XML crate metadata and
  chooses exact-span hardening as the next synthetic-only lane. `roxmltree`,
  `quick-xml`, `xml`, and `xmltree` remain evaluation candidates only; none is
  adopted for AEPX writing in this slice.
- The first exact-span hardening slice now adds metadata-only
  `hardening_gate` evidence to `aepx_synthetic_preservation_proof`: `.aepx`
  output extension, no traversal components, generated-root containment,
  XML-attribute-safe replacement values, quoted-tag scanner hardening, duplicate
  `xml_id` ambiguity rejection, and exact byte-diff preservation outside the
  approved span. It adds only synthetic fixtures and still keeps production
  `aepx_patch_probe apply` disabled.
- `aepx_synthetic_preservation_proof` now also validates CLI report output:
  create-new `.json` only, under `target/aepx-synthetic-preservation-proof`,
  no traversal components, and no outside/private report path creation.
- Both proof output and CLI report output now re-check the canonical parent
  directory immediately before create-new writes; the parent must resolve under
  `target/aepx-synthetic-preservation-proof`.
- An opportunistic Windows symlink-parent escape test now exercises the
  write-time canonical parent check: when symlink creation is available, an
  output parent that lexically sits under the synthetic target root but resolves
  outside is rejected as `write_failed`, creates no outside file, and does not
  echo escape paths in the report.
- An opportunistic Windows junction-parent escape test now covers the same
  boundary with a directory junction created by the test harness. The synthetic
  proof report still records `external_process_invoked == false`; only the
  test setup may call `mklink /J`, and it skips if unavailable.
- The synthetic proof corpus now includes a non-ASCII UTF-8 fixture and verifies
  exact byte preservation outside the approved replacement span while keeping
  fixture names, selectors, old/new values, and Japanese text out of reports.
- The corpus now also includes a UTF-8 BOM + CRLF synthetic fixture. The proof
  verifies that the BOM and CRLF line endings survive the span replacement while
  report output still omits fixture names, sentinels, selectors, and edit
  values.
- The synthetic proof now supports a bounded `operations` array for
  multi-operation all-or-nothing proof. It resolves every exact `xml_id`
  `rename_comp` span before writing, rejects any failing guard or overlapping
  span, applies replacements in reverse span order, and reports only counts and
  booleans.
- Added the production-lane gate artifacts
  `analysis/AEPX_PRODUCTION_LANE_GATE_2026-06-01.md` and
  `analysis/AEPX_PRODUCTION_LANE_GATE_SCHEMA_2026-06-01.json`. They keep
  `aepx_patch_probe apply`, real `.aepx` input, production XML writing, source
  overwrite, AE launch, and external writer process invocation closed until a
  separate parent-approved slice has green production path-boundary,
  exact-byte-diff, all-or-nothing, report-privacy, license, real-fixture
  review, and approval evidence.
- Added `aepx_production_lane_gate_preflight`, a metadata-only checker with
  report schema
  `analysis/AEPX_PRODUCTION_LANE_GATE_PREFLIGHT_REPORT_SCHEMA_2026-06-01.json`.
  It reads only the production-lane gate schema, returns
  `apply_gate_closed_ready` while the gate remains closed and complete, and
  returns `blocked_gate_drift` if the gate drifts toward apply, real `.aepx`
  input, XML writing, source overwrite, AE launch, external writer processes,
  AEX loading, or OFX routing. Its report now carries explicit binary `.aep`
  no-read/no-write fields so a ready preflight still proves
  `binary_aep_writer_enabled=false`, `binary_aep_read_performed=false`, and
  `binary_aep_write_performed=false`.

Still not implemented: AEPX XML writing, binary `.aep` parsing/writing, AE
runtime launching, project save verification, resolver-backed stable-id marker
editing, relink application in JSX, `.ffx` parsing, and `.aex` loading.

# AEPX / JSX Patch Tool Spec - 2026-05-31

Purpose: define the first safe external project-editing route for After Effects
projects without writing binary `.aep` files directly.

This spec complements:

- `analysis/AEX_DIRECT_HOST_AND_AEP_EDIT_STRATEGY_2026-05-31.md`
- `analysis/AE_SNAPSHOT_SCHEMA_2026-05-31.md`
- `analysis/AEPX_PATCH_REQUEST_SCHEMA_2026-05-31.json`
- `analysis/JSX_TRANSACTION_REQUEST_SCHEMA_2026-05-31.json`
- `analysis/AE_PROJECT_EDIT_OPERATOR_RUNBOOK_2026-06-01.md`

## Direction

There are two v0 tracks:

1. `.aepx` structural patching for XML project files.
2. Generated JSX transactions for binary `.aep` projects, letting After
   Effects open, mutate, validate, and save the project itself.

Both tracks use an allowlisted patch vocabulary and write only to explicit new
output paths. In the schemas, top-level `operation` means the tool mode, while
`operations[].kind` means the individual edit operation.

## Non-Negotiable Boundary

- Never overwrite the source project in v0.
- Never infer an output path by changing the extension in-place.
- Never embed arbitrary JavaScript in the patch request.
- Never copy private `.aep`, `.aepx`, `.ffx`, `.aex`, image, audio, or video
  payloads into public fixtures.
- Never log full XML payloads, layer text corpora, private media lists, or
  binary project contents.
- Treat the single local `.aepx` inventory hit as local manual-smoke material
  only, not as a committed fixture.

Known local `.aepx` manual-smoke candidate:

- `D:\Projects\01_Project\04_Tools\AEP2Autograph\aftereffects.aepx`

## Modes And Patch Vocabulary

Top-level AEPX modes:

- `inspect_metadata`
- `noop_validate`
- `dry_run`
- `apply`

Top-level JSX modes:

- `validate`
- `generate_jsx_transaction`
- `manual_smoke_plan`

V0 edit operation kinds:

| Operation | Track | Meaning |
| --- | --- | --- |
| `rename_comp` | AEPX, JSX | Rename an explicitly selected composition. |
| `rename_layer` | AEPX, JSX | Rename an explicitly selected layer, ideally scoped by comp. |
| `set_comment` | AEPX, JSX | Set a user-supplied project/item/layer comment when supported. |
| `set_marker` | AEPX, JSX | Add or update a marker with user-supplied time/comment metadata. |
| `replace_text_source` | AEPX, JSX | Replace a targeted text layer/source string with a required old-value guard. |
| `relink_asset_path` | AEPX, JSX | Relink a targeted asset to an explicit user-supplied replacement path. |

All mutating operation requests require an `expected_old_value` guard before
dry-run, JSX generation, or future apply can pass.

Selector preference:

1. stable XML id or AE item/layer id when known;
2. exact comp/layer/item name with scope;
3. exact path for asset relink;
4. ambiguous selectors fail unless a later explicit mode is designed.

## Track A: AEPX Patcher

The `.aepx` patcher operates on XML only.

Default mode is `dry_run`. `apply` requires `output_aepx`.

Required behavior:

- validate request schema and file size limits;
- reject source/output equality;
- reject existing output paths in v0;
- compare source/output paths after Windows-style normalization: absolute path,
  separator normalization, case-insensitive comparison, and canonical path
  resolution where available;
- parse XML and locate targets;
- preserve unknown elements, attributes, ordering, comments, CDATA, namespaces,
  and whitespace wherever the XML library permits;
- touch only the minimal text or attribute nodes named by approved operations;
- fail before writing if preservation cannot be guaranteed and the request did
  not opt into a future explicit normalization mode.

V0 should not canonicalize, pretty-print, reorder, or regenerate the whole
project.

### AEPX Preservation Report

Reports should include preservation fields:

- `unknown_nodes`
- `unknown_attributes`
- `xml_declaration`
- `encoding`
- `comments`
- `cdata`
- `namespace_prefixes`
- `whitespace`

Each preservation field may be:

- `preserved`
- `rewritten`
- `not_written`
- `unknown`

`encoding` may additionally be `normalized` when an implementation explicitly
records that it rewrote encoding metadata instead of preserving it byte-for-byte.

## Track B: JSX Transaction Runner

The JSX route is for projects that must be opened by After Effects, especially
binary `.aep`.

The patch JSON is pure data. Generated JSX is a thin interpreter that:

1. validates `schema_version`;
2. opens the source project selected by the user;
3. applies only allowlisted operations;
4. collects warnings and unsupported operations;
5. saves only to the explicit output path.

V0 should generate JSX and a report. Actual AE execution remains an explicit
manual-smoke step until the runtime boundary is reviewed on the target machine.

Development status (2026-06-01): JSX generation now implements `rename_comp`,
`rename_layer`, guarded composition/layer `set_comment`, owner-scoped
composition/layer `set_marker`, and guarded layer `replace_text_source`. The
generated interpreter preflights every operation before applying any mutation,
then saves once to the explicit output path. AE execution remains manual-only;
automated tests inspect generated JSX and do not launch After Effects. No-AE
reports expose an execution gate and source metadata from filesystem metadata
only; `project_body_read=false` means the Rust probe did not read the source
project payload.

Forbidden by default in generated JSX:

- `system.callSystem`
- `File.openDialog`
- `Folder.selectDialog`
- `app.executeCommand`
- `BridgeTalk`
- `eval`
- `Function`
- `$.evalFile`
- `File.execute`
- `ExternalObject`
- `Socket`
- arbitrary `File.open`, `File.write`, `File.remove`, `File.rename`, or
  `File.copy`
- invoking `afterfx`, `aerender`, `cmd.exe`, `powershell`, `wscript`, or
  external shells
- network calls
- implicit save to the source project
- bare `app.project.save()` without an explicit `new File(output_path)`
- modal `alert()` spam as the main report channel

Allowed save behavior:

- only `app.project.save(new File(output_project_path))`;
- only after source/output path validation has already happened on the broker
  side;
- only when output path is user supplied and not the source path.

Forbidden-token scanning should run on generated code outside the escaped JSON
payload. If a forbidden token appears inside user-supplied patch data, v0 should
reject the request unless the implementation has a parser-level scan that can
prove the token is inert data and cannot be executed.

## Status Vocabulary Pool

The following status names are the shared vocabulary pool. Individual AEPX and
JSX schemas expose the subset that applies to their track.

- `ok`
- `dry_run_ok`
- `generated_jsx`
- `manual_smoke_plan_ok`
- `ae_run_ok`
- `invalid_request`
- `schema_error`
- `source_not_found`
- `output_exists`
- `output_same_as_source`
- `unsupported_operation`
- `ambiguous_target`
- `target_not_found`
- `expected_value_mismatch`
- `parse_error`
- `preservation_failed`
- `write_failed`
- `ae_not_run`
- `ae_runtime_failed`
- `forbidden_jsx_api`
- `internal_error`

Canonical AEPX/JSX naming:

- JSX request fields use nested `source_project.path`, `output_project.path`,
  and `generated_jsx.path`;
- AEPX request fields use `input_aepx`, `output_aepx`, and `operations[].kind`;
- report fields may expose flattened `source_project_path`,
  `output_project_path`, and `generated_jsx_path` for easy CLI consumption;
- `forbidden_jsx_api` is the canonical status for forbidden API scan failures;
- `invalid_request` is the usual schema/path validation failure, while
  `schema_error` may be used when a caller needs a narrower parse/schema
  category.
- same-source/output path should report `output_same_as_source` in v0.

Publication policy:

- schema artifact `publication_status` may be `local-only design artifact`;
- request `publication_status` uses `local-only | public-candidate | unknown`;
- v0 should fail closed for `unknown` before writing;
- generated JSX that embeds local paths or private text remains `local-only`;
- source project report metadata is limited to path normalization, existence,
  file/type, and byte length from filesystem metadata;
- `report_private_payloads=true` is invalid in both tracks.

Per-operation statuses:

- `planned`
- `applied`
- `skipped`
- `failed`
- `unsupported`

## Fixture Strategy

Use only synthetic fixtures first.

Recommended synthetic fixtures:

- tiny `.aepx`-like XML with fake comps/layers/text/assets;
- unknown XML islands;
- unknown attributes;
- comments;
- CDATA;
- namespace prefixes;
- odd whitespace;
- duplicate names that force `ambiguous_target`;
- expected-old-value mismatch cases.

The local real `.aepx` may be used only for manual metadata/noop/dry-run smoke
after explicit local approval. It must not be committed, copied, minimized, or
quoted in public artifacts.

## First Development Slice

Implementation should be contract-first:

1. parse request JSON;
2. validate mode/operation names and path rules;
3. reject overwrites;
4. return structured dry-run reports;
5. generate JSX text for neutral operations without running AE;
6. add tests for forbidden JSX strings;
7. add synthetic `.aepx` preservation tests only if a preservation-safe XML
   library choice has been reviewed.

Do not add an Adobe SDK dependency, OFX SDK dependency, or binary `.aep` parser
for this slice.

## Explicit Non-Goals

- binary `.aep` parser or writer;
- full AE project model reconstruction;
- effect parameter editing;
- keyframe/timeline mutation beyond simple markers/comments;
- media copying/import/export;
- `.ffx` parsing;
- `.aex` loading;
- AE runtime launch in automated tests;
- publication of private project payloads.

## Handoff Summary

The correct first tool is not a full project editor. It is a safe patch contract
that can prove:

- `.aepx` can be patched without damaging unknown XML;
- `.aep` can be edited through generated JSX without AviUtlas writing binary
  project files;
- every write uses an explicit new output path;
- unsupported operations become warnings/reports rather than project damage.

## Implementation Status - 2026-05-31

- **AEPX:** the current probe is intentionally fail-closed for XML writes.
  Synthetic preservation sentinels are guarded by contract tests, but the
  preservation fields remain `not_written`; no XML parser/writer has been
  selected or claimed safe. Reports expose `write_gate` as not applied,
  XML writer not implemented, user approval required before write, and no
  output/source write performed. Reports also expose `io_gate` so current
  dry-run metadata reports explicitly state that no XML body was read/written,
  no XML body or private patch payload was embedded, no external process ran,
  and After Effects was not invoked.
- **AEPX writer spike:** `analysis/AEPX_XML_PRESERVATION_WRITER_SPIKE_2026-06-01.md`
  now records a synthetic-only create-new span-splice experiment. It proves a
  narrow exact-id composition rename can preserve untouched unknown XML
  sentinels, and it fails closed for ambiguous name selectors, expected-old
  mismatch, and existing output paths. This is not production `apply`, not an
  XML library choice, and not an AE compatibility claim.
- **JSX:** generated transaction scripts now support `rename_comp`,
  `rename_layer`, guarded `set_comment` for composition/layer comments,
  owner-scoped `set_marker` for composition/layer marker streams, and guarded
  layer `replace_text_source` through a fixed interpreter with
  preflight-before-mutation and one explicit
  `app.project.save(new File(outputPath))` after successful preflight.
  Automated tests still do not run After Effects.

## Continuation Update - 2026-06-01

- **AEPX dry-run path gate:** when `output_aepx` is supplied, dry-run,
  noop-validate, and metadata requests now validate it with the same
  explicit-new-path boundary used by apply: absolute `.aepx`, distinct from
  `input_aepx` after Windows-style normalization, and non-existing. The probe
  still does not read XML bodies or write AEPX.
- **AEPX I/O gate:** AEPX patch reports now include `io_gate` alongside
  `write_gate`; contract tests keep every gate false and check the synthetic
  preservation fixture text is unchanged after dry-run and the unsupported
  apply boundary.
- **AEPX report binding:** AEPX patch reports now include a metadata-only
  `aepx_patch_report_binding` with both `fnv1a64-v1-noncryptographic` and
  `sha256-v1` values. The binding covers report status/path/metadata/gate/count
  fields and excludes elapsed time, XML bodies, selectors, expected/new values,
  and private patch payloads.
- **AEPX payload shape:** marker dry-run now requires structured
  `time_seconds`/`comment` guards and payloads, and `relink_asset_path`
  requires `expected_old_value` to match `selector.asset_path`.
- **JSX transaction report gate:** every no-AE v0 receipt now carries an
  explicit execution gate: `application_status=not_applied`,
  `user_approval_status=required`, `after_effects_status=not_launched`, and
  `project_save_performed=false`.
- **JSX artifact gate:** no-AE v0 receipts now also distinguish the one
  allowed Rust-side artifact write, generated JSX with create-new semantics,
  from prohibited source/output project writes. Rust still never creates the
  output `.aep` / `.aepx` project.
- **JSX report binding:** JSX transaction reports now include a metadata-only
  `jsx_transaction_report_binding` with both
  `fnv1a64-v1-noncryptographic` and `sha256-v1` values. The binding covers
  report status, path aliases, source metadata, operation ids/statuses,
  forbidden-token scan status, artifact gate, and execution gate. It excludes
  elapsed time, request bodies, generated JSX bodies, project payloads, and
  private patch payloads.
- **JSX snapshot export:** `export_ae_jsx` now scans generated JSX for the
  same high-risk forbidden-token surface and fails closed if user snapshot data
  would place those tokens into the script. It can now also emit an optional
  create-new report sidecar (`analysis/AE_JSX_EXPORT_REPORT_SCHEMA_2026-06-01.json`)
  that records forbidden-scan pass, a generated JSX body digest, generated JSX
  create-new writes, no AE launch, no JSX execution, no project write, no source
  overwrite, and no snapshot/generated-JSX body embedding in the report.
- **JSX export manifest:** `export_ae_jsx --manifest manifest.json` can now emit
  a create-new, metadata-only artifact manifest
  (`analysis/AE_JSX_EXPORT_MANIFEST_SCHEMA_2026-06-01.json`). The manifest
  binds the report path, generated JSX path, byte/warning counts, no-AE/no-write
  gates, and a required-pending approval gate without embedding the snapshot JSON
  or generated JSX body. It carries a generated JSX artifact digest plus a
  metadata-only FNV checksum and `sha256-v1` digest over manifest fields; the
  manifest binding covers the generated JSX digest field without embedding the
  body. It is review metadata, not approval to run AE.
- **JSX export artifact verification:** `ae_jsx_export_artifact_verify` now
  provides a standalone pre-approval check for export manifests and generated
  JSX artifacts, following
  `analysis/AE_JSX_EXPORT_ARTIFACT_VERIFY_REPORT_SCHEMA_2026-06-01.json`. It
  recalculates the manifest metadata binding, hashes the local generated JSX
  artifact body, checks the manifest path and byte count, and can optionally
  compare the export report sidecar. `verified` is local consistency evidence
  only; it is not an approval receipt and not tool-side execution permission.
- **Standalone JSX export approval:** `ae_jsx_export_approval` now validates
  operator-filled approval receipts for standalone export manifests, following
  `analysis/AE_JSX_EXPORT_APPROVAL_RECEIPT_SCHEMA_2026-06-01.json`. It accepts
  only `ae_jsx_export_artifact_manifest`, rejects
  `ae_project_edit_ir_jsx_request_pair`, and does not launch AE, execute JSX,
  read project payload bytes, or write project files. It recalculates the manifest's metadata-only FNV checksum and `sha256-v1` digest before producing
  a draft template or accepting a receipt, and now also requires a matching
  `ae_jsx_export_artifact_verification` report with
  `verification_status=verified`. The receipt copies the generated JSX artifact
  digest and verified preflight status. An approved
  `ae_jsx_export_manual_approval_receipt` records an operator decision only; it
  is not tool-side execution permission.
- **Standalone JSX export closeout:** `ae_jsx_export_smoke_closeout` now
  validates manual smoke closeout JSON for standalone export artifacts,
  following
  `analysis/AE_JSX_EXPORT_MANUAL_SMOKE_CLOSEOUT_REPORT_SCHEMA_2026-06-01.json`.
  It can emit a `not_run` template and validate operator-filled completed,
  failed, or aborted closeouts against the export manifest and export approval
  receipt. It reads JSON only, recalculates the export manifest binding, and
  copies the generated JSX artifact digest. It also requires the approval
  receipt to carry a reviewed verified artifact preflight prerequisite. It still
  does not launch AE, execute JSX, read project payload bytes, or write project
  files. Its accepted status is not an AE/runtime oracle.
- **Approval scope split:** `ae_project_edit_approval` currently accepts only
  `ae_project_edit_ir_jsx_request_pair` manifests as approval scope. It must
  reject `ae_jsx_export_artifact_manifest` before producing approval templates or
  accepting receipts, because export manifests do not carry the IR request-pair
  validate/generate evidence chain. Standalone export approval remains in the
  separate `ae_jsx_export_approval` route. It now also requires a matching
  `ae_project_edit_review_packet` report with `validation_status=review_ready`
  before an approval receipt can validate as `approved`, and approved receipts
  must copy the packet's metadata-only `review_packet_binding.checksum_hex` and
  `review_packet_binding.cryptographic_digest.digest_hex`. When that packet
  includes supplemental standalone export manifest or artifact verification
  evidence, the project-edit approval receipt must acknowledge the reviewed
  artifacts while keeping them out of approval scope. When it includes
  synthetic AEPX preservation proof evidence, the receipt must acknowledge that
  proof while still keeping production AEPX apply disabled.
- **JSX transaction artifact verification:** `ae_jsx_transaction_artifact_verify`
  verifies a generated JSX transaction artifact before review by reading the
  generated-JSX transaction report and the local generated JSX bytes. It checks
  that the observed path, byte count, and `sha256-v1` digest match
  `generated_jsx_artifact_digest`, emits
  `ae_jsx_transaction_artifact_verification`, and does not launch AE, execute
  JSX, read project payload bytes, or write project files.
- **Review packet validator:** `ae_project_edit_review_packet` checks a
  metadata-only packet before approval: request-pair manifest, downstream
  validate report, generated JSX report, optional not-run closeout template, and
  optional export artifact manifest. The downstream validate and generated JSX
  reports must carry their metadata-only bindings/digests. Generated JSX reports
  must also carry `generated_jsx_artifact_digest` metadata with `sha256-v1`,
  positive byte count, and `generated_jsx_embedded=false`. It can also attach an
  optional AEPX `dry_run_ok` report as no-write evidence, but that AEPX report
  must carry the metadata-only binding/digest described above. The
  packet report now carries a
  metadata-only `review_packet_binding` with both FNV and `sha256-v1` values so
  approval/closeout artifacts can bind to the reviewed packet without embedding
  request, JSX, XML, project, or private patch payload bodies. `review_ready` is pre-approval consistency evidence only;
  it does not launch AE, execute JSX, save projects, accept an
  export artifact manifest as approval, or permit AEPX XML apply. Optional
  export artifact manifests must also match their own metadata-only
  checksum/digest before the packet can become `review_ready`. Optional export
  artifact verification reports can be attached only with the export artifact
  manifest and must match path, byte count, generated JSX digest, manifest
  binding, and report sidecar fields with `verification_status=verified`; this
  remains supplemental review metadata, not approval. Optional generated JSX
  transaction artifact verification reports can also be attached and must match
  the generated-JSX transaction report path, byte count, and `sha256-v1` digest
  with `verification_status=verified`; this is supplemental integrity evidence,
  not approval. Optional synthetic AEPX
  preservation proof reports can also be attached only as local-fixture
  exact-byte-diff evidence with `production_apply_enabled=false`; they do not
  permit production AEPX XML apply. Optional AEPX production-lane preflight
  reports can also be attached only as gate-closed evidence with
  `status=apply_gate_closed_ready`; they do not permit production AEPX XML
  apply, real `.aepx` input, or binary `.aep` editing.
- **AEPX production-lane preflight:** `aepx_production_lane_gate_preflight`
  reads only the local production-lane gate schema and keeps production AEPX
  apply closed. Its ready report now also makes the binary `.aep` boundary
  machine-visible with `binary_aep_writer_enabled=false`,
  `binary_aep_read_performed=false`, and `binary_aep_write_performed=false`.
  Project-edit review/approval/closeout can now carry this report as optional
  evidence, but must keep
  `aepx_production_lane_preflight_accepted_as_apply=false`.
- **Project-edit manual smoke closeout:** `ae_manual_smoke_closeout` now
  carries the optional standalone export manifest / artifact verification
  evidence chain through closeout validation. If those supplemental artifacts
  are present in the supplied review packet, the approval receipt must carry the
  matching reviewed acknowledgements and closeout validation still requires the
  export artifacts to remain `accepted_as_approval=false`. It also carries the
  optional synthetic AEPX preservation proof acknowledgement through closeout and
  still requires the proof to remain `accepted_as_apply=false`. It also carries
  the optional production-lane preflight acknowledgement and still requires that
  preflight to remain gate-closed and `accepted_as_apply=false`.
- **Synthetic project-edit chain contract:**
  `aviutl-rs/tests/ae_project_edit_e2e_contract.rs` connects the current
  JSON-only path from IR planning through JSX transaction reports, review
  packet, approval receipt validation, and manual smoke closeout validation. It
  is a local contract over synthetic data and generated artifacts under
  `target`, including a synthetic AEPX preservation proof; it is not an AE
  launch, project save, render, production AEPX apply, or binary `.aep` edit.
- **Report artifacts:** AE project edit IR planning, AEPX patch probing, and
  JSX transaction probing now use create-new semantics for CLI report outputs
  instead of overwriting an existing report path.
- **JSX optional path validation:** `validate` and `manual_smoke_plan` remain
  no-write modes, but when callers supply `output_project` or `generated_jsx`
  paths those paths now pass the same absolute extension, pairwise-distinct,
  and non-existing gate before the report is accepted.

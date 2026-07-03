# AE Project Edit IR - 2026-06-01

Purpose: define a neutral, data-only edit intent layer for After Effects
project round-trip planning. This sits above the current `.aepx` dry-run probe
and JSX transaction generator so both routes can share one allowlisted edit
vocabulary without AviUtlas writing binary `.aep` files.

Machine-readable companion:

- `analysis/AE_PROJECT_EDIT_IR_SCHEMA_2026-06-01.json`
- `analysis/AE_PROJECT_EDIT_APPROVAL_RECEIPT_SCHEMA_2026-06-01.json`
- `analysis/AE_PROJECT_EDIT_MANUAL_SMOKE_PROTOCOL_SCHEMA_2026-06-01.json`
- `analysis/AE_PROJECT_EDIT_MANUAL_SMOKE_CLOSEOUT_REPORT_SCHEMA_2026-06-01.json`
- `analysis/AE_JSX_EXPORT_ARTIFACT_VERIFY_REPORT_SCHEMA_2026-06-01.json`

Operator checklist companion:

- `analysis/AE_PROJECT_EDIT_OPERATOR_RUNBOOK_2026-06-01.md`

Synthetic contract fixture:

- `aviutl-rs/tests/fixtures/ae_roundtrip_expectations.synthetic.json`

## Boundary

- Do not parse or write binary `.aep`.
- Do not launch After Effects in automated tests.
- Do not overwrite source `.aep` or `.aepx`.
- Do not infer output paths from source paths.
- Do not accept arbitrary JavaScript.
- Do not copy private project, media, `.ffx`, or `.aex` payloads into tests.
- Do not depend on `.aex` loading or `.ffx` parsing for project editability.

Binary `.aep` remains AE-owned. The safe route for binary projects is generated
JSX reviewed by the user and run only as a manual smoke step. `.aepx` writes
remain gated on preservation proof for unknown XML.

## IR Shape

The IR describes intent only:

- source project reference;
- explicit new output project path;
- optional generated JSX path;
- route preference: `aepx`, `jsx`, or both;
- allowlisted edit operations;
- stable selectors and fallback exact-name selectors;
- `expected_old_value` guards;
- privacy flags;
- per-route support status.

The operation vocabulary is intentionally the same as the AEPX/JSX schemas:

- `rename_comp`
- `rename_layer`
- `set_comment`
- `set_marker`
- `replace_text_source`
- `relink_asset_path`

Tool modes such as `dry_run`, `apply`, `validate`, and
`generate_jsx_transaction` are not operation kinds.

## Selector Policy

Selectors should prefer stable identity before display text:

1. neutral `stable_id`;
2. route-native id such as `xml_id` or `ae_item_id`;
3. exact scoped names;
4. exact source path for asset relink.

Ambiguous selectors fail closed. Every mutating operation must carry an
`expected_old_value` guard before any route can generate mutation output.
Unknown target kinds and unknown operation fields are rejected before any route
adapter is allowed to generate output.

Index bases are explicit:

- JSX layer selectors use AE's 1-based layer indexes.
- AEPX marker selectors use zero-based marker indexes in the IR.

Real requests must use explicit absolute source, output, and generated JSX
paths and compare them after Windows-style normalization. Synthetic fixtures may
use `synthetic://` placeholders or generated `target/` paths only because they
are not runnable project requests.

## Operation Notes

`rename_comp`, `rename_layer`, composition/layer `set_comment`, owner-scoped
composition/layer `set_marker`, guarded layer `replace_text_source`, and
guarded asset `relink_asset_path` are the first implemented JSX operations.
They are still represented in the neutral IR so AEPX and JSX can agree on
selector and expected-value semantics.

The current JSX `set_marker` route is intentionally narrow: it requires
`user_supplied=true`, a marker object with `time_seconds` and `comment`, an
explicit composition or layer owner selector, and an expected-current marker
comment guard at that time. Route-neutral `target.kind="marker"` can now
lower to JSX only when the selector contains route-native owner fields such as
`comp_name` or `comp_ae_item_id` and does not include `marker_index`,
`stable_id`, or `xml_id`. Marker ids/indexes still require a resolver and
remain unresolved.

The current JSX `replace_text_source` route is intentionally narrow: it requires
`target.kind="text_source"`, `user_supplied=true`, a string
`expected_old_value`, a string `new_value`, and explicit composition/layer
selectors. It mutates only the AE Source Text property after all operations
preflight.

The current JSX `relink_asset_path` route is intentionally narrow: it requires
`target.kind="asset"`, only `selector.asset_path`, `expected_old_value`
matching that source path, a non-empty string `new_value`, and
`user_supplied=true`. Generated JSX preflights a single matching `FootageItem`,
checks the old path before mutation, rejects duplicate replacement paths, and
then calls AE-side footage replacement. Rust still does not launch After
Effects or verify real project-save/relink side effects; that remains a manual
AE smoke / Native Oracle boundary.

## Automated Contract

Automated tests should validate only schema and synthetic semantic
expectations:

- all six operation kinds are represented;
- source, output, and generated JSX path policies are explicit;
- publication status `unknown` fails closed before writes;
- `report_private_payloads=true` is invalid;
- text replacement and asset relink require `user_supplied=true`;
- binary `.aep`, `.ffx`, and `.aex` payloads stay out of fixtures;
- AE runtime execution remains manual-only.

The synthetic expectation corpus is not an `.aepx` or `.aep` fixture. It is a
small neutral before/after semantic model used to prevent operation vocabulary
drift.

## Manual Smoke Protocol

Manual smoke is copy-project-only. It is a human-operated bridge between
report-first planning and any real AE project application, not an automated
runtime path.

The protocol requires:

- an IR adapter report with no project write;
- a metadata-only request-pair manifest;
- a validate-mode JSX transaction request and `dry_run_ok` report;
- reviewed generated JSX;
- an explicit approval receipt matching the reviewed source, output, generated
  JSX path, and request-pair manifest.

Even with an approved receipt, the tool must not launch AE, execute JSX, save a
project, overwrite the source path, or directly write binary `.aep`. The
approved scope is limited to an operator manually opening a reviewed local copy,
running the reviewed JSX, and saving only to the explicit `output_project.path`.
Any closeout report from this step is sanitized evidence, not an automated AE
oracle.

The IR adapter may emit a closeout template only after it has produced a
planned JSX request. That template starts as `not_run`, records no automated AE
oracle, and carries false/null defaults for AE launch, JSX execution, project
save, and approval receipt id. A human may fill it after an explicitly approved
manual smoke step, but an acceptable filled report still must state that source
overwrite was not observed and no private payloads were embedded.

The request-pair manifest now includes a metadata-only binding checksum and a
metadata-only `sha256-v1` digest over the same canonical manifest fields. These
fields cover reviewed manifest metadata such as IR id, normalized paths,
operation counts, request modes, request byte counts, and approval-gate status.
They deliberately do not embed or hash private patch payload bodies. The
`fnv1a64-v1-noncryptographic` checksum remains a local compatibility guard; the
`sha256-v1` digest is stronger review metadata for harder audit trails, while
still excluding request bodies and private payloads.

`ae_manual_smoke_closeout` is the current closeout validator. It reads only the
request-pair manifest, closeout report, optional approval receipt JSON, and
optional review packet report JSON. It does not launch AE, execute JSX, read
project payload bytes, or write project files. `accepted` now requires both an
approval receipt carrying
`prerequisite_reports.review_packet_status=review_ready` /
`review_packet_report_reviewed=true` and a matching `review_ready` review packet
report artifact, so older receipt-only shapes fail closed. `accepted` means
those artifacts are internally consistent, request-pair digest matched, and
approval gated; it is not an automated AE compatibility oracle.

`ae_project_edit_review_packet` reports now include their own metadata-only
`review_packet_binding` checksum plus a `sha256-v1` cryptographic digest.
Approval receipts copy both values, and the approval/closeout validators compare
the copied values against the supplied review packet report. This reduces
operator transcription drift without embedding request bodies, JSX bodies,
XML/project payloads, or private patch payloads.

When the review packet also carries supplemental standalone export manifest or
export artifact verification evidence, both the approval and closeout
validators keep that evidence visible without treating it as approval. Approval
receipts must acknowledge the reviewed export artifacts, and closeout validation
checks those acknowledgements against the supplied review packet while still
requiring `*_accepted_as_approval=false`.

Downstream JSX transaction reports now also expose a metadata-only
`jsx_transaction_report_binding` with FNV and `sha256-v1` values. Review packet
validation requires this binding for both the validate-mode `dry_run_ok` report
and the generated-JSX report, then copies their checksums/digests into the
review packet metadata. This binds the pre-approval evidence chain without
embedding generated JSX bodies, request bodies, project payloads, or private
patch payloads.

Generated-JSX transaction reports now also expose
`generated_jsx_artifact_digest` with `sha256-v1`, generated byte count, and
`generated_jsx_embedded=false`. Review packet validation requires that digest
metadata and copies it into the review packet binding, while approval and
closeout validators reject review packets that omit it, use malformed digest
metadata, or claim the generated JSX body was embedded in the report.

Generated JSX transaction artifact verification is now available as an optional
JSON-only pre-approval step. `ae_jsx_transaction_artifact_verify` hashes the
actual local generated JSX file and checks that its observed path, byte count,
and `sha256-v1` digest match the generated-JSX transaction report. Review
packet, approval, and closeout validators can carry this evidence when present,
but they still require
`generated_jsx_artifact_verification_accepted_as_approval=false`.

The approval and manual closeout validators also surface those review-packet
copied JSX transaction checksums/digests. A missing or malformed downstream
validate/generated-JSX report binding now fails approval and closeout validation
before any human-operated AE smoke step. This is audit evidence only; it does
not authorize tools to launch AE, execute JSX, save projects, or write `.aep`.

Optional AEPX `dry_run_ok` reports attached to a review packet must now carry a
metadata-only `aepx_patch_report_binding` with both FNV and `sha256-v1` values.
The review packet copies and validates those values as supplemental no-write
evidence only. The binding excludes elapsed time, XML bodies, selectors,
expected/new values, and private patch payloads, and it still does not authorize
AEPX apply.

Optional AEPX synthetic preservation proof reports can now be attached to the
same review packet as supplemental local-fixture evidence. The review packet
requires `status=synthetic_preservation_proof_ready`, preserved unknown XML
sentinels, exact-byte-diff verification outside approved spans, privacy gates
that omit paths/selectors/edit values/XML bodies, create-new output under the
synthetic `target` proof root, and `production_apply_enabled=false`. Approval
receipts and closeout reports must acknowledge the proof when present, but the
proof is still not production AEPX apply permission.

`ae_project_edit_approval` validates approval receipts before the closeout
stage. It reads the request-pair manifest and a proposed approval receipt JSON,
checks the binding checksum, SHA-256 manifest digest, path scope,
validate/generate operation names, review prerequisites, and forbidden overwrite
flags, then reports `approved` or `invalid_receipt`. It does not create
approval, launch AE, execute JSX, read project payload bytes, or write project
files.

The same tool can emit a `draft_unapproved` approval template from a reviewed
request-pair manifest. The template copies scope and binding fields to reduce
transcription mistakes, but it deliberately leaves receipt id, operator, time,
review acknowledgements, and execution effects unapproved. The validator must
report `invalid_receipt` for the draft until a human fills the receipt and sets
`approval_status=approved`.

`aviutl-rs/tests/ae_project_edit_e2e_contract.rs` now exercises the synthetic
JSON-only chain from IR planning through JSX validate/generate reports, review
packet, approval receipt validation, and manual smoke closeout validation. It
also attaches a standalone export artifact verification report as supplemental
review evidence plus a local synthetic AEPX preservation proof as supplemental
non-apply evidence. The test creates only generated artifacts under `target`; it
does not launch AE, execute JSX in AE, write project output, apply production
AEPX XML, or prove AE runtime compatibility.

## Next Implementation Options

1. Extend the current IR-to-JSX adapter beyond rename/comment/marker operations
   only when the JSX backend supports the same operation.
2. Add a stronger marker-owner resolver if route-neutral marker ids/indexes
   without owner fields need to emit JSX rather than remaining AEPX/resolver
   work.
3. Review an XML writer against preservation sentinels before enabling AEPX
   apply.
4. Add public/out-of-band digest verification or signed receipts if
   harder-audit workflows need more than local metadata-only JSON matching.

None of those steps should enable binary `.aep` writing or automated AE
launching by default.

## Implementation Status

The first IR-to-JSX planner now exists:

- `aviutl-rs/examples/ae_project_edit_ir.rs`
- `aviutl-rs/tests/ae_project_edit_ir_adapter_contract.rs`
- `aviutl-rs/tests/fixtures/ae_project_edit_ir.supported_renames.json`

It accepts the neutral IR only when every operation can be represented by the
current JSX v0 backend. Today that means `rename_comp`, `rename_layer`,
composition/layer `set_comment`, owner-scoped composition/layer `set_marker`,
guarded layer `replace_text_source`, guarded asset `relink_asset_path`, and
route-neutral marker targets that carry only route-native owner fields. If
marker targets include unresolved marker ids/indexes, lack owner fields, or if
asset relink selectors use unresolved stable ids instead of an exact
`asset_path`, the planner reports `invalid_request` or `unsupported_operation`
and emits no JSX transaction request.

The planner translates route-neutral selectors into the narrower
`ae_jsx_transaction` selector vocabulary. `stable_id` and `xml_id` are not
passed through to JSX unless a later resolver maps them to route-native
selectors. Real request paths must remain absolute, explicit, new, and
pairwise distinct.

The planner still does not generate JSX directly, launch After Effects, parse
or write projects, write AEPX, or parse binary `.aep`.

## Continuation Inventory - 2026-06-01

Current implemented surface:

- Neutral IR contract and synthetic semantic corpus cover all six edit
  operation kinds.
- IR-to-JSX planning emits transaction request JSON only for the JSX v0 subset:
  rename comp/layer, set comp/layer comment, owner-scoped comp/layer marker,
  and guarded text-source replacement.
- AEPX probe remains metadata/report-only. Dry-run now validates supplied
  output paths as explicit new `.aepx` paths, and reports `not_written` for
  all preservation fields. Reports also expose a no-write gate: patch not
  applied, XML writer not implemented, output write not performed, and source
  overwrite not performed.
- JSX transaction generation remains report-first. Reports now expose a
  no-run execution gate: not applied, user approval required, AE not launched,
  and no project save performed.
- JSX transaction reports now also expose an artifact gate: generated JSX writes
  are recorded only for `generated_jsx` receipts using create-new semantics,
  while Rust-side output project writes and source overwrites remain false.
- Generated-JSX transaction reports now carry a `sha256-v1`
  `generated_jsx_artifact_digest` and byte count without embedding the script
  body.
- `ae_jsx_transaction_artifact_verify` now verifies the actual local generated
  JSX artifact bytes against a generated-JSX transaction report path, byte
  count, and `sha256-v1` digest. The verifier reads only the transaction report
  JSON and generated JSX file bytes; it does not launch AE, execute JSX, read
  project payload bytes, or write project files.
- JSX transaction reports now include source metadata from filesystem metadata
  only: normalized source path, existence, file/type, optional byte length, and
  `project_body_read=false`.
- IR adapter reports now expose `route_gates` with the downstream JSX
  execution-gate vocabulary, JSX artifact-gate vocabulary, and AEPX write-gate
  vocabulary, so callers can see not-applied/not-launched/not-written status
  before invoking route tools. The adapter's own JSX artifact gate remains
  `not_written`; generated JSX creation is a separate downstream
  `ae_jsx_transaction` generate step.
- IR adapter reports now include metadata-only `path_summary`: caller source,
  output, and generated JSX paths, normalized path strings, source filesystem
  metadata, output/generated existence flags, absolute/distinct checks, and
  `project_body_read=false`.
- IR adapter now fails closed with `output_exists` before emitting a JSX
  transaction request if the output project path or generated JSX path already
  exists.
- IR adapter can now emit a separate validate-mode JSX transaction request
  (`operation=validate`) and reports `downstream_jsx_validation` so callers can
  run a no-AE dry-run check before any generated JSX write step.
- IR adapter can also emit a metadata-only JSX request-pair manifest. It records
  generate/validate operation names, request byte counts, path summary, and
  no-AE/no-project-write expectations without embedding request bodies or patch
  payloads. The manifest now carries `human_approval_gate` as
  `required_pending` with a missing approval receipt, and requires that receipt
  before AE execution or project writes.
- A separate approval receipt schema now defines the explicit human approval
  artifact. Even an approved receipt can only authorize AE launch, JSX
  execution, and saving to the explicit output path; source overwrite and
  direct binary `.aep` writing remain forbidden.
- A separate manual smoke protocol schema now defines the copy-project-only
  handoff from dry-run artifacts to a human-operated AE smoke step. It requires
  the request-pair manifest, downstream validate `dry_run_ok`, reviewed
  generated JSX, and an approved receipt before any manual AE execution.
- A separate manual smoke closeout report schema and adapter-emitted template
  now provide a sanitized `not_run` evidence shell. It records explicit paths
  and safety checks but does not claim AE was launched, JSX was executed, or a
  project was saved.
- Request-pair manifests now expose both a metadata-only non-cryptographic
  binding checksum and a metadata-only `sha256-v1` digest. The closeout template
  copies the digest and checksum so an approval/closeout chain can refer to the
  same reviewed manifest without embedding private patch payload bodies.
- `ae_manual_smoke_closeout` now validates operator-filled closeout artifacts
  against the request-pair manifest, approval receipt, and matching
  `review_ready` packet validation report, including the request-pair digest
  and the review packet checksum/digest copied into the receipt. It also
  requires the review packet to expose metadata-only downstream validate and
  generated-JSX report checksums/digests. It reports `accepted`, `not_run`,
  failure/abort, or `invalid_closeout` without running AE or writing projects.
- `ae_project_edit_approval` now validates approval receipt artifacts against
  the request-pair manifest and a matching `review_ready` review packet report
  before manual smoke. It checks binding/path/review/safety gates, including a
  receipt-copied request-pair digest plus review packet checksum and SHA-256
  digest, and now requires the review packet's copied downstream validate and
  generated-JSX report checksums/digests. It reports `approved` or
  `invalid_receipt` without granting tools permission to launch AE or write
  projects.
- `ae_project_edit_approval` can also emit a `draft_unapproved` approval
  template from the request-pair manifest. This is a transcription aid only:
  validators reject it until a human fills the receipt and explicitly approves.
- JSX snapshot export now fails closed if generated importer JSX would contain
  high-risk forbidden tokens from user snapshot data. It can also emit an
  optional create-new report sidecar that records generated JSX artifact writes,
  generated JSX body digest, forbidden-token scan pass, no AE launch, no JSX
  execution, no project write, no source overwrite, and no snapshot/generated-
  JSX body embedding.
- JSX snapshot export can also emit a create-new metadata-only export manifest
  (`analysis/AE_JSX_EXPORT_MANIFEST_SCHEMA_2026-06-01.json`) that binds the
  generated JSX/report artifact pointers and approval-pending gates without
  embedding snapshot JSON or generated JSX bodies. The manifest now carries a
  `generated_jsx_artifact_digest` for the JSX body plus metadata-only FNV and
  `sha256-v1` manifest bindings that cover that digest field without embedding
  the body.
- `ae_jsx_export_artifact_verify` adds a standalone pre-approval verifier for
  export manifests and generated JSX artifacts, following
  `analysis/AE_JSX_EXPORT_ARTIFACT_VERIFY_REPORT_SCHEMA_2026-06-01.json`. It
  recalculates the export manifest metadata binding, reads the local generated
  JSX artifact bytes, compares path, byte length, and `sha256-v1` body digest,
  and optionally checks the export report sidecar. A `verified` report is
  consistency evidence only; it is not an approval receipt and does not launch
  AE, execute JSX, read project payload bytes, or write projects.
- `ae_jsx_export_approval` adds a separate JSON-only approval validator for
  standalone `ae_jsx_export_artifact_manifest` sidecars, following
  `analysis/AE_JSX_EXPORT_APPROVAL_RECEIPT_SCHEMA_2026-06-01.json`. This route
  can validate an operator-filled `ae_jsx_export_manual_approval_receipt` for
  manual review/run decisions, and it now recalculates the export manifest's
  metadata-only FNV checksum and `sha256-v1` digest before accepting a draft or
  receipt. It also now requires a matching
  `ae_jsx_export_artifact_verification` report with `verification_status=verified`
  before producing a draft approval template or accepting a receipt. The
  receipt copies the generated JSX artifact digest and artifact verification
  status so a reviewed body hash can flow forward without embedding the JSX
  body. It still does not launch AE, execute JSX, read project payload bytes, or
  write projects.
- `ae_jsx_export_smoke_closeout` adds the matching JSON-only manual smoke
  closeout validator for standalone export artifacts, following
  `analysis/AE_JSX_EXPORT_MANUAL_SMOKE_CLOSEOUT_REPORT_SCHEMA_2026-06-01.json`.
  It can emit a `not_run` template and validate operator-filled completed,
  failed, or aborted closeouts against the export manifest and export approval
  receipt. It copies the generated JSX artifact digest, recalculates the export
  manifest metadata binding, requires the approval receipt to carry a reviewed
  verified artifact preflight prerequisite, and keeps tool-side AE launch, JSX
  execution, project body reads, and project writes hard false.
- `ae_project_edit_approval` now reports its accepted manifest scope and
  explicitly rejects `ae_jsx_export_artifact_manifest` as a request-pair
  approval scope. Export manifests can now seed approval templates only through
  the separate standalone export approval route, not through the IR request-pair
  approval validator.
- `ae_project_edit_review_packet` now validates a metadata-only pre-approval
  packet: request-pair manifest, downstream validate report, generated JSX
  report, optional manual-smoke closeout template, and optional export artifact
  manifest. It now requires both JSX transaction reports to carry metadata-only
  bindings and `sha256-v1` digests. It can also attach an optional AEPX
  `dry_run_ok` report as no-write evidence, now requiring that AEPX report's
  metadata-only binding and `sha256-v1` digest. Its report carries a
  metadata-only review packet binding checksum that approval/closeout artifacts
  can copy. A `review_ready` packet still requires a separate approval receipt
  before AE launch, JSX execution, project save, or any future AEPX apply.
  Generated JSX artifact digest metadata is required and carried through this
  binding without embedding the script body.
  Optional generated JSX transaction artifact verification reports can also be
  attached and must prove the local generated JSX file path, byte count, and
  `sha256-v1` digest match the generated-JSX transaction report; this remains
  supplemental integrity evidence only, not approval.
  Optional export artifact manifests are also checked against their own
  metadata-only checksum/digest, so stale or tampered manifest summary fields
  fail before review can become `review_ready`. Optional export artifact
  verification reports can also be attached and must match the export manifest
  path, byte count, generated JSX digest, manifest binding, and report sidecar
  fields with `verification_status=verified`; this remains supplemental review
  metadata only, not approval. Optional synthetic AEPX preservation proof
  reports can also be attached, but only as local-fixture exact-byte-diff
  evidence with `production_apply_enabled=false`, not production XML apply.
  Optional AEPX production-lane preflight reports can also be attached as
  gate-closed evidence when `status=apply_gate_closed_ready`; review,
  approval, and closeout require `aepx_production_lane_preflight_accepted_as_apply=false`
  plus production, real `.aepx`, and binary `.aep` gates closed.
- `ae_manual_smoke_closeout` now carries the same optional standalone export
  evidence chain through closeout validation. If a review packet includes export
  manifest or artifact verification evidence, the approval receipt must carry
  matching reviewed acknowledgements and closeout validation still requires the
  evidence to remain `accepted_as_approval=false`.
  If a review packet includes synthetic AEPX preservation proof evidence, the
  approval receipt must carry
  `prerequisite_reports.aepx_preservation_proof_reviewed=true` and closeout
  validation still requires `aepx_preservation_proof_accepted_as_apply=false`.
  If a review packet includes AEPX production-lane preflight evidence, the
  approval receipt must carry
  `prerequisite_reports.aepx_production_lane_preflight_reviewed=true` and
  closeout validation still requires
  `aepx_production_lane_preflight_accepted_as_apply=false`.
- CLI report outputs for IR planning, AEPX probing, and JSX transaction probing
  now refuse existing report paths with create-new semantics.
- JSX validate/manual-smoke reports still do not write JSX or projects, but
  supplied output/generated paths are validated before being accepted into a
  report.

Still unverified or not implemented:

- No preservation-safe AEPX XML writer has been selected or validated.
- No binary `.aep` parser/writer exists in this slice.
- No automated After Effects launch, JSX execution, or project-save oracle has
  been run.
- No real approval receipt has been reviewed by an operator; the approval
  validator is covered with synthetic JSON artifacts only.
- Approval/closeout chains without a matching `review_ready` packet report are
  now rejected; this has not yet been exercised with a real operator-filled
  receipt or real manual smoke run.
- No real standalone export approval receipt has been reviewed by an operator;
  the new export approval validator is covered with synthetic JSON artifacts
  only. The required artifact verification report is also synthetic/local-only
  evidence in current tests.
- Standalone export approval is an operator review record only. It is not a
  tool-side AE launcher, JSX executor, or project-save executor.
- Standalone export closeout is internal JSON consistency evidence only. It is
  not an automated AE compatibility, import-success, render, or project-save
  oracle.
- Export manifest binding verification covers metadata summary fields and the
  generated JSX artifact digest field only. `ae_jsx_export_artifact_verify`
  separately hashes the local generated JSX artifact body when supplied, but it
  still does not embed snapshot JSON, generated JSX, or project bodies.
- Review packet validation is internal JSON consistency evidence only; it does
  not prove AE compatibility, generated JSX semantic correctness inside AE, or
  project save success.
- Optional generated JSX transaction artifact verification evidence inside
  review packets proves only that local generated JSX bytes matched the
  generated-JSX transaction report path, byte count, and digest at verification
  time. It does not prove AE runtime compatibility or authorize any tool-side
  execution.
- Optional standalone export artifact verification evidence inside review
  packets proves only that local generated JSX bytes matched the manifest digest
  at verification time. It does not prove AE runtime compatibility or authorize
  any tool-side execution.
- Optional AEPX dry-run reports inside review packets must be binding/digest
  checked and still do not enable XML writes; AEPX apply remains blocked until a
  preservation-safe writer is implemented and separately reviewed.
- Optional AEPX synthetic preservation proof reports inside review packets prove
  only a local synthetic fixture exact-span writer behavior. They do not enable
  production AEPX writes, real project rewrites, or any `.aep` binary editing.
- The AEPX production-lane preflight remains metadata-only and now surfaces
  binary `.aep` no-read/no-write fields explicitly:
  `binary_aep_writer_enabled=false`, `binary_aep_read_performed=false`, and
  `binary_aep_write_performed=false`. When attached to a review packet, it
  proves only that the gate artifact is currently closed and reviewed; it does
  not enable production AEPX apply, real `.aepx` input, or binary `.aep`
  editing.
- No tool-generated approval template is treated as approval; draft templates
  remain rejected until human-filled.
- No real operator-filled manual AE smoke closeout report has been produced;
  the validator is covered with synthetic JSON artifacts only.
- Current SHA-256 digests are metadata-only review aids, not public
  notarization; they do not cover request bodies, generated JSX bodies,
  XML/project payloads, or private patch payloads.
- Source metadata does not prove project readability or AE compatibility; it
  only proves metadata lookup without reading project payload bytes.
- Relink application, stable-id marker resolution, `.ffx` parsing, `.aex`
  loading, and effect parameter editing remain deferred to future scoped work
  or other responsible agents.

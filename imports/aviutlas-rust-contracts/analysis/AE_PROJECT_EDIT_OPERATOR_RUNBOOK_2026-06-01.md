# AE Project Edit Operator Runbook - 2026-06-01

Purpose: describe the human-operated path from generated JSON/JSX review
artifacts to a manual After Effects smoke step. This runbook is an operating checklist only. It is not approval, not an executor, and not an AE compatibility oracle.

## Scope

Use this runbook only for the AE/AEP/AEPX/JSX/project-edit IR surface:

- JSON IR planning and request-pair manifests.
- AEPX dry-run reports.
- AEPX synthetic preservation proof reports.
- AEPX production-lane preflight reports.
- JSX transaction reports and generated JSX artifacts.
- Standalone JSX export manifests, artifact verification reports, approval
  receipts, and closeout reports.

Out of scope:

- Direct binary `.aep` writes.
- Tool-side After Effects launch.
- Tool-side JSX execution.
- Source project overwrite.
- AEX, OFX, ExEdit, AviUtl core, and plugin loading work.
- Publishing private project/media/FFX/AEX/binary payloads.

## Hard Stops

Stop before any manual AE action if any item below is true:

- Any validator reports `invalid_*`, `blocked`, or a non-empty
  `blocked_reasons` array.
- Any generated path equals the source project path after normalization.
- Any expected output project path already exists unless the exact manual step
  explicitly reviewed that existing destination.
- Any report claims `after_effects_launched_by_tool=true`,
  `jsx_executed_by_tool=true`, `project_write_performed_by_tool=true`, or
  `source_overwrite_performed_by_tool=true`.
- Any artifact embeds source snapshot JSON, generated JSX body, XML/project
  payloads, private patch payloads, or binary AEP data where the schema says
  metadata-only.
- Any approval template still has `approval_status=draft_unapproved`.
- Any artifact verification report is missing, not `verified`, or does not
  match the generated JSX digest before standalone export approval.
- Any review packet is missing or not `review_ready` before project-edit
  request-pair approval.

## Standalone JSX Export Chain

This chain is for snapshot-to-JSX export artifacts. It is separate from the
project-edit request-pair approval route.

1. Generate create-new JSX/report/manifest artifacts:

   ```powershell
   cargo run --example export_ae_jsx --no-default-features -- snapshot.json generated.jsx --report export-report.json --manifest export-manifest.json
   ```

2. Verify the actual generated JSX file body against the export manifest:

   ```powershell
   cargo run --example ae_jsx_export_artifact_verify --no-default-features -- export-manifest.json generated.jsx --export-report export-report.json --report artifact-verify-report.json
   ```

   Required status: `verification_status=verified`.

3. Produce a draft approval receipt template:

   ```powershell
   cargo run --example ae_jsx_export_approval --no-default-features -- draft export-manifest.json artifact-verify-report.json approval-template.json
   ```

   The template is not approval.

4. The operator reviews these local files before editing the receipt:

   - `generated.jsx`
   - `export-report.json`
   - `export-manifest.json`
   - `artifact-verify-report.json`
   - `approval-template.json`

5. The operator may fill an approval receipt only after review:

   - set `approval_status=approved`;
   - set receipt id, operator, and UTC approval time;
   - set all required `prerequisite_reviews.*` booleans to true, including
     `artifact_verification_report_reviewed=true`;
   - allow only manual AE launch and manual JSX execution;
   - keep all tool execution and project write permissions false.

6. Validate the filled approval receipt:

   ```powershell
   cargo run --example ae_jsx_export_approval --no-default-features -- validate export-manifest.json artifact-verify-report.json approval-receipt.json approval-validation-report.json
   ```

   Required status: `validation_status=approved`.

7. Produce a not-run closeout template:

   ```powershell
   cargo run --example ae_jsx_export_smoke_closeout --no-default-features -- draft export-manifest.json approval-receipt.json closeout-template.json
   ```

8. Optional manual AE smoke is copy-project-only and operator-only:

   - open AE manually;
   - run only the reviewed `generated.jsx`;
   - save only to the reviewed explicit output path when a save is part of the
     smoke;
   - never overwrite the source project.

9. Validate the operator-filled closeout:

   ```powershell
   cargo run --example ae_jsx_export_smoke_closeout --no-default-features -- validate export-manifest.json approval-receipt.json closeout.json closeout-validation-report.json
   ```

   `accepted` means internal JSON consistency evidence only, not an automated
   AE runtime oracle.

## Project-Edit Request-Pair Chain

This chain is for neutral AE project edit IR requests and generated JSX
transaction artifacts.

Required pre-approval artifacts:

- request-pair manifest from `ae_project_edit_ir`;
- downstream validate report with `status=dry_run_ok`;
- generated JSX report with `status=generated_jsx` and
  `generated_jsx_artifact_digest.generated_jsx_embedded=false`;
- optional generated JSX transaction artifact verification report with
  `verification_status=verified`, produced by hashing the local generated JSX
  file against the generated JSX report;
- optional not-run manual smoke closeout template;
- optional standalone export manifest;
- optional standalone export artifact verification report paired with that
  export manifest;
- optional AEPX dry-run report with `status=dry_run_ok`;
- optional AEPX synthetic preservation proof report with
  `status=synthetic_preservation_proof_ready`;
- optional AEPX production-lane preflight report with
  `status=apply_gate_closed_ready`.

Verify the generated JSX transaction artifact if you want file-body evidence:

```powershell
cargo run --example ae_jsx_transaction_artifact_verify --no-default-features -- generated-report.json generated.jsx --report transaction-artifact-verify.json
```

Validate the review packet:

```powershell
cargo run --example ae_project_edit_review_packet --no-default-features -- --request-pair-manifest request-pair.json --downstream-validate-report validate-report.json --generated-jsx-report generated-report.json --generated-jsx-artifact-verification-report transaction-artifact-verify.json --manual-smoke-closeout-template closeout-template.json --export-artifact-manifest export-manifest.json --export-artifact-verification-report artifact-verify-report.json --aepx-dry-run-report aepx-report.json --aepx-preservation-proof-report proof-report.json --aepx-production-lane-preflight-report aepx-production-preflight.json --report review-packet-report.json
```

Required status: `validation_status=review_ready`.

`review_ready` is pre-approval consistency evidence only. It does not approve AE
launch, JSX execution, project save, direct `.aep` write, source overwrite, or
AEPX XML apply.

After a `review_ready` packet, use the project-edit approval route
(`ae_project_edit_approval`) for request-pair manifests. Do not use standalone
`ae_jsx_export_approval` for request-pair project edits.

If the review packet includes generated JSX transaction artifact verification
evidence, the project-edit approval receipt must acknowledge it via
`generated_jsx_artifact_verification_report_reviewed=true`. This acknowledgement
does not approve execution; it records that the local generated JSX file hash
was reviewed.

If the review packet includes standalone export artifact manifest or artifact
verification evidence, the project-edit approval receipt must acknowledge those
artifacts as reviewed via `export_artifact_manifest_reviewed=true` and
`export_artifact_verification_report_reviewed=true` as applicable. These
acknowledgements do not turn the export manifest or verification report into
approval scope.

If the review packet includes synthetic AEPX preservation proof evidence, the
project-edit approval receipt must acknowledge it via
`aepx_preservation_proof_reviewed=true`. This acknowledgement does not enable
production AEPX XML apply; it only records that the local synthetic preservation
proof was reviewed.

If the review packet includes AEPX production-lane preflight evidence, the
project-edit approval receipt must acknowledge it via
`aepx_production_lane_preflight_reviewed=true`. This acknowledgement does not
enable production AEPX XML apply, real `.aepx` input, or binary `.aep` editing;
it only records that the gate-closed preflight was reviewed.

After a filled project-edit approval receipt is validated as
`validation_status=approved`, validate any operator-filled project-edit manual
smoke closeout with the same request-pair manifest, approval receipt, and
review packet report:

```powershell
cargo run --example ae_manual_smoke_closeout --no-default-features -- --request-pair-manifest request-pair.json --closeout closeout.json --approval-receipt approval-receipt.json --review-packet-report review-packet-report.json --report closeout-validation-report.json
```

Required accepted status for a completed manual smoke is
`validation_status=accepted`. This is operator-reported closeout evidence only,
not automated AE compatibility proof.

## Evidence Rules

- A metadata binding proves only that the reviewed JSON metadata fields have not
  drifted.
- A generated JSX artifact digest proves only that local generated JSX bytes
  matched the corresponding generated report or manifest digest at verification
  time.
- A generated JSX transaction artifact verification report proves only that the
  local generated JSX bytes matched the generated JSX report path, byte count,
  and digest at verification time.
- A standalone export generated JSX artifact digest proves only that the local
  generated JSX bytes matched the export manifest at verification time.
- A review packet proves only that supplied JSON artifacts were internally
  consistent.
- An approval receipt records an operator decision only; it is not tool-side
  execution permission.
- A closeout records operator-reported smoke evidence only; it is not an
  automated AE/render/save oracle.

## Handoff Notes

Next agents can safely extend this runbook by adding new JSON-only preflight
artifacts, schema contract tests, or manual checklist fields. Do not add AE launch, JSX execution, direct `.aep` write, AEX/OFX loading, or AviUtl core actions to this runbook without a separate explicit approval and implementation boundary review.

The synthetic chain test `aviutl-rs/tests/ae_project_edit_e2e_contract.rs` is
useful as a local smoke of the JSON evidence route. Treat it as contract
coverage only; it is not a manual AE smoke result and not proof that After
Effects accepted or saved a project.

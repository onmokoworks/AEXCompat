# AEPX Production Lane Gate - 2026-06-01

Purpose: define the gate between the current synthetic AEPX preservation proof
and any future production `aepx_patch_probe apply` or real `.aepx` input.

## Decision

Keep production AEPX apply closed.

The current repo has useful evidence for a narrow synthetic exact-span writer,
but it does not yet have production evidence for real `.aepx` input, a general
XML writer, AE round-trip acceptance, or private project safety. The next
production lane must be a separate, parent-approved slice; it must not be
quietly unlocked by the synthetic proof.

Current required false values:

- `aepx_patch_probe_apply_enabled=false`
- `real_aepx_input_enabled=false`
- `production_xml_writer_enabled=false`
- `binary_aep_writer_enabled=false`
- `source_overwrite_enabled=false`
- `after_effects_tool_launch_enabled=false`
- `external_process_invocation_in_writer_enabled=false`

## Current Evidence

Already useful:

- `aepx_patch_probe` validates request/report boundaries and keeps apply
  unsupported.
- `aepx_synthetic_preservation_proof` proves exact-span composition rename on
  checked-in synthetic fixtures only.
- Synthetic contract coverage now includes scanner hardening, duplicate id
  rejection, unsafe replacement rejection, all-or-nothing multi-operation
  behavior, report-output policy, canonical parent containment, Windows symlink
  and junction parent escape coverage, UTF-8 non-ASCII, and UTF-8 BOM + CRLF
  preservation.
- Reports stay metadata-only and avoid XML bodies, selectors, expected values,
  edit values, sentinels, and private patch payloads.

Read-only sidecar alignment:

- AEPX reviewer `019e8306-e6e4-7db2-a6fc-fce466eb856d` confirmed the current
  synthetic proof is preservation evidence only. Missing production evidence
  remains production writer code, production path-boundary proof, real `.aepx`
  corpus/review evidence, report/privacy coverage for private payloads,
  dependency/license review, AE manual smoke closeout, and AEX/OFX no-bypass
  proof.
- AEX/OFX reviewer `019e8306-fb4b-7ee2-92a3-ef2ecb4bbb0d` confirmed the parent
  should consume AEX fixture gate, readiness, synthetic fixture, and identity
  smoke metadata only. Native `.aex` load/describe/render and OFX routing stay
  forbidden until separate approvals exist.

Still missing for production:

- no real `.aepx` fixture review receipt;
- no production writer path-boundary contract wired into `aepx_patch_probe`;
- no production exact-byte-diff contract for real input;
- no direct XML dependency license audit for a chosen writer dependency;
- no AE-open/save acceptance evidence;
- no parent approval receipt allowing a write-enabled apply slice.

## First Candidate Scope

The first production candidate, when it exists, should remain the smallest
operation class:

- `rename_comp`;
- exact `xml_id` selector only;
- one target;
- exact `expected_old_value` guard;
- one quoted XML attribute value span;
- XML-attribute-safe replacement value only;
- create-new output only;
- no source overwrite;
- no AE launch;
- no binary `.aep` handling.

Any widening beyond this should require a separate gate.

## Required Green Evidence

Before any production apply code can exist, require all of:

- synthetic preservation proof contract green;
- synthetic fixture matrix covers scanner, encoding, path, and multi-operation
  boundaries;
- production writer path-boundary contract green;
- production writer exact-byte-diff contract green;
- production writer all-or-nothing contract green;
- production report privacy contract green;
- direct XML dependency license audit green, or a recorded decision that no new
  dependency is used;
- local-only real `.aepx` fixture review receipt;
- manual AE smoke protocol reviewed if the slice claims AE acceptance;
- explicit parent approval receipt for a write-enabled apply slice.

Dry-run evidence and synthetic proof evidence are review inputs only. They do
not grant apply permission.

## Hard Forbid List

The first apply slice must not include:

- binary `.aep` parsing or writing;
- source overwrite;
- automated After Effects launch;
- external process invocation inside the writer;
- name-only selector apply;
- ambiguous selector apply;
- text-node edits;
- CDATA edits;
- node insertion or deletion;
- namespace rewriting;
- entity normalization;
- `.aex` loading;
- OFX routing.

## Failure Policy

Any missing gate must fail closed before writing.

Reserved production failure statuses remain:

- `invalid_request`
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

## Report Boundary

Production reports may carry only metadata needed for review and binding.

They must not embed:

- XML bodies;
- private patch payloads;
- selector values;
- expected old values;
- new values;
- sentinel values;
- private project content.

Metadata-only FNV/sha256 binding remains required so review packets can verify
the report without copying private project bodies into analysis docs.

## Next Parent Action

The first safe implementation now exists as a production-lane preflight:

- `aviutl-rs/examples/aepx_production_lane_gate_preflight.rs`
- `aviutl-rs/tests/aepx_production_lane_gate_preflight_contract.rs`
- `analysis/AEPX_PRODUCTION_LANE_GATE_PREFLIGHT_REPORT_SCHEMA_2026-06-01.json`

The preflight reads only the local gate schema and reports
`apply_gate_closed_ready` when the apply gate is still closed and all required
future-evidence requirements are present. It reports `blocked_gate_drift` if the
gate drifts toward enabling apply, real `.aepx` input, XML writing, source
overwrite, AE launch, external writer processes, AEX loading, or OFX routing.
Its report also carries explicit binary `.aep` no-read/no-write gates:
`binary_aep_writer_enabled=false`, `binary_aep_read_performed=false`, and
`binary_aep_write_performed=false`.
The report can be attached to AE project-edit review packets as supplemental
gate-closed evidence. Approval and closeout must acknowledge it when present,
but must still keep `aepx_production_lane_preflight_accepted_as_apply=false`.

This is still not production apply. It is a machine-checkable guard that keeps
the production lane closed until a later parent-approved write-enabled slice
has all required evidence.

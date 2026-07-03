# AEPX Writer Strategy Decision - 2026-06-01

Purpose: decide the next safe AEPX XML-writing strategy after the synthetic
preservation proof artifact, without enabling production `aepx_patch_probe
apply`.

## Decision

Do not add a production AEPX writer yet.

The next implementation lane should harden the existing exact-span strategy on
synthetic fixtures before any real `.aepx` apply path exists. The current
production status remains:

- `production_aepx_apply_enabled=false`
- binary `.aep` parsing/writing deferred
- automated After Effects launch disabled
- source overwrite forbidden
- private project payload reporting forbidden

The first production-candidate operation class is still only a guarded scalar
attribute replacement:

- one exact `xml_id` selector;
- one target;
- exact `expected_old_value` guard;
- replace only one quoted attribute value;
- create-new output only;
- prove the output bytes equal the input bytes outside the approved span.

## Evidence Used

Local artifacts:

- `analysis/AEPX_XML_PRESERVATION_WRITER_SPIKE_2026-06-01.md`
- `analysis/AEPX_SYNTHETIC_PRESERVATION_PROOF_SCHEMA_2026-06-01.json`
- `analysis/AEPX_JSX_PATCH_DEVELOPMENT_HANDOFF_2026-05-31.md`
- `analysis/AEPX_PATCH_REQUEST_SCHEMA_2026-05-31.json`
- `analysis/AEPX_PATCH_REPORT_SCHEMA_2026-06-01.json`
- `analysis/AE_PROJECT_EDIT_MANUAL_SMOKE_PROTOCOL_SCHEMA_2026-06-01.json`
- `analysis/AE_PROJECT_EDIT_MANUAL_SMOKE_CLOSEOUT_REPORT_SCHEMA_2026-06-01.json`
- `analysis/AEPX_JSX_PATCH_LICENSE_NOTES_2026-05-31.md`
- `LEGAL_CLEANROOM.md`
- `PUBLICATION_BOUNDARY.md`
- `aviutl-rs/examples/aepx_synthetic_preservation_proof.rs`
- `aviutl-rs/tests/aepx_synthetic_preservation_proof_contract.rs`
- `aviutl-rs/tests/fixtures/aepx_writer_spike_preservation.aepx`
- `aviutl-rs/tests/fixtures/aepx_writer_spike_ambiguous.aepx`

Read-only sidecar reviews:

- `019e82d9-ab5c-7ff3-9533-5309997af0c6` / Hooke: exact-span operation and
  missing gate review.
- `019e82d9-ac7f-7ea2-9d0e-9e1196a6ff1e` / Descartes: local license and XML
  dependency criteria review.
- `019e82d9-ad0f-7813-86e7-b8f7ea3309a6` / Archimedes: manual AE acceptance
  protocol review.

External public references checked on 2026-06-01:

- `quick-xml` docs.rs latest page:
  https://docs.rs/crate/quick-xml/latest
- `quick-xml` package metadata source:
  https://docs.rs/crate/quick-xml/latest/source/Cargo.toml.orig
- `roxmltree` docs.rs latest page:
  https://docs.rs/crate/roxmltree/latest
- `roxmltree` package metadata source:
  https://docs.rs/crate/roxmltree/latest/source/Cargo.toml.orig
- `xml` docs.rs latest page:
  https://docs.rs/crate/xml/latest
- `xml` package metadata source:
  https://docs.rs/crate/xml/latest/source/Cargo.toml.orig
- `xmltree` package metadata source:
  https://docs.rs/crate/xmltree/latest/source/Cargo.toml.orig
- `xmltree` license file:
  https://docs.rs/crate/xmltree/latest/source/LICENSE

## Candidate Strategy Matrix

| Strategy | Near-term decision | Why |
| --- | --- | --- |
| Exact-span engine, no new XML dependency | Recommended for the next synthetic hardening slice | It is the only route currently proven to preserve untouched XML bytes, and it keeps dependency, license, and whole-document rewrite risk low. |
| `roxmltree` as a read-only validator plus span edits | Evaluate later, not adopted now | Current public docs describe a read-only tree with position support and MIT/Apache-2.0 licensing. It may help target validation, but it does not provide writing and still needs exact license-file and behavior tests. |
| `quick-xml` streaming reader/writer | Evaluate only as parser/tooling evidence, not as a preservation writer yet | Current public docs show an MIT-licensed high-performance reader/writer, but a streaming rewrite is not automatically byte-preserving and would need exact diff proof before AEPX use. |
| `xml` / xml-rs streaming reader/writer | Evaluate only as parser/tooling evidence, not as a preservation writer yet | Current public docs show MIT licensing and streaming reader/writer support, but it still implies generated output rather than untouched-byte preservation. |
| `xmltree` tree model | Not recommended for the first preservation writer | Current public metadata shows MIT licensing, but a tree write model is a whole-document serialization risk until proven otherwise against sentinels. |

Current repo note: `aviutl-rs/Cargo.toml` has no direct AEPX XML dependency.
`quick-xml 0.39.4` appears in `Cargo.lock` through GUI/Wayland target
dependencies, but that transitive presence is not permission to use it for
AEPX preservation.

## Required Gates Before Any Production Apply

The next synthetic hardening slice should add proof for these gates before any
real `.aepx` input is accepted:

- Canonical path gate: output must be an explicit new `.aepx` path, must not
  equal the input after normalization, must reject traversal/root escape, must
  reject existing output, and must avoid source overwrite.
- Exact byte-diff gate: patched output must equal source bytes except the
  approved replacement span. The proof must include XML declaration, encoding,
  comments, CDATA, namespace prefixes, unknown nodes, unknown attributes,
  whitespace, trailing whitespace, and CRLF/LF behavior.
- Scanner hardening gate: tag scanning must handle quoted `>`, single and
  double quotes, attributes in any order, self-closing tags, malformed tags,
  duplicate ids, namespace prefixes, and similarly named false positives.
- Replacement value gate: until escaping policy is implemented, reject new
  values that require XML escaping, contain quotes for the active delimiter,
  contain `<`, contain raw `&`, contain control characters, or are not safe for
  the declared encoding.
- All-or-nothing gate: for any future multi-operation proof, discover all spans
  before writing, reject overlaps, require every old-value guard to pass, and
  apply replacements in reverse byte order.
- Report/privacy gate: reports must omit XML bodies, fixture paths, selector
  values, expected values, new values, sentinels, private media paths, `.ffx`,
  `.aex`, and private patch payloads.
- License/dependency gate: if any XML crate becomes direct, record exact crate
  version, exact license files, transitive dependency licenses, and preservation
  behavior tests before import.
- Manual AE gate: any AE acceptance remains a human-operated smoke closeout
  after review/approval. It is not automated AE compatibility proof and cannot
  convert AEPX dry-run evidence into apply permission.

## Plausible Next Synthetic Operations

Allowed to explore only after the gates above are made stronger:

1. Strengthen `rename_comp` exact `xml_id` proof with byte-diff and scanner
   hardening.
2. Add `rename_layer` only if it is the same scalar quoted-attribute
   replacement class: exact layer `xml_id`, one target, exact old-value guard,
   create-new output, byte-diff proof.
3. Consider other scalar attribute replacements only when each field is proven
   to be one quoted attribute value and the replacement value is XML-safe.

Stay blocked:

- name-only selector fallback;
- ambiguous selectors;
- text-node edits;
- CDATA edits;
- marker insertion/deletion;
- node insertion/deletion;
- namespace rewriting;
- entity normalization;
- binary `.aep` editing;
- source overwrite;
- automated AE launch;
- production `aepx_patch_probe apply`.

## Manual AE Acceptance Boundary

The AE manual-smoke path is a separate human-operated chain:

1. no-write planning artifacts;
2. review packet with metadata-only bindings/digests;
3. approval receipt with matching binding/digest/path evidence;
4. operator-launched After Effects;
5. operator-executed reviewed JSX or operator-opened generated artifact;
6. sanitized closeout report.

This can record that an operator observed an AE smoke result. It does not prove
automated AE compatibility, does not authorize binary `.aep` writes, does not
authorize production AEPX apply, and does not permit source overwrite.

## Next Implementation Slice

Recommended next code slice, still synthetic-only:

- add a byte-diff proof helper to `aepx_synthetic_preservation_proof`;
- add fixtures for quoted `>`, single quotes, duplicate ids, namespace variants,
  CRLF, and non-ASCII;
- add tests for traversal/root escape and non-`.aepx` output rejection;
- add replacement-value rejection tests;
- keep all reports metadata-only;
- keep production apply disabled.

No AviUtl / ExEdit compatibility files should be edited for this lane.

## Implementation Update 125

The first synthetic hardening slice is now implemented in
`aviutl-rs/examples/aepx_synthetic_preservation_proof.rs` and
`aviutl-rs/tests/aepx_synthetic_preservation_proof_contract.rs`.

Implemented gates:

- output request path must have `.aepx` extension;
- output request path must not contain `.` or `..` traversal components;
- output request path must remain under
  `target/aepx-synthetic-preservation-proof`;
- output parent directory must resolve canonically under the same target root
  immediately before create-new writes;
- an opportunistic Windows symlink-parent escape test verifies that a parent
  path which lexically lives under the target root but resolves outside is
  rejected at write time without creating the outside file or echoing escape
  paths in the report;
- an opportunistic Windows junction-parent escape test now exercises the same
  canonical parent write gate with a directory junction created only by the
  test harness; the proof process still reports no external process invocation;
- CLI report output must have `.json` extension, must not contain traversal
  components, and must remain under the same synthetic proof target root;
- CLI report output parent must also resolve canonically under the same target
  root immediately before create-new writes;
- replacement values are rejected unless they are safe for unescaped XML
  attribute span replacement;
- scanner ignores `<xmp:comp` text inside comments and CDATA;
- scanner rejects similarly named tags such as `xmp:composition` and
  `xmp:companion`;
- scanner handles quoted `>` inside attributes and single-quoted attributes;
- duplicate `xml_id` targets fail closed as `ambiguous_target`;
- non-ASCII UTF-8 fixture text is preserved outside the approved replacement
  span without being echoed in reports;
- UTF-8 BOM and CRLF line endings are preserved in a dedicated synthetic
  fixture outside the approved replacement span, without echoing fixture names,
  sentinel values, selectors, or replacement values in reports;
- multi-operation proof resolves all spans before writing, rejects overlapping
  spans, and applies replacements in reverse span order only after every guard
  has passed;
- ready reports now expose a metadata-only `hardening_gate`;
- ready writes verify exact byte equality outside the approved replacement
  span.

Added synthetic fixtures:

- `aviutl-rs/tests/fixtures/aepx_writer_spike_scanner_edges.aepx`
- `aviutl-rs/tests/fixtures/aepx_writer_spike_duplicate_id.aepx`
- `aviutl-rs/tests/fixtures/aepx_writer_spike_unicode_edges.aepx`
- `aviutl-rs/tests/fixtures/aepx_writer_spike_multi_comp.aepx`
- `aviutl-rs/tests/fixtures/aepx_writer_spike_crlf_bom.aepx`

Still deferred:

- reparse-point coverage beyond the opportunistic Windows symlink and junction
  parent tests;
- production `aepx_patch_probe apply`;
- any real `.aepx` input.

## Implementation Update 133

Added a separate production-lane gate artifact:

- `analysis/AEPX_PRODUCTION_LANE_GATE_2026-06-01.md`
- `analysis/AEPX_PRODUCTION_LANE_GATE_SCHEMA_2026-06-01.json`

This gate does not enable production apply. It records the evidence that must
be green before any future write-enabled `aepx_patch_probe apply` or real
`.aepx` input exists:

- synthetic preservation proof contract green;
- synthetic fixture matrix covering scanner, encoding, path, and multi-op
  boundaries;
- production writer path-boundary, exact-byte-diff, all-or-nothing, and report
  privacy contracts;
- direct XML dependency license audit, or a recorded no-new-dependency
  decision;
- local-only real `.aepx` fixture review receipt;
- reviewed manual AE smoke protocol if AE acceptance is claimed;
- explicit parent approval receipt for a write-enabled apply slice.

The gate keeps dry-run and synthetic proof evidence as review inputs only. It
requires `aepx_patch_probe_apply_enabled=false`,
`real_aepx_input_enabled=false`, `production_xml_writer_enabled=false`,
`source_overwrite_enabled=false`, and `after_effects_tool_launch_enabled=false`
until a later parent-approved slice satisfies every required evidence item.

## Implementation Update 134

Added a metadata-only production-lane preflight:

- `aviutl-rs/examples/aepx_production_lane_gate_preflight.rs`
- `aviutl-rs/tests/aepx_production_lane_gate_preflight_contract.rs`
- `analysis/AEPX_PRODUCTION_LANE_GATE_PREFLIGHT_REPORT_SCHEMA_2026-06-01.json`

The preflight reads `analysis/AEPX_PRODUCTION_LANE_GATE_SCHEMA_2026-06-01.json`
and reports `apply_gate_closed_ready` only when:

- all current apply / real-input / writer / source-overwrite / AE-launch /
  external-writer-process flags remain false;
- the first candidate scope stays exact `xml_id` `rename_comp`, one target,
  old-value guarded, create-new only, and no source overwrite;
- every required future-evidence item, fail-closed status, and hard-forbidden
  first-slice item is still present;
- dry-run and synthetic proof evidence are not accepted as apply or AE
  compatibility permission;
- report privacy fields stay metadata-only.

It reports `blocked_gate_drift` if the gate drifts open. It still does not read
XML bodies, accept real `.aepx` input, write AEPX output, launch AE, invoke
external processes, load AEX, route through OFX, or enable production apply.

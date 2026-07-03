# AEPX XML Preservation Writer Spike - 2026-06-01

Purpose: evaluate a tiny, synthetic-only preservation path before any real
`.aepx` writer is enabled in `aepx_patch_probe`.

## Boundary

- Synthetic `.aepx`-like fixtures only.
- No binary `.aep` parsing or writing.
- No After Effects launch.
- No local real `.aepx` payload copying, quoting, or fixture minimization.
- No production `aepx_patch_probe` apply integration in this spike.
- No new XML crate or third-party dependency was added.

## Files

- `aviutl-rs/tests/fixtures/aepx_writer_spike_preservation.aepx`
- `aviutl-rs/tests/fixtures/aepx_writer_spike_ambiguous.aepx`
- `aviutl-rs/tests/aepx_preservation_writer_spike.rs`

## Result

The spike proves one narrow operation on synthetic XML:

- exact-id composition rename;
- exact old-value guard;
- create-new output only;
- source overwrite never performed;
- unknown elements, unknown attributes, XML declaration, UTF-8 declaration,
  comments, CDATA, namespace prefixes, and odd whitespace sentinels remain
  byte-visible after the splice.

The spike also proves fail-closed behavior for:

- ambiguous name-only composition selectors;
- expected-old-value mismatch;
- existing output paths.

## Decision

This is **not** a production XML writer. It is a useful lower bound for a future
writer strategy: a bounded span replacement can preserve untouched XML bytes for
simple exact-attribute edits, but it does not provide a general XML model,
entity handling, text-node editing, namespace rewriting guarantees, or AE
round-trip acceptance.

Next production work should choose between:

1. a reviewed preservation-aware XML library with exact license and behavior
   audit; or
2. a deliberately limited span-edit engine that supports only exact guarded
   edits whose byte ranges can be proven without rewriting the document.

Either route must keep `preserve_unknown_xml=required`, write only to explicit
new `.aepx` paths, refuse existing outputs, and report `preservation_failed`
before writing when the preservation proof is incomplete.

## License Notes

No new dependency was adopted in this spike, so no new license obligation was
introduced. Future XML library candidates still require exact license-file audit
and behavior tests for comments, CDATA, namespace prefixes, whitespace, XML
declaration, and encoding handling before integration.

## Verification

```powershell
cargo test --test aepx_preservation_writer_spike --no-default-features
cargo test --test aepx_patch_contract --no-default-features
```

Passing this spike does not claim real After Effects compatibility. It only
shows that a synthetic, guarded, create-new edit can preserve untouched XML
sentinels without launching AE or writing a private project.

Summary boundary: this is not a production XML writer, not an XML library choice, and not an AE compatibility claim.

## Follow-Up Review Note

A read-only sidecar review recommended that the next AEPX preservation step
promote the synthetic span-preservation proof as a separate proof artifact
before any production `aepx_patch_probe apply` path is enabled.

Recommended next boundary:

- synthetic fixtures only;
- exact `xml_id` and old-value guarded composition rename only;
- create-new output only;
- no AE launch or external process;
- no private `.aepx` payloads;
- report preservation statuses/counts/gates only;
- never echo XML bodies, sentinel text, selectors, expected values, or new
  values.

This keeps the preservation proof useful for review while avoiding an accidental
claim that the general AEPX writer or AE round-trip path is production-ready.

## Synthetic Proof Artifact

The recommended follow-up proof artifact now exists:

- `aviutl-rs/examples/aepx_synthetic_preservation_proof.rs`
- `aviutl-rs/tests/aepx_synthetic_preservation_proof_contract.rs`
- `analysis/AEPX_SYNTHETIC_PRESERVATION_PROOF_SCHEMA_2026-06-01.json`

It promotes the spike into a local reportable contract while keeping the same
synthetic-only boundary. The ready path supports only an exact `xml_id`
composition rename with an old-value guard, writes only a create-new output
under `target/aepx-synthetic-preservation-proof`, preserves the original
fixture, and emits preservation/gate/privacy statuses without embedding XML
bodies, fixture paths, selector values, expected values, new values, or sentinel
text.

This artifact is still not production `aepx_patch_probe apply`, not a general
AEPX writer, not binary `.aep` support, and not AE round-trip validation.

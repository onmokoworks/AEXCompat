# Descriptor Manifest Promotion

`tools/aex_descriptor_manifest_promotion.py` regenerates an AEX parameter
descriptor candidate from a successful isolated L2 report and compares it with
the reviewed manifest. It does not load native code.

The reviewed manifest supplies only curation decisions: stable parameter ids,
assignability, and the expected numeric kind. The tool always regenerates the
plug-in digest, L2 receipt, slot order, observed type, display name, ranges, and
defaults from the worker report.

## Command

```powershell
python tools/aex_descriptor_manifest_promotion.py `
  target/l2-results/scattermap-conditional-selector-policy-20260713.json `
  profiles/scattermap/parameter_descriptors.json `
  target/descriptor-manifest-promotion/scattermap-candidate.json `
  target/descriptor-manifest-promotion/scattermap-comparison.json
```

Both outputs are create-new files. Input and output paths are restricted to
their broker-owned roots.

Exit code `0` means the regenerated candidate exactly matches the reviewed
manifest and their canonical SHA-256 values agree. Exit code `3` means a valid
observation changed or contains a descriptor requiring review. Exit code `2`
means an input, identity, lifecycle result, descriptor, path, or output is
invalid.

Promotion remains a reviewed operation. The tool never overwrites
`profiles/<plugin>/parameter_descriptors.json`; a reviewer must inspect and
explicitly replace the manifest and update its pinned registry digest.

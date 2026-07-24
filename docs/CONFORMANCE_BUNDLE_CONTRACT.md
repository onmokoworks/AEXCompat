# Conformance bundle contract / ???bundle??

??????????English follows each section.

## ?? / Purpose

Issue #4 runner????????fixture???????????manifest?????identity??????????report?depth??selector?world?failure classification?Suite???oracle?????????

This is the minimum fixture and result contract generated and verified by the Issue #4 runner. The manifest pins pre-run identities and execution conditions. The report records selectors, worlds, failure classifications, Suite history, and oracle state for each depth.

## ?? / Rules

- ?artifact?bundle root?????POSIX path?lowercase SHA-256?byte size???????
- validator???????file handle????hash?????Windows??`CreateFileW`?`GetFinalPathNameByHandleW`?POSIX??`dirfd`?`O_NOFOLLOW`??????????handle-based open???????platform?fail-closed???
- ??field???path??directory???backslash path?Windows??????????
- captured oracle?requested depth???????depth?artifact map?????result?expected hash???depth?artifact??????
- `exact: true`?AE oracle?captured?identity????render???raw output?????hash????pixel mismatch?0??????????
- manifest????parameter??premultiplication?color management?linear light?renderer???????
- report?parameter metadata?depth?input/output world?raw artifact?Suite timeline???????
- AEX?????DLL????runner??identity?????????

All artifacts use bundle-relative POSIX paths, lowercase SHA-256, and byte size. The validator hashes the opened file handle itself. It uses `CreateFileW` plus `GetFinalPathNameByHandleW` on Windows and `dirfd` plus `O_NOFOLLOW` on POSIX; unsupported platforms fail closed. Captured oracle artifacts form a depth-keyed map that exactly matches requested depths, and each result binds to the oracle artifact for the same depth. Exact agreement additionally requires matching identities, a successful render, a real raw output, matching hashes, and zero mismatched pixels.

## Files

- `schemas/conformance-manifest.schema.json`: create-new run input
- `schemas/conformance-report.schema.json`: normalized depth results
- `tools/conformance_bundle_validator.py`: schema, cross-document, and artifact validator
- `tests/fixtures/conformance/basic-manifest.json`: valid synthetic example

## One-command runner

Run `python tools/run-conformance-bundle.py --manifest <fixture>/manifest.json --out <new-bundle>`.
The destination must not already exist. A native run copies the pinned AEX, declared DLLs,
input, harness, and the three native workers into the new bundle before dispatch. The report's
`identities.workers` entries bind the exact L2, classic-render, and SmartFX worker bytes used by
the run; validation reopens and hashes those bundle-local artifacts. Adapter-backed tests record
an empty worker list because no native worker is executed. An adapter receives the requested
`--render-path` and must return `parameter_metadata` from its own immutable inspection step;
the runner never derives defaults, types, or ranges from requested assignments. Metadata must
be identical at every requested depth.

Parameter values are limited to the typed sidecar transport: finite JSON numbers, strings,
booleans (encoded as checkbox-compatible 0/1 scalars), or numeric component arrays. Null and
mixed-type arrays are rejected by the manifest schema before any AEX inspection begins.
Current native execution supports disabled color management, no working space, linear light
disabled, and the `AEXCompat CPU`/`software` renderer aliases. Generated bundle paths
(`manifest.json`, `report.json`, and the `diagnostics`, `outputs`, `raw`, `requests`, and
`target` namespaces) are reserved and cannot be used by pinned artifacts.

## Agent-facing harness contract

The native harness exposes its existing CLI surface without requiring an agent to parse
`main.rs` or guess whether a command falls through to the GUI:

```powershell
.\aexcompat-harness.exe --print-cli-contract | ConvertFrom-Json
.\aexcompat-harness.exe --help
```

`--print-cli-contract` writes the versioned `aexcompat.harness-cli-contract` JSON document to
stdout. It lists the inspection, dependency, render, probe, and AEGP routes, their positional
argument shapes, JSON result channel, failure channel, and the explicit GUI fallback for unknown
or malformed arguments. This is command metadata only; it does not load an AEX. Native AEX
execution remains isolated worker execution and is not a security sandbox. For hash-pinned,
redacted evidence, use the conformance bundle runner above.

For LLM, CI, and other headless callers, prefix a command with `--headless`. In that mode,
unknown or malformed arguments never launch the GUI; they write a bounded structured diagnostic
to stderr and exit with code 64. The default no-argument/GUI behavior remains unchanged.

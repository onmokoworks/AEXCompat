# Conformance bundle contract / 互換性bundle契約

日本語を正とします。English follows each section.

## 目的 / Purpose

Issue #4 runnerが生成・検証するfixtureと結果の最小契約です。manifestは実行前のidentityと実行条件を固定し、reportはdepth別のselector、world、failure classification、Suite履歴、oracle状態を記録します。

This is the minimum fixture and result contract generated and verified by the Issue #4 runner. The manifest pins pre-run identities and execution conditions. The report records selectors, worlds, failure classifications, Suite history, and oracle state for each depth.

## 規則 / Rules

- 全artifactはbundle rootからの相対POSIX path、lowercase SHA-256、byte sizeで識別します。
- validatorは実際に開いたfile handleを検証・hash化します。Windowsでは`CreateFileW`と`GetFinalPathNameByHandleW`、POSIXでは`dirfd`と`O_NOFOLLOW`を使用します。安全なhandle-based openを提供できないplatformはfail-closedです。
- 未知field、絶対path、親directory参照、backslash path、Windows予約名を拒否します。
- captured oracleはrequested depthと完全一致するdepth別artifact mapを持ち、各resultのexpected hashを同じdepthのartifactへ結びます。
- `exact: true`はAE oracleがcaptured、identityが一致、render成功、raw outputが存在し、hash一致かつpixel mismatchが0の場合だけ有効です。
- manifestは時間、parameter値、premultiplication、color management、linear light、rendererを固定します。
- reportはparameter metadata、depth別input/output worldとraw artifact、Suite timelineを保持します。
- AEX本体、依存DLL、入力、runnerは別identityとして保持します。

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

The runner returns exit code `0` only when every requested depth is `ok` or the legal SmartFX
`empty_result`. A report containing another classification is still written for diagnosis, but
the default exit code is `3` and `diagnostics/run.json` records `completed_with_failures`. Batch
collectors that intentionally aggregate failed cells must pass `--allow-failures`; that keeps exit
code `0` while retaining the `completed_with_failures` state.

On a nonzero native exit, zero width, height, and row bytes describe no rendered frame
only when the remaining world fields are schema-valid and match the requested depth,
alpha mode, and input bounds. That absent frame is recorded as `world: null`, with
the failure and selector error unchanged. The original stdout is retained under the
existing size limit in `diagnostics/run.json`; its truncation flag must be checked.
Positive worlds are retained; other malformed layouts and zero-size successful outputs
still fail the existing report validator.

Raw checkpoint selection uses the requested transport's `.rgba8`, `.rgba16le`, or
`.rgba32f-le` suffix, with untyped `.raw` retained for legacy adapters only when no
matching typed checkpoint exists. A different-depth checkpoint is never relabeled.
Without an input checkpoint the known input is converted to the declared transport;
deep output uses the harness's preserved raw sidecar, never the PNG preview. Missing
deep output remains `invalid_output`, and malformed matching checkpoints still fail
strict layout validation. All original checkpoints remain under `outputs/<depth>-worlds`.

The result depth describes the host transport, not necessarily the plug-in's dispatch
depth. The original native stdout's `depth_provenance` records advertised support and
planned/actual dispatch pixel bytes; a successful converted transport is not evidence
of native deep support. Check the existing stdout truncation flag before using it.

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

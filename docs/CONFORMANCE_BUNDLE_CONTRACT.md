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

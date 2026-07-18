# Conformance bundle contract / 互換性bundle契約

日本語を正とします。English follows each section.

## 目的

Issue #4のrunnerが生成・検証するfixtureと結果の最小契約です。manifestは実行前のidentityを固定し、reportはdepth別のselector、world、failure classification、oracle状態を記録します。

This is the minimum fixture and result contract for the Issue #4 runner. The manifest pins pre-run identities; the report records selector, world, failure classification, and oracle state for each depth.

## 規則

- すべてのartifactはbundle rootからの相対POSIX path、lowercase SHA-256、byte sizeで識別します。
- 未知field、絶対path、親directory参照、backslash pathを拒否します。
- `exact: true`はAE oracleが`captured`、identityが一致し、比較結果が0 mismatchの場合だけ有効です。
- `not_requested`と`not_captured`は失敗ではありませんが、exact一致の根拠にはできません。
- AEX本体、依存DLL、入力、runnerを別identityとして保持します。

All artifacts use bundle-relative POSIX paths, lowercase SHA-256, and byte size. Unknown fields and unsafe paths are rejected. Exact agreement requires a captured AE oracle, matching identity, and zero mismatched pixels. The AEX, dependency DLLs, input, and runner retain separate identities.

## Files

- `schemas/conformance-manifest.schema.json`: create-new run input
- `schemas/conformance-report.schema.json`: normalized depth results
- `tests/fixtures/conformance/basic-manifest.json`: valid synthetic example

# Issue #17: broker / worker verbosity 制御

作業ログ (時系列追記、観察と仮説を分離)。

## 調査で判明した前提のズレ (観察)

- issue の「worker が読むデバッグ env は `AEXCOMPAT_TEST_ASYNC_CANCEL_GATE` のみ」は不正確。
  worker は Issue #15 で導入済みの `TraceWriter` (`instruments/common/trace_writer.cpp`)
  を通じ `AEX_INSTRUMENT_TRACE_HANDLE` を読み、opt-in で JSONL trace-file を出力する。
  broker 側は `trace_policy.rs` が `AEX_INSTRUMENT_TRACE_DIR` を検証してハンドルを継承させる
  (`windows_process.rs` の `create_trace_file_for_launch`)。end-to-end で配線済み。
- 既に発火している trace イベント: `session_start` / `session_end` /
  `selector_dispatch` / `suite_acquire` / `suite_release`。
- `TraceWriter::world_descriptor` / `callback_invoke` / `error` は API 定義のみで
  **どこからも呼ばれていない (dead API)**。これが留意点の言う
  「trace_writer の production 配線 (別 issue)」の正体。
- trace event schema (`contracts/trace/host_trace_event.schema.json`,
  `tools/trace_contract_validator.py`) は `world_descriptor` / `callback_invoke` /
  `error` / `unimplemented` を既にサポート済み。配線するだけで契約変更は不要。
- `checkout` は schema 未定義の新規 kind。追加すると schema/validator/intake/
  conformance/selftest 全てに波及するため、dead API 配線とは risk 階層が異なる。

## broker/harness に verbosity 制御が無い (観察)

- `broker/crates/broker/Cargo.toml` に tracing/log/env_logger 依存なし。
- 可観測出力は致命エラー時の `eprintln!` のみ。RUST_LOG 相当が存在しない。
  これが #17 の中核ギャップ (依存ゼロで未着手)。

## 決定したスコープ (ユーザ確認済み 2026-07-20)

1. broker/harness に `tracing` + `tracing-subscriber`(env-filter) を導入。
   - `AEXCOMPAT_LOG` を優先、無ければ `RUST_LOG`、どちらも無ければ既定 `warn`。
   - 出力先は **stderr** (stdout は JSON レポート専用チャネルのため厳守)。
   - 既定レベルでは新規出力ゼロ (instrumentation は debug/trace/info、既定 warn で隠れる)。
     既存 `eprintln!` (CLI usage / 致命エラー) は据え置き、出力量を現状維持。
   - init は idempotent (`try_init`、複数呼び出し・テスト安全)。
2. worker: dead API (`world_descriptor` / `callback_invoke` / `error`/`unimplemented`)
   を choke point に配線。schema 済みのため契約変更不要。
   - verbosity level env `AEX_INSTRUMENT_TRACE_VERBOSE` を追加。
     high-frequency な `callback_invoke` は verbose(=1) 時のみ発火させ、
     既定は 4096 イベント上限を食い潰さない低頻度イベントに限定。
   - env は子プロセスへ自然継承される (`child_environment` の filter 対象外)。
3. `checkout` の新規 schema kind 化は本 PR に含めない。契約全体への波及が大きく、
   dead API 配線 (= 留意点の「未配線」) とは別物。必要なら別 issue で切り出す。

## 配線 choke point (観察)

- checkout: `worker_param_checkout_runtime.cpp` `checkout_param`
  (ただし checkout は schema 未対応のため本 PR では見送り)。
- world: 描画確定点 (`worker_render_report.cpp` 等)。要精査。
- callback: `worker_handle_runtime.cpp` の `callback:*` stderr マーカー地点。
- error/unimplemented: worker のエラー/未実装レポート地点。要精査。

## 検証

- Rust: `cargo test --manifest-path broker/Cargo.toml --workspace`。
- worker 再ビルド後: instruments selftest + `uv run python -m pytest -q`、broker 統合テスト。

## 実装ログ (時系列追記)

### broker/harness (Rust) — 完了

- `tracing` + `tracing-subscriber`(env-filter) を workspace に追加。
- `observability.rs`: `resolve_directives` (AEXCOMPAT_LOG→RUST_LOG→warn の優先解決、
  単体テスト可能) と idempotent な `init()` (stderr 出力、`try_init`)。5 unit test 通過。
- broker main / harness main の先頭で `init()`。
- instrumentation: `windows_process.rs` の launch(worker basename・pid・feature flags)/
  exit(classification・exit_code・truncation) を `debug!`、truncation を `warn!`。
  harness の effect matrix 開始点を `debug!`。private absolute path は出さず basename のみ。
- `cargo test --workspace` 全 29 バイナリ通過、clippy クリーン。

### worker (C++) — 完了

- 観察: worker 詳細ログは Issue #15 の `TraceWriter`(trace-file) が既に担い、
  ユーザ選択の「trace-file 経路」は既存インフラ。よって新規 stderr channel は作らず、
  dead API を実 worker choke point に配線する方針に確定。
- 逸脱理由付き決定: `callback_invoke` の verbose gate は **TraceWriter 内部ではなく
  worker 呼び出し側**に置いた。TraceWriter の emit を変えると
  `test_production_worker_trace.py::test_native_writer_keeps_bounded_and_ordered_jsonl`
  (callback_invoke=513 を固定する契約) が壊れるため。TraceWriter には副作用のない
  `verbose()` アクセサ (`AEX_INSTRUMENT_TRACE_VERBOSE`) のみ追加。
- 配線 choke point (3点、いずれも schema 済み kind で契約変更不要):
  - `error`/`unimplemented` → `worker_suite_registry.cpp::reject_unknown` (未知 suite =
    未実装 host capability)。常時 (低頻度)。
  - `world_descriptor` → `worker_world_registry.cpp::new_world` (extern g_trace_writer)。
    verbose 時のみ。pixel format は contract 文字列へマッピング。
  - `callback_invoke` → `worker_handle_runtime.cpp` の new_handle/lock_handle。verbose 時のみ。
- 3 worker (l2/render/smart) + instruments selftest すべて MSVC ビルド成功。
- source-text テスト `Issue17VerbosityWiringTests` を `test_production_worker_trace.py` に追加 (5件通過)。
  既存 handle ベース動的テスト (513 callbacks) も通過 = TraceWriter 契約は不変。

### 別 issue へ切り出した発見

- #245: `test_instruments_trace_writer.py` の selftest 動的テストが stale
  (DIR/HANDLE 不整合・期待イベント数 7 と実 513 の乖離)。selftest exe を
  ビルドした時のみ露見。標準 CI (instruments 未ビルド) では skip。#17 では直さない。

### ローカル full pytest の既存 fail (私の変更由来ではない)

- 6件: `cp932` UnicodeDecodeError (日本語 Windows locale でソース `.read_text()`、
  encoding 未指定。私が触っていない `image_render.rs` 等)。UTF-8 locale の CI では通る。
- 2 error: `CMake Error: Could not create named generator "Visual Studio 18 2026"`
  (テスト build スクリプトが未対応 VS 世代を要求)。
- 1件: #245 の stale test。

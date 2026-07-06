# AEXCompat 実装ロードマップ(2026-07-06)

本文書は `docs/PROJECT_DESIGN_2026-07-03.md` の設計を、**LLM 実装エージェント
(Codex 等)が1タスクずつ直接実装できる粒度**に分解した正式ロードマップである。
設計判断の背景は設計書を参照。本文書は「何を・どの順で・どう検証して作るか」
だけを規定する。

---

## 0. 実装エージェントへの共通指示

1. **1タスク=1コミット**。タスクをまたいで編集しない。着手前と完了後に
   `python -m unittest discover -s tests` を実行し、全緑を確認してからコミットする。
2. **CLAUDE.md の Default Safety Rules が本文書より優先される。**
   本文書のどの記述も、`.aex` のオープン/ハッシュ/コピー/ロード、
   `EffectMain`/PF セレクタ/AE/OFX 呼び出し、`.aepx`/`.aep` 書き込み、
   `imports/` 編集、既存 `target/` 成果物の上書きを許可しない。
3. **🔒 マークのタスクは、§8 の Safety Gate 全項目が成立するまで着手禁止。**
   ゲート成立は人間が明示宣言する。エージェントが自己判断で開けてはならない。
4. 仕様に曖昧さを見つけたら、既存の類似ツール(例: `tools/aex_fixture_*`)の
   実装パターンに合わせる。既存 68+ ツールの規約が正である。
5. Python は**標準ライブラリのみ**(タスクが明示的に許可しない限り)。
6. 各タスクの「完了条件」はすべて機械検証可能に書いてある。満たせない場合は
   実装を止めて、満たせない理由をコミットせずに報告する。

### 共通実装規約(全 Python ツール)

- CLI: `argparse`、必須 `--out`、入力は明示フラグのみ(位置引数でパスを受けない)。
- 出力: `target/<tool-owned-subdir>/` 配下 create-new only。既存ファイルが
  あれば上書きせずエラー。ファイル名は `ae-<kind>-<epoch-ms>.local.json` 形式。
- パス検証: トラバーサル成分(`..`)拒否、tool-owned root 外への書き込み拒否
  (`tools/aex_load_gate_check.py` の `resolve_under_root` パターンを踏襲)。
- レポート必須フィールド: `schema_version`(int)、`report_kind`(str)、
  `generated_utc`(ISO 8601)、`local_only: true`、および「やらないこと」を
  明示する safety フラグ群(例: `native_load_performed: false`)。
- 成果物に絶対パス・生ペイロード・バイナリ断片・ユーザー名を書かない。
- fail-closed: 入力欠落・スキーマ不一致・矛盾は、部分成功ではなく
  非ゼロ exit + 明確なエラーで停止する。
- テスト: `tests/test_<tool_name>.py`。ポジティブ系と**同数以上のネガティブ系**
  (拒否すること自体のテスト)を必須とする。`tempfile` で隔離し、リポジトリの
  実 `target/` を汚さない。

### コミットメッセージ規約

`<task-id>: <一行要約>` (例: `A-1: Add fixture provenance answer intake tool`)。
本文にテスト結果(`Ran NNN tests ... OK`)を1行記録する。

---

## 1. 現在地(2026-07-06 時点)

完了済み(コミット `60314aa`, `61c8472`):

- 初回ベースラインコミット、`CLAUDE.md`、`.gitignore`(Python bytecode 除外)。
- `contracts/aex/` へ 4 契約昇格済み + `contracts/PROVENANCE.md`
  (image_probe_request / worker_capability_report / loader_readiness_gate /
  image_probe_allowlist.example)。
- `tools/contract_schema_validator.py`(warning-first)+
  `tests/test_contract_schema_validator.py` + `tests/test_contracts_provenance.py`。
- テストスイート: **294 tests, OK**。

未着手: labctl、provenance intake、trace 基盤(contracts/trace 以下すべて)、
`pipelines/`、`broker/`、`instruments/`、`minihost/`、`worker/`。

readiness 状態: 「manual fixture review」1件 pending のまま。native load gate、
実レンダー、OFX ルート、AEPX 書き込みはすべて intentionally closed。

---

## 2. フェーズ構成と依存グラフ

- **Phase A**(ゲート不要・Python のみ・即着手可): A-1 〜 A-8
- **Phase B**(ゲート不要・ネイティブ**ビルド**のみ、実行/ロードなし): B-1 〜 B-3
- **Phase H**(人間の手動作業。LLM は実装しない。ブロッカーとして記載): H-1 〜 H-4
- **Phase D**(🔒 Safety Gate 成立後のみ): D-1 〜 D-4

```
A-1 ──────────────► H-1(人間: provenance 回答)──┐
A-7 ─► A-4 ─► A-5 ─► A-6                          │
              │        └──► (D-4 適合比較)         ├─► §8 ゲート判定 ─► D-1 ─► D-2 ─► D-3 ─► D-4
A-8 ──────────┼───────────► (D-3/D-4 画素比較)     │
A-2, A-3 (独立)│                                   │
B-1 ──────────┴───────────────────────────────────┤
H-2(SDK入手)─► B-2 ─► B-3 ─► H-4(人間: AEでトレース採取)
H-3(cleanroom 決定文書)───────────────────────────┘
```

推奨実行順(直列): **A-1 → A-7 → A-4 → A-5 → A-6 → A-8 → A-2 → A-3 → B-1 →
(H-2 完了後)B-2 → B-3**。H 系は人間作業のため並行。D 系はゲート後。

---

## 3. Phase A: no-load Python 基盤(ゲート不要)

### A-1 fixture provenance 回答 intake ツール

- **状態**: 未着手 / **依存**: なし / **ゲート**: 不要
- **目的**: readiness 唯一の pending「manual fixture review」を閉じるための、
  人間回答 JSON の検証器。これが無いため 2026-06 からチェーンが停止している。
- **読むファイル**:
  `tools/aex_fixture_provenance_answer_template.py`(テンプレート構造)、
  `tools/aex_fixture_provenance_answer_validator_selftest.py`
  (**合成 accept/reject ルールの正**。この 8 ケースのルールを実回答に適用する)、
  `tests/test_aex_fixture_provenance_answer_validator_selftest.py`。
- **作成ファイル**:
  `tools/aex_fixture_provenance_answer_intake.py`、
  `tests/test_aex_fixture_provenance_answer_intake.py`。
- **実装仕様**:
  - CLI: `--answer-template <json>`(既存テンプレート成果物)、
    `--answers <json>`(人間が書いた回答ファイル)、`--out <json>`。
    出力 root: `target/fixture-provenance-answer-intake/`。
  - 回答ファイル要件: テンプレートの全 question id(8問)に対し 1:1 で
    非空文字列回答が存在。余分なキー・欠落・空文字・非文字列は reject。
  - 承認系フィールド(`approve*`, `approval*`, `token` 等のキー名)が回答
    ファイルに含まれていたら reject(intake は検証のみで、承認効果を持たない)。
  - 回答値に Windows 絶対パス(`re: ^[A-Za-z]:\\` を含む部分文字列)があれば
    reject(成果物への私有パス混入防止)。
  - 出力レポート: `report_kind: "fixture_provenance_answer_intake"`、
    `intake_state: "accepted_for_manual_review" | "rejected"`、
    `question_count`, `answered_count`, `rejection_reasons: []`、
    safety フラグ(`approval_manifest_created: false`,
    `fixture_approval_satisfied: false`, `native_load_gate: "closed"`,
    `accepted_aex_path: null`)。**回答本文はレポートに転記しない**
    (回答の長さ・充足のみ記録)。
- **禁止事項**: 承認 manifest 生成、承認トークンの保存/照合、AEX への一切の
  アクセス、回答値のレポートへのエコー。
- **完了条件**: 正しい 8 回答で `accepted_for_manual_review`、下記ネガティブ系が
  すべて非ゼロ exit または `rejected`。テスト全緑。
- **テストケース(最低)**: accept 1 / reject 6(欠落・空回答・余剰キー・
  承認キー混入・絶対パス混入・テンプレート不整合)/ 出力上書き拒否 1。

### A-2 labctl パイプラインランナー

- **状態**: 未着手 / **依存**: なし / **ゲート**: 不要
- **目的**: README に生コマンド列として散在する成果物チェーンの配線を宣言化し、
  以後のフェーズの再実行コストを固定する。
- **作成ファイル**: `tools/labctl.py`、`pipelines/no_load_chain.json`、
  `tests/test_labctl.py`。
- **実装仕様**:
  - パイプライン manifest(JSON): `{"pipeline_name": str,
    "schema_version": 1, "stages": [{"stage_id": str, "tool": str,
    "args": [str, ...], "inputs": {argname: "artifact:<kind>" | "literal:<value>"},
    "output_root": str}]}`。
  - サブコマンド: `list`(manifest のステージ一覧表示)、
    `dry-run`(解決済みコマンド列を表示のみ。**プロセスを一切起動しない**)、
    `run`(直列実行。1ステージ失敗で即停止、以降実行しない)。
  - `tool` は `tools/` 直下のファイル名のみ許可(パス区切り・`..` を含む値は
    拒否)。実行は `subprocess.run([sys.executable, tools/<tool>, ...])`、
    `shell=True` 禁止。
  - `artifact:<kind>` の解決: `--artifact <kind>=<path>` の明示指定を最優先。
    未指定なら `target/artifact-index/` の最新 index から kind で引く。
    解決不能なら fail-closed。
  - `pipelines/no_load_chain.json` の初版は、README「Latest canonical ...」
    チェーンのうち再実行安全な区間(artifact-index → readiness-matrix)から
    開始してよい(全 69 ツールの網羅は求めない。後続タスクで漸増)。
- **禁止事項**: `tools/` 外の実行、並列実行、失敗ステージのスキップ継続、
  既存成果物の上書き。
- **完了条件**: `dry-run` がコマンド列を決定的に出力。`run` がテスト内の
  フェイクツール2段構成で成功し、1段目失敗時に2段目が実行されないことを検証。
- **テストケース**: dry-run 出力スナップショット / run 成功 / run 途中失敗停止 /
  tool 名にパス区切り含む manifest の拒否 / artifact 解決不能時の fail-closed。

### A-3 AEPX エッジケース fixture の編入

- **状態**: 未着手 / **依存**: なし / **ゲート**: 不要
- **目的**: imports 凍結資産のうち Python 側に対応物がない AEPX writer spike
  fixture(BOM/CRLF/Unicode/重複ID 等)を round-trip 検証のテスト素材に昇格。
- **読むファイル**: `imports/aviutlas-rust-contracts/aviutl-rs/tests/fixtures/
  aepx_writer_spike_*.aepx`(7本)+ `aepx_preservation_sentinels.aepx`。
- **作成ファイル**: `tests/fixtures/aepx/`(コピー先)、
  `tests/test_aepx_edge_fixtures.py`、`contracts/PROVENANCE.md` に対応表追記。
- **実装仕様**:
  - **バイト同一コピーのみ**(生成・整形・改行変換をしない)。テスト内で
    コピー元とのバイト同一性を検証する(imports は読み取りのみ)。
  - CLAUDE.md の「.aepx を書かない」規則との関係: 本タスクは AE プロジェクトの
    **生成・編集ではなく、凍結済みテスト fixture の byte-identical 複製**である。
    この例外根拠を `tests/fixtures/aepx/README.md` に1段落で明記する。
  - 各 fixture について `tools/aepx_static_probe.py` 相当の解析関数を
    (subprocess または import で)実行し、「parse 成功/明確な拒否」の期待値を
    テストに固定する。round-trip validator の in-memory 比較も、probe が
    成功する fixture に対して実施する。
- **禁止事項**: fixture の内容変更、imports 側の変更、`.aepx` の新規生成。
- **完了条件**: 8 fixture すべてに決定的な期待結果(成功 or 拒否理由)が
  テストで固定され、バイト同一性チェックが green。

### A-4 trace 契約スキーマ(Host Behavior Oracle の土台)

- **状態**: 未着手 / **依存**: なし(A-7 を先にやると検証が楽) / **ゲート**: 不要
- **目的**: 「本物の AE の挙動トレース」と「将来の minihost の挙動トレース」を
  同一スキーマで表現し、適合比較(A-6)を機械化する契約の制定。
- **作成ファイル**:
  `contracts/trace/host_trace_event.schema.json`、
  `contracts/trace/host_trace_session.schema.json`、
  `contracts/trace/conformance_rules.json`、
  `contracts/trace/examples/synthetic_session.jsonl`(合成例。実測値を含まない)、
  `tests/test_trace_contracts.py`、`contracts/PROVENANCE.md` 追記
  (由来 = `docs/PROJECT_DESIGN_2026-07-03.md` §Host Behavior Oracle。
  imports 由来ではないことを明記)。
- **実装仕様(event スキーマの必須フィールド)**:
  - `schema_version: 1`、`event_index`(0起点の単調増加 int)、
    `event_kind`: enum `["session_start", "selector_dispatch", "suite_acquire",
    "suite_release", "callback_invoke", "world_descriptor", "error",
    "unimplemented", "session_end"]`。
  - `host_kind`: enum `["after_effects_manual", "minihost"]`、
    `host_version_label`(str。例 `"AE 24.x"` / `"minihost 0.1"`)。
  - `plugin_label`(str。**ファイル名 stem のみ**。絶対パス正規表現に一致する
    値はスキーマ違反)。
  - kind 別ペイロード: `selector`(str 名称)、`suite: {name, version, granted}`、
    `world: {width, height, rowbytes, pixel_format}`(数値と enum のみ)、
    `error: {code_label, message}`(message に絶対パス禁止)。
  - 禁止フィールド(存在したら違反): `raw_payload`, `binary_payload`,
    `pixels`, `pointer`, ドライブレター絶対パスを含む任意の文字列値。
  - session スキーマ: `session_id`(uuid)、`event_count`、`trace_complete`
    (session_start/end が揃い index が連続)。
- **実装仕様(conformance_rules.json)**: event_kind × フィールド単位で
  `"must_match" | "should_match" | "informational"` を割り当てる表。初版は
  `selector_dispatch.selector` の**順序列**を must_match、`suite_acquire.name`
  の集合を must_match、`world.*` 数値を should_match、他を informational。
- **完了条件**: `tools/contract_schema_validator.py` が contracts/trace を
  警告ゼロで通過。合成例 JSONL の全行がスキーマ適合。禁止フィールド入り
  イベントの拒否テストが green。

### A-5 AE トレース intake ツール

- **状態**: 未着手 / **依存**: A-4 / **ゲート**: 不要
- **目的**: 人間が(将来 H-4 で)手動採取する AE トレースを、検証・redaction
  してからでないとラボに入れない入口を先に固定する。
- **作成ファイル**: `tools/ae_trace_intake.py`、`tests/test_ae_trace_intake.py`。
- **実装仕様**:
  - CLI: `--raw-trace <jsonl>`、`--out <json>`(intake レポート)、
    `--sanitized-out <jsonl>`(`target/ae-trace-corpus/` 配下 create-new)。
  - 各行を A-4 の event スキーマで検証。1行でも違反があれば既定は全体 reject。
  - `--redact` 指定時のみ: 文字列値中のドライブレター絶対パスを
    `<redacted-path>` に置換し、置換件数をレポートに記録して受理する。
    それ以外の違反(禁止フィールド等)は redact では救済しない。
  - intake レポート: `report_kind: "ae_trace_intake"`、`line_count`,
    `accepted`, `redaction_count`, `rejection_reasons`, safety フラグ
    (`ae_invoked: false` — 本ツール自身は AE を起動しない、を明記)。
- **禁止事項**: AE の起動、トレースの自動採取、raw トレースの `target/` への
  無検証コピー。
- **完了条件**: 合成の正常トレース受理 / 絶対パス入り(--redact なし)拒否 /
  --redact で置換受理+件数一致 / 禁止フィールド行は --redact でも拒否 /
  create-new 違反拒否、が全て green。

### A-6 trace normalizer + conformance diff

- **状態**: 未着手 / **依存**: A-4(A-5 とは独立) / **ゲート**: 不要
- **目的**: 「AE 実測トレース vs minihost トレース」の機械比較器。実トレースが
  存在しない現段階では合成トレースで完成させ、D フェーズで実データを流すだけに
  しておく。
- **作成ファイル**: `tools/trace_normalizer.py`、
  `tools/trace_conformance_diff.py`、`tests/test_trace_normalizer.py`、
  `tests/test_trace_conformance_diff.py`。
- **実装仕様**:
  - normalizer: 入力 JSONL → volatile フィールド(タイムスタンプ、session_id)
    を除去し、event_index を 0 起点に再採番した正規形 JSON を
    `target/trace-normalized/` に出力。決定的(同入力→バイト同一出力)。
  - diff: `--reference <normalized>`(AE 側)、`--candidate <normalized>`
    (minihost 側)、`--rules contracts/trace/conformance_rules.json`、`--out`。
    出力: `report_kind: "trace_conformance"`、
    `must_match_failures: [{rule, reference_summary, candidate_summary}]`、
    `should_match_mismatches: [...]`、`informational_diff_count`、
    `conformance_state: "conformant" | "nonconformant"`(must_match 失敗が
    1件でもあれば nonconformant)。
  - selector 順序比較は列の完全一致。suite 集合比較は名称+バージョンの集合差。
- **完了条件**: 合成トレース対で conformant / 順序入替で nonconformant /
  suite 欠落で nonconformant / rules 不在で fail-closed、が green。決定性
  (2回実行でレポートの比較対象部分が一致)テストが green。

### A-7 contract_schema_validator の strict モード

- **状態**: 未着手 / **依存**: なし / **ゲート**: 不要
- **目的**: 現行 warning-first(CLAUDE.md 記載の方針)を維持したまま、
  CI/ゲート用途の fail-closed 実行形態を追加する。
- **編集ファイル**: `tools/contract_schema_validator.py`(追記)、
  `tests/test_contract_schema_validator.py`(ケース追加)。
- **実装仕様**: `--strict` フラグ追加。strict 時は warning を error に昇格し
  非ゼロ exit。既定動作は現状維持(後方互換)。レポート出力に
  `strict_mode: bool` を追加。
- **完了条件**: 既存テスト全緑のまま、strict での違反検出 exit code テストが
  追加で green。

### A-8 画素比較オラクル(合成入力で先行完成)

- **状態**: 未着手 / **依存**: なし / **ゲート**: 不要
- **目的**: 将来の「minihost レンダー出力 vs 人間が AE で書き出した参照
  フレーム」比較器。実データ到着(D-3)前に、既存 PPM fixture 基盤の合成
  データで完成させる。
- **読むファイル**: `tools/ppm_fixture_tool.py`(PPM 読み書きの既存規約)、
  `tools/aex_image_fixture_validation.py`。
- **作成ファイル**: `tools/aex_compat_oracle.py`、
  `tests/test_aex_compat_oracle.py`、
  `contracts/aex/compat_oracle_report.schema.json`(+ PROVENANCE 追記)。
- **実装仕様**:
  - CLI: `--reference <ppm>`、`--candidate <ppm>`、`--tolerance <int 0-255>`
    (既定 0)、`--out`。出力 root: `target/compat-oracle/`。
  - 検証: 寸法一致必須(不一致は比較せず nonmatching で確定)。画素ごとの
    チャンネル絶対差の最大値・平均値・許容超過画素数を計測。
  - レポート: `report_kind: "compat_oracle"`、`match_state:
    "identical" | "within_tolerance" | "nonmatching"`、`max_channel_delta`,
    `mean_channel_delta`, `exceeding_pixel_count`, `tolerance`、
    safety フラグ(`render_performed: false` — 本ツールは比較のみ)。
    **画素値そのものはレポートに書かない**(統計量のみ)。
  - 入力は `target/` 配下または `tests/` fixture の PPM のみ受理
    (`.aex` パス等は拒否)。
- **完了条件**: identity 比較 identical / invert 比較 nonmatching /
  tolerance 内の微差 within_tolerance / 寸法不一致 nonmatching /
  非 PPM 入力拒否、が green。

---

## 4. Phase B: ネイティブビルド(実行なし・ゲート不要)

> Phase B は「コンパイルとプロセス隔離の実証」まで。**AEX ファイルには
> 一切触れない。** B 系のどのバイナリも `.aex` パスを受け取るコードを
> 持ってはならない(ガードテストで機械強制する)。

### B-1 Rust sandbox broker + ダミーワーカー

- **状態**: 未着手 / **依存**: なし(参照実装として
  `imports/aviutlas-rust-contracts/aviutl-rs/examples/` を読むこと) /
  **ゲート**: 不要
- **目的**: Job Object / ハンドル非継承 / タイムアウト / クラッシュ分類という
  プロセス隔離基盤を、AEX と無関係な安全ペイロードで完成・実証する。
  §8 ゲート条件 G-4 の証拠を作るタスク。
- **作成ファイル**:
  - `broker/Cargo.toml`(workspace)、`broker/crates/broker/`(lib+bin)、
    `broker/crates/dummy-workers/`(bin 3種: `dummy_exit0`, `dummy_sleep`,
    `dummy_abort`)。
  - `contracts/broker/broker_selftest_report.schema.json`(+ PROVENANCE 追記)。
  - `tests/test_native_code_guards.py`(Python 側ガード)。
- **実装仕様**:
  - 依存 crate は `windows-sys`(または `windows`)のみ許可。
  - broker 機能: (1) 子プロセスを Job Object
    (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`)配下で起動、(2) 継承ハンドルは
    明示リスト方式(`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`)。テスト用 sentinel
    ハンドルが子に**継承されない**ことを検証する selftest を含む、
    (3) wall-clock タイムアウトで kill(Job ごと)、(4) exit 分類 enum:
    `ok / nonzero_exit / timeout_killed / crashed`(例外系 exit code
    0xC0000000 台を crashed に分類)、(5) stdout/stderr をサイズ上限つきで
    捕捉し、ドライブレター絶対パスを `<redacted-path>` に置換してから
    レポートへ、(6) `broker selftest --out <json>` サブコマンドが 5 シナリオ
    (正常終了/タイムアウト/クラッシュ/ハング kill/sentinel 非継承)を
    ダミーワーカーで実行し、`contracts/broker/` スキーマ準拠のレポートを
    create-new 出力する。
  - レポート safety フラグ: `accepts_aex_path: false`,
    `dll_load_performed: false`, `native_load_enabled: false`。
- **禁止事項**: `LoadLibrary` 系 API の呼び出し・シンボル参照、`.aex` という
  文字列/拡張子の受理コード、ネットワーク API、`shell` 経由起動。
- **`tests/test_native_code_guards.py` の仕様**(Python unittest):
  - `broker/` 配下の全 `.rs` ソースに `LoadLibrary` / `GetProcAddress` /
    `.aex` が出現しないことを assert。
  - `minihost/` が存在する場合(D フェーズ以降)、その全ソースが
    `instruments/` および `AE_SDK` パスを include しないことを assert
    (cleanroom 境界の機械強制。存在しない間は skip)。
- **完了条件**: `cargo test`(broker ディレクトリ)で 5 シナリオ green。
  `python -m unittest` 全緑(ガードテスト含む)。selftest レポートが
  `contract_schema_validator.py --strict` を通過。
- **注記**: Rust toolchain が環境に無い場合、`cargo` 系テストは
  skip 可能な構成にし(環境変数 `AEXCOMPAT_HAS_CARGO`)、skip した事実を
  レポートすること。ガードテスト(ソース文字列検査)は toolchain 不要で
  常時実行。

### B-2 instruments 共通トレースライブラリ + pf-null-echo(ビルドのみ)

- **状態**: 未着手 / **依存**: A-4(トレーススキーマ)、**H-2(AE SDK 入手)
  が無い場合はビルドを skip する構成にする** / **ゲート**: 不要
  (自作ソースのコンパイルは安全規則の対象外。**AE での実行は H-4 の人間作業**)
- **目的**: 本物の AE 内でホスト挙動を記録する計装プラグイン群の1本目。
  同時に、将来の minihost にとって最良の(由来が自明な)fixture になる。
- **作成ファイル**:
  - `instruments/common/trace_writer.hpp`(+ `.cpp`):SDK 非依存の JSONL
    トレースライタ。A-4 スキーマのイベントを書く。出力先はビルド時ではなく
    実行時に AE 側から人間が与える(環境変数 `AEX_INSTRUMENT_TRACE_DIR`。
    未設定時は**何も書かない**)。
  - `instruments/common/trace_writer_selftest/`(小さな exe):SDK なしで
    trace_writer を叩き、A-5 intake が受理する JSONL を吐けることを検証。
  - `instruments/pf-null-echo/`(プラグインソース + CMakeLists)。
  - `instruments/README.md`(ビルド手順、`AE_SDK_ROOT` 環境変数、
    「このディレクトリのみ SDK include 可」の cleanroom 境界宣言)。
  - `tests/test_instruments_trace_writer.py`(selftest exe が存在する場合のみ
    実行し、出力 JSONL を A-4 スキーマで検証。exe 不在なら skip)。
- **実装仕様(pf-null-echo)**: classic PF effect。PARAMS_SETUP はパラメータ
  0個、RENDER は入力 world を出力 world へ恒等コピー。全セレクタ受信を
  trace_writer で記録。
- **禁止事項**: `instruments/` 外からの SDK include、トレースの
  ネットワーク送出、AE の自動起動、ビルド済み `.aex` のリポジトリへの
  コミット(バイナリはコミットしない。`.gitignore` 済み拡張子)。
- **完了条件**: SDK 不在環境で: trace_writer selftest がビルド・実行でき、
  出力が A-5 intake を通過。SDK 在環境で: pf-null-echo がビルド成功
  (人間が確認)。Python テスト全緑。

### B-3 pf-callback-tracer + pf-crashkit(ビルドのみ)

- **状態**: 未着手 / **依存**: B-2 / **ゲート**: 不要(同上)
- **作成ファイル**: `instruments/pf-callback-tracer/`、
  `instruments/pf-crashkit/`、README 追記。
- **実装仕様**:
  - callback-tracer: 全セレクタ到着を順序つきで記録。`PF_InData` の
    数値フィールド(バージョン、time 系、quality、extent_hint 等)を
    `world_descriptor` / `callback_invoke` イベントに記録。既知 PICA
    スイート名リストへの `AcquireSuite` 試行結果を `suite_acquire` として
    記録(suite-census 機能を統合してよい)。
  - crashkit: パラメータ(popup)で故障モード選択:
    `none / crash(nullptr write) / hang(無限ループ) / bigalloc / pf_error 返却`。
    既定は `none`。**AE 内で人間が明示選択しない限り無害**であること。
  - どちらも文字列値に絶対パスを書かない(trace_writer 側で二重に検査)。
- **完了条件**: SDK 在環境でビルド成功(人間確認)。trace_writer 経由の
  出力形式テスト(合成呼び出し)green。param-zoo / world-inspector は
  D フェーズ前の追加タスクとして任意(同パターンで増設)。

---

## 5. Phase H: 人間の手動作業(LLM 実装対象外・ブロッカー)

LLM エージェントはこれらを**実行しない**。状態確認と、完了成果物の検証
(A-1 / A-5 のツールで)だけを行う。

- **H-1 provenance 回答の記入**: 人間が `AEPluginBuild\ScatterMap.aex`
  について 8 問に回答した JSON を書き、A-1 intake → 既存
  `tools/aex_fixture_decision.py`(明示フラグ+トークン)で承認 manifest を
  作る。→ §8 G-1。
- **H-2 AE SDK の入手とライセンス記録**: SDK 取得、利用条件の確認結果を
  `analysis/AE_SDK_LICENSE_NOTE_<date>.md` に記録(SDK 本体はリポジトリに
  入れない。`AE_SDK_ROOT` 環境変数で参照)。→ B-2/B-3 の前提。
- **H-3 minihost ABI 由来決定**: 「SDK 準拠 or 公開文書からの cleanroom
  定義」の決定と根拠を `docs/ABI_PROVENANCE_DECISION_<date>.md` に記録。
  → §8 G-8、D-1 の前提。
- **H-4 AE での計装トレース採取**: 人間が手元の AE に instruments を入れ、
  トレースを採取し、A-5 intake で `target/ae-trace-corpus/` に取り込む。
  ラボ側は AE を起動しない。→ D-2 以降の適合比較の参照データ。

---

## 6. Phase D: 🔒 ゲート後ネイティブ実行

> **着手条件: §8 の G-1〜G-8 がすべて成立し、人間がゲート開放を明示宣言して
> いること。** それまでは D 系ディレクトリの作成もしない(B-1 のガードテストが
> `minihost/` の cleanroom 境界を監視する)。

### D-1 🔒 minihost worker: Stage L1–L2(load + PiPL 照合 + 即 unload)

- **依存**: B-1(broker)、H-1/H-2/H-3、§8 全成立
- **作成**: `minihost/`(C++、CMake)。broker からのみ起動される。
- **仕様骨子**: broker が allowlist から解決した単一パスを、broker→worker の
  固定パイプ経由でのみ受領(CLI 引数でパスを渡さない)。
  `LoadLibraryExW`(探索パス固定)→ リソースから PiPL を読み、
  `target/aex-static-probe/` の記録メタデータ(型/サイズ/エントリポイント名)
  と一致照合 → 不一致は即 abort(exit 分類 `identity_mismatch`)→
  `GetProcAddress` でエントリ解決確認 → `FreeLibrary` → capability report
  (contracts/aex/worker_capability_report.schema.json 準拠)を broker 経由で
  出力。**セレクタ呼び出しはしない。**
- **テスト**: 承認済み自作 fixture での L1/L2 green。破損 DLL(テスト用に
  生成した非 PE ファイル)で `crashed`/`load_failed` が broker に隔離捕捉
  されること。ゲート成果物欠落時に worker が起動前拒否されること
  (fail-closed テスト)。

### D-2 🔒 Stage L3: describe(ABOUT / GLOBAL_SETUP / PARAMS_SETUP)+ suite registry

- **依存**: D-1、H-4(参照トレース)
- **仕様骨子**: minihost に suite registry(実装スイート最小集合+
  fail-soft スタブ。未実装要求は文書化エラー返却+ `unimplemented` トレース
  記録)と、A-4 スキーマの minihost 側トレース出力を実装。
  pf-null-echo → pf-callback-tracer → ScatterMap の順に describe。
  パラメータ一覧を capability report に記録。
- **完了条件**: pf-callback-tracer の「AE 実測トレース(H-4)」と
  「minihost トレース」を A-6 diff にかけ、must_match 失敗ゼロ
  (selector 順序・suite 集合)。ScatterMap describe が成功または
  「不足スイートの特定」までを capability 結果として記録。

### D-3 🔒 Stage L4: 1フレームレンダー + aex-image-probe CLI

- **依存**: D-2、A-8
- **仕様骨子**: `PF_Cmd_RENDER` 1フレーム(8bpc、straight alpha、単一入力)。
  pf-null-echo で恒等性を A-8 oracle で校正(identical 必須)→ ScatterMap を
  既定パラメータでレンダーし出力ハッシュの安定性(3回実行同一)を確認。
  Python 側 `tools/aex_image_probe.py` が catalog / describe / apply を
  labctl(A-2)経由で束ねる。
- **完了条件**: null-echo identical / ScatterMap 決定的出力 /
  タイムアウト・クラッシュがすべて capability 結果として分類され、
  broker/呼び出し元が生存。

### D-4 🔒 適合レポートと対象拡大

- **依存**: D-3、H-4
- **仕様骨子**: ScatterMap の AE 参照フレーム(人間書き出し)と minihost 出力を
  A-8 で比較し、(a) トレース適合率(A-6)+(b) 画素適合(A-8)の 2 軸
  capability card を出力。以後、自作ビルド群(MaskOffset、AdaptiveFilter、
  MedianPro、DepthAnythingV2)へ **1本ずつ** H-1 相当の承認を経て拡大。
  公開物は既存 publication boundary audit を通過したサマリーのみ。

---

## 7. タスク一覧(サマリー)

| ID | 内容 | ゲート | 依存 | 主な新規パス |
|----|------|--------|------|--------------|
| A-1 | provenance 回答 intake | 不要 | — | `tools/aex_fixture_provenance_answer_intake.py` |
| A-2 | labctl ランナー | 不要 | — | `tools/labctl.py`, `pipelines/` |
| A-3 | AEPX エッジ fixture 編入 | 不要 | — | `tests/fixtures/aepx/` |
| A-4 | trace 契約スキーマ | 不要 | — | `contracts/trace/` |
| A-5 | AE トレース intake | 不要 | A-4 | `tools/ae_trace_intake.py` |
| A-6 | normalizer + conformance diff | 不要 | A-4 | `tools/trace_normalizer.py`, `tools/trace_conformance_diff.py` |
| A-7 | validator strict モード | 不要 | — | (既存編集) |
| A-8 | 画素オラクル(合成) | 不要 | — | `tools/aex_compat_oracle.py` |
| B-1 | Rust broker + ダミーワーカー | 不要 | — | `broker/`, `contracts/broker/`, `tests/test_native_code_guards.py` |
| B-2 | trace_writer + pf-null-echo | 不要 | A-4, H-2 | `instruments/` |
| B-3 | callback-tracer + crashkit | 不要 | B-2 | `instruments/` |
| H-1〜H-4 | 人間作業(回答/SDK/ABI決定/AE採取) | — | — | `analysis/`, `docs/` への記録 |
| D-1 | 🔒 worker L1–L2 | **必要** | B-1, H-1..3 | `minihost/` |
| D-2 | 🔒 L3 describe + suites | **必要** | D-1, H-4 | `minihost/` |
| D-3 | 🔒 L4 render + probe CLI | **必要** | D-2, A-8 | `tools/aex_image_probe.py` |
| D-4 | 🔒 2軸適合 + 対象拡大 | **必要** | D-3 | — |

---

## 8. Safety Gate チェックリスト(D フェーズ着手条件)

すべて**成果物の存在と内容**で判定する。1つでも欠ければ D 系は着手禁止。
判定は人間が行い、開放宣言もこのリポジトリ(docs/ 配下の日付つき文書)に残す。

| # | 条件 | 検証方法 |
|---|------|----------|
| G-1 | fixture 承認 manifest(H-1 経由、単一 AEX を SHA-256+サイズで特定) | `target/fixture-approval/` の manifest + A-1 intake レポート |
| G-2 | loader 承認 receipt(fixture 承認とは別、有効期限つき) | `contracts/aex/loader_readiness_gate.schema.json` 系の検証を strict で通過 |
| G-3 | 依存 DLL レビュー完了(対象候補の default-deny 行ゼロ) | 既存 `target/dependency-review/` 最新成果物 |
| G-4 | プロセス隔離実証(broker selftest 5 シナリオ green) | B-1 selftest レポート + `cargo test` 記録 |
| G-5 | パス allowlist が broker 内部のみ(リクエスト経由のパス受理なし) | 既存 path-policy selftest + B-1 ガードテスト |
| G-6 | クラッシュ隔離のフォールトインジェクション green | B-1 の abort シナリオ + D-1 破損 DLL テスト計画 |
| G-7 | ログ redaction と create-new 出力の二重検証 | B-1 redaction テスト + 既存 output policy テスト |
| G-8 | cleanroom/ライセンス決定文書(H-2, H-3) | `analysis/AE_SDK_LICENSE_NOTE_*.md` + `docs/ABI_PROVENANCE_DECISION_*.md` |

ゲート開放後も、worker 能力は Stage L1→L4 の段階解放とし、各 Stage の
selftest green を次 Stage の入力契約とする(設計書 §6.2)。

---

## 9. フェーズ検収基準(Definition of Done)

- **Phase A 完了** = A-1〜A-8 全コミット済み、`python -m unittest discover -s
  tests` 全緑、`contract_schema_validator.py --strict contracts` 通過、
  readiness matrix 再生成で回帰なし。
- **Phase B 完了** = 上記に加え `cargo test` green(または skip 記録)、
  ガードテスト green、broker selftest レポートが strict 検証通過。
- **Phase D 各段完了** = 各 Stage の selftest green + capability report が
  スキーマ通過 + 既存 68+ no-load テストが**無改変のまま**全緑
  (「やらない宣言」を弱める変更が無いことの証明)。

AEXCompat 詳細設計案(2026-07-03)

> 注記 (2026-08-04): 本書は superseded な旧設計案。本文中の「sandbox broker」
> 「ネットワーク遮断」等の語彙は現行の主張に引き継がれていない。worker isolation
> は crash containment であって confidentiality sandbox ではなく、ネットワーク
> 制限は実装されていない。現行の実装状態は `docs/ISOLATION_INVENTORY_2026-08-04.md`
> と `CLAUDE.md` (Execution Floor and Safety Rules) を正とすること。

1. 現状理解
1.1 リポジトリの構成
注意: 現時点で git リポジトリは初期化済みだがコミットが1つもなく、全ファイルが untracked です。 これ自体が最初の作業項目になります(§10)。

tools/ — Python 製の no-load 解析ツール群 69本。系統は大きく5つ:
静的プローブ系: aex_static_probe.py(PE/リソース/PiPL メタデータ)、aepx_static_probe.py(AEPX XML 構造)、aex_pipl_resource_catalog.py
分類・マトリクス系: aex_candidate_matrix.py、aex_dependency_matrix.py、aex_readiness_matrix.py
ゲート・承認系: aex_load_gate_check.py、aex_fixture_decision.py(APPROVE_AEX_LOAD_GATE トークン必須)、fixture provenance 系一式
ワーカー/ブローカー系: aex_no_load_worker.py(JSONL プロトコル、PPM identity のみ、load_aex を fail-closed 拒否)、aex_native_loader_broker.py(パス受け取りすら拒否)
監査系: aex_safety_chain_audit.py、aex_artifact_index.py、aex_publication_boundary_audit.py
tests/ — 上記とほぼ 1:1 対応の unittest 68本。各ツールの「やらないこと」(no-load 不変条件)を契約としてテストしている。
analysis/AEX_COMPAT_LAB_PLAN_2026-06-05.md — 4,795行の実行ログ兼計画書。実測値として: Ae_Plugins 配下に .aex 40本、PE 有効 40、PiPL シグナル 40、EffectMain マーカー 37(残り3本は AEGP 系)。最新 readiness は「45 satisfied / 1 pending(manual fixture review)/ 2 intentionally closed / 0 failed」。
target/(gitignore 済み)— create-new only の .local.json 成果物チェーン。canonical chain は artifact-index → readiness-matrix で索引化されている。
1.2 既存 no-load 方針が守っているもの
一貫して以下をコード上の不変条件として保持しています:

.aex を OS ローダーで開かない・ハッシュしない・コピーしない(メタデータ読み取りのみ)
EffectMain / PF セレクタ呼び出し禁止、AE 起動禁止、OFX ルート禁止
AEPX/AEP への書き込み禁止、テキスト/bdata ペイロードの値エクスポート禁止(redacted メタデータのみ)
成果物は target/ 配下 create-new only、パストラバーサル拒否、絶対パスの成果物への漏出防止
承認は「明示フラグ+トークン+人間のレビュー」でしか生成できず、承認があってもそれ単体ではロードを許可しない多段ゲート
つまりこのラボは「実行せずに証明できることをすべて証明し尽くし、実行が必要な地点を1点に絞り込んだ」状態です。

1.3 AviUtlas 由来 Rust 契約資産が追加するもの
imports/aviutlas-rust-contracts/ は由来保存(provenance)込みのステージング取り込みで、3層:

analysis/(約70点) — Python 側にまだない将来層の契約定義が中心:
ロード後世界のスキーマ: AEX_IMAGE_PROBE_REQUEST_SCHEMA、AEX_WORKER_CAPABILITY_REPORT_SCHEMA、loader preflight / approval receipt / readiness gate 各スキーマ
プロセス隔離の具体要件: AEX_WORKER_START_POLICY(Job Object kill-on-close、handle_inheritance_status=sentinel_not_inherited、サンドボックス preflight)
戦略文書: AEX_DIRECT_HOST_AND_AEP_EDIT_STRATEGY(「直接ホスト=ブローカー媒介・別プロセス・allowlist 制」の定義、classic CPU effect を第一目標に、SmartFX/GPU/AEGP/AEIO は据え置き)、OFX_AEX_BRIDGE_STRATEGY
証拠分類の方法論: AEX_LOADER_READINESS_GATE_EVIDENCE_MATRIX(Merged/Measured/Approx/Blocked-by-Native-Oracle の4分類)
aviutl-rs/examples/(20本) — 上記スキーマを emit する Rust CLI 例(Python 版と機能重複が多い)
aviutl-rs/tests/ + fixtures — スキーマ契約テストと、AEPX writer spike 用のエッジケース fixture(CRLF/BOM、Unicode、重複ID等)。AEPX 保存書き込み(preservation writer)の検証素材は Python 側に存在しない付加価値。
要するに imports は「Python ラボがまだ踏み込んでいない、実ロード後・実書き込み後の世界の契約とテスト素材」を先回りで定義した資産です。

2. 正しいゴール定義
2.1 プロジェクトの正式な定義
AEXCompat は、After Effects エフェクトプラグイン(.aex)のための「静的解析ラボ+検証ハーネス+段階的な最小互換ホスト」である。

具体的には4つの製品面の合成体です:

AEX 静的解析ラボ — ロードせずに PE/PiPL/依存 DLL/エントリポイントから互換性見込みを分類する(すでにほぼ完成)
最小実行プローブ(aex-image-probe) — allowlist 済みの classic CPU effect 1本を、隔離ワーカー内で「パラメータを describe し、静止画1フレームをレンダーできるか」だけ検証する
検証ハーネス+互換性オラクル — 決定的な画像 fixture と、本物の AE で生成した参照出力(ground truth)を比較し、capability report を生成する
AEPX 静的編集面 — AEPX プロジェクトの読み取り専用解析と、将来の保存書き込み(round-trip 保証つき)
2.2 何ができれば価値があるか
最終価値1(検査): 任意の .aex を渡すと、実行なしで「クラス(PF effect / AEGP / …)、依存、リスク、ホスト可能性」を JSON レポートで返せる
最終価値2(検証): 自前ビルドの classic effect について「describe 成功/パラメータ一覧/1フレーム identity・実効レンダー結果/クラッシュ・タイムアウト挙動」を再現可能な capability report として出せる
最終価値3(基盤): 将来の任意のホスト(AviUtlas 等)が「この plugin はこの範囲で動く」と主張するときの証拠チェーンを供給する
2.3 やらないこと(非目標)
After Effects の完全エミュレーションは目標ではない。SmartFX、GPU レンダー、AEGP スイート群、AEIO、Artisan、エクスプレッション、UI 統合は明示的に対象外(将来も「段階的追加候補」であって約束しない)
AE 本体の起動・自動操作(参照出力の生成を人間が手動で行う場合を除く)
私有 Adobe 資産・サードパーティ商用プラグインの再配布・公開。成果物は publication boundary audit を通るまでローカル限定
Adobe SDK ヘッダ・ABI 情報の無審査取り込み(§8 の cleanroom リスク参照)
AEPX/AEP への書き込み(round-trip 検証と明示承認が揃うまで)
3. 推奨アーキテクチャ
3.1 コンポーネント一覧
┌─────────────────────────── Python 層(信頼・非実行)───────────────────────────┐
│ static analyzer      … 既存 aex_static_probe / aepx_static_probe               │
│ PiPL/resource catalog … 既存 + 実ペイロード TLV adapter(レビュー後に昇格)      │
│ capability matrix    … 既存 candidate/dependency/readiness matrix              │
│ contracts/ + schema registry … imports の JSON Schema を正規化し jsonschema 検証 │
│ fixture manager      … PPM/PNG 決定的 fixture 生成・検証(既存)                │
│ loader gate + approval intake … 既存ゲート + 実際の人間回答を受ける intake      │
│ compatibility oracle … worker 出力 vs AE 参照出力の画素比較・レポート           │
│ labctl(新規)       … 上記69ツールのチェーン実行を宣言的に束ねるオーケストレータ │
└──────────────┬───────────────────────────────────────────────────────────────┘
               │ JSONL / JSON レポート(パス allowlist・スキーマ検証済みのみ通過)
┌──────────────▼─────────── Rust 層(半信頼・プロセス管理)──────────────────────┐
│ sandbox broker … ワーカー起動専任: Job Object kill-on-close、ハンドル非継承、   │
│                  タイムアウト、stdout/stderr 捕捉、クラッシュ分類、ログ redaction │
└──────────────┬───────────────────────────────────────────────────────────────┘
               │ 固定プロトコル(JSONL over pipes)、AEX パスは broker だけが解決
┌──────────────▼─────────── C/C++ 層(非信頼・使い捨てワーカー)─────────────────┐
│ aex-worker(minimal native host)… LoadLibraryEx → PiPL 照合 → entry 解決 →     │
│   PF_Cmd_ABOUT / GLOBAL_SETUP / PARAMS_SETUP / RENDER(段階解放)               │
│ image probe worker … RGBA8 フレーム1枚の identity/実効レンダー                  │
└────────────────────────────────────────────────────────────────────────────────┘
3.2 言語の役割分担
言語	役割	理由
Python	静的解析、スキーマ検証、ゲート、オラクル、オーケストレーション	既存69ツールの資産。実行しない層は開発速度最優先
Rust	sandbox broker(プロセス起動・Job Object・ハンドル制御・タイムアウト)	imports/aviutl-rs に契約資産あり。Win32 プロセス制御をメモリ安全に書ける。ワーカーがどれだけ暴れても broker は落ちない必要がある
C++	aex-worker 本体(PF ABI との会話)	PF_InData/PF_OutData/PF_ParamDef は C ABI 構造体。ここだけはネイティブ必須。使い捨て前提・状態を持たない
C	(必要なら)worker のクラッシュハンドラ/最小シム	SEH・最小依存が欲しい箇所のみ
3.3 データフローと安全境界
境界0(ファイルシステム): .aex の実パスは Python 層の成果物には入らない(既存方針を維持)。broker だけが allowlist(AEX_IMAGE_PROBE_ALLOWLIST 形式)から実パスを解決する。
境界1(Python→Rust): labctl は「probe request JSON(スキーマ検証済み)」だけを broker に渡す。broker はゲート成果物(load-gate report + approval receipt)の存在と内容を自分でも再検証する(二重検査、片方の失敗で fail-closed)。
境界2(Rust→C++): broker がワーカーを起動。Job Object kill-on-close、継承ハンドル明示リスト(sentinel 検査)、CPU/メモリ/実行時間上限、ネットワーク遮断(可能なら restricted token / AppContainer)。ワーカーは1リクエスト1プロセス。
境界3(ワーカー内): LoadLibraryEx(LOAD_WITH_ALTERED_SEARCH_PATH なし・DLL 探索固定) → 静的プローブで記録済みの PiPL/エクスポートと実物が一致するか照合(identity revalidation)→ 一致時のみ entry 解決。クラッシュ・例外は「期待される capability 結果」としてレポート化し、broker が exit code / Job Object 通知で分類。
出力: ワーカーは RGBA8 バッファと capability JSON のみ返す。ログにバイナリペイロード・私有パスを書かない(redaction は broker の責務)。
4. AviUtlas からの移管方針
4.1 本体へ昇格すべきもの
資産	昇格先	形
JSON Schema 群(probe request / capability report / loader preflight / approval receipt / readiness gate / AEPX patch request・report)	新設 contracts/(例: contracts/aex/, contracts/aepx/)	日付サフィックスを外し schema_version フィールドで版管理。jsonschema で CI 検証
AEX_WORKER_START_POLICY の要件	broker 実装(M4)の受け入れ条件 + analysis/ に正式版を再録	文書+テスト化
AEPX writer spike の fixture 群(aviutl-rs/tests/fixtures/aepx_writer_spike_*.aepx)	新設 tests/fixtures/aepx/	Python round-trip validator のエッジケーステストに転用
probe allowlist example(aex_image_probe_allowlist.classic.json 等)	contracts/aex/ の example として	allowlist スキーマの canonical 例
Evidence Matrix の4分類方法論(Merged/Measured/Approx/Blocked)	README または analysis/ の方法論文書	以後のすべての主張のラベル語彙にする
4.2 provenance として残すだけのもの
aviutl-rs/examples/*.rs(20本)— Python 版と機能重複。broker(M4)を書くときの参照実装としてのみ価値がある。昇格せず imports に凍結。
AviUtlas 固有の handoff/goal-orchestration 文書(AVIUTLAS_DEVELOPMENT_CHAT_HANDOFF 参照系)— 歴史記録。
日付つき戦略文書の旧版・ライセンスノート — 由来監査用に原文保持。
4.3 将来 AviUtlas 側から削除してよいもの / 残すべきもの
削除可: ここに完全コピー済みの analysis/ スキーマ・仕様書、aviutl-rs/examples の AEX/AEPX/OFX 系(AviUtlas 本体ビルドが参照していないことが条件)。
AviUtlas 側に残す: AviUtlas 本体(GUI/exedit)が import している Rust コード、AviUtlas 自身のゴール管理文書、.auf/.exo 系の AviUtl 資産(このラボのスコープ外)。
4.4 削除前に必要な確認
imports/ 側とバイト同一性の照合(コピー漏れ・改変検出)
AviUtlas の Cargo.toml / モジュールツリーが対象ファイルを参照していないこと(cargo check が通ること)
AviUtlas 側 git 履歴に残ること(履歴ごと消さない)
本リポジトリの初回コミット完了(§10 タスク1)— コミットされていない今の状態で元を消すのは厳禁
5. 実装ロードマップ(8 マイルストーン)
M0. ベースライン確定(即日)

目的: 消失リスクの排除と再現性の起点。
成果物: 初回 git commit、python -m unittest discover -s tests 全緑の記録、CLAUDE.md(実行コマンド・安全方針の要約)。
完了条件: クリーン checkout からテスト全緑。
リスク: なし(読み取りのみ)。
M1. contracts/ 正規化とスキーマ検証基盤(1〜2日)

目的: imports のスキーマを実働資産に。69ツールの出力に「宣言されたスキーマ」を与える。
成果物: contracts/ ディレクトリ、tools/contract_schema_validator.py(jsonschema で target/ 成果物と contracts を検証)、provenance 対応表(contracts/PROVENANCE.md)。
完了条件: canonical chain の全成果物がスキーマ検証を通過、または差分が既知課題として列挙される。
リスク: 既存成果物とスキーマの不整合 → 検証は最初 warning モード、fail-closed 化は次段。
M2. labctl オーケストレータ(2〜3日)

目的: 69ツールの手動チェーン(現状 README に生コマンド列)を宣言的パイプラインに。現在の最大の運用コストはツールの多さではなく配線。
成果物: tools/labctl.py + pipelines/*.json(ステージ、入力成果物 kind、出力 kind を宣言)。artifact-index と統合。
完了条件: python tools/labctl.py run no-load-chain が既存チェーンを再現し、readiness matrix まで到達。
リスク: 既存ツールの CLI 差異 → ラッパー方式(ツール本体は不改変)で吸収。
M3. fixture 承認の実クローズ(人間作業含む、1〜2日)

目的: readiness の唯一の pending「manual fixture review」を、自前ビルド classic effect(第一候補 AEPluginBuild\ScatterMap.aex、次点 MaskOffset.aex)で正式に閉じる。
成果物: tools/aex_fixture_provenance_answer_intake.py(既存 validator selftest のルールで実回答を検証)、人間が記入した回答成果物、aex_fixture_decision.py による承認 manifest。
完了条件: readiness matrix が「fixture approval satisfied」に遷移。native load gate はまだ closed のまま(承認はロード許可ではない、を維持)。
リスク: 自前ビルドの由来確認漏れ → ScatterMap のソース所在・ビルド手順を回答に必須記載。
M4. Rust sandbox broker(実ロードなし、3〜5日)

目的: プロセス隔離基盤を先に完成・検証する。AEX には一切触れない段階で Job Object / ハンドル制御 / タイムアウト / クラッシュ分類を実証。
成果物: broker/(cargo プロジェクト)、ダミーワーカー(無害な exe)での起動・強制終了・タイムアウト・sentinel 非継承のテスト、broker selftest レポート(既存 native-loader-runtime-contract スキーマ準拠)。
完了条件: WORKER_START_POLICY の全要件が自動テストで green。AEX パス受理コードは存在しないこと(grep で検証)。
リスク: Windows API の細部(Job Object のネスト等)→ 参照実装として aviutl-rs/examples を活用。
M5. C++ 最小ワーカー: load + identity revalidation + describe(SDK/cleanroom レビュー完了後、5〜10日)

目的: 承認済み fixture 1本を隔離ワーカーで LoadLibraryEx し、PiPL 照合 → PF_Cmd_ABOUT / GLOBAL_SETUP / PARAMS_SETUP まで。レンダーはまだしない。
成果物: worker/(C++)、capability report(パラメータ一覧をスキーマ準拠で)、クラッシュ・タイムアウトの分類レポート。
完了条件: ScatterMap の describe 成功、静的プローブ記録との PiPL 一致、意図的破損 DLL でのクラッシュが broker で隔離捕捉されること。§6 のゲート全条件成立が前提。
リスク: AE SDK ヘッダの取り扱い(§8 法的リスク)。M5 着手前にライセンスレビューを完了させることが hard blocker。
M6. 1フレーム画像プローブ(render_png、3〜5日)

目的: AEX_IMAGE_PROBE_TOOL_SPEC の apply を実装。既存 PPM fixture 基盤を入力に、RGBA8 1フレームをレンダー。
成果物: aex-image-probe CLI(catalog/describe/apply)、render capability report。
完了条件: ScatterMap でデフォルトパラメータの1フレームレンダーが再現的に成功し、出力ハッシュが安定。
リスク: PF world/rowbytes/カラー深度の解釈ミス → identity 系エフェクト(自作 no-op AEX を新規ビルドして基準にする)で先に校正。
M7. compatibility oracle と参照出力比較(3〜5日)

目的: 「動いた」を「AE と同じに動いた」に格上げする装置。
成果物: tools/aex_compat_oracle.py(worker 出力 vs 人間が AE で手動書き出しした参照フレームの画素差分・許容誤差・レポート)、report schema。
完了条件: ScatterMap で AE 参照との一致(または差分の定量記録)。
リスク: 色管理・アルファ前提の差 → 最初は 8bpc・straight alpha・単一レイヤーに限定。
M8. 対象拡大とカタログ公開準備(継続)

目的: allowlist を自前ビルド群(AdaptiveFilter、MedianPro、DepthAnythingV2 等)へ段階拡大し、capability matrix を実測値で埋める。
成果物: 40本の実測 capability matrix(実行はallowlist承認分のみ)、publication boundary audit を通した公開可能サマリー。
完了条件: 各 plugin に Merged/Measured/Approx/Blocked ラベルの実測行。
リスク: サードパーティ製の実行 → 自前ビルド以外は明示承認を都度必須のまま。
6. Native/AEX Loading Safety Gate
6.1 絶対条件(1つでも欠ければロード禁止)
実 .aex を LoadLibraryEx する前に、すべてが機械検証可能な成果物として存在すること:

fixture 承認: target/fixture-approval/ に、実際の人間回答(M3 intake 済み)に基づく承認 manifest。トークン APPROVE_AEX_LOAD_GATE + --explicit-user-approval。対象は単一の AEX を SHA-256 とサイズで特定(承認時点で初めてハッシュを解禁)。
loader 承認 receipt: fixture 承認とは別個のロード実行承認(AEX_LOADER_APPROVAL_RECEIPT_SCHEMA 準拠)。有効期限(例: 30日)と対象ハッシュを含む。
依存 DLL レビュー: dependency review packet が対象候補について「default-deny 行ゼロ」。manual-review 行は人間の disposition 記録つき。ワーカーの DLL 探索パスは System32 + ワーカー自身のディレクトリに固定。
プロセス分離の実証: M4 broker selftest green(Job Object kill-on-close、sentinel 非継承、タイムアウト kill、子プロセス残存ゼロ)。
パス allowlist: broker 内部の allowlist にのみ実パス。リクエスト JSON からのパス受理は恒久禁止(既存 path-policy selftest の維持)。allowlist 変更はコミット履歴に残す。
クラッシュ隔離: ワーカークラッシュが broker/呼び出し元 Python を巻き込まないことのフォールトインジェクションテスト(意図的 abort ワーカー)green。
ログ・成果物ポリシー: ワーカー stdout/stderr は broker が redaction(絶対パス・バイナリ断片除去)後にのみ保存。レポートは target/ create-new only。秘密情報(ライセンスキー、私有プロジェクトデータ)はワーカー環境変数から遮断(明示 allowlist 環境のみ継承)。
cleanroom/ライセンスレビュー完了: PF ABI 定義の由来(Adobe SDK 利用条件 or 公開文書からの独自定義)が文書で確定していること(§8)。
6.2 段階解放(ゲート内ゲート)
条件成立後も一括解放しない。ワーカーの能力は capability flag で段階解放し、各段に selftest を置く:

Stage L1: LoadLibraryEx + GetProcAddress + 即 unload(セレクタ呼び出しなし)
Stage L2: + PiPL 実照合(静的記録と一致しなければ abort)
Stage L3: + PF_Cmd_ABOUT / GLOBAL_SETUP / PARAMS_SETUP(describe)
Stage L4: + PF_Cmd_RENDER 1フレーム(M6)
各 Stage の解放は broker のビルド時定数ではなく、署名相当のゲート成果物(readiness gate report)を broker が起動時に検証して決める。検証失敗・成果物欠落・スキーマ不一致は全て即終了(fail-closed)。

7. テスト戦略
既存 no-load contract の保全: tests/ の68本は今後も無改変で全緑を維持。ネイティブ系を追加しても、既存ツールの「やらない宣言」フィールドを弱める変更は禁止。CI(またはローカル pre-commit)で python -m unittest discover -s tests を必須化。
スキーマ検証層(M1): contracts/ の JSON Schema に対し、(a) スキーマ自体の妥当性、(b) canonical chain 成果物の適合、(c) 「禁止フィールド(絶対パス・raw payload)不在」のネガティブ検証。
fixture 検証: 既存 PPM 検証に加え、imports の AEPX エッジ fixture(BOM/CRLF/Unicode)を round-trip validator のテストに編入。
sandbox/worker smoke(M4): ダミー exe による「正常終了・タイムアウト・クラッシュ・ハング・子プロセス増殖」5シナリオの自動テスト。AEX 非関与でプロセス隔離だけを検証。
native host staged test(M5+): Stage L1→L4 ごとに独立テスト。各 Stage は「前 Stage の成果物が green であること」を入力契約にする。実 AEX を使うテストは既定でスキップし、環境変数+ゲート成果物が揃った時のみ実行(CI では永久スキップ)。
fail-closed の検証方法: ポジティブテストと同数以上のネガティブテストを義務化 —「承認欠落」「スキーマ改竄」「ハッシュ不一致」「タイムアウト」「allowlist 外パス」を与えて拒否すること自体をテストする。既存ツール群のスタイル(blocked メッセージの selftest)をネイティブ層にも延長する。
oracle 回帰(M7): 参照フレームとの差分レポートをスナップショットとして保存し、worker 変更時の画素回帰を検出。
8. リスクと未知数
分類	リスク	軽減策
技術	PF ABI の理解不足(world 構造、rowbytes、quality flags)でレンダー結果が壊れる	自作 no-op/identity AEX をこちらのソース管理下で新規ビルドし校正基準にする。8bpc・単一入力に限定して開始
技術	プラグインが AE 固有スイート(PICA suites)を PF_Cmd_GLOBAL_SETUP 段階で要求して即死する	「未実装スイート要求=capability 結果」として扱う設計(spec 通り)。最初から失敗を正常系に含める
技術	32/64bit・CRT 混在・依存 DLL 欠落	既存 dependency matrix/preflight を Stage L1 の前提条件に接続
法的	Adobe AE SDK のライセンスはプラグイン開発向けであり、ホスト実装への利用可否は自明でない。imports の契約も allow_aex_sdk_or_abi_import=false を維持している	M5 の hard blocker として文書レビューを先行。選択肢: (a) SDK 条項の確認と記録、(b) 公開ドキュメント(ae-plugins.docsforadobe.dev)からの cleanroom 構造体定義+由来記録。どちらでも contracts/ に由来宣言を残す
法的	サードパーティ .aex の解析・実行	実行は自前ビルドのみから。静的メタデータであっても publication boundary audit 通過まで非公開(既存方針維持)
セキュリティ	悪意ある/破損 AEX のロード(任意コード実行そのもの)	§6 のゲート+allowlist は自前ビルド限定+Job Object+使い捨てワーカー+ネットワーク遮断。「ロード=そのユーザ権限でのコード実行」であることを承認文書に明記
セキュリティ	成果物への秘密・私有パス漏出	既存 redaction 方針の維持+broker 側 redaction の二重化+ネガティブテスト
PM	手続き成果物の増殖(現状69ツール、実行系より紙が厚い)により前進が停滞する	M2 labctl で配線コストを固定化。以後「新ゲートを足す」より「既存 pending を閉じる」を優先する運用ルール
PM	未コミットのまま作業続行 → 消失・監査不能	M0 を最優先(下記タスク1)
PM	AviUtlas 側との二重管理	§4.4 の確認手順を経てから片側削除。それまで imports は凍結(編集禁止)
9. 実装タスク分解(Codex 向け・1スレッド順次実行)
共通禁止事項(全タスク): .aex のオープン/ハッシュ/コピー/ロード、AE 起動、OFX 実ルート、AEPX/AEP 書き込み、imports/ 配下の編集、既存 target/ 成果物の上書き。T-09 以降のネイティブ実行系タスクは §6 ゲート成立前に着手禁止。

T-01 初回コミットとベースライン
編集対象: なし(git 操作のみ)。git add -A && git commit。
完了条件: git status クリーン、python -m unittest discover -s tests 全緑をコミットメッセージに記録。
テスト: 既存68本。
T-02 CLAUDE.md 追加
編集対象: CLAUDE.md(新規)。テスト実行法、no-load 方針の要約、imports 凍結ルール、成果物命名規約。
完了条件: 1ページ以内で新規セッションが迷わない内容。
テスト: なし(文書)。
T-03 contracts/ 骨格と第1弾昇格
編集対象: contracts/aex/*.json, contracts/PROVENANCE.md(新規のみ)。imports から probe request / capability report / allowlist / loader readiness の4スキーマをコピー整形(schema_version 付与)。
禁止: imports 原本の変更。
完了条件: PROVENANCE.md に「昇格元→先」対応表。
テスト: tests/test_contracts_provenance.py(新規)— 昇格ファイルが valid JSON Schema であること。
T-04 contract_schema_validator ツール
編集対象: tools/contract_schema_validator.py, tests/test_contract_schema_validator.py(新規)。標準ライブラリ縛りなら簡易検証器、jsonschema 導入可なら requirements 明記。
完了条件: contracts の全スキーマ検証+禁止フィールド(絶対パス/raw payload)検出が動く。warning モード既定。
テスト: 正常系+改竄スキーマ拒否のネガティブ系。
T-05 labctl パイプラインランナー
編集対象: tools/labctl.py, pipelines/no_load_chain.json, tests/test_labctl.py(新規)。既存ツールを subprocess で順次起動する薄いラッパー。既存ツール本体は不改変。
完了条件: dry-run(コマンド列の表示のみ)と実行の両モード。実行は create-new 規約を継承。
テスト: dry-run のコマンド列スナップショット+モックステージでの実行順検証。
T-06 provenance 回答 intake ツール
編集対象: tools/aex_fixture_provenance_answer_intake.py, tests/(新規)。aex_fixture_provenance_answer_validator_selftest.py の合成ルールを実回答 JSON に適用。
禁止: 承認 manifest の自動生成(intake は検証のみ。承認は既存 aex_fixture_decision.py の人間フロー経由)。
完了条件: 8問すべての検証、値欠落・矛盾の fail-closed。
テスト: 合成 accept 2 / reject 6 ケースの実ファイル版。
T-07 AEPX エッジ fixture の編入
編集対象: tests/fixtures/aepx/(imports からコピー)、tests/test_aepx_roundtrip_validator.py への追加ケース。
完了条件: BOM/CRLF/Unicode/重複ID fixture で round-trip validator の挙動(成功 or 明確な拒否)が固定される。
T-08 broker(Rust)スケルトン — ダミーワーカーのみ
編集対象: broker/(cargo new)。Job Object、継承ハンドル明示リスト、タイムアウト kill、exit 分類。ワーカーは同梱ダミー exe(即終了/スリープ/abort の3種)。
禁止: AEX パス・DLL ロードに関するコードの存在(LoadLibrary 文字列が broker crate に現れないことを CI grep で保証)。
完了条件: cargo test で5シナリオ(正常/タイムアウト/クラッシュ/ハング/子増殖)green。
テスト: Rust 統合テスト+Python 側から broker selftest レポートをスキーマ検証。
T-09 🔒 worker(C++)Stage L1–L2 — §6 全条件+ライセンスレビュー完了が前提。着手前に人間承認を明示確認
編集対象: worker/。LoadLibraryEx→PiPL 照合→即 unload。
完了条件: 承認済み fixture で L1/L2 green、破損 DLL で隔離クラッシュ捕捉。
T-10 🔒 Stage L3 describe / T-11 🔒 Stage L4 render 1 フレーム / T-12 oracle 比較(oracle 自体は Python なので非ネイティブ、ただし入力に L4 成果物が必要) — 各々前 Stage green を入力契約に。
10. 直近3タスク
① 初回 git commit(T-01)+ CLAUDE.md(T-02)

なぜ今: 69ツール+68テスト+由来資産のすべてが untracked。事故一発で監査チェーンごと消える。他のどの作業よりも先。
見るファイル: git status 全体、.gitignore(target/ 除外は確認済み・適切)。
完了条件: コミット済み+テスト全緑記録+CLAUDE.md 1枚。
② contracts/ 昇格第1弾+スキーマ検証器(T-03/T-04)

なぜ今: imports のスキーマは現状「読める文書」でしかない。ここを実働の検証器にすると、以後のすべての成果物(そして将来の broker/worker のレポート)が同じ契約で縛れる。ネイティブ着工前に契約を先に固める、が本ラボの流儀。
見るファイル: imports/aviutlas-rust-contracts/analysis/AEX_IMAGE_PROBE_REQUEST_SCHEMA_2026-05-31.json、AEX_WORKER_CAPABILITY_REPORT_SCHEMA_2026-05-31.json、AEX_LOADER_READINESS_GATE_SCHEMA_2026-06-03.json、AEX_IMAGE_PROBE_ALLOWLIST.example.json。作るのは contracts/ と tools/contract_schema_validator.py。
完了条件: 4スキーマ昇格+PROVENANCE 対応表+検証器のテスト green。
③ fixture provenance 回答 intake(T-06)と人間レビューのクローズ準備

なぜ今: readiness matrix の唯一の pending が「manual fixture review」。8問の回答を受けて検証する道具がないため、システム全体がここで6月から止まっている。intake ができれば、あとは人間(あなた)が AEPluginBuild\ScatterMap.aex について回答を書くだけで M3 が閉じ、M4(Rust broker)へ進める。
見るファイル: tools/aex_fixture_provenance_answer_template.py、tools/aex_fixture_provenance_answer_validator_selftest.py、target/fixture-provenance-answer-template/ae-fixture-provenance-answer-template-1780660829966.local.json。
完了条件: 実回答 JSON を受けて accept/reject を fail-closed で判定するツール+テスト。承認 manifest の生成は従来どおり人間の明示操作のみ。
最後に設計上の要点を一つ。このラボの真の強みは「69本のツール」ではなく、「実行しない証拠を積んでから、実行を1点(承認済み自前ビルド1本×隔離ワーカー×1フレーム)に絞る」という規律です。ロードマップはその規律を壊さないよう、ネイティブ実装(M5)より先にプロセス隔離の実証(M4)を置き、M4 は AEX に一切触れずに完結するよう設計しています。「完全エミュレーター」ではなく、この順序で作られた段階的互換ホスト+検証基盤が、最も忠実で、最も速く、最も安全に到達できる形です。

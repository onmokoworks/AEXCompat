# 検証・強制機構の全数監査と方向転換の材料 (2026-08-05)

目的: 「AEX を呼び出して実行するだけのプロジェクトにしては検証・sandbox 機構が
過剰に手厚い。なぜそうなったのか、必要なのか」を洗い出し、方向転換の判断材料に
する。docs (PROJECT_DIRECTION / ISOLATION_INVENTORY_2026-08-04 /
EVIDENCE_POLICY_2026-07-18 / WINDOWS_NATIVE_HARDENING_PLAN_2026-07-16 /
PROJECT_DESIGN_2026-07-03)、broker/minihost ソース、issue/PR 履歴 (2026-07-18
以降) を突き合わせた。観察 (事実) と推論を分けて記す。

## 1. なぜ手厚いのか: 出自は 3 つ (観察)

手厚さは「悪意あるバイナリからの防御」として積まれたものではない。docs と履歴が
一貫して示す出自は次の 3 つ。

### 1.1 no-load 時代の法的・clean-room 統制 (2026-07-03 〜 07-13)

- 初期リポジトリは「AEX を一切ロードしない静的解析ラボ」(Python ツール 69 本)。
  承認トークン (`APPROVE_AEX_LOAD_GATE`)、多段ゲート、fail-closed、publication
  boundary audit はこの時代の設計 (`docs/PROJECT_DESIGN_2026-07-03.md`)。
- ネイティブ実行の解禁は人間の Safety Gate (`HUMAN_GATE_HANDOFF_2026-07-13.md`)
  を経由。ゲートの中身は H-1 fixture の出所・権利、H-2 SDK ライセンス、H-3
  clean-room 由来決定。つまり動機は法的リスク管理。
- 「approval receipt」という語彙と期限付き receipt の建付けはこの手続きの直系。
  `expected_sha256` pin は最初のネイティブローダー D-1 (2026-07-13) から存在。

### 1.2 エージェント駆動開発への統制 (継続)

- `docs/EVIDENCE_POLICY_2026-07-18.md` §1 が動機を明言している: "disciplines
  agent-driven development, leaves an audit trail for native-execution
  escalation, acts as a next-best substitute for CI in a single-machine
  environment"。
- 実測: 2026-07-01 以降 5 週間で 1315 コミット。テスト 289 ファイル、凍結
  evidence 128 ファイル、tools 152 本。並行エージェントセッションの無承認実行・
  過大主張・退行を止める官僚機構として、機構の相当部分が積まれた。
- 機構が実際に検出してきた事故もエージェント由来が主 (#50 の owner レビュー無視
  merge、#90 の refresh 回し忘れ、#412 の 6,405 行誤削除、#651/#665 のビルド
  成果物混在)。

### 1.3 2026-07-16 hardening plan の一括投入

- `WINDOWS_NATIVE_HARDENING_PLAN_2026-07-16.md` が TOCTOU (path reopen レース、
  隣接 DLL 差し替え) を脅威モデルとして明文化し、sealed load tree → schema-v2
  receipt → restricted token + protected DACL → module audit v2 を計画。
- 実装は issue も PR もない一括コミット bc931957 (2026-07-17) で investing され、
  「production dispatch 全部、normal-token fallback なし」として配線された。
  observation 用途と evidence 用途を区別しないままの全経路適用が、以後 3 週間の
  摩擦 (§3) の源泉。
- 同 plan は当初から「これは integrity 指向の互換性境界で confidentiality
  sandbox ではない」と明記していた。mode 表の後段 (mitigation policy、UILIMIT、
  low IL、AppContainer) は今も実装ゼロ (`ISOLATION_INVENTORY_2026-08-04.md` §6)。

推論: 「セキュリティのために手厚い」という説明は当たらない。手厚さの実体は
(a) 法的統制の遺産、(b) 対エージェント官僚機構、(c) evidence 用 provenance で、
純粋なクラッシュ封じ込め (Job Object + 別プロセス) は全体のごく薄い層にすぎない。

## 2. 現状の強制ポイント全数 (観察、要約)

詳細な棚卸しは付録 A のマトリクス参照。要点:

- 同じプラグインバイト列に対し SHA-256 照合が最大 5 層 (選択時 / dispatch 入口 /
  sealed staging 3 回 / worker 実行体 / worker 内 C++ 再照合)。
- receipt (allowlist v1/v2) が必須なのは `l1`/`l2`/`render*`/`smart*`/
  `render_request` の evidence 生成用 CLI 経路のみ。allowlist の実体は
  `target/` 配下 (.gitignore) で、l2/render/smart 用のジェネレータは存在しない。
  clean clone ではそもそも動かない = 事実上使われていない経路。
- 日常の観察経路 (harness GUI / `render-video-batch` /
  `InteractiveRenderSession`) は receipt 不要で、ソースコメント自身が "This is
  the default (crash-containment) tier" を自称している。CLAUDE.md の「default
  tier は未整備」という記述より実装は先に進んでいる。
- 検証を無効化する env var は存在しない。一方で `AEXCOMPAT_TRUSTED_STAGING` の
  既定は再ハッシュをスキップする緩い側 (=0 が厳格化)。

### 見かけより弱い点 (観察)

強制の「セキュリティとしての実効性」は薄い:

1. receipt の `expires` は日付比較ではなく文字列一致
   (`approved_artifact.rs:107,149`)。期限切れは発生しない。
2. worker は同一ユーザートークン。ファイル・ネットワークアクセスは防げない
   (docs が明言)。
3. 凍結 evidence テストは「JSON が worktree からドリフトしていない」ことしか
   証明せず、JSON を書き換えれば green (EVIDENCE_POLICY §2 が自己批判済み)。
4. frozen worker trust constants は「守る境界が存在しない (全部同一
   user-writable checkout)」として 2026-07-18 に撤去済み。

推論: 防御としては薄く、開発コストだけが本物、という非対称がこの機構群の特徴。

## 3. 機構が開発を止めてきた実績 (観察、時系列)

- #90 (07-19): refresh 忘れで認証テスト 38/45 fail。
- #177 (07-19): fail-closed 契約の評価タイミング退行で「パラメーターを宣言する
  任意の AEX」が全経路 exit 3。
- #185/#304/#315/#351/#394: module audit と sealed 封入が正当な DLL
  (CUDA driver / 依存 closure / WinSxS COMCTL32 / IPP dispatcher / モジュール数
  上限) を次々に unknown 扱いし、実 AEX corpus の discovery を段階的に全滅させた。
  audit が検出したのは攻撃ではなく正当なランタイムばかり。
- #354: 封入量依存で discovery の 26/353 が偽 timeout。owner 判断で discovery
  から deadline を撤廃し、「timeout は常時オン invariant」を降格。
- #381→#399: staging の 2 回フルリードが sweep の 64% を占め (最重クラスタで
  約 880 秒)、hard link 化 + trusted staging キャッシュで緩和。
- #335: hosted CI runner では restricted token worker が起動できず、windows_e2e
  32 件が常時 skip。
- #653 (open): staging 中断の hardlink 残骸で fail-closed が恒久 fail 化、手動
  削除まで回復しない。
- #691 (08-04): source-text grep テスト (149+72 ファイル、marker 457 個) を
  「振る舞い保存リファクタで壊れ、振る舞い破壊で通る」逆向き特性を理由に全廃。

既に剥がされたもの: frozen worker trust constants (§3 amendment)、discovery
timeout (#354)、grep テスト (#691)、三重ゲート付きトレース提案 (#28→#34)。
リポジトリは 07-18 以降、自分で過剰装備を訂正し続けている。

## 4. 要/不要の判定 (推論。裏付けは §1-3 の観察)

### 残すべき床 (常時オン、コスト小・効果実証済み)

- 別プロセス + kill-on-close Job Object + メモリ上限
- private desktop + dialog sweep (#351)
- 出力バウンド、フレーム出力検証、suite/handle 所有権 (壊れたプラグインを
  診断に変換する層。#177 のような退行はあったが機構自体の価値は履歴が支持)
- 依存 closure の自動解決 (#304/#360。これは制限ではなく互換性機能)
- broker 検証済みターゲットのハンドル渡し (#18→#66。診断成果物の object
  identity 保護)
- 記録としてのハッシュ (record)。観察をバイト列に結びつけるのは安価で有用

### evidence tier 限定に降格すべきもの (oracle corpus 生成時のみ)

- sealed load tree + 事前ハッシュ enforce (record と enforce の分離は CLAUDE.md
  が既に規定。enforce 側は provenance 用途にしか意味がない)
- receipt / allowlist (実態として evidence 経路にしか残っていない)
- module audit (検出実績が正当 DLL のみ = 既定 tier では誤検知製造機)
- restricted token (confidentiality を提供せず、CI で起動不能。守っているのは
  staged tree の完全性のみで、それは evidence の関心事)

### 廃止・置換候補 (どの tier でも見合わない)

- worker freshness gate の fail 強制 (`secure_image_dispatch.rs:524`)。
  minihost/src を 1 ファイル触ると全 image dispatch が opt-out なしで死ぬ。
  「古い worker での誤観察防止」という目的は警告 + 診断への記載で足りる。
  少なくとも明示 override が要る
- `fixture_profiles/mod.rs` のハードコード群 (descriptor manifest digest、
  out_flags 固定値、preflight.rs:24 の fixture sha256)。Direction 1 (fixture 名
  非依存の任意 AEX) と正面衝突する fixture 時代の遺物
- `expires` 文字列一致 (機能していない儀式)
- `l1` の normal-token `run_isolated` (sealed でない最後のプラグインロード経路。
  default tier を公式化するならその実装に置換、しないなら削除)
- 凍結 evidence 48 テスト + refresh スクリプト運用の縮小継続 (behavioral
  self-test への移行は EVIDENCE_POLICY §5 の既定方針。残作業が多いだけ)
- conftest の 4 マニフェスト txt + source_owners.py の owner 追記運用 (テストの
  リネームごとに手動更新。grep テスト全廃後の残存管理コスト)

### 対エージェント統制の扱い (推論)

無承認実行・過大主張・退行の防止という動機自体は実績があり正当。ただし手段を
「実行時 enforcement」から「behavioral self-test + レビュー運用 + 記録
(record-not-enforce)」へ移すのが、リポジトリ自身が §3 amendment 以降歩んで
きた方向。実行時に殺す機構は #90/#177/#354/#651/#653 のとおり、エージェント
ではなく開発そのものを止める。

## 5. 方向転換の選択肢 (推論)

本丸は既に issue になっている: **#678** (open、コメント 0)。AviUtl2 経路を
題材に「crash containment・出力妥当性・handle 所有権・依存解決・診断可能な
失敗は要る / 暗号学的照合・receipt pin・事後証明は要らない」という仕分け表と
実測 (SHA-256 は往復の約 5%) を提示済みで、owner の線引き判断を要求している。
#36 (receipt-free default tier) は「緩和の設計未確定」を理由に not-planned で
close されており、owner は緩和自体に反対していない。#678 がその再挑戦になる。

段階案:

1. **#678 の仕分け表を owner が承認し、tier の線引きを確定する** (方針決定。
   コード変更なし)
2. default tier の公式化: 実装済みの receipt-free 経路 (`render-video-batch` /
   `InteractiveRenderSession` / `dispatch_secure_image` の自己ハッシュ admission)
   を「これが default tier」と文書と CLAUDE.md に固定し、`l1` を移行または削除
3. freshness gate の fail→warn 化 (または明示 override)、fixture ハードコードの
   撤去、`expires` の儀式の廃止
4. module audit / sealed enforce / receipt を evidence 経路 (refresh runner と
   oracle capture) だけに限定
5. 凍結 evidence テストの behavioral self-test への置換を、モジュールを触った
   ついでに継続 (EVIDENCE_POLICY §5.3 の既定運用)

## 付録 A: 経路 × 機構マトリクス (観察、2026-08-05 時点)

| 経路 | receipt | sealed tree | restricted token | module audit | freshness gate | 事前ハッシュ enforce |
|---|---|---|---|---|---|---|
| `selftest` (プラグイン非ロード) | ✗ | ✗ | ✗ | ✗ | ✗ | - |
| `l1` | v1 | ✗ | ✗ (normal token) | ✗ | ✗ | worker 側照合 |
| `l2` / `render*` / `smart*` | v2 | ✓ | ✓ | ✓ | ✗ | ✓ |
| `render_request` 系 4 経路 | v2 | ✓ | ✓ | ✓ | ✗ | ✓ + descriptor digest |
| `render-video-batch` | ✗ | ✓ | ✓ | ✓ | ✓ | 自己ハッシュ bind |
| `dispatch_secure_image` (inspect/probe) | ✗ | ✓ | ✓ | ✓ | ✓ | 選択時ハッシュ |
| session / cluster session (interactive) | ✗ | ✓ | ✓ | close 時 | ✓ | 選択時ハッシュ |

主な強制実装の所在: receipt 検証 `host_core/approved_artifact.rs:94,131`、
sealed tree `sealed_load_tree.rs:646`、worker admission + freshness
`secure_image_dispatch.rs:425,524`、module audit `worker_module_audit.rs:62`、
descriptor digest `host_core/descriptor_manifest.rs:68` +
`fixture_profiles/mod.rs:61,142`、セッション invariant
`render_session.rs:2797`、handle 所有権 `host-core/src/handle.rs` +
`minihost/src/worker_handle_runtime.cpp`。

## 追記欄 (時系列)

- 2026-08-05: 初版。issue/PR 考古学とコード棚卸しはエージェント調査結果に
  基づき、freshness gate (`secure_image_dispatch.rs:557`) と `expires` 文字列
  一致 (`approved_artifact.rs:107,149`) は本体セッションで直接確認済み。
- 2026-08-05 (同日追記): owner が #678 の keep 側 (crash containment、依存
  closure、出力境界・ハンドル所有権) に同意し、さらに「プラグイン経路以外の
  用途でも残りは不要ではないか」との見解。これを受けた分析 (仮説):
  - evidence 用途ですら必要なのは record (ロード実バイトのハッシュ、モジュール
    一覧、環境の記録) であって enforce ではない。比較時にハッシュ不一致の
    evidence を捨てれば足り、生成時 fail-closed の理由がない。
  - #651/#665 系のビルド事故は、gate ではなくキャッシュキーに plugin+worker
    ハッシュを含めることで構造的に解決できる。freshness gate は fail から
    警告 + 記録へ。
  - 訂正: §4 で restricted token を「evidence tier 限定に降格」としたが、
    実機能は staged tree の deny ACE の主体のみで sealed tree の付属品。
    tree を enforce から外すなら token も撤去対象 (CI 起動不能 #335 も解消)。
    クラッシュ封じ込めの実体は Job Object + 別プロセス + private desktop。
  - 帰結: 「observation/evidence の二層 tier」ではなく「実行は常に同一の軽い
    床 + record の濃さだけが違う」単層構造が最も単純。#678 の提案より一歩
    踏み込んだ形になる。
- 2026-08-05 (同日追記 2): 上記を owner 決定として #678 にコメント投稿
  (issuecomment-5186799469)。決定の骨子: 単層 + record 方針、restricted token
  を keep 側から除外、module audit の記録化、receipt 廃止、freshness gate の
  warn 化 (multifilter discovery cache の BuildFingerprint パターンをビルド
  事故対策の正とする)、ハッシュ不一致は #309 の形 (エラーでなく再検証トリガー)
  に統一。実装は 7 項目に分解して別 issue 化する。
- 2026-08-05 (同日追記 3): 実装 issue を起票。#728 (docs の単層 + record
  書き分け、他 issue の前提)、#729 (freshness gate warn 化)、#730 (module
  audit 記録化)、#731 (restricted token / sealed ACL 撤去と staging 簡素化)、
  #732 (receipt / allowlist 整理と l1 の始末)、#733 (fixture_profiles
  ハードコードと expires 撤去)。フレーム毎ハッシュの非暗号化は既存 #690。

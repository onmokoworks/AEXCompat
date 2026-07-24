# クラスタセッション プロトコル v1 (issue #405)

status: 設計確定版。実装は本書に従い、逸脱は実装 PR で「逸脱」として記録する。
前提文書: `docs/RENDER_SESSION_PROTOCOL_2026-07-19.md` (以下 RS 書)、
`docs/AE_EFFECT_LOADING_INVESTIGATION_2026-07-22.md` (封入コスト実測)。

## 1. 位置づけとスコープ

同一の依存 DLL クロージャ (以下 closure) を共有する複数の AEX プラグインを、
**1 worker プロセスに closure を 1 回だけマップし、プラグイン本体だけを
差し替えるセッション** (クラスタセッション) で処理する。

動機 (実測): 39 件のプラグインが同一の 308 DLL closure を持つクラスタで、
従来はプラグインごとに新規プロセスへ closure 全体を staging・認証・
ロードしていた (計 ~530 秒、sweep 全体の半分以上)。クラスタセッションでは
staging・ACL・LoadLibrary・Adobe ランタイムの DllMain 初期化がクラスタで
1 回に償却される。

対象経路:

- **discovery** (L2 `--l2-params-only` 相当のパラメーター inspect)。
  従来 one-shot 専用だったものにセッションモード `--discovery-session-v1`
  を新設する。
- **render** (RS 書の `--render-session-v1` 系)。既存セッションに
  `swap_plugin` メッセージを追加する。

スコープ外:

- one-shot 経路の audit module 数上限 (128) の引き上げ (#394 本体)。
  本書の宣言集合による検証はクラスタセッション経路にだけ適用する。
- YMM4 / OpenFX など aviutl2-multifilter 以外の bridge への展開。
- 異なる session 構成 (深度・寸法・time 系) 間での render プラグイン
  差し替え (§6)。

## 2. 信頼モデル: クラスタマニフェスト

RS 書 §3 の原則「**セッションの信頼判断・構成は launch 時 argv で全て確定し、
起動後に信頼を変えるメッセージは存在しない**」を維持する。差し替え対象の
プラグイン集合は launch 前に全て認証し、swap メッセージは認証済み集合内の
**index の選択**だけを運ぶ。パス・ハッシュはメッセージに載せない。

### 2.1 cluster-manifest-v1

broker が launch 前に生成し、worker に渡す JSON (転送方式は §2.3 追記を
参照: 初版では `--aux-manifest-v1` / `--parameter-animation-v1` と同型の
「broker 作成・一時ファイル・argv で絶対パスを渡す」転送を予定していたが、
worker の sealed root 検証モデルに合わせ、sealed root 内 staging に確定した):

```json
{
  "schema": "cluster-manifest-v1",
  "plugins": [
    {"basename": "A.aex", "sha256": "<64hex>", "payload": "v2|...|..."}
  ],
  "dependencies": [
    {"basename": "dvacore.dll", "sha256": "<64hex>", "size": 1234567}
  ],
  "module_bound": 400
}
```

- `plugins`: そのセッションでロードを許すプラグインの順序付きリスト。
  index は 0 始まりの配列添字。`payload` は render セッション用の optional
  (符号化・上限 16384 バイト・ASCII のみ、は launch argv payload と同一規則。
  discovery セッションでは key 自体を置かない)。
- `dependencies`: 共有 closure の全エントリ。broker は
  `plugin_dependency_closure` の解決結果と `session_dependency_manifest` の
  検証 (basename 規則・衝突拒否・reparse point 拒否・single-link・サイズ +
  SHA-256 再照合) をそのまま通したものだけを書く。緩和はしない。
- `module_bound`: 宣言する module 数上限 (§5)。
- manifest 全体の上限: `plugins` ≤ 256 件、`dependencies` 以下の合計で
  `module_bound` ≤ 4096、本文 ≤ 4 MiB。超過は broker 側でクラスタセッション
  不成立 (fail-closed、従来経路へ)。

### 2.2 launch argv との関係

render セッションは argv 位置引数の契約 (RS 書 §3) を変えない。
`--render-session-v1 <plugin> <plugin_sha256> <payload> ...` は
**plugins[0]** を指し、auxiliary option として
`--cluster-manifest-v1 <path>` を tail に追加する。worker は argv の
plugin/sha256 が `plugins[0]` と一致することを launch 時に検証する
(不一致は起動失敗)。ペイロードは argv 側を優先し、manifest 側 `payload`
は plugins[1..] の swap 時にだけ使う。

discovery セッションは位置引数を新設する:

```
aex_render_worker.exe --discovery-session-v1 --cluster-manifest-v1 <path>
```

プラグインの初期ロードは行わず、最初の `inspect_plugin` (§4.2) で
plugins[N] をロードする。

### 2.3 sealed tree

クラスタの全プラグインと closure 全エントリを **1 つの sealed root** に
staging する。root はフラットなので、差し替えたプラグインのモジュールも
audit の `plugin` 分類規則 (親 dir == プラグインの親 dir) をそのまま通る。
staging・ハッシュ・ACL・trusted worker stage はクラスタで 1 回。

**追記 (2026-07-23 結合確認で確定)**: cluster manifest 本文も sealed root
内に staging する。broker は manifest を broker 所有の一時ファイルに書き
出し、それを staging ソースとして sealed tree の 1 エントリ
(`cluster-manifest-v1.json`、ハッシュ検証・ACL 適用は他エントリと同一)
として sealed root に格納し、argv の `--cluster-manifest-v1 <path>` には
**staged 後の sealed root 内パス**を渡す。worker は「manifest の親 dir が
canonical に sealed root (render では argv plugin の親 dir と一致)」である
ことを検証するため、一時ディレクトリ配置は成立しない。staging ソースの
一時ファイルは staging 完了後に削除し、sealed コピーは tree の寿命で
管理される。`cluster-manifest-v1.json` の basename は予約名で、プラグイン
や依存が同名を名乗ると衝突拒否で fail-closed となる。

**追記 (2026-07-23 実機結合で確定)**: sealed root は Rust 側
`fs::canonicalize` 由来の verbatim (`\\?\`) 形式で worker に渡るが、MSVC
の `std::filesystem::canonical` は verbatim 入力を拒否する。worker は
manifest パス (および render の launch plugin root) の canonical 同一性
チェックの前に `\\?\` / `\\?\UNC\` プレフィックスを剥がす
(`worker_cluster_manifest.cpp` の `cluster::normalize_verbatim`)。
「broker が書いた exact path と一致する」契約自体は維持する (#231 の
aux manifest で起きた問題と同型の MSVC 差異)。

## 3. closure のピン留め

プラグインを `FreeLibrary` すると、その import 経由でロードされた依存 DLL
の refcount が減り、他に参照がなければ依存 DLL もアンロードされる。これでは
closure 保持の意味がないため:

- worker はセッション開始時 (discovery: 最初の inspect 前、render: launch
  admission 時) に、manifest `dependencies` の全エントリを **1 つずつ明示
  `LoadLibraryExW` (sealed root + System32、現行と同フラグ) し、HMODULE を
  セッション寿命まで保持**する (ピン)。
- ピン成功後に各 HMODULE の実体を manifest の sha256/size と照合する
  (staging 済みファイルの照合は broker 側 SealedLoadTree が既に行っている。
  worker 側の照合はロードしたモジュールのパスが sealed root 直下である
  ことの確認で、fail-closed の二重化)。
- ピンの 1 つでも失敗すればセッション不成立。既にピンした分を逆順に
  `FreeLibrary` して非 0 で終了する (broker は従来経路へフォールバック)。
- `swap_plugin` で `FreeLibrary` するのはプラグイン本体の HMODULE のみ。
  ピン済み依存は refcount が残りアンロードされない。
- セッション close 時にプラグイン → ピン依存の順で解放する。

## 4. 制御チャネル: メッセージ仕様

フレーミング・strict 検証は RS 書 §4.1 と同一 (u32 LE 長さ接頭辞 + UTF-8
JSON、strict exact-key、未知 `type` / `v` はプロトコル違反でセッション
無効化)。1 メッセージ上限は従来 64 KiB だが、discovery の `inspect_done`
だけはパラメーター一覧を運ぶため **4 MiB** を別途許容する (それ以外の
メッセージは 64 KiB のまま)。worker はパス文字列を開かない原則は維持し、
パラメーター JSON は応答パイプで返す。

### 4.1 render セッションへの追加: swap_plugin

broker → worker:

```json
{"v":1,"type":"swap_plugin","plugin_index":1}
```

- 許可キーは `v` / `type` / `plugin_index` のみ。`plugin_index` は
  manifest `plugins` の添字で、範囲外・現在と同一 index はプロトコル違反。
- swap の列 (worker 側、§5 の audit と同期):

  1. SEQUENCE_SETUP 済みなら SEQUENCE_SETDOWN、続いて GLOBAL_SETDOWN。
     **GLOBAL_SETUP / GLOBAL_SETDOWN は対**であり、worker は
     setup/setdown の対状態を保持して、未対の setup が残る場合にだけ
     SETDOWN を発行する (2026-07-23 実機結合で確定。discovery の
     inspect 列は one-shot 同等に dispose までの SETDOWN を内包して
     おり、対状態を持たない実装は二重 SETDOWN で実 AEX が非 0 を返し
     swap 失敗 = exit 25 になった)。
  2. quiesce (`WorkerSession::set_pre_unload_hook` / `quiesce_once` の
     現行機構。コールバックが走りうる間は unload しない)。
  3. quiesce 不合格・GLOBAL_SETDOWN 失敗・`FreeLibrary` 失敗 →
     **セッション即時無効化**。terminal audit を capture し、専用の非 0
     exit code (§7) で終了する。汚染の可能性がある状態で次に進まない。
  4. pre_unload audit snapshot を epoch として記録 (§5)。
  5. plugins[N] を LoadLibrary → sha256 照合 → GLOBAL_SETUP →
     post_load audit snapshot を epoch に記録。以降の PARAMS_SETUP /
     SEQUENCE_SETUP は RS 書 §5 の現行契約どおり (遅延 SEQUENCE_SETUP は
     最初の render_frame 受信時)。
  6. `payload` があれば launch argv payload と同じ器で適用する。

worker → broker:

```json
{"v":1,"type":"swap_done","plugin_index":1,"status":"ok"}
```

- `status:"error"` は `global_setup_error` を持つ専用形。GLOBAL_SETUP の
  非 0 返しは**プラグイン局所エラー**でセッションは継続可能 (次の
  render_frame で -47 系の継続不可応答を返す現行契約に揃えるか、
  broker が続行判断する)。quiesce / unload / audit の失敗は応答を返さず
  worker 終了で表す (§7。臨終メッセージは設けない RS 書 §4.3 の方針)。
- swap 完了までは `render_frame` を受理しない (broker は swap_done を
  待ってから次フレームを送る。単純な逐次契約)。

### 4.2 discovery セッション: inspect_plugin

broker → worker:

```json
{"v":1,"type":"inspect_plugin","plugin_index":0,"request_index":0}
```

- 許可キーは `v` / `type` / `plugin_index` / `request_index` のみ。
  `request_index` は 0 始まりの通し番号 (重複・逆順はプロトコル違反)。
- 現在のプラグインと異なる index が来たら §4.1 と同じ swap の列
  (GLOBAL_SETDOWN → quiesce → unload → load → GLOBAL_SETUP) を先に行い、
  続いて `--l2-params-only` と同じ inspect (ABOUT 省略・PARAMS_SETUP) を
  実行する。同一 index の連続 inspect も許す (再 inspect)。

worker → broker:

```json
{"v":1,"type":"inspect_done","plugin_index":0,"request_index":0,
 "status":"ok","report":{ ... 従来の --l2-params-only 最終 JSON ... }}
```

- `report` は従来 one-shot が stdout に出す JSON と同じ形 (module_audit
  を除く。audit は §5 の epoch / 最終レポートが担う)。broker は従来の
  inspect 結果と同じ器で消費でき、one-shot との A/B 等価性を直接比較
  できる。
- `status:"error"` はパラメーター局所の失敗 (entrypoint 解決不可、
  PARAMS_SETUP 非 0 など従来 exit 12/20 に相当) を構造化して運び、
  セッションは継続可能。続行判断は broker。
  **追記 (2026-07-23 結合確認で確定)**: error 形は
  `{"v":1,"type":"inspect_done","plugin_index":N,"request_index":M,
  "status":"error","error_kind":"entrypoint_unresolved"|"selector_error",
  "report":{...optional(部分レポート)...}}` とする。broker は
  `error_kind` を必須・既知値限定で strict 検証する。
- `close` は RS 書 §4.2 と同じ `{"v":1,"type":"close"}`。
- **追記 (2026-07-23 実機結合で確定)**: discovery セッションの最終
  レポート (stdout) の envelope は `stage:"discovery_session"` +
  `status:"discovery_session_completed"` で、broker の clean 判定は
  この 2 キー + module_audit 検証に基づく。worker が最終レポートを
  残せずに死んだ場合の診断用に、broker は worker の `stderr_tail`
  (末尾 4 KiB) を回収して invalidation 診断に含める。

### 4.3 render の swap 適用条件

render の launch 構成 (深度・`max_width`/`max_height`・`time_step`/
`time_scale`・layer・mask/spatial/render context・GPU backend) は
argv で固定される。`swap_plugin` が変えるのはプラグイン image と
payload だけなので、**同一構成のプラグイン間でだけ swap できる**。
構成が違うプラグインは新規セッションを開く (現行どおり)。broker は
この条件を呼び出し側の契約として強制し、worker は構成の一部を
メッセージで受け取らない (受け取る時点で信頼変更であり設計違反)。

## 5. module audit: epoch モデルと宣言集合

- 現行の 1 レポート = post_load / pre_unload / observed_union の 3
  snapshot に、swap ごとの **epoch** を追加する:

```json
"epochs": [
  {"plugin_index": 0,
   "pre_unload": { ...snapshot... },
   "post_load":  { ...snapshot... }}
]
```

  epoch N の pre_unload は plugins[N] の解放直前、post_load は
  plugins[N+1] への swap 完了直後 (最後のプラグインの pre_unload は
  従来の終端 pre_unload が兼ねる)。observed_union は従来どおり全
  snapshot の累積 union で、単調増加を壊さない。

- **宣言集合による検証**: クラスタセッション経路では、固定上限
  `MAX_AUDITED_MODULES = 128` (`worker_module_audit.rs:7`) の代わりに
  manifest の宣言を使う。

  - `plugin` クラスの snapshot エントリは、manifest の
    `plugins[].basename` ∪ `dependencies[].basename` の部分集合で
    なければならない。
  - snapshot の総 module 数は `module_bound` 以下。worker 側
    `kMaxAuditedModules = 512` (`runtime_module_audit.cpp:21`) も
    クラスタセッションでは manifest の `module_bound` に置き換える。
  - `unknown_count == 0`、system32/winsxs の分類規則、basename 検証、
    重複拒否、post_load ⊆ union / pre_unload ⊆ union は従来どおり
    fail-closed。緩和しているのは「数の上限の根拠を固定値から launch
    時認証済みの宣言に変える」点だけで、許容集合そのものは宣言で
    狭まっている。
  - one-shot 経路の validator は変更しない (#394 は別 Issue)。

## 6. fail-closed フォールバック

セッション無効化 (プロトコル違反、host-protection invariant 失敗、
quiesce/unload 失敗、audit 不一致、watchdog タイムアウト、worker 死)
が起きた時点で:

- そのプラグインの結果は失敗として構造化記録する (成功に丸めない)。
- クラスタの未処理メンバーは**従来の per-plugin 新規 dispatch**
  (one-shot inspect / 新規 RenderSession) で処理する。
- フォールバックが発生したこと・何件目で・理由は、sweep の結果
  レポートに構造化して残す。
- crash containment (Job Object / restricted token / sealed staging /
  stdout capture) は現行セッションと同一構成。

## 7. 安全境界・exit 契約

- watchdog: render は per-frame deadline (RS 書 §7)、discovery は
  per-inspect deadline を broker が張る。従来の params-only が期限なし
  (`image_render.rs:3206`、issue #354) であるのに対し、セッションは
  常駐するため「応答が永遠に来ない」状態を broker が検出できる必要が
  ある。deadline 超過はセッション無効化 + TerminateJobObject で、
  個別プラグインの成否を wall-clock で決めない (結果は「不明」として
  フォールバック先の one-shot が改めて判定する)。
- exit 契約: 正常終了は close (またはパイプ EOF) 後の exit 0 のみ。
  自発的終了は (a) プロトコル違反、(b) host-protection invariant 失敗、
  (c) **swap 失敗 (quiesce / unload / audit)** の 3 系統で、いずれも
  terminal audit → 最終レポート → 専用の非 0 exit code。(c) は
  23/24 (プロトコル違反 / invariant) とは別のコードを割り当て、broker
  が「汚染疑いによる中断」と「クラッシュ」を診断上区別できるようにする
  (具体値は実装時に確定し本書に追記する)。
  → **(c) の具体値は 25 に確定** (2026-07-23 worker 実装時)。render
  セッションの swap 失敗 (SEQUENCE/GLOBAL_SETDOWN 失敗・quiesce 不合格・
  FreeLibrary 失敗・audit 不一致・認証済みプラグインの load 失敗) と
  discovery セッションの同系統失敗の両方に使う。
- worker 終了の検知・3 者待ち・臨終メッセージを設けない方針は RS 書
  §7 と同一。

## 8. 実装マッピング

worker (minihost):

- manifest ロード・検証: strict_json exact-key、`load_aux_manifest`
  (`worker_pf_ae_channel_runtime.cpp`) と同型のパス検証。
- ピン留め: `worker_runtime_admission.cpp` の admit 列に dependencies の
  明示 LoadLibrary を追加 (render)。discovery セッションは新モード
  `--discovery-session-v1` を `classify_worker_mode` に追加。
- swap: `WorkerSession::set_pre_unload_hook` / `quiesce_once` /
  `unload_module` (`worker_session.cpp`) を「プラグインのみ unload・
  cookie とピンは保持」に拡張。render session ループ
  (`worker_render_session.cpp`) に `swap_plugin` 処理を追加。

broker:

- manifest 型・生成・検証は `session_dependency_manifest.rs` と同じ
  `broker/crates/broker/src/` に新モジュール。`SealedLoadTree` に複数
  プラグインエントリの staging と basename 解決を追加
  (`sealed_load_tree.rs:226-235` の先頭固定を拡張)。
- `dispatch_secure_image_session` のクラスタ版。`RenderSession::
  swap_plugin(index)`、新規 `DiscoverySession` (open / inspect / close)。
  `SecureSessionProcess` / 3-way wait / `invalidate` を流用。
- audit validator に epoch 検証と宣言集合モードを追加
  (`worker_module_audit.rs`)。
- フォールバック orchestration は呼び出し側 (bridge) が持つ。

bridge (aviutl2-multifilter):

- `discover_all` で closure identity (依存エントリ集合の正規化ハッシュ)
  によりクラスタリングし、クラスタごとに 1 DiscoverySession。単独
  クラスタは従来 one-shot。
- render は (closure identity, session 構成) キーのセッションプールで
  `open_mf_session` を包み、同キーのエフェクト切替を `swap_plugin` で
  処理する。

## 9. 検証

- fixture worker (`session_protocol_worker`) に swap_plugin / discovery
  セッションの応答を追加し、broker 統合テストでプロトコル往復を証明。
- 等価性: 同一プラグイン群を (a) 従来 one-shot / 個別セッション、
  (b) クラスタセッションで処理し、discovery の params JSON・render の
  出力 + 公開レポートが一致すること (A/B)。**速度のために結果を変え
  ない**ことが本 Issue の第一の完了条件。
- 異常系: quiesce 不合格の注入 → セッション無効化 + フォールバック、
  manifest 外 index → プロトコル違反、audit 不一致 → fail-closed。
- 計測: `examples/sweep_stage_timing.rs` をクラスタ対応に拡張し、
  同一クラスタの sweep 時間を before/after で記録する。

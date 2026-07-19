# 常駐レンダリングセッション プロトコル v1 (issue #98 段階1)

status: 設計確定版 (段階0 調査の結論に基づく)。実装は本書に従い、実装中の
逸脱は `docs/RENDER_SESSION_INVESTIGATION_2026-07-19.md` に「逸脱」として
追記する。根拠となる観察はすべて同調査ノートにある。

## 1. 位置づけとスコープ

フレーム列 (動画) を主対象とする broker↔worker の常駐レンダリング
セッション。1 worker プロセスが SEQUENCE_SETUP を 1 回だけ発行し、
sequence data を保持したまま複数フレームをレンダーする。既存の one-shot
argv 経路は不変のまま残す (縮退ケース化は独立 PR、issue #98 段階1-3)。

v1 のスコープ:

- Classic 経路 (`render_once(manage_sequence=false)` の前例あり、
  `l2_main.cpp:7250-7280` の `persistent_sequence`)。
- パラメーターは `--parameter-animation-v1` タイムラインの事前転送 +
  レンダー時刻評価 (`apply_parameter_animation`、`l2_main.cpp:2406`)。
- tier は default tier (crash containment)。hash の記録は現行どおり行うが、
  セッションであることによる追加の enforce はしない。

v1 のスコープ外 (プロトコルは拡張点を予約する):

- SmartFX セッション。`smart_render_runtime` に `manage_sequence` 相当が
  無く (`l2_main.cpp:4998` で常に SEQUENCE を張る)、worker 側の追加工事が
  要る。メッセージ仕様は selector 非依存なので v1.1 で worker 側のみ拡張。
- per-frame の動的パラメーター割当 (#107 GUI ライブ操作、AviUtl2 ブリッジが
  要求)。`render_frame` メッセージの追加フィールドとして予約 (§4.2)。
- SEQUENCE_RESETUP の発行。レンダー専用文脈での実発行頻度は AE 実機観測
  (段階0 項目1・2) 待ち。v1 は発行しない。
- リングバッファ / 先読み。単一スロット逐次で開始 (設計ドラフト v1 合意)。

## 2. 全体構造

```
broker (RenderSession)                    worker (--render-session-v1)
  launch: argv で静的構成を確定 ──────────→ AEX ロード、GLOBAL_SETUP →
  (plugin, descriptor, params,             ABOUT → PARAMS_SETUP →
   timeline, 上限寸法, 深度)               SEQUENCE_SETUP (1 回)
  ┌──────────── フレームループ ─────────────┐
  │ 入力スロットへ書込 + generation 更新    │
  │ control pipe: render_frame ──────────→ │ 入力スロット読取 →
  │                                        │ FRAME_SETUP → RENDER →
  │                                        │ FRAME_SETDOWN →
  │ ←────────── frame_done : control pipe  │ 出力スロット書込 + generation
  │ 出力検証 (bounds/pixel/checksum)        │
  └────────────────────────────────────────┘
  control pipe: close ──────────────────→ SEQUENCE_SETDOWN →
  プロセス exit 待ち + 最終レポート回収 ←── GLOBAL_SETDOWN → stdout に
                                            最終 JSON レポート → exit 0
```

チャネルは 3 本。すべて broker が作成し、`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`
で継承を絞り、handle 番号を環境変数で伝達する (trace handle
`AEX_INSTRUMENT_TRACE_HANDLE` と同型の前例、`windows_process.rs:155,349`、
worker 側パースは `trace_writer.cpp:16-28` の型)。worker はパス文字列を
一切開かない (#18 の教訓)。

| チャネル | 実体 | 環境変数 | 方向 |
|---|---|---|---|
| 制御 (要求) | 匿名パイプ read 端 | `AEXCOMPAT_RENDER_SESSION_REQUEST_HANDLE` | broker→worker |
| 制御 (応答) | 匿名パイプ write 端 | `AEXCOMPAT_RENDER_SESSION_RESPONSE_HANDLE` | worker→broker |
| データ | 無名 file mapping | `AEXCOMPAT_RENDER_SESSION_SECTION_HANDLE` | 双方向 |

決定の根拠 (調査ノート項目5):

- stdin/stdout は使わない。stdout は最終 JSON レポート専用・stdin は null と
  いう現行契約 (`windows_process.rs:385`) を壊さない。
- 無名 file mapping は名前も パスも ACL も介さず継承 handle だけで
  restricted token の worker から `MapViewOfFile` できる (spike 実証)。
- 共有 view は Job Object の `ProcessMemoryLimit` に課金されない (spike
  実証: cap 128MiB < view 256MiB で全ページ touch 後も private commit 増分
  ~0.5MiB)。mapping サイズは crash containment のメモリ予算と独立に決めて
  よい。ただし §6 の上限は別途設ける。
- 同期往復は 数十μs 級 (spike: event 対で median 16.6μs)。パイプの
  往復もレンダー本体 (warm FHD 123.5ms) に対して誤差であり、v1 は診断
  しやすい length-prefixed メッセージを選ぶ (設計ドラフト v1 の提案どおり)。
  イベントのみの低レイテンシ構成は対話ブリッジで不足したときの拡張。

## 3. launch 時の静的構成 (argv)

セッションの信頼判断・構成は launch 時 argv で全て確定し、起動後に信頼を
変えるメッセージは存在しない (設計ドラフト v1 の「open メッセージを持たない」
合意)。既存 one-shot の位置引数 (`worker_request_parser.cpp:73-95`) を
可能な限り踏襲する:

```
aex_render_worker.exe --render-session-v1 <plugin> <plugin_sha256> <payload>
    <max_width> <max_height> <time_step> <total_time> <time_scale>
    [v2|<mask context>] [spatial:v*|<...>] [render:v1|<...>]
    [--aux-manifest-v1 <path>] [--parameter-animation-v1 <path>]
    [--dump-worlds-v1 <dir>] [--output-checksum-detail-v1 1] [--minidump-v1 <dir>]
```

静的コンテキスト trailer は one-shot と同じ位置引数順 (mask → spatial →
render) で、auxiliary option ペアより前 (argv 上ではコンテキストが先、
auxiliary option が tail)。worker は auxiliary option を tail から剥がした
後、render → spatial → mask の順に位置引数末尾から剥がして 10 スロットの
セッション契約に還元する。W1-3 では mask (`v2|`)、spatial (`spatial:v1/v2/v3`)、
render-environment (`render:v1|`) を broker が送出する (host_context がある
ときは one-shot と同じく mask trailer を常に送る、空 mask scene でも "v2|")。
alpha-as-coverage と aux channels は layer 作業 (W1-4) と同 PR。値域・検証は
one-shot と同一 (parse_mask_context_payload / parse_spatial_context_payload
/ parse_render_environment_payload)。full-resolution 寸法を宣言する spatial は
遅延 SEQUENCE_SETUP と全フレームの in_data に反映される。

one-shot との差分:

- 入力 raw パス・出力パスを取らない (共有メモリスロットに置換)。
- `current_time` を取らない (per-frame メッセージで供給)。`time_step` /
  `total_time` / `time_scale` は launch 固定。検証規則は one-shot と同じ
  (`worker_request_parser.cpp:81-82`)、加えて `time_step > 0` は FRAME 系
  文脈のため必須 (調査ノート項目3)。
- `width`/`height` の代わりに `max_width`/`max_height` (スロット寸法上限) を
  申告する。実フレーム寸法は per-frame にヘッダで渡すが、v1 は
  「全フレーム同寸法」を要求し、上限=実寸法とする (縮小フレームの扱いは
  リングバッファと同時に検討)。
- 深度はコマンド語で表現する (one-shot の `--render-image` /
  `--render-image16` / `--render-image32` に倣い、`--render-session-v1` /
  `--render-session16-v1` / `--render-session32-v1`)。

## 4. 制御チャネル: メッセージ仕様

### 4.1 フレーミング

u32 LE の長さ接頭辞 + UTF-8 JSON 本文。1 メッセージ上限 64 KiB (制御情報
のみでピクセルは載らないため十分。超過は fail-closed でセッション無効化)。
応答パイプの読取は broker 側で per-frame deadline を張る (§7)。

### 4.2 broker → worker

```json
{"v":1,"type":"render_frame","frame_index":0,
 "current_time":{"value":0,"scale":30}}
```

- `frame_index`: 0 始まりの通し番号。generation 検証 (§6) に使う。
- `current_time`: 有理数時刻。`scale` は launch の `time_scale` と一致必須
  (不一致はフレーム単位エラー応答)。任意時刻を許す: セッションは順序・
  単調性を仮定しない (Effect API にレンダー順序契約は存在しない、調査
  ノート項目3)。
- **メッセージ検証は strict (fail-closed)**: worker は exact-key 検証
  (`strict_json` の `json_exact_keys` と同じ流儀) を行い、未知フィールド・
  未知 `type`・`v != 1` はプロトコル違反としてセッションを終了する (§7)。
  黙って無視する経路は設けない: 将来の per-frame `parameters` (#107 /
  AviUtl2 向け) や `param_epoch` (RESETUP 意味論確定後) は `v` の増分と
  ともに導入し、旧 worker に送ると fail-closed になることで「stale な
  launch 時パラメーターのまま ok を返す」誤動作を構造的に排除する。

```json
{"v":1,"type":"close"}
```

- SEQUENCE_SETDOWN → GLOBAL_SETDOWN → 最終レポート → exit 0。
- broker がパイプを閉じた場合 (close 送信前のプロセス異常等) も worker は
  read エラーで同じ終了列に入る (孤児化しても自走しない)。

### 4.3 worker → broker

```json
{"v":1,"type":"frame_done","frame_index":0,"status":"ok",
 "output":{"width":1920,"height":1080,"rowbytes":7680,
           "pixel_format":"argb8","checksum":"<sha256>",
           "guards_intact":true},
 "render_error":0,
 "generation":1}
```

- `status`: `"ok"` | `"error"`。`"error"` のうち**フレーム局所の互換性診断**
  (selector 非 0、`render_error` 非 0、時刻 scale 不一致) のみセッション
  継続可能で、続行判断は broker 側 (バッチ CLI は既定で中断)。
- **SEQUENCE_SETUP の失敗も継続不可**: 遅延発行された SETUP が非 0 を返した
  セッションは何もレンダーできないため、専用コード (-47) の error 応答を
  最後に受理を停止する。broker はこれをフレーム局所診断として再利用させず
  セッションを無効化する (plug-in 自身の setup エラー値は最終レポートの
  `persistent_sequence_setup_error` が運ぶ)。
- **host-protection invariant の失敗は継続不可**: `guards_intact` false、
  出力 bounds/寸法検証の失敗、generation 不一致、ヘッダ改変の検出は
  フレーム局所エラーではなくセッション無効化。worker は該当 frame_done を
  送信した後、以降の `render_frame` を受理せず終了列 (§7) に入る。broker は
  該当 frame_done (または worker 死) を観測した時点でセッションを無効化
  する。壊れた可能性のある worker 状態を次フレームへ引き回さない。
- `status:"error"` の応答は `output` と `generation` を持たない専用形:

```json
{"v":1,"type":"frame_done","frame_index":0,"status":"error","render_error":-40}
```

  レンダー前に拒否されたフレーム (時刻 scale 不一致等) では出力スロットも
  `output_generation` も更新されないため、stale な値を運ばない。broker は
  error 応答ではスロットを読まない。
- `output` の各値は broker 側 per-frame 検証 (§7) の入力。`checksum` は
  **出力スロットへ転送した RGBA バイト列 (broker が読むバイトそのもの) の
  sha256**。one-shot の `output_hash` は内部 ARGB 論理バッファの hash
  (`worker_classic_execution.cpp:43-58`) で定義が異なることに注意。最終
  レポート (§4.4) の `output_hash` は従来定義のままとし、フレーム転送の
  検証には `frame_done.output.checksum` を使う。
- プロセス死・ハングは frame_done が来ないことで broker が検出する。
  `type":"fatal"` のような臨終メッセージは設けない (クラッシュ時に送れる
  保証がなく、二重の死亡経路は診断を曖昧にする)。

### 4.4 最終レポート (stdout)

exit 直前に現行 one-shot と同型の JSON レポートを stdout へ 1 回出力する
(`l2_main.cpp:7371-7466` の型を流用)。内容はセッション集計: フレーム数、
suite イベント (現行の bounded 記録)、lifecycle セレクタの発行回数と
エラー、missing suites。per-frame の詳細は frame_done 側が正本で、最終
レポートは集計と突き合わせ用。stdout capture 上限 24MiB
(`windows_process.rs:35`) の現行契約に収まるよう、per-frame 明細は含めない。

## 5. worker 側 lifecycle

```
launch → admit_worker_entry → AEX ロード → GLOBAL_SETUP → ABOUT →
PARAMS_SETUP (effect_bootstrap::run) → SEQUENCE_SETUP (1 回。発行は最初に
レンダーに到達した render_frame の受信時で、in_data にはそのフレームの
current_time を seed する — one-shot が SEQUENCE_SETUP 前に要求時刻を seed
するのと同じ観測になる。invoke_sequence_selector と同経路で
publish_effect_sequence) →
  loop {
    render_frame 受信 → 入力スロット読取・検証 →
    apply_parameter_animation(current_time, time_scale) →
    FRAME_SETUP → RENDER → FRAME_SETDOWN
    (render_once(manage_sequence=false) の既存境界、l2_main.cpp:4836/4563) →
    出力スロット書込 + generation 更新 → frame_done 送信
  }
→ close 受信 (または制御パイプ EOF) → SEQUENCE_SETDOWN → GLOBAL_SETDOWN →
最終レポート → exit 0
```

- sequence data はループ間で保持する (`render_lifecycle.cpp:81` の
  transfer_pointer 済み handle を `manage_sequence=false` でループ外に
  ホイスト。`persistent_sequence` の前例どおり)。
- 非 MFR Classic 意味論: RENDER 中の sequence_data 書き換えは許容し、同じ
  handle を次フレームへ引き回す (AE 単一スレッド Classic と同挙動、調査
  ノート項目3)。MFR (`SUPPORTS_THREADED_RENDERING`) を宣言する plugin は
  診断に flag を記録する。render 中 NULL 化の再現は v1 では行わない
  (項目3 の設計課題として残置。行わないことも診断に明示する)。
- フレームループ中の suite/handle ownership・出力 bounds・pixel 検証は
  one-shot と同一の fail-closed を per-frame 適用する (host-protection
  invariant、常時オン)。

## 6. データチャネル: 共有メモリレイアウト

無名 file mapping 1 本 (SEC_COMMIT、PAGE_READWRITE)。broker が作成・両者が
全域 map。サイズは launch 構成から決定的に計算され、双方が独立に同じ値を
計算して照合する (worker は section の実サイズが期待値未満なら起動失敗)。

```
offset 0        : SessionHeader (1 ページ 4096B)
offset 4096     : 入力スロット   (max_width * max_height * 4)
align 4096      : 出力スロット   (max_width * max_height * bpp)
align 4096      : レイヤースロット × n (launch の layer 数、各 max_width * max_height * 4)
```

- 入力・レイヤースロットは RGBA8 (4B/px) 固定。one-shot の入力 raw
  transport と同一 (`load_rgba` は深度によらず w*h*4 を要求し、深度昇格は
  worker 内部の `rgba8_to_argb` が行う。深い入力転送は #57 系の既存課題で、
  セッションで新設しない)。出力スロットのみ深度で bpp が決まる
  (8bpc=4B/px、16bpc=8B/px、32f=16B/px。one-shot の出力 raw と同一)。
- rowbytes = width * bpp の密詰め (one-shot raw ファイルと同じ)。
- **スロットは転送専用で、plugin にスロットへのポインタは渡らない**:
  worker は入力スロットを worker 私有バッファへコピーしてから
  PF_EffectWorld を構築し、レンダー先も現行 one-shot と同じ guard 付き
  私有バッファ (`OutputPixelBuffer` の sentinel + NOACCESS ページ、
  `render_pixel_buffer.cpp`) を使う。完成フレームを host コードが
  スロットへ長さ固定でコピーする。plugin の out-of-bounds 書き込みは
  私有バッファの guard で検出され (per-frame `guards_intact`)、mapping 内の
  隣接スロットやヘッダには届かない。section 内に guard 領域を置く必要が
  生じるのは plugin に直接スロットを描かせる zero-copy 構成 (v2 検討) で
  あり、v1 では採らない。
- 上限: `max_width`/`max_height` は one-shot と同じ
  `validate_image_buffer_layout` の上限 (`MAX_DIMENSION=4096`,
  `MAX_PIXELS=16M`) に従う。section 全体は 32f フル構成でも
  16M px × 16B × (2+n) スロット + ヘッダで抑えられ、spike の結果から
  ProcessMemoryLimit とは独立に予算化できるが、broker 側で section 合計
  1 GiB を hard cap とする (異常構成の fail-closed)。

SessionHeader (すべて u32 LE、予約領域は 0 埋め):

```
magic            "AEXS"        (0x53584541)
version          1
depth_code       8 | 16 | 32
max_width, max_height
layer_slot_count
input_generation   (broker が render_frame 送信前にインクリメント)
output_generation  (worker が出力書込完了後に input_generation の値を書く)
frame_width, frame_height     (v1 では max と同値)
```

generation 契約 (stale 応答の fail-closed):

- broker: 入力ピクセル書込 → `input_generation = frame_index + 1` に更新 →
  `render_frame` 送信。
- worker: 受信時に `input_generation == frame_index + 1` を検証 (不一致は
  エラー応答)。出力書込完了後に `output_generation = input_generation` を
  書き、その後 `frame_done` を送信。
- broker: `frame_done` 受信時に `output_generation == frame_index + 1` と
  `frame_done.generation` の一致を検証。不一致 (worker が古いスロットを
  返した等) はセッション無効化。
- ヘッダの他フィールドを worker が書き換えていないことも frame ごとに検証
  する (malformed plugin の host-protection、調査ノートの host 保護方針)。

メモリ同期: 制御パイプのメッセージ順序が happens-before を与える (broker の
スロット書込 → 送信 → worker 受信 → スロット読取)。同一マシン内の
MapViewOfFile 共有ページはキャッシュ一貫で、パイプ往復の同期点で十分。
generation は追加の整合性検証であって同期プリミティブではない。

## 7. 安全境界 (per-frame 化)

- **watchdog**: per-launch timeout を per-frame deadline に置換。broker は
  `render_frame` 送信ごとにデッドライン (既定は現行
  `INTERACTIVE_RENDER_TIMEOUT_MS = 30_000` を流用) を張り、超過で
  `TerminateJobObject`。セッションの Job Object / restricted token /
  sealed staging は現行 `run_isolated_impl` と同じ構成で、生存期間だけ
  セッションに延ばす。
- **クラッシュ/タイムアウト時**: セッション全体を無効化し、診断は
  「frame N で死亡、N 以降は未レンダー」を構造化して返す。自動再起動は v1
  では行わない。再開する場合は新しいセッション (SEQUENCE_SETUP からの
  やり直し) であることを診断に明示する (temporal 状態の喪失を隠さない)。
- **worker 終了の検知**: フレームループ中も broker はプロセス handle を
  監視対象に含める (frame_done 待ちは「応答パイプ or プロセス死 or
  deadline」の 3 者待ち)。
- **exit 契約**: 正常終了は close (またはパイプ EOF) 後の exit 0 のみ。
  それ以外に worker が自発的に終了するのは次の 2 つの fail-closed 経路で、
  どちらも SEQUENCE_SETDOWN → GLOBAL_SETDOWN → 最終レポートを経て
  **非 0 の専用 exit code** で終了し、broker はセッションを無効化する:
  (a) プロトコル違反 (strict 検証に落ちるメッセージ、フレーミング違反、
  §4.2)、(b) host-protection invariant の失敗 (§4.3。該当 frame_done を
  送ってから終了する)。上記以外の自発的 exit・クラッシュは全て
  セッション異常として扱う。

## 8. broker 側 API と実装マッピング

```rust
pub struct RenderSession { /* process, job, pipes, section view, config */ }
impl RenderSession {
    pub fn open(request: SessionOpenRequest) -> io::Result<RenderSession>;
    pub fn render_frame(&mut self, frame: FrameRequest<'_>)
        -> io::Result<FrameResult>;   // 入力ピクセル書込→検証済み出力返却
    pub fn close(self) -> io::Result<SessionReport>;  // 最終レポート
}
```

再利用する既存部品 (broker 側調査より):

- sealed staging + admission: `dispatch_secure_image` の構成部品
  (`secure_image_dispatch.rs:67-94`、`admit_local_worker` 同 102-112、
  `TrustedWorkerStage::populate` `trusted_worker_stage.rs:56-71`) を
  セッション生存期間で保持する形に延命。
- launch: `run_isolated_impl` (`windows_process.rs:303`) を「作成・Job 割当・
  resume まで」と「exit 待ち・回収」に分離し、間にフレームループを挟める
  セッション版を追加する (one-shot 経路は不変)。handle list に section +
  制御パイプ 2 本を追加。suspended cleanup guard は現行のまま。
- per-frame 出力検証: `render_with_artifact` 内の検証列
  (`image_render.rs:4478-5011` の selector/bounds/size/pixel/guard 検証) を
  純粋関数に切り出し、one-shot とセッションで共用する。
- 最初の消費者: バッチ動画レンダー CLI (PNG 連番)。`main.rs` の argv 分岐に
  `render-video-batch` を追加し、`render_request::execute` と同型の
  `fn(repository, request_json, output_json) -> io::Result<bool>` を新設。

worker 側の実装マッピング (worker 側調査より):

- モード追加: `classify_worker_mode` (`l2_cli_dispatch.cpp:63`) に
  `--render-session-v1` 系を追加。`strip_auxiliary_options` は既存のまま
  `--parameter-animation-v1` を受ける。
- 環境変数 handle 受領: `trace_writer.cpp:16-28` の数値パース + 妥当性検証の
  型を踏襲 (パイプは `GetFileType == FILE_TYPE_PIPE`、section は
  `MapViewOfFile` の成否 + サイズ検証で確認)。
- フレームループ: `persistent_sequence` パターン (`l2_main.cpp:7250-7280`) を
  `render_once(..., manage_sequence=false)` (`l2_main.cpp:4836,4576`) で
  ループ化し、`external_current_time` をメッセージから、入出力を共有メモリ
  スロットから差し替える。
- 時刻評価: `apply_parameter_animation` (`l2_main.cpp:2406`) は時刻引数で
  評価するため per-frame 変更なしで流用可能。

## 9. 実装 PR 分割 (段階1)

1. **PR-A**: 本設計ドキュメント (レビューで設計を先に固定する)。
2. **PR-B**: worker `--render-session-v1` モード + フレームループ +
  機械可搬な behavioral self-test (Python から worker を直接駆動し、
  パイプ/共有メモリ契約と selector 発行列・sequence data 継続を検証)。
  実装はレイヤースロット 0 本の構成から始める (プロトコルは n 本を許すが、
  worker はレイヤー付き session 構成を明示的に拒否する。追加は v1.1)。
3. **PR-C**: broker `RenderSession` + バッチ動画レンダー CLI (PNG 連番) +
  broker 統合テスト (worker 実プロセス、per-frame 検証・watchdog・
  クラッシュ無効化の異常系込み)。
4. **PR-D** (独立): 既存 one-shot 経路の「長さ 1 セッション」wrapper 化。
  contract test + conformance の挙動不変を確認してから。

段階0 の残観測 (項目1・2・6) の反映先: RESETUP 発行方針 (§4.2 の
`param_epoch`)、per-frame パラメーター (v2)、SmartFX checkout スロット割当
(v1.1) はいずれもメッセージの追加フィールド + worker 側拡張で入る設計に
してあり、v1 実装をブロックしない。

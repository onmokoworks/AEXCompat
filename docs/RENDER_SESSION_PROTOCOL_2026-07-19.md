# 常駐レンダリングセッション プロトコル v1 (issue #98 段階1)

status: 設計確定版 (段階0 調査の結論に基づく)。実装は本書に従い、実装中の
逸脱は `docs/RENDER_SESSION_INVESTIGATION_2026-07-19.md` に「逸脱」として
追記する。根拠となる観察はすべて同調査ノートにある。
改訂 2026-07-20 (issue #107): §4.2 の予約どおり、`render_frame` の v:2
変種 (per-frame `parameters`) を §4.2.1 として定義した。launch 構成・
ヘッダレイアウト・応答スキーマは v1 のまま変わらない。

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

v1.1 (issue #98 W3) で追加されたスコープ:

- SmartFX セッション (§9.1)。メッセージ仕様 (§4) は selector 非依存のまま
  変更なしで、worker 側のフレームループと broker 側の起動配線のみの拡張。

v1 のスコープ外 (プロトコルは拡張点を予約する):
- per-frame の動的パラメーター割当 (#107 GUI ライブ操作、AviUtl2 ブリッジが
  要求)。当初 `render_frame` メッセージの追加フィールドとして予約し、
  2026-07-20 に §4.2.1 の v:2 変種として定義済み (スコープ外ではなくなった)。
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
    [session-layers:v2|<slot,w,h,handle | slot,w,h,time,scale,handle;...>]
    [v2|<mask context>] [spatial:v*|<...>] [render:v1|<...>]
    [--alpha-as-coverage-v1 <slot,slot,...>]
    [--conformance-render-settings-v1 <v1|mode|0|-|0|renderer>]
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
W1-4c では alpha-as-coverage の parameter slot 群を `--alpha-as-coverage-v1
<slot,...>` auxiliary option で送る。worker (Render entry) は one-shot と共有の
auxiliary フック (`parse_l2_alpha_coverage`) でこれを parse し、launch 時に一度
alpha-coverage provider をグローバルへ publish する。classic render runtime が
毎フレームこれを読むため、session の「一度設定して全フレーム再利用」ライフタイムに
一致し、worker 側の変更は不要。broker open は one-shot と同一の値域検証 (sort・重複
禁止・slot <= 1024) を launch 前に行う。#211 で aux channels
(`host_context.aux_channels`) も session に載った: wrapper は one-shot と同じ
`prepare_aux_transport` で aux channels を検証し、broker 所有の
`target/image-transport` 配下に sidecar (`.f32le`) と `aux-manifest-v1` 形式の
manifest を書き出し、その絶対パスを `SessionOpenRequest::aux_manifest` に渡す
(session/one-shot 双方が同一 manifest を `--aux-manifest-v1 <path>` で同一 worker
に送る)。manifest は broker が一度構築して両 transport で共有し、二重構築しない。
これで HostContext の全フィールド (mask / spatial / render-environment /
alpha-as-coverage / aux channels) が session-representable になり、classic
レンダーに残る one-shot 専用の host context 構成は無くなった。値域・検証は
one-shot と同一 (parse_mask_context_payload / parse_spatial_context_payload /
parse_render_environment_payload / prepare_aux_transport)。full-resolution
寸法を宣言する spatial は遅延 SEQUENCE_SETUP と全フレームの in_data に反映される。

W1-4 では secondary layer を `session-layers:v2|slot,w,h,handle;...` trailer
(context trailer より前) で運ぶ。**layer の RGBA8 ピクセルは §6 の共有 section
には置かず、broker が `target/image-transport` に per-layer の一時ファイルとして
書き、その path-authenticated な read HANDLE を継承ハンドルリスト (section/pipe
と同じ機構) で worker に渡す (#268)**。trailer は各 layer の slot/寸法に加えて
その handle 値を最終フィールドとして運ぶ (handle は継承なので worker 側で同じ数値)。
worker は open 時に各 handle から `w*h*4` バイトちょうどを private vector へ一度
読み、handle を閉じて全フレームで使い回す (plug-in は mapping もファイルも見ない)。
layer がファイル転送になったことで **layer 枚数・サイズは section の aggregate cap
を一切占めず**、section は header + input + output のみ (常に cap 未満) に境界化され、
primary より大きい layer も任意枚数扱えて one-shot layered per-file 転送と同等の
capability になる (#264 の per-layer スロットは #268 でファイル転送に置換)。header の
`layer_slot_count` は broker が書き、両者が trailer の layer 数と突き合わせて検証する
(section を指さなくなったので「注入された layer ファイル数」の意味)。broker は一時
ファイルを session の間保持し、session 終了時 (drop) に削除する。継承 read HANDLE は
broker が所有し drop で閉じる。

W1-4b では timed layer を同 trailer の 6 フィールド形式 `slot,w,h,time,scale,handle`
で運ぶ (4 フィールド `slot,w,h,handle` は static secondary)。物理スロットは layer 配列の
index ごとに割り当てられるため、同じ semantic `slot` を持つ複数の timed layer
(異なる rational time) はそれぞれ独立スロットを占有し、worker は各フレームの
current_time に対し one-shot と同じ有理時刻一致 (`same_time` / `same_rational_time`)
でマッチするエントリを選ぶ。dedup 規則は one-shot の layered_image_mode parser と
完全一致させ、session が one-shot と同じ集合を受理・拒否するようにする (適格な
構成で wrapper が無言 fallback しない)。同一 slot の判定: static 同士は拒否
(フレームごとに曖昧)、timed 同士は有理時刻が等しいとき拒否、**static と timed の
混在は許可** (layer parameter を current_time = static と他時刻 = timed で
サンプルする正当な表現)。broker open と worker parse の双方が同一規則。broker の
`secondary_layers` 診断フィールドは one-shot と揃えるため static layer のみ列挙し、
timed layer は含めない (両ルートで同一集合になる)。

実 AEX での layer 消費 (static/timed 双方) の等価性検証は layer parameter を
宣言する probe fixture を要する (#195)。それまでは fixture worker
(`session_protocol_worker`) 経由の統合テストで trailer 往復とスロット配置を
検証する。alpha-as-coverage (W1-4c) は auxiliary option で運ばれ、実 worker +
pf_sampling_probe による wrapper A/B (byte 一致) で session/one-shot 等価を
直接検証済み (provider の意味的効果は probe が alpha-coverage を消費しないため
byte 差としては現れないが、両ルートが同一オプションを同一 worker に送ることの
等価性は確認できる)。aux channels (#211) の broker 側配線
(aux_channels -> manifest -> `SessionOpenRequest::aux_manifest`) は機械可搬な
単体テスト (`prepare_aux_transport_output_satisfies_the_session_aux_manifest_contract`)
が、wrapper の manifest 出力が session の `--aux-manifest-v1` 前提 (絶対パス・
実在ファイル・v1 スキーマの top-level 契約) を満たすことを実 worker 無しで保証する。
実 worker での byte 一致 A/B は #231 修正後に有効。加えて実 worker +
pf_sampling_probe の wrapper A/B (`render_session_wrapper.rs`) が session/
one-shot の公開レポート全フィールド + PNG バイト一致を検証する (probe は depth
channel を消費しないが、両ルートが同一 manifest を同一 worker に load させること
を担保する)。

#231 の経緯 (2026-07-20 解決): sealed classic-render 経路は当初 aux 付き render
を session/one-shot 双方で `exit_code 3` (stderr 空・stage event 無し) で早期
拒否していた。根本原因は worker の aux loader
(`worker_pf_ae_channel_runtime.cpp` `load_aux_manifest`) が各パスを
`absolute().lexically_normal() == canonical()` で検証する点にある: MSVC の
`std::filesystem::canonical` は Windows の `\\?\` verbatim (extended-length)
prefix を剥がすが `absolute` は保持するため、`Path::canonicalize()` が生む
`\\?\C:\...` 形式の manifest / sidecar パスが自身の canonical 形と一致せず false
となり argv strip が失敗していた。regular render (`load_rgba`) はパスを直接 open
するだけでこの検証が無いため aux のみが落ちていた (transport self-test も同 manifest
を plain パスで渡していたため成功していた)。修正は broker 側で `prepare_aux_transport`
が transport root から verbatim prefix を剥がし (既存の minidump/trace のパス処理と
同じ `strip_extended_prefix`)、worker に plain な絶対パスを渡すもの。worker 側の
fail-closed パス検証はそのまま維持。回帰は機械可搬な単体テスト
(`aux_transport_de_verbatims_manifest_and_sidecar_paths`) が守る。

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
- SmartFX セッション (v1.1) は smart worker (`aex_smart_worker.exe`) の
  コマンド語で、位置引数の契約は同一: `--smart-session-v1` /
  `--smart-session16-v1` / `--smart-session32-v1`。ARGB32f は one-shot の
  `--smart-image32[-cpu|-opencl|-directx]` に倣い GPU backend をコマンド語で
  固定する: `--smart-session32-cpu-v1` / `--smart-session32-opencl-v1` /
  `--smart-session32-directx-v1` (無印は CUDA/自動交渉)。backend は launch で
  確定し、セッション中に変わらない。

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
- **メッセージ検証は strict (fail-closed)**: worker は許可キー集合検証
  (`strict_json` の `json_exact_keys` を基に、optional キーの presence 規則を
  加えたもの) を行い、未知フィールド・未知 `type`・未知 `v` はプロトコル
  違反としてセッションを終了する (§7)。黙って無視する経路は設けない。
- **`v` は「機能フラグ」ではなく「worker が要求する語彙レベル」(単調増加)**
  として扱う (改訂 2026-07-20、issue #238 owner 判断 = #201 の B)。per-frame の
  動的属性 (`parameters` #107、`ui_action` #238、将来の `param_epoch` /
  audio #239) は `v` を機能ごとに掛け算で増やすのではなく、v:2 メッセージ内の
  **独立フィールドの presence** で運ぶ (§4.2.1)。`v` の増分は「worker が新しい
  フィールド語彙を要するとき」に限定する。旧 worker (v:1 のみ理解) に v:2 を
  送ると `v` 検証で fail-closed になり、「stale な launch 時パラメーターのまま
  ok を返す」誤動作を構造的に排除する — この安全性は `v` の単調性が担保し、
  個々のフィールドが必須か optional かとは独立している。broker と worker は
  同一リポジトリで lockstep ビルドされるため、v:2 を理解するが特定フィールドを
  知らない中間世代の worker は実運用に存在しない。

#### 4.2.1 render_frame v:2 (per-frame 動的属性、issue #107 / #238)

`v:2` の `render_frame` は per-frame の動的属性を運ぶ一般メッセージ。属性は
独立フィールドの presence で表現し、現在 2 つある: `parameters` (#107) と
`ui_action` (#238、custom UI)。将来の属性 (audio #239、`param_epoch`) も
同じ v:2 にフィールドを足して運び、`v` は増やさない (§4.2 の語彙レベル方針)。

```json
{"v":2,"type":"render_frame","frame_index":3,
 "current_time":{"value":3,"scale":30},
 "parameters":"v4|param_1@1:f64=12.5"}
```

```json
{"v":2,"type":"render_frame","frame_index":4,
 "current_time":{"value":4,"scale":30},
 "ui_action":"click:v1|64|48|1|0|0|1"}
```

- **許可キー集合**: `v`,`type`,`frame_index`,`current_time` は必須。
  `parameters` と `ui_action` は optional だが**少なくとも一方が present**
  (両方無いフレームは v:1 を送る。両方 present も許可し、同一フレームで
  パラメーター更新と UI イベントを 1 往復で運べる — GUI ライブ操作向け)。
  未知キーは従来どおりプロトコル違反 (§7)。`close` は v:1 のみ。
- **`parameters`**: launch argv payload と同一の符号化
  (`encode_interactive_payload` が生成する `v2|`〜`v5|` 形式、上限 16384
  バイト、ASCII のみ)。意味論は**そのフレーム限りの完全置換**: メッセージ内の
  割当が launch 時 payload の割当を丸ごと置き換える (overlay ではない。
  載っていない slot はプラグイン既定値に戻る)。後続の v:1 フレームは launch 時
  payload に戻る。worker 側の適用器は one-shot と同一 (`render_once` が毎フレーム
  definitions を再初期化 → requested → parameter animation の順に適用) で、
  適用順序も one-shot と変わらない: v:2 の割当の上に
  `--parameter-animation-v1` タイムラインが重なる。
- **`ui_action`**: one-shot の custom UI argv trailer と同一符号化を再利用
  (`click:v1|x|y|r|g|b|a` / `draw:v1`、ASCII のみ)。新しい直列化形式は
  導入しない。意味論は**そのフレーム限りの UI 駆動**で、実現は one-shot と
  同一機構: worker は該当フレームの render 前に one-shot の argv 経路と同じ
  custom UI テレメトリ (render_click_enabled / 座標 / picker color、または
  render_draw_enabled) を立てるだけで、UI イベント列 (new_context → activate →
  click/draw → **render** → close_context) の駆動と context のライフタイムは
  render 経路 (`render_once`) が one-shot と同じ点で行う。**context は render 中も
  開いたままで、close は render 後の cleanup で行われる** (effect は render 中に
  active な UI context を観測する。one-shot の `dispatch_render_click` /
  `close_render_ui_context` と同順)。session が別の pre-render 列を送ることは
  しない。UI アクションの無いフレームは `ui_action` を載せない。フレーム間で
  テレメトリが漏れないよう、worker は各フレームの render 前に per-render の
  custom UI 観測フィールドを既定に戻す (register 系の setup-time フィールドは
  保持)。これで各フレームが fresh な one-shot と同じ初期状態から観測される
  (session 生存期間の context 保持は将来最適化)。
- **検証は strict fail-closed**: パース不能・非 ASCII・長さ超過・launch/one-shot
  と同じ構文検証に落ちる `parameters` / `ui_action` はプロトコル違反として
  セッションを終了する (§7)。broker は送信前に one-shot と同じ検証を済ませて
  いるため (`ui_action` は color 有限性・範囲 0..=1 を含む)、worker に届く
  不正 payload は broker の欠陥または改竄であり、フレーム局所エラーにしない。
  宣言済みパラメーターとの型不一致など render 時検証はフレーム局所診断のまま
  (v:1 と同じ)。
- 応答 (`frame_done`) のスキーマは v1 のまま変わらない。共有メモリ
  レイアウト (§6) と launch 構成 (§3) も不変で、ヘッダの `version`
  フィールドは 1 のままとする (レイアウト版であってメッセージ版ではない)。

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
- **空 SmartFX result (#278)**: PreRender が合法な空 result_rect を返し render
  selector をスキップした場合、`output` は `"width":0,"height":0,"rowbytes":0`
  と **`"empty_result":true`** を含む `status:"ok"` を返す (checksum はゼロバイト
  の sha256、one-shot の空出力と一致)。broker はこの明示フラグがあるときのみ
  ゼロ寸法を合法な空レンダーとして受理し (フラグ無しのゼロ寸法は次項の invariant
  失敗)、出力スロットは読まず空ピクセルの `Rendered` を返す。generation は通常
  フレームと同様に前進する。classic フレームはこのフラグを立てない。
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

#### 出力リサイズ (可変寸法 / `resize_needed`) — #261

エフェクトが `PF_OutFlag_I_EXPAND_BUFFER` / `PF_OutFlag_I_SHRINK_BUFFER` で
出力を入力と異なる寸法にする場合 (one-shot 本流と同じ範囲: 各次元 ≤ 4096、
≤ 16,777,216 px)、frame は render 寸法 (`max_width`/`max_height`) と異なる
実寸法で完了しうる。**レンダー寸法 (in_data の extent / full_resolution) と
出力スロット容量を分離**する:

- **shrink / スロット内 expand**: worker は実寸法で出力スロットに書き、
  `frame_done.output.width/height` に実寸法を報告。`output.checksum` は
  実寸法の packed バイト (`width*height*bpp`) を覆う。broker は実寸法バイト
  だけ読む (スロット全体ではない)。
- **スロット超過 expand (session 内 grow, #262)**: RENDER は worker private
  buffer に既に1回だけ完了している (共有スロットへのコピーは lifecycle 外)。
  worker は溢れ書きせず
  `{"v":1,"type":"frame_done","frame_index":N,"status":"resize_needed",
  "width":W,"height":H,"render_error":0}` を返し、**同一 worker のまま** broker の
  grow 応答を待つ (出力スロット・generation 不変)。broker は要求寸法を
  再キャップ (各次元 ≤ 4096、≤ 16,777,216 px) した上で、より大きい匿名 section を
  作成・静的ヘッダを初期化し、その handle を **`DuplicateHandle` で worker プロセス
  に複製**して
  `{"v":1,"type":"grow","section_handle":"<value>","output_capacity_width":W,
  "output_capacity_height":H}` を request pipe に送る。worker は
  `adopt_grown_section` で旧 section を unmap/close して grown section を map し
  (`geometry` の出力容量のみ更新、入力/出力スロット offset は render 寸法固定で不変)、
  **その同じ frame の描画済みピクセルを拡大スロットへ転送**して `status:"ok"` を返す。
  SEQUENCE/FRAME setup・RENDER・setdown は各1回で one-shot と一致する
  (再オープンして lifecycle を第2 worker で replay しない)。handle は path でなく
  複製ハンドルで渡すため TOCTOU 面を作らない。
- section は grow で拡大するため、開始時の出力スロットは常にレンダー寸法で確保し
  launch はバイト不変。共有メモリレイアウト (§6) の出力スロットは容量寸法で確保する
  (grow 後は新容量)。grow は length-1 wrapper・multi-frame バッチ・ライブセッション
  すべてで機能する (render_frame が内部で処理し `resize_needed` は呼び出し元へ
  表出しない)。
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
align 4096      : 出力スロット   (max_width * max_height * bpp、grow で拡大 #262)
```

（レイヤーは #268 で section から外れ、継承ファイル HANDLE 経由で転送される。
section は header + input + output のみ。）

- 入力スロットは RGBA8 (4B/px) 固定。one-shot の入力 raw
  transport と同一 (`load_rgba` は深度によらず w*h*4 を要求し、深度昇格は
  worker 内部の `rgba8_to_argb` が行う。深い入力転送は #57 系の既存課題で、
  セッションで新設しない)。レイヤーも RGBA8 (4B/px) で、per-layer の一時
  ファイルとして継承 HANDLE で運ぶ (#268)。出力スロットのみ深度で bpp が決まる
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
  `MAX_PIXELS=16M`) に従う。layer が section を離れた (#268) ため section 全体は
  header + input (<= 64 MiB) + 出力 worst-case-expand (32f で <= 256 MiB) ≈ 320 MiB
  で、layer 枚数・サイズに依らず常に 1 GiB hard cap 未満。broker 側で section 合計
  1 GiB を hard cap とするのは異常構成の fail-closed の defense in depth として残す
  (適格構成では発火しない)。

SessionHeader (すべて u32 LE、予約領域は 0 埋め):

```
magic            "AEXS"        (0x53584541)
version          3   (レイアウト版。layer slot が per-layer サイズ化 #264 で 1→2、
                     layer が section を離れ継承ファイル HANDLE 転送になり #268 で
                     2→3。制御メッセージの `v` とは別軸で、mismatch build を両方向で
                     fail-closed にする。audio session は layout 不変で 1 のまま)
depth_code       8 | 16 | 32
max_width, max_height
layer_slot_count   (#268 以降は「注入された layer ファイル数」。section は指さない)
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
  制御パイプ 2 本 + per-layer read HANDLE 群 (#268) を追加。suspended cleanup
  guard は現行のまま。
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
`param_epoch`)、SmartFX checkout スロット割当 (v1.1) はいずれもメッセージの
追加フィールド + worker 側拡張で入る設計にしてあり、v1 実装をブロック
しない。per-frame パラメーターは §4.2.1 の v:2 として 2026-07-20 に導入
済み (issue #107)。

### 9.1 v1.1: SmartFX セッション (issue #98 W3)

§4 のメッセージ仕様・§6 の共有メモリレイアウト・§7 の安全境界は一切変更
しない。変わるのはフレームの中身と起動配線のみ。

- **worker lifecycle (§5 の smart 版)**: 共有のフレームループ
  (`run_session_frame_loop`) が SEQUENCE_SETUP を classic と同じ契約で遅延
  ホイストし (最初にレンダー到達した `render_frame` の時刻を in_data に
  seed して発行、失敗は -47 で継続不可)、各フレームは FRAME_SETUP →
  SMART_PRE_RENDER → SMART_RENDER (GPU 交渉時は GPU_DEVICE_SETUP /
  SMART_RENDER_GPU / GPU_DEVICE_SETDOWN を内包) → FRAME_SETDOWN を
  `smart_render_once(session)` として実行する
  (`begin_frame_lifecycle` / `end_frame_lifecycle` に差し替え、one-shot の
  `begin_render_lifecycle` 経路は不変)。GPU デバイスの setup/setdown は
  one-shot と同じくフレーム内で完結する (コンテキストのフレーム間保持は
  将来の最適化であり、v1.1 は one-shot と同観測を優先する)。
- **frame_done の意味論**: フレーム局所エラーは one-shot の優先順位
  (GPU setup → PreRender → render/finalize → GPU setdown) で 1 つの
  `render_error` に畳む。ROI/rect 系の診断 (result_rect、
  extra_pixels_contract_violation 等) はフレームを落とさず最終レポート側に
  残る。**全フレーム = launch 寸法の契約 (§3) は smart でも同一**で、
  partial / empty result_rect を含む寸法逸脱は -44 (dimension mismatch) の
  セッション無効化になる。部分レンダーの受容はレイヤースロット・リング
  バッファと同じ将来拡張。
- **最終レポート (§4.4 の smart 版)**: smart worker の one-shot 型レポート
  (`stage:"smartfx_render"`) に session 集計フィールドを追加する:
  `session_mode` / `session_frames_attempted` /
  `session_sequence_setup_error` / `session_sequence_setdown_error` /
  `session_render_error` / `session_protocol_violation` /
  `session_invariant_failure`。smart 数値フィールド (pre_render_error 等) は
  最終フレームの値で、クリーン判定には使わない (フレーム局所エラー後の
  clean close は classic と同じく成功)。broker の clean 判定
  (`final_report_clean`) は classic では従来キー、smart では session_* キーを
  fail-closed に要求する。exit code 23/24 の契約は共通。
- **broker 配線**: `SessionOpenRequest` に `smart` / `gpu_backend` /
  `gpu_runtime_policy` を追加。GPU 起動 (smart × ARGB32f × runtime backend
  あり) は one-shot と同じ認証列 (`authenticate_gpu_worker_report` →
  `authorize_dispatch`) を `dispatch_secure_gpu_image_session` として通す。
  セッションは飛行中のリトライができないため、one-shot の Auto GPU
  preflight フォールバックは open 時点に畳む: Auto かつ policy 無しは CPU
  コマンドで開き、Auto かつ policy ありは CUDA (CPU リトライなし)、明示
  GPU backend かつ policy 無しは open で fail-closed。`render-video-batch`
  は `smart` / `gpu_backend` を受けるが policy 配線を持たないため GPU
  backend は CPU 縮退 (Auto) か拒否 (明示) になる。

## 10. Audio セッション (issue #239)

改訂 2026-07-20 (issue #239): 段階0 の AE 実機観測
(`docs/RENDER_SESSION_AUDIO_STAGE0_2026-07-20.md`) に基づく audio セッションの
設計。image セッション (§2-§8) とは**独立した経路**で、§4 の image メッセージ・
§6 の RGBA スロットは一切変更しない。

### 10.1 位置づけ (段階0 の結論)

観測で確定した audio の性質:

- audio は per-frame ではなく**期間一括**でレンダーされる (1 AUDIO_SETUP →
  1 AUDIO_RENDER が全期間 → 1 AUDIO_SETDOWN)。AUDIO_RENDER は
  `start_samp` / `dur_samp` の sample 区間を 1 チャンクで要求する。
- audio は image RENDER とは**別スレッド・別 sequence インスタンス**で並行する。
- audio フォーマット (rate / channels / sample_size) は host 交渉値。

したがって audio は image frame loop (§5) に相乗りさせず、**期間指定の audio
要求を運ぶ別メッセージ + audio sample バッファの別チャネル**で session 化する。
custom UI (§4.2.1 の per-frame `ui_action`、image frame loop 相乗り) とは
対照的な選択。

### 10.2 launch 時の静的構成 (argv)

one-shot audio (`--render-audio`、`worker_audio_execution.cpp` の
`run_audio_mode`) の位置引数を踏襲し、resident audio session のコマンド語を
足す:

```
aex_render_worker.exe --render-audio-session-v1 <plugin> <plugin_sha256>
    <payload> <max_samples> <channels> <time_scale>
    [--parameter-animation-v1 <path>] [--minidump-v1 <dir>]
```

- 位置引数は 7 個 (command 含む `effective_argc == 8`)。`l2_cli_dispatch.cpp`
  がこの数で分類し、`worker_request_parser.cpp` が `argv[4]=payload`,
  `argv[5]=max_samples`, `argv[6]=channels`, `argv[7]=time_scale` を読む。
- 入力 audio の raw パス・出力パスは取らない (§10.4 の共有メモリチャネルに置換)。
- `max_samples` / `channels` は §10.4 共有バッファの入出力スロット寸法 (geometry)
  の上限。v1 は mono 固定 (`channels == 1`、それ以外は launch で reject)、
  `max_samples` は `1 <= n <= 16 Mi`。これは**バッファ確保のための境界**であり、
  下の host 交渉フォーマットとは別物。
- audio フォーマット (rate / channels / sample_size) は host 交渉値なので
  launch では申告しない。実際の値は AUDIO_RENDER 時に worker が観測し、
  `audio_done` (§10.3) と共有バッファヘッダに書く。tier は default tier。

### 10.3 制御チャネル: メッセージ仕様

§4.1 のフレーミング (u32 LE 長さ接頭辞 + UTF-8 JSON、上限 64 KiB、strict
exact-key) を共用。image の `render_frame` とは別 `type`:

broker → worker:

```json
{"v":1,"type":"audio_render","request_index":0,"input_samples":48000}
```

- 受理キーは strict exact: `v` / `type` / `request_index` / `input_samples`
  の 4 個のみ (`worker_audio_execution.cpp` の `json_exact_keys`)。それ以外の
  キーはプロトコル違反。broker (`AudioRenderSession::render_span`) もこの縮約形
  だけを送る。
- `request_index`: 0 始まりの通し番号 (§6 と同型の generation 検証に使う)。
- `input_samples`: broker が入力スロット先頭に書いた f32 sample 数。要求区間は
  暗黙に `[0, input_samples)`、区間先頭は常に slot offset 0。`max_samples`
  (§10.2 geometry) を超える値は reject。
- **v1 の未実装 (今後の拡張余地)**: 明示 `start_sample` / `duration_samples`
  区間指定と、parameter animation 評価用の有理数 `time` は v1 要求では運ばない
  (段階0 の `start_sampL` / `dur_sampL` を活かす区間レンダーは後続作業)。現状は
  1 要求 = 1 連続区間を先頭から。
- `close`: `{"v":1,"type":"close"}` (exact key `v` / `type`) で GLOBAL_SETDOWN
  → 最終レポート → exit 0。

worker → broker:

```json
{"v":1,"type":"audio_done","request_index":0,"status":"ok",
 "output":{"start_sample":0,"sample_count":48000,"rate":48000,
           "channels":2,"sample_size":4,"checksum":"<sha256>",
           "guards_intact":true},
 "audio_render_error":0,"generation":1}
```

- host 交渉フォーマット (rate/channels/sample_size) と実 sample_count を報告。
- `status:"error"` は `output`/`generation` を持たない専用形 (§4.3 と同型)。
  AUDIO_SETUP 失敗はセッション無効化 (§4.3 の -47 に相当する専用コード)。
- host-protection invariant (guards_intact false、bounds/寸法検証失敗、
  generation 不一致) はフレーム局所でなくセッション無効化 (§4.3 と同方針)。

### 10.4 データチャネル: audio sample バッファ

§6 とは**別の無名 file mapping** (image セッションの RGBA スロットとは混ぜない)。
broker が作成・継承 handle で渡す (環境変数
`AEXCOMPAT_AUDIO_SESSION_SECTION_HANDLE`)。レイアウト:

```
offset 0     : AudioSessionHeader (1 ページ 4096B)
offset 4096  : 入力 audio スロット  (max_samples * max_channels * 4、float)
align 4096   : 出力 audio スロット  (max_samples * max_channels * 4、float)
```

- サンプルは f32 interleaved。one-shot の `host_audio::Runtime` が float 入力を
  扱う契約 (`set_source(const std::vector<float>*, sample_count)`) に一致。
- `max_samples` の上限は broker が hard cap する (長尺は §10.7 の分割チャンクで
  対応、当面は上限内 1 チャンク)。48kHz stereo float で 10 秒 ≈ 3.8MB。
- generation 契約は §6 と同型 (broker が入力書込→request 送信、worker が
  出力書込→generation 更新→audio_done)。plugin にスロットポインタは渡さない
  (worker 私有バッファ経由、§6 と同じ host-protection)。

### 10.5 worker 側 lifecycle

```
launch → admit_worker_entry → AEX ロード → GLOBAL_SETUP → PARAMS_SETUP →
  loop {
    audio_render 受信 → 入力スロット該当区間読取 →
    apply_parameter_animation(time) → AUDIO_SETUP(start/dur) → AUDIO_RENDER →
    AUDIO_SETDOWN (run_audio_mode の selector 列を区間駆動に流用) →
    出力スロット書込 + generation → audio_done 送信
  }
→ close → GLOBAL_SETDOWN → 最終レポート → exit 0
```

- `run_audio_mode` (`worker_audio_execution.cpp`) の SETUP/RENDER/SETDOWN 駆動と
  `host_audio::Runtime` の checkout/checkin/get_data 契約をループ外にホイスト
  して各 audio_render で再利用する (image の `persistent_sequence` に相当)。
- audio は image と別 sequence (段階0 観測2) なので、image セッションの
  sequence data 保持機構とは独立。SEQUENCE 系は audio セッションでは発行しない
  (one-shot audio と同じく AUDIO 系のみ)。

### 10.6 broker 側 API

```rust
pub struct AudioRenderSession { /* process, job, pipes, section, config */ }
impl AudioRenderSession {
    pub fn open(request: AudioSessionOpenRequest) -> io::Result<Self>;
    pub fn render_span(&mut self, request_index: u32, start: u32, duration: u32,
                       time: i32, samples: &[f32]) -> io::Result<AudioSpanResult>;
    pub fn close(self) -> io::Result<AudioSessionReport>;
}
```

- launch / Job Object / restricted token / sealed staging は image セッションの
  `run_isolated_impl` 分離 (§8) を流用。handle list に audio section + 制御パイプ
  2 本。per-request deadline (§7) を audio_render ごとに張る。
- wrapper: `render_with_artifact` の audio 経路 (`audio.is_some()`) と
  `render_experimental_audio` を、単発 audio を「長さ1 audio セッション」として
  この経路に載せる (image の length-1 wrapper と同型、挙動不変)。適格条件から
  `audio.is_none()` 除外を外す。

**実装再利用マップ (継続用、worker 側は実装済み・検証済み)**: broker
`AudioRenderSession` は `render_session.rs` の `RenderSession` と同一モジュール内に
置き、private helper をそのまま再利用する — `inheritable_pipe` / `inheritable_security`
/ `read_exact_handle` / `write_all_handle` / `SessionTransport` (view + header +
`send_message` + `read_output_slot`、`write_input_slot` は HEADER_BYTES 直後書込で
audio 入力スロットにそのまま使える) / `SecureImageDispatch` +
`dispatch_secure_image_session` / `SessionChildHandles` / `SessionEvent` + reader
thread + process-death watcher / `SecureSessionProcess` + `CollectedExit` /
`await_frame_response` の 3-way wait / `invalidate` パターン。audio 固有部のみ新規:
(a) geometry = HEADER + 2×align(max_samples×channels×4)、(b) header 書込 (magic
"AAUS"=`kHeaderMagic`、version、max_samples、channels、input/output generation)、
(c) argv `--render-audio-session-v1 <sha> <payload> <max_samples> <channels>
<time_scale>` (worker の `worker_request_parser.cpp` パースと対、`WorkerKind::Render`
の `dispatch_secure_image_session`)、(d) `render_span(request_index, &[f32])`:
入力 f32 を `write_input_slot`、`input_generation=request_index+1` 書込、
`{"v":1,"type":"audio_render","request_index":N,"input_samples":M}` 送信、
`audio_done` を await→検証 (generation・guards_intact・checksum)、出力 f32 を
`read_output_slot(output_slot_offset, output_samples*4)` で回収、(e) close:
`{"v":1,"type":"close"}` 送信→exit 回収→最終レポート (`stage:"audio_session"`) を
parse。worker 側メッセージ・exit コード (23/24)・レポート形は実装済み
(`worker_audio_execution.cpp` の `run_audio_render_session` /
`emit_audio_session_report`)。A/B は既存 `SDK_Backwards.aex` (audio fixture) で
session/one-shot の出力 f32 バイト一致を検証する。

### 10.7 検証

- fixture worker (audio 版、または `session_protocol_worker` の audio 拡張) で
  audio_render 到達・generation・区間往復を broker 統合テストで証明。
- 実 worker A/B: 同一入力 audio を session / one-shot でレンダーし、出力 audio
  サンプルのバイト一致 + 公開レポート (audio_* フィールド) 一致を証明。
- 長尺 comp で AUDIO_RENDER が複数チャンクに分割されるかは追加観測の候補
  (段階0 次アクション item)。分割される場合、`audio_render` を複数 request に
  分けるか、リングバッファ (§6 の将来拡張と同型) で対応する。

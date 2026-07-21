# AviUtl2 フィルタプラグインブリッジ 調査・設計ノート (issue #269)

時系列追記。観察 (事実) と仮説 (推論) を分ける。結論が覆っても古い項目は
消さず「否定」「訂正」を追記する。

## 位置づけ

#98 の消費者 #3「外部ホストブリッジ (AviUtl2 フィルタプラグイン等)」の実装。
#98 で常駐レンダリングセッション (`RenderSession`) とライブ駆動する
リファレンスホスト (`broker/crates/harness` の egui + `InteractiveRenderSession`)
が実装済み。本 issue の主眼は AviUtl2 SDK への配線と形式変換であり、
レンダリングエンジンの新規実装ではない。

## 2026-07-21 観察: broker 側 API (一次情報, リポジトリ直読)

- `aexcompat_broker` は `[lib] name = "aexcompat_broker"` を公開し
  `pub mod render_session` / `pub mod image_render` を持つ。
- `render_session::RenderSession`:
  - `open(SessionOpenRequest) -> io::Result<RenderSession>` — worker
    サブプロセスを起動 (継承 HANDLE + 共有メモリ file mapping + Job Object)。
    **AEX 本体は worker 側 = out-of-process**。crash containment を保持。
  - `render_frame_with_parameters(frame_index: u32, current_time: i32,
    rgba: &[u8], parameters: Option<&[InteractiveParameter]>) -> io::Result<FrameOutcome>`
  - `close(self) -> Value`
  - 戻り `FrameOutcome.status` は `FrameStatus::Rendered { pixels: Vec<u8>,
    checksum, width, height }` / `FrameError { render_error }`。
- `image_render::InteractiveRenderSession` は harness/conformance 向けに
  PNG + raw sidecar を**ファイル出力**する。AviUtl2 はメモリ pixels が欲しい
  ので、この wrapper ではなく `RenderSession` を直に使う。
- 型パス: `image_render::{RenderPixelFormat (Argb8/Argb16/Argb32f),
  RenderGpuBackend, InteractiveParameter}`、
  `secure_image_dispatch::ApprovedImageArtifact`。
- **worker バイナリ解決** (`secure_image_dispatch.rs`): worker_kind Render は
  `<repository>/target/minihost-build/aex_render_worker.exe` の**固定相対パス**。
  `admit_local_worker` がローカルビルドを dispatch 時にハッシュ admit
  (frozen trust 定数なし、`docs/EVIDENCE_POLICY_2026-07-18.md` §3 準拠)。
  `validate_broker_trace_directory(repository)` を通す必要あり。
- したがってプラグインは runtime に **`repository` パス (ビルド済み worker が
  ある場所) を知る必要がある**。段階1 は env で渡す。shipping は worker 同梱
  + repository をバンドルに向ける (段階2+ のパッケージング課題)。
- `SessionOpenRequest.dependencies: Vec<ApprovedImageArtifact>` は依存 DLL 用。
  依存無しの AEX なら空。

## 2026-07-21 観察: aviutl2 クレート API (一次情報, sevenc-nanashi/aviutl2-rs @ 0.40.0)

- フィルタ (.auf2) 経路: `#[aviutl2::plugin(FilterPlugin)]` + `impl FilterPlugin`
  (`new` / `plugin_info() -> FilterPluginTable` / `proc_video(&self, config,
  video: &mut FilterProcVideo)`) + `register_filter_plugin!`。
  パラメーターは `#[aviutl2::filter::filter_config_items]` で **Rust 側定義**
  (Lua 不要)。先例 `examples/pixelsort-filter`。
- `FilterProcVideo`:
  - `video_object.width/height: u32`
  - `object: ObjectInfo { effect_id: i64, frame: u32, frame_total: u32,
    time: f64, time_total: f64, ... }`
  - `scene: SceneInfo { frame_rate: Rational32, ... }`
  - `get_image_data<T>(&mut self, &mut [T]) -> usize` / `set_image_data<T:
    IntoBytes+Immutable>(&self, &[T], w, h)`。
- `RgbaPixel { r, g, b, a: u8 }` (zerocopy IntoBytes/FromBytes)。**broker の
  RGBA8 packed とバイト順が同一**。`get_image_data` を `Vec<RgbaPixel>` で
  受け `as_bytes()` で broker 入力へ、broker 出力 `Vec<u8>` を
  `set_image_data(&bytes, w, h)` で戻す (u8 は IntoBytes+Immutable)。
- 先例 ntsc-rs.anm2 は ScriptModule (.anm2, 純 Rust in-process) を使うが、
  本ブリッジは AEX の out-of-process 隔離が固有要件なのでフィルタ (.auf2)
  経路を採り、パラメーターを Rust 側で定義する。

## 2026-07-21 設計判断

- **クレート配置**: `bridges/aviutl2/` を broker ワークスペース外の独立 crate
  にする。理由: `aviutl2` (0.40) の依存ツリー (tracing / num-rational / zerocopy
  等) を broker ワークスペースの `cargo test --workspace` gate に持ち込まない。
  broker lib は path dep で参照。
- **スレッドモデル (watch-item #2 への対処)**: `RenderSession` は raw pointer
  (view: *mut u8) 保持で `!Send`。`proc_video(&self)` が AviUtl2 の任意スレッド
  から呼ばれうる。→ **effect インスタンス (effect_id) ごとに専用スレッドが
  `RenderSession` を所有**し、mpsc channel で render 要求 (rgba + current_time)
  を受けて pixels を返す設計にする。プラグイン構造体は
  `Mutex<HashMap<i64, BridgeSession>>` (channel sender + join handle、これは
  Send) を持つ。呼び出しスレッドと RenderSession を物理的に分離し、
  AviUtl2 のスレッド配布に依存しない。これが最も確度の高い形。
- **frame_index と current_time の分離**: `frame_index` (generation 用) は
  セッション内部の単調カウンタにする。AviUtl2 はランダムアクセス (フレーム
  順序保証なし・同一フレーム再要求) なので `object.frame` を generation に
  使わない。`current_time` は `object.frame` から AE 時刻へ写像する。
- **時刻写像**: AviUtl2 `scene.frame_rate = rate/scale` (Rational32) に対し、
  AE 時刻を `time_scale = rate`, `time_step = scale`,
  `current_time = object.frame * scale`, `total_time = frame_total * scale`
  とする。`current_time / time_scale = frame/fps = 秒` で整合。
  - 仮説: `frame * scale` は 29.97 系 (scale=1001) で長尺 comp だと i32 を
    超えうる (frame ~2M で ~2e9)。段階1 は許容、境界は段階2 で扱う。

## 段階1 のスコープ (最初の縦スライス)

- 固定 AEX: env `AEXCOMPAT_AVIUTL2_PLUGIN` (AEX パス) と
  `AEXCOMPAT_AVIUTL2_REPOSITORY` (ビルド済み worker のある repo root)。
  sha256 は open 時に計算。
- 8bit RGBA 固定 (`RenderPixelFormat::Argb8`)。パラメーター無し (`None`)。
- `proc_video`: `get_image_data` → rgba → セッションスレッドへ →
  `render_frame_with_parameters` → pixels → `set_image_data`。
- AviUtl2 上で 1 フレーム出れば成立を実証。同時に watch-item 2 点
  (入れ子 Job Object / スレッドモデル) が初回ロードで判明する。

## 未検証 (段階1 実機で確認する項目)

- **入れ子 Job Object**: AviUtl2 が Job 内で動く場合の worker Job のネスト
  可否 (`open()` 成否で判明、breakaway 対応が要るか)。
- **出力バイト順**: `RenderPixelFormat::Argb8` セッションの `FrameStatus::
  Rendered.pixels` が RGBA 順か ARGB 順か (プロトコル §4.3 は "argb8"、
  §6 は入力 RGBA8 と記載)。段階1 の初フレームで色を見て確定する。仮説は
  入出力とも RGBA 順 (worker 内部で argb 変換する前提)。
- **α前乗算**: AviUtl2 8bit RGBA が straight か premult か。段階1 で確認し、
  PA64/HF64 (段階2) の変換規則の基準にする。

## 2026-07-21 実装: 段階1 骨格 (`bridges/aviutl2/`)

観察 (事実):

- crate `aexcompat-aviutl2-bridge` (cdylib) を作成。`cargo check` / `cargo build`
  とも成功し、`target/debug/aexcompat_aviutl2_bridge.dll` (3.1MB) を生成。
  broker lib (`aexcompat-broker` path dep) + `aviutl2` 0.40 + bridge が
  クリーンにコンパイル (開発機 rustc 1.93.1)。
- 実配線: `proc_video` が effect インスタンスごとの専用スレッド上の
  `RenderSession::open` / `render_frame_with_parameters` を driveする。
  `RenderSession` (!Send) は生成スレッドに固定し mpsc channel で往復。
- broker ワークスペースの `members` は未変更 (独立 crate、path dep のみ)。
  `cargo test --workspace` の依存グラフに `aviutl2` は入らない。

パッケージング (段階1 手動):

- `.dll` を `.auf2` にリネームして AviUtl2 の Plugin フォルダに置く。
- runtime env: `AEXCOMPAT_AVIUTL2_PLUGIN` = AEX パス、
  `AEXCOMPAT_AVIUTL2_REPOSITORY` = ビルド済み worker
  (`target/minihost-build/aex_render_worker.exe`) のある repo root。

未実施 (実機が要る、次アクション):

- AviUtl2 実機で 1 フレーム描画 (段階1 exit 条件)。これで watch-item 2 点
  (入れ子 Job Object / `proc_video` スレッドモデル) と出力バイト順・α前乗算が
  同時に判明する。実機は worker ビルド + AEX fixture + AviUtl2 インストールを要する。
- 観察: filter プラグインは `.auf2` 拡張子で読まれる (aviutl2-rs の Rakefile で
  `_filter` → `.auf2`、`aviutl2.toml` の artifacts が `.dll` を
  `Plugin/<name>.auf2` にコピー)。本 crate は
  `target/debug/aexcompat_aviutl2_bridge.dll` → `<AviUtl2>/Plugin/aexcompat.auf2`。

## 2026-07-21 実機検証: 段階1 成功 (AviUtl2 実機)

セットアップ (開発機):

- worker: origin/main (@ 6cd3f7c) から Release ビルドし
  `target/minihost-build/aex_render_worker.exe` に配置。broker lib と同一
  リビジョンでプロトコル一致 (メイン worktree の #264 ブランチ産 worker は
  使わない = ドリフト排除)。
- 固定 AEX: `pf_sampling_probe.aex` (session A/B テスト実績)。Plugin dir は
  `C:\ProgramData\aviutl2\Plugin` (UAC 不要)。env は User scope。
- AviUtl2 は `C:\Program Files\AviUtl2`。フィルタ効果として
  「AEXCompat (AEX bridge)」を図形/画像オブジェクトの後ろに追加。

観察 (事実):

- **クラッシュせずフレームが表示された**。端から端まで
  (get_image_data → 専用スレッドの RenderSession → worker (out-of-process) で
  AEX 実行 → pixels → set_image_data) が AviUtl2 実機で通った。
- 出力は**約6列周期の縦しま模様**。
- 縦しまは `pf_sampling_probe` の**設計出力**であってバグではない
  (`instruments/pf-sampling-probe/pf_sampling_probe.cpp:186-187`:
  `row[x] = samples[x % SampleCase::kCount]`、kCount=6)。probe は入力を数点
  サンプリングし、その色を列ごとに周期配置する。

watch-item の結着:

- **watch-item 1 (入れ子 Job Object)**: 解決。`RenderSession::open` が
  AviUtl2 プロセス内から成功した (worker 起動 + Job 割当が通った)。
  AviUtl2 が Job 内で動いていても kill-on-close Job のネストは機能した。
  breakaway 対応は不要だった。
- **watch-item 2 (proc_video スレッドモデル)**: 解決。effect インスタンス
  ごとの専用スレッドが RenderSession を所有し mpsc で捌く設計で、
  AviUtl2 のフィルタ呼び出しから正しくレンダーできた。

これで #98 で先送りしていた「AviUtl2 内から常駐 RenderSession を駆動できるか」
= 設計を崩しうる唯一の未知が実機で解消した。段階1 exit 条件を満たす。

## 2026-07-21 実機検証: 色/バイト順・α (pf_fill_premultiply_probe)

固定 AEX を `pf_fill_premultiply_probe.aex` に差し替え (worktree で cmake でなく
既存 build スクリプトの cl/link 経路をビルド。スクリプトの VS パス直書きは
`-VisualStudio "C:\Program Files\Microsoft Visual Studio\18\Community"` で上書き)。
この probe は 5 列周期で seed (a,r,g,b) を書き premultiply-forward する:
列0 a0 / 列1 a1,r255 / 列2 a128,r255 / 列3 a128,**r127** / 列4 a255,r255、
green は行パリティ (偶191/奇64)、blue ~128。

観察 (事実、実機の見え、左→右):

- 列0 黒、列1 黒、列2 暗い赤〜黄ピンク間、列3 暗い青か緑、列4 赤〜黄ピンク間。
- 期待値 (premult 後: 列2 r128g96b64 赤寄り / 列3 r64g96b64 緑寄り / 列4 明るい
  赤ピンク) と一致。

結論 (仮説の結着):

- **バイト順 RGBA が端まで一貫 (R↔B スワップ無し)**。赤 seed 列が赤/ピンクに
  見える (青/シアンでない)。決定打は列3: seed で red を 127 に落とした列だけが
  緑寄りに見える = R チャンネルが正しい位置にある裏取り。bridge に色入替は不要。
- **α (0,1,128,128,255) が正しく運ばれ、premultiply-forward の合成が AviUtl2 で
  破綻しない**。8bit RGBA 経路では straight/premult の整合が取れている。
  深い PA64/HF64 の乗算済みα変換は段階2。

運用の落とし穴 (記録):

- User scope の env 更新は、更新前に起動済みのプロセスや、更新をブロードキャスト
  前に起動した別プロセス子には伝播しない。最初「全部真っ黒」だったのは env 未伝播
  の AviUtl2 起動 (別プロセス経由) が原因。プラグインが AEX を読めず render error
  → 黒。正しく env を継いだ起動で fill probe が描画された。実機テストは env 更新後に
  新規プロセスツリーから AviUtl2 を起動すること。

## 2026-07-21 段階2a: パラメーターマッピング (実装)

段階2 の第一段。固定 AEX のパラメーターを AviUtl2 の設定項目に出し、値を
per-frame でセッションに渡す。

- **discovery**: `new()` (プラグインロード時) で
  `inspect_experimental_with_diagnostics(repo, plugin, sha)` を呼び、AEX の
  パラメーター (`Vec<InteractiveParameter>`: name/kind/min/max/value/choices/color)
  を取得。失敗は非致命 (warn して段階1 の launch 既定値動作に縮退)。
- **マッピング** (`config_item_for`): float/slider/angle → `Track`、integer →
  `Track(step=1)`、checkbox → `Checkbox`、color → `Color` (0x00RRGGBB、AEX の
  α は非公開)、popup → `Select` (choices → items)。point/layer/comp/button/
  custom/group markers は未公開 (discovered 既定のまま、段階3)。範囲が退化
  (min>=max, 非有限) の numeric は公開しない。
- **保持**: `FilterConfigItem` は `Send+Sync` でない (Button/Data variant 由来) ため
  `Send+Sync` 必須の plugin 構造体に保持できない。→ 保持は `param_template:
  Vec<InteractiveParameter>` (プレーンデータ) のみ。config items は
  `plugin_info()`、slots は `proc_video()` で `exposed_config()` から都度構築
  (deterministic なので順序一致)。
- **session open**: `SessionOpenRequest.parameters` に template (discovered 既定) を
  baseline として渡す (段階1 は None)。
- **per-frame** (`proc_video`): AviUtl2 が渡す `config: &[FilterConfigItem]` の
  現在値を template に overlay し (`apply_config_values`、slot で対応付け)、
  full set を `render_frame_with_parameters` に渡す。protocol §4.2.1 の per-frame
  parameters は launch payload を完全置換するので、未マップ slot が既定に戻らない
  よう全 slot を送る。
- build/clippy クリーン。stage2 build を `.auf2` に再配置済み。

### 2026-07-21 実機 (fill probe): 設定出ず + render worker crash — 原因と修正

観察 (実機): 段階2 build で fill probe を適用したところ、(1) 設定に control が
一切出ない、(2) **段階1 で描画できた render が worker exit する**
(`render session invalidated (worker_exited): the worker was gone before frame 0`)。
discovery 失敗 warn はログに無し (= discovery は成功、template 非空)。

原因 (broker source 直読):

- **popup が "integer" 化する**: inspection の `runtime_kind` は popup (observed_type 7)
  を `_ => "integer"` に落とす (`image_render.rs:3139`)。かつ popup は valid_min/max を
  持たず `host_min==host_max==default` (退化 range)。→ 私の `bounded_range` が None を
  返し config item にならない。**fill probe は 3 popup のみなので control ゼロ**。
- **worker crash の差分は「parameters 送出」**: 段階1 は `parameters: None` で payload を
  渡さず動いた。段階2 は discovered template 全体 (退化 integer の popup 含む) を session
  baseline + per-frame に渡していた。`encode_interactive_payload` は "integer" を i32 と
  して載せる (filter 対象外) ため、popup 値が worker に渡り render を壊す (popup value の
  index 不整合で fill probe render が不正 branch → crash と推測)。段階1 が動いたのは
  何も送らなかったから。

修正 (実装):

- **exposed subset のみ送る**: `exposed_config` が UI 公開する (config_item_for が Some を
  返す) パラメーターだけを返し、session baseline も per-frame もその subset のみに。
  full template は送らない。fill probe → exposed 空 → None → 段階1 と同一経路で描画
  (crash しない)。実 slider/checkbox/color を持つ AEX → それらのみ送出。
- 結果: fill probe は描画できるが control は出ない (全 popup で公開対象ゼロ、正しい挙動)。
  段階2 を体感するには float slider 等を持つ AEX が要る。

### 2026-07-21 訂正: worker crash の真因は「ステール worker」、popup ではない

上の「worker crash は popup 送出が原因」という仮説は**誤りだった (訂正)**。standalone
repro (`bridges/aviutl2/examples/repro_render.rs`、broker を直に叩く) で切り分けた結果:

- **discovery が AviUtl2 で失敗していたのは `aex_l2_worker.exe` 未ビルド**。inspect は
  `WorkerKind::L2` を使う (`image_render.rs:2353`) が、私は render worker しか flat パスに
  置いていなかった。→ l2/smart worker をビルドして揃えたら discovery 成功
  (echo probe → `float "Echo" min=0 max=255` を正しく取得)。
- **render の worker_exited は render worker binary がステール**だったのが真因。
  l2/smart を VS generator でビルドした際に共有の `aex_worker_runtime_core` object が
  再コンパイルされ、先にコピーしていた flat の `aex_render_worker.exe` (19:24) が
  不整合になった。probe/パラメーターと無関係で、**pf_sampling_probe (段階1 で描画実績) すら
  `parameters: None` で render_error -1 / exit 23 で落ちていた**のが決定的証拠。
  → render worker を現ソースで再ビルド (21:36) したら **全 probe が描画成功**。
- **per-frame パラメーター適用は byte 単位で実証**: echo probe を Echo=0 → 出力
  RGBA=[0,255,128,255]、Echo=200 → [200,55,128,255] (赤=値, 緑=255-値)。段階2 の
  パラメーターマッピング + per-frame 反映は正しく動く。バイト順 RGBA も再確認。

教訓: **worker は全種 (l2/render/smart) を同一ソースから一括で flat に揃える**。
片方だけ再ビルドすると共有 object の再コンパイルで他がステール化する。canonical な
Ninja flat ビルド (`docs/BUILD_REQUIREMENTS.md`) を使うのが安全。

`exposed-only` 送出の判断について: popup が crash 原因という前提は誤りだったが、
「UI 公開できたパラメーターだけ送る」設計自体は維持する (退化 range の popup を
slider として出しても無意味で、送らなくても AEX 既定値のままで差が無い)。popup を
dropdown として正しく公開する件は段階2b (choices は保持されるので "integer+choices" を
Select に写像可能、value の index 符号化を検証してから)。

### 2026-07-21 段階2b: popup を dropdown 公開 (実装)

popup は runtime_kind "integer" + 非空 `choices` + **1-based** (min=1, value=1) で来る
(fill probe で確認: Suite operation min=1 max=4)。config_item_for の "integer" arm で
`!choices.is_empty()` を先に見て Select に写像 (items の value = index+1、default =
discovered value)。apply は Select 値をそのまま integer 送出。AviUtl2 は item.value を
キーにするので 1..N で問題ない (0始まり/連続不要、aviutl2 config.rs:157-178)。harness の
ComboBox (main.rs:4298-4313) と同じ 1-based。fill probe で value 1 → 描画、value 4 →
frame_error 516 (AEX の float premultiply 診断、bridge は FrameError で握る) を確認 =
popup 値が正しく適用される。

コンパイル fix も同梱: main #275 が `SessionOpenRequest.conformance_render_settings:
Option<&str>` を追加。bridge は broker workspace 外で CI に拾われず、段階2a マージが
main の bridge をコンパイル不能にしていた (issue #284)。両コンストラクタに `None` 追加。

未解決 → 段階2b 以降:
- **angle の scalar value 範囲**: encode は全 kind で untouched な scalar `value ∈ [min,max]`
  を検査する (`image_render.rs:4362`)。bridge は angle で `components[0]` のみ更新し `value` は
  discovery の default scalar のまま。plugin の default scalar が valid range 外だと毎フレーム
  payload 拒否になりうる (angle fixture が無く未検証、稀と思われる)。

### 2026-07-21 Codex 指摘対応 (段階2 PR #281)

Codex が P2×3。broker source で3件とも妥当と確認して修正:

- **checkbox** (`runtime_kind` で "integer" 化、"checkbox" arm は死にコード) → "integer かつ
  range [0,1] かつ choices 空" を Checkbox に写像。2択 popup (choices あり) は誤検出しない。
- **angle** (encode は `components` から読む) → config/apply とも `components[0]` を使う。
- **color** (`InteractiveParameter.color` は ARGB `[a,r,g,b]`、encode も `argb8=`) → pack/unpack
  を ARGB 順に (RGB 更新・alpha 保持)。
- 死にコード ("slider"/"checkbox"/"popup" arm、Select apply arm、未使用 import) を整理。
  integer apply は `.round()` で防御 (encode は fractional integer で payload 全体を拒否)。

float パスは回帰なし (echo probe [0,255,128]/[200,55,128] を維持)。color/angle/checkbox の
実機 fixture は無いため broker source との一致で正しさを担保。

追加 (Codex re-review): **hidden (`visible == false`) パラメーターを公開しない**。AE が
private/条件付きで隠すパラメーターにコントロールを出して値を送ると意図を上書きしてしまう。
harness (`main.rs:4181`) と同じく exposed_config で skip する (AEX 既定値のまま)。enabled/
supervised は dynamic/soft な状態で AviUtl2 の静的 config では表現しづらいため今は据え置き。

## 段階1 総括

段階1 exit 条件を満たした:
- AviUtl2 実機で AEX を out-of-process 実行し 1 フレーム描画 (crash containment 維持)。
- watch-item 1 (入れ子 Job Object) / 2 (proc_video スレッドモデル) を解決。
- 転送・バイト順・α (8bit RGBA) の正しさを既知出力 AEX で確認。

未着手 (段階2):

- **パラメーターマッピング**: AEX パラメーター discovery → `aviutl2` 設定項目 +
  `InteractiveParameter`。現状は launch 既定値のみ。
- **PA64/HF64 深度**: 16bit 乗算済みα / fp16 の変換規則。
- **固定 AEX の脱却**: env 直書きでなく設定項目や同梱でプラグイン選択。
- worker 同梱パッケージング (shipping)。

## 2026-07-21 ローカルレビュー対応 (Codex 前)

段階1 コミット後、ローカルレビューエージェントに並行性・FFI・リーク観点で
レビューさせ、3 点を段階1 内で修正した (残りは検証済み or 段階2 に送る)。

修正 (実装):

- **死んだセッションの reap/reopen** (HIGH): worker が timeout/クラッシュ/
  host-protection invalidation で終了しても map に残り、以降 send 失敗を握り
  潰して回復しなかった。→ reply を enum `FrameReply` 化し
  `Rendered`/`FrameLocal`/`SessionLost` を区別。`SessionLost` (worker 消失・
  invalidation・pipe 破損) のとき `remove_session` で除去し次フレームが再 open。
  `FrameLocal` (frame 局所診断、セッション生存) では除去しない (毎フレーム
  再 open storm を避ける)。
- **ロック保持中のブロッキング排除** (HIGH/MED): `RenderSession::open` (数秒) と
  stale セッションの drop/join (最大 frame deadline) がまだ map lock 内だった。
  → `existing_sender` (短ロックで sender clone、stale はロック外 drop) と
  `open_and_get_sender` (open はロック外、double-check insert、余剰は
  ロック外 drop) に分割。ロックはマップ操作の間だけ。
- **時刻パラメーターの staleness** (MED): `total_time`/`time_scale`/`time_step` は
  open 固定で、オブジェクト長や fps 変更で以降フレームが total_time 超過→無描画
  だった。→ `SessionIdentity` に幾何+時刻を入れ、不一致で再 open。

段階2 送り (レビュー指摘、段階1 では未対処):

- 拡大/縮小出力エフェクトを filter object で `set_image_data(w!=obj, h)` する件
  (AviUtl2 が filter mode でサイズ変更を許すか要確認)。段階1 の probe は resize
  しないので未発生。
- 極端フレーム数での `current_time = frame*scale` の i32 飽和衝突 (実用外)。

不対処 (検証済みで問題なし):

- pixel チャンネル順 `Argb8` スロット vs `RgbaPixel`: fill-premultiply probe の
  実機結果 (赤 seed が赤に見える) で RGBA 一貫を確認済み。コード変更不要。

## 2026-07-21 レビューループ (ローカル→Codex)

方針を「ローカルエージェントレビューをループで clean にしてから Codex」に修正
(ユーザー指示、[[review-order-local-then-codex]])。単発ローカル1回→Codex では、
ローカル指摘対応で入れた変更が生む回帰 (下記 deadlock) を Codex 側で拾う羽目に
なった反省。

- **Codex 1st**: P2×2 (`init()`→`try_init()` で二重初期化 panic 回避、
  filter object で resize フレームを拒否)。対応 `d99f233`。
- **Codex 2nd**: P1 deadlock (`bc62bb5` で修正)。`open_and_get_sender` の
  double-check insert (ローカル指摘の「ロック外化」対応で導入) で、race に
  負けたとき事前取得した sender clone が `drop(discard)` の join より長生きし、
  join がその clone を待って永久ブロック。sender clone を install パス内でのみ
  取るよう修正。→ **ローカルを先にループで回していれば Codex 前に捕まえられた**。
- **ローカル 2周目** (bc62bb5): deadlock 修正を検証 (正しい) + 2 件:
  - P2 (leak): 削除/放棄 effect のセッション (worker サブプロセス + スレッド +
    共有メモリ) が reap されず leak。aviutl2 の FilterPlugin に teardown
    コールバックが無く `effect_id` は起動ごと固有のため、再レンダーされない
    effect は evict 経路に乗らない。→ **idle reaping** で修正: `BridgeSession` に
    `last_used: Instant`、`open_and_get_sender` で `SESSION_IDLE_TIMEOUT` (120s、
    frame deadline 30s を余裕で超過) 超過を sweep してロック外 drop。active に
    使われている effect のセッションは last_used 更新で残る (正当。hard cap で
    スラッシュさせない)。放棄セッションは次の open 時に刈られ、unbounded 増加が
    bounded 残留になる。
  - P3: `remove_session` が key 消去で、並行 reopen が入れた健全セッションを
    巻き込みうる。→ `BridgeSession` に `serial: u64` (static AtomicU64)、
    `remove_session(effect_id, serial)` が serial 一致時のみ除去。
- **ローカル 3周目**: P2/P3 修正を検証 (正しい、新規問題なし)。carry-over は
  「off-lock join は render_on が frame deadline で返る前提」= broker §7 watchdog の
  既存不変条件で bridge 起因でない。→ **ローカル clean 到達、Codex へ**。

## 2026-07-22 段階3: SmartFX 対応 (実装)

ntsc-rs のような実効果は SmartFX で、classic セッションでは
`PF_Err_BAD_CALLBACK_PARAM` (516) で弾かれる (oracle: host_render_path=smartfx)。
harness の live per-frame (InteractiveRenderSession) も classic 固定だが、broker の
`RenderSession` は `smart: true` (SmartFX 常駐セッション、#98 W3) を持つ。bridge は
RenderSession を直に叩くので、これを配線した:

- **SmartFX 検出**: discovery の diagnostics `advertised_out_flags2` の
  `PF_OutFlag2_SUPPORTS_SMART_RENDER` (bit 10、worker_effect_bootstrap.cpp:98) を見る。
- **smart セッション**: 検出時 `RenderSession::open` に `smart: true` を渡す。8bit smart は
  GPU 非対象 (`gpu_capable = smart && ARGB32f`) なので gpu_backend Auto=CPU、runtime policy 不要。
  smart worker (`aex_smart_worker.exe`) を使う。
- パラメーターマッピング (段階2) はそのまま流用。

検証 (repro, ntsc-rs-ae.aex): SmartFX=true 検出、86 params discovery、Random seed を
変えて 2 フレーム描画 (frame0 [1,2,5] / frame1 [10,14,12] = seed でノイズが変わる)。
classic では両フレーム 516 だったのが smart で描画成功。実機は AviUtl2 で確認。

## 2026-07-22 段階4: 実行中の AEX 切替 (File 設定項目)

要件 (ユーザー): 環境変数指定は「固定」扱い。**AviUtl2 を再起動せず実行中に AEX を
差し替えられる**べき。AviUtl2 は設定項目をロード時に一度だけ読む (静的 config) ので、
実行中選択された AEX のパラメーターは個別コントロールにはできない (段階2 のマッピングは
ロード時 discovery した env AEX にのみ効く)。設計:

- **File 設定項目 "AEX" を config[0] に前置**。空 = env AEX (パラメーターコントロール付き)。
  別の .aex を選ぶとその AEX にライブ切替、そちらは自前の既定値で描画 (パラメーター非公開)。
- `resolve_aex(config)`: File 値があれば override、なければ env_plugin。sha を計算し、
  `is_default = env_sha == sha` (**content 比較**。ファイルダイアログでパスが再正規化されても
  env AEX のコントロール値が効き続ける)。smart は is_default なら load 時の値、別 AEX なら
  `smart_for` (bit 10 を sha キャッシュ付きで inspect)。
- **SessionIdentity に plugin_sha256 + smart を追加**。AEX を切替えると sha が変わり identity
  不一致 → 既存セッションを evict して新 AEX で reopen。effect インスタンス単位で分離。
- **sha_for**: path + (mtime, len) で sha をキャッシュ。毎フレーム multi-MB の read+hash を
  避ける (export の数千フレームで効く)。rebuild は mtime/len 変化で無効化 → 固定 AEX の
  再ビルドも検出して reopen。Windows (`.auf2` 唯一の対象) は NTFS で mtime 常時取得可。

ローカルレビュー (2 巡): P2×2 を指摘・修正。(1) 毎フレーム read+hash → sha_for キャッシュ化。
(2) is_default のパス比較 → env_sha content 比較。再レビューで両者解消・新規欠陥なしを確認。

### Codex round 2 (PR #294) 対応

- P2「セレクタは前置でなく後置」: File "AEX" を config[0] 前置していたのを**末尾に後置**に変更。
  前置はパラメーターの位置インデックスを全てずらす (後のレイアウト変更/保存プロジェクトで
  値がずれる)。`apply_config_values` は `config.zip(slots)` で slots(長さm)が先に尽きるため、
  full config を渡しても末尾の File は消費されない。proc_video の `config.get(1..)` を撤去。
- P3「is_default は content だけでなく同一パスも要求」: 別ディレクトリの同一バイトコピーは
  sha 一致だけで is_default=true になり env のパラメーターが適用されてしまう (コピーは隣接
  リソースが違えば別挙動)。**is_default を canonical path 比較に変更** (`env_canonical` を
  load 時に確定、`resolve_aex` で `canonicalize(selected)==env_canonical`)。canonical は
  大文字小文字/区切りを正規化するので**再選択のパス正規化と別ディレクトリ判別を同時に満たす**
  (sha より正しい)。env_sha フィールドは撤去。

## 2026-07-22 段階5 調査: フォルダ内各 AEX を別々のキーフレーム可能フィルタに

要件 (ユーザー): AviUtl2 のキーフレームは登録済み config 項目 (Track 等) にしか効かない。
実行中に選んだ AEX のパラメーターをキーフレーム可能にする迂回策が要る。無ければ**フォルダ内の
各 .aex を、それぞれ固定パラメーター付きの別フィルタとして登録**したい。

### ABI 調査 (aviutl2-sys 0.40 = SDK ヘッダの手書き Rust 転写)

- **Q1 ロード後の動的 config 登録: 不可。** config 項目は `InitializePlugin`/テーブル生成時に
  凍結 (`GetFilterPluginTable` が1テーブル返し items を leak)。再宣言フック無し・個数可変無し。
  キーフレーム対象 (Track) もロード時固定。ただし `plugin_info()` はロード時に走るので、
  **ロード時点で判る情報 (フォルダ走査結果) から Track を組み立てるのは可能**。
- **Q2 単一 DLL で複数フィルタ: 可能 (SDK レベル)。** `register_filter_plugin!` (単一専用、
  `no_mangle GetFilterPluginTable` 1個) ではなく、**generic 経路** (`register_generic_plugin!`
  + `HostAppHandle::register_filter_plugin` を N 回) で N フィルタ登録できる。各フィルタは
  独自の名前・固定 config を持てる。
- **per-filter fn ポインタが必須 (共有不可)。** `func_proc_video(video: *mut FILTER_PROC_VIDEO)`
  はテーブルポインタも context も受け取らない。`OBJECT_INFO.effect_id` はあるが
  **effect_id → フィルタ名/定義 の対応 API は sys 全体に存在しない**。よって共有 proc では
  1オブジェクトに自分のフィルタが複数載った時に発呼元を区別できない。正しく捌くには
  フィルタ毎に別の fn ポインタ identity が要る。
- **値はキー名で読める。** `get_object_track_value(object, effect名, 項目名, frame, &value)`
  でキーフレーム済み値を名前ベースで取得できる (leak した item ポインタに縛られない)。
- **generic プラグインの拡張子は `.aux2`** (filter=.auf2 / input=.aui2 / output=.auo2)。
  `.auf2` で置くと AviUtl2 が `GetFilterPluginTable` を探して `GetProcAddress failed`
  (HRESULT 0x8007007F) で失敗する (2026-07-22 に踏んだ)。logger の `[Plugin::vi5.aux2]` が根拠。

### 実装方針 (ユーザー決定): libffi closure で上限なしの実行時 N

per-filter fn ポインタを実行時に必要数だけ得る手段は (A) libffi closure でAEX毎のC
コールバックを実行時生成 (上限なし) か (B) コンパイル時スロットプール (上限あり) の 2 択。
ユーザーは「128 は近い天井、フォルダの AEX が 128 超は普通にある」として (A) を選択。
libffi (成熟クレート、実行可能メモリ管理込み) で AEX を捕捉した
`extern "C" fn(*mut FILTER_PROC_VIDEO)->bool` を実行時生成する。

### スパイク検証 (aviutl2-multifilter-spike, `.aux2`)

観察 (実機、2026-07-22): generic `.aux2` から 2 フィルタ (Spike Tint R / Spike Tint G、
各1 Track "Amount") を crate の generic 経路 (compile-time 型) で登録。**両方が別フィルタ効果
として出現し、独立に機能 (R/G を各々着色) し、各 Amount に独立してキーフレームを打てた。**
→ 「1 DLL から複数フィルタ登録 + 各フィルタ独立キーフレーム」を AviUtl2 が honor することを
立証。段階5 の土台が成立。**未検証: libffi closure 生成の fn ポインタが func_proc_video として
機能するか** (スパイクは compile-time 型で、libffi 経路は次に別スパイクで確認する)。

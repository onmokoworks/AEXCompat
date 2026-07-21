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

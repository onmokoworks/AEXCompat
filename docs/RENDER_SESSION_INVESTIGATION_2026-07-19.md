# issue #98 段階0 調査ノート: 常駐workerレンダリングセッション

時系列追記。観察 (事実) と仮説 (推論) を分けて記録する。項目番号は
issue #98 本文「段階 0」の調査項目に対応する。

## 2026-07-19

### 項目5: 継承 HANDLE + file mapping の机上調査 (観察)

- `broker/crates/broker/src/windows_process.rs` の `run_isolated_impl` は既に
  `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` で継承 handle を明示指定している
  (stdout/stderr パイプ write 端 + opt-in の trace file handle、
  `windows_process.rs:349`)。`bInheritHandles=1` だが handle list で継承対象を
  絞る構造。
- trace file handle の worker への伝達は環境変数
  `AEX_INSTRUMENT_TRACE_HANDLE` に handle 番号を書く方式
  (`windows_process.rs:155` `child_environment`)。broker が作成した handle を
  番号で伝える前例はここにある。
- restricted token 経路 (`CreateProcessAsUserW`) と通常経路 (`CreateProcessW`)
  のどちらも同じ attribute list / 継承機構を通る。Job Object は
  `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_PROCESS_MEMORY` 付き。
- `hStdInput` は null (`windows_process.rs:385`)。worker への入力パイプは現状
  存在しない。また stdout は worker の JSON レポート出力に使用中。
- 現行 merged コードの minidump 転送は `--minidump-v1 <dir>` のパス渡し
  (`l2_main.cpp:22304` で worker が `is_directory` を再検証)。CLAUDE.md に
  ある「#66 の broker 作成継承 dump handle」はこの worktree の HEAD
  (origin/main a9ed8a2) では未確認。要追跡。

### 項目5: 設計への含意 (仮説)

- フレームループの制御チャネルを stdin/stdout に載せる案は、stdout が既に
  最終 JSON レポート専用である点と衝突する。制御チャネルは専用の継承パイプ
  handle (HANDLE_LIST + 環境変数番号伝達、trace file と同型) にする方が
  既存構造と整合すると思われる。
- 無名 file mapping (`CreateFileMapping(INVALID_HANDLE_VALUE, ...)`) は名前も
  パスも持たないため、restricted token の ACL に依存せず継承 handle だけで
  worker から `MapViewOfFile` できる可能性が高い。spike で要実証。
- Job Object の `ProcessMemoryLimit` はプロセスの private commit を制限する
  もので、共有 section の view が worker 側の課金にどう乗るかは未確認。
  FHD 数スロット分 (数十MB) が上限に食い込まないか spike で要確認。

### 項目4: 現行 one-shot 経路の性能ベースライン (観察)

計測方法: `aex_render_worker.exe` を直接起動 (broker の sealed staging・
hash 検証・PNG エンコードは含まない)。fixture は `pf_sampling_probe.aex`、
`--render-image` (8bpc)、N=12、warm。`tools/bench_oneshot_render.py` にて。
機材はこの開発機 (Windows 11)。数値は機材・AV スキャン状態依存の参考値で、
frozen evidence ではない。

| 計測 | median | min | max |
|---|---|---|---|
| プロセス起動フロア (即 exit 2) | 254.7ms | 26.7ms | 308.7ms |
| 37x23 レンダー一式 | 38.6ms | 35.6ms | 571.5ms |
| 1920x1080 レンダー一式 | 123.5ms | 76.4ms | 164.6ms |

- worker exe の hash は本番 admission (`admit_local_worker` = ディスクから
  open して stream hash) と同じ経路で median 1.394ms (868KB、warm、disk/AV
  込み)、probe AEX は median 0.697ms (18KB)。broker は dispatch ごとに worker
  exe を hash するが、起動フロア 35〜40ms に対して支配項ではない。
  (訂正 2026-07-19: 当初は read_bytes 後の CPU-only sha256 で 0.55ms/回と
  記録していたが、これはディスク/AV スキャンのコストを落としていた。Codex
  PR #101 P2 指摘を受け、本番と同じ open+stream 計測に修正。結論=支配項では
  ない、は変わらず。)
- 入力 raw 書き込みは FHD 8.3MB で 11.1ms。
- 起動フロアの median 254ms は最初に計測した (cold) 系列で、後続の
  37x23 系列 (median 38.6ms、起動込み) と矛盾する。ばらつきの原因は
  ウイルススキャン等の初回実行効果と思われる (仮説)。warm 状態の
  プロセス起動+AEXロード+lifecycle 固定費は 35〜40ms 程度と読むのが妥当。

### 項目4: 含意 (仮説)

- warm でも FHD 1フレーム 123ms ≒ 8fps が worker 直叩きの上限で、broker の
  staging・検証・PNG 変換を足すと実効はさらに落ちる。25fps (40ms/frame) には
  one-shot 構造のままでは固定費だけで届かない。常駐セッション + 共有メモリ化
  の動機を数値で裏付けた。
- レンダー本体 (probe の per-pixel sampling) と転送の内訳分離は未実施。
  セッション実装後の比較では同一 fixture・同一寸法で before/after を取る。

### 項目7: AviUtl2 フィルタプラグイン API の制約 (観察)

一次情報: 公式SDK zip (2026/7/18 版、MIT License) を展開し `filter2.h` /
`plugin2.h` / `cache2.h` / `output2.h` / 同梱ドキュメントを直接確認した。
配布元は「AviUtlのお部屋」
<https://spring-fragrance.mints.ne.jp/aviutl/> (SDK:
`aviutl2_sdk.zip`)。非公式ミラー
<https://github.com/aviutl2/aviutl2_sdk_mirror> は公式 zip と一致確認済み。

- フィルタプラグイン (.auf2) API は公開済み。`GetFilterPluginTable()` が
  `FILTER_PLUGIN_TABLE` (設定項目 + `func_proc_video` / `func_proc_audio`)
  を返す方式。API はほぼ毎週拡張されており流動的。
- 画像バッファは packed のみ (planar 形式は存在しない):
  - 基本経路 `get_image_data` / `set_image_data` は 8bit RGBA 密詰め
    (stride 引数なし)。
  - pitch 指定の拡張経路 `get/set_image_resource_data` があり、取得側は
    RGBA (R8G8B8A8_UNORM) / PA64 (R16G16B16A16_UNORM) / HF64
    (R16G16B16A16_FLOAT)。内部フォーマットは HF64 = fp16 RGBA 乗算済みα
    (`output2.h` コメント)。書き込み側は加えて BGRA / BGR / YUY2 / YC48。
  - `get_image_texture2d()` で `ID3D11Texture2D*` も取得可 (フィルタ処理
    終了までのみ有効)。
- 呼び出しモデル: `func_proc_video` は同期コールバック。false 返却で以降の
  フィルタ・出力が中断。画像と音声は別スレッドと明記。設定値はホストが
  呼び出し直前にグローバル構造体へ書き込む方式。毎呼び出しで `OBJECT_INFO`
  (frame 番号、総フレーム数、effect_id、レイヤー等) が渡る。
- フレーム順序保証の記載なし。`cache2.h` のサンプルはエフェクト毎・フレーム
  番号毎のキャッシュ構成で、同一フレームの再要求・ランダムアクセス前提の
  設計。プレビュー/出力の区別はフィルタ API 自体には無い
  (`plugin2.h` の `get_edit_state()` 側にはある)。
- ホストの合成済みフレームキャッシュを読む API は無い。ソース素材の任意
  フレームは `CACHE_HANDLE::get_video_file_cache` で取得可。汎用プラグイン
  (.aux2) 側の `rendering_scene_video(frame, callback)` で現在シーンの任意
  フレームを非同期レンダリング依頼できる (出力中は失敗)。
- 解像度上限のヘッダ記載なし。音声は PCM float32 2ch。

### 項目7: 設計への含意 (仮説)

- **ブリッジは非順次前提が必須**: フレームが時間順に来る保証がなく同一
  フレーム再要求もあるため、AviUtl2 消費者はセッション仕様の非順次
  アクセス扱い (項目3 の `NON_SEQUENTIAL_RENDER` 意味論) に直接依存する。
  時間依存 AEX をランダムアクセス下でどう扱うかが設計課題として確定的に
  効いてくる。
- **同期呼び出しなのでフレームあたりレイテンシがそのまま UX**: ホストの
  レンダリングは応答までブロックする。常駐セッションの per-frame 往復を
  低く抑える設計 (共有メモリ + イベント) の妥当性を裏付ける。
- **深度変換が必要**: AviUtl2 の 16bit は PA64 (full-range unorm、乗算済みα)、
  AE の 16bpc は 0..32768 white point かつ straight α が基本。HF64 (fp16
  乗算済み) ⇔ AE 32f straight の変換含め、α前乗算の解除/再適用と値域変換を
  ブリッジ層で規定する必要がある。
- 設定値グローバル書き込み方式から、同一プラグインの video 呼び出しが並列
  多重化されることは無いと思われる (video/audio 間の競合修正が更新履歴に
  ある点が傍証)。セッションは effect_id 単位で 1 本ずつ持てば足りる見込み。
- ユーザーが当初言及した「平面画像データ」について: AviUtl2 の転送形式は
  packed のみで planar は存在しなかった。ブリッジの転送スロットは packed
  RGBA 系 (8bit / PA64 / HF64) を前提にできる。

### 設計判断: 消費者プロセス (AviUtl2 等) への in-process AEX ロードは採用しない (合意 2026-07-19)

検討の経緯: AviUtl2 のプロセスに AEX を直接ロードする形式を検討し、
却下した。理由:

1. **crash containment の喪失**: AEX のクラッシュ・ハング・ヒープ破壊が
   ホストアプリ (編集中のユーザープロジェクト) を道連れにする。ハングは
   in-process スレッドでは安全に停止できない。crash containment は常時オン
   の床 (Project Direction 5) であり、これを外す形式は採らない。
2. **性能利得が小さい**: one-shot 経路の遅さの支配項はプロセス起動 + AEX
   ロード固定費 (warm 35〜40ms) とファイル I/O であり、常駐セッション +
   共有メモリで両方消える。セッション化後に残るプロセス間コストはイベント
   同期 (数十μs級) + コピー (FHD memcpy 1〜2ms、設計次第でゼロ) 程度で、
   レンダー本体に対して誤差の範囲 (仮説、セッション実装後の実測で検証)。
3. **付随コスト**: 消費者ホストのスレッドモデルへ AEX lifecycle 期待を直接
   合わせる必要が生じる。minihost ホスト核のライブラリ化と二重保守が要る。
   worker 境界が無いと診断の再現性・信頼性が落ちる。

再検討の条件: セッション + 共有メモリ実装後の実測で、同期コストが支配項に
なると示された場合に限り、明示オプトインの高速パスとして再検討する。

### 項目3: レンダー順序契約と sequence data 意味論 (観察)

一次情報: ローカル SDK 25.2 ヘッダ (`AE_Effect.h` / `AE_EffectSuites.h` /
`AE_EffectSuitesOld.h` / `AE_GeneralPlug.h`) と docsforadobe ガイド
(<https://ae-plugins.docsforadobe.dev/> の PF_OutData / command-selectors /
global-sequence-frame-data / multi-frame-rendering-in-ae)。

- **訂正**: issue #98 本文と設計ドラフト v1 に記載した
  `PF_OutFlag_NON_SEQUENTIAL_RENDER` という flag は **Effect SDK に存在
  しない** (SDK 25.2 全ヘッダ検索・docsforadobe とも該当なし)。名前が似た
  ものは AEIO (メディア入出力) 側の `nonSequentialOk` /
  `AEIO_MFlag_CAN_ADD_FRAMES_NON_LINEAR` で、Effect API とは別物。
- Effect API には**レンダー順序を宣言・保証する仕組み自体がない**。random
  access が契約上の前提で、順序依存 (シミュレーション系) プラグインへの
  公式指針は「sequence_data に自前キャッシュを持ち、任意フレーム要求時に
  `PF_HaveInputsChangedOverTimeSpan` (旧) / `PF_GetCurrentState` +
  `PF_AreStatesIdentical` (現行) でキャッシュを検証して履歴を再構成する」
  というプラグイン側義務 (`AE_EffectSuitesOld.h` 40-56 行)。
- ホストのフレーム単位の義務は FRAME_SETUP → RENDER → FRAME_SETDOWN と、
  時刻フィールド (current_time / time_step / time_scale / total_time) の
  正確な供給。time_step は可変フレームレート時の SEQUENCE_SETUP で 0 に
  なり得るが FRAME 系では常に正値。
- sequence lifecycle の発行タイミングは全てホスト都合のイベント駆動
  (保存・複製・スレッド配布・読込)。レンダー専用セッションの最小構成は
  SETUP (UI 相当文脈で 1 回) → [RESETUP、入力 NULL 許容、
  `PF_InFlag_PROJECT_IS_RENDER_ONLY` ヒント付き] → フレームループ →
  SETDOWN。FLATTEN は保存/複製時のみで、発行しない運用も契約違反ではない
  (ただし flatten 経路を通さないと NEEDS_FLATTENING 系のバグは観察できない)。
- 時間方向 flag: `PF_OutFlag_WIDE_TIME_INPUT` (時間外 checkout に応じる
  義務、AE10 以降は非推奨) / `PF_OutFlag2_AUTOMATIC_WIDE_TIME_INPUT`
  (SmartFX 専用、ホストに checkout 追跡と時間選択的キャッシュ無効化の
  義務) / `PF_OutFlag_NON_PARAM_VARY` (時刻自体が入力になる宣言)。
- MFR (`PF_OutFlag2_SUPPORTS_THREADED_RENDERING`): レンダー中
  `in_data->sequence_data` は **NULL** になり、const 読みは
  `PF_EffectSequenceDataSuite1` 経由。書き込みは
  `MUTABLE_RENDER_SEQUENCE_DATA_SLOWER` の複製 + 定期破棄パスのみ。
- 非 MFR Classic では RENDER 中の sequence_data 書き換えは禁止されて
  いない (handle はホストがロックして渡す)。次フレームへ同じ handle を
  引き回すのが AE 単一スレッド Classic と同じ挙動。

### 項目3: 設計への含意 (仮説)

- 未決事項「非順次アクセス (シーク・逆再生) の扱い」への回答が出た:
  **セッション仕様は「任意時刻の render_frame を受ける」で AE と等価**。
  順序保証をプロトコルに入れる必要はなく、入れても AE より寛容になる
  だけ。AviUtl2 側のランダムアクセス前提 (項目7) ともそのまま整合する。
- 順序依存プラグインの互換性問題は flag では検出できない。checkout の
  時刻引数と RENDER 中の sequence_data 書き換えを診断に記録する観察
  アプローチが適切と思われる。
- MFR flag を立てるプラグインに sequence_data を渡し続けると AE 実機と
  観察が食い違う可能性が高い。「MFR 宣言時は render 中 NULL + suite 経由
  const 読みを再現するか」をセッション v1 の設計課題に追加する。

### 未着手 (このセッションで継続中)

- 項目1・2・6 (AE 実機観測、SmartFX checkout) と項目5 の実証 spike は未着手。
- 項目7・3 の机上調査は完了。AE 実機観測 (項目1・2) の焦点は、机上で
  確定できなかった「実際の selector 発行頻度」(RESETUP がレンダー専用
  文脈でいつ来るか、パラメーター変更時の実挙動) に絞れる。

### 項目5: 実証 spike の結果 (観察、2026-07-19 追記)

上記「未着手」のうち項目5 の実証 spike を完了した (訂正: 項目5 は以降
未着手ではない)。実装は `broker/crates/broker/tests/render_session_shm_spike.rs`
(broker 側) と `broker/crates/dummy-workers/src/bin/session_shm_probe.rs`
(worker 側 fixture)。launch は `run_isolated_impl` と同型
(CreateProcessAsUserW + PROC_THREAD_ATTRIBUTE_HANDLE_LIST + Job assign 後
resume)、restricted token + `protect_sealed_load_tree` 済みディレクトリから
起動。数値は開発機の参考値で frozen evidence ではない。

構成: 無名 file mapping (pagefile 裏、SEC_COMMIT) 256MiB、Job
`ProcessMemoryLimit` 128MiB (意図的に section より小さくした)、上限超の
private VirtualAlloc probe 192MiB、auto-reset event 対で ping-pong 2000 回。

- **継承 handle だけで map 可能**: restricted token の worker が、名前や
  パス、ACL を介さず継承 handle のみで `MapViewOfFile(FILE_MAP_ALL_ACCESS)`
  に成功。双方向の読み書きも成立 (broker のパターンを worker が検証し、
  worker の ACK・レポートを broker が回収)。handle 番号の伝達は環境変数
  (trace handle と同型) で機能した。
- **共有 view は ProcessMemoryLimit に課金されない**: worker が 256MiB の
  view 全ページを write touch しても、worker の private commit
  (PagefileUsage) は 712KiB → 1228KiB (増分 ~0.5MiB)。Job の
  `PeakProcessMemoryUsed` も 1296KiB。cap 128MiB < view 256MiB の構成で
  worker は正常終了した。同時に、上限超 192MiB の private `VirtualAlloc` は
  失敗しており、cap 自体はこのプロセスに実効している (「課金されない」観測
  の証拠力を担保)。working set には共有ページが乗る (touch 後 ~261MiB)。
- **親 (broker) 側にも section 全量は課金されない**: section 作成 + view +
  パターン書き込みで親の private commit 増分は ~0.5MiB。SEC_COMMIT の commit
  charge はシステム commit に乗り、どちらのプロセスの
  ProcessMemoryLimit 予算も消費しない。
- **イベント往復レイテンシ**: 2000 回の ping-pong で median 16.6μs、
  min 2.1μs、max 429.2μs。
- **worker 終了後の回収**: broker は自分の view を保持していれば worker の
  exit 後にレポートを読める (今回レポート回収は exit 後に実施)。

### 項目5: 実証結果の設計への含意 (仮説)

- 共有メモリスロットは Job のメモリ上限と独立に予算化できる。FHD 8.3MiB ×
  数十スロットでも 512MiB cap を圧迫しない。`memory_limit_reached` 検知
  (worker 自身の PeakPagefileUsage ベース) も共有 view では誤検知しない
  と思われる。
- per-frame の同期コスト (往復 数十μs) は warm レンダー 123.5ms に対して
  誤差。in-process ロード不採用判断の再検討条件 (同期コストが支配項になる
  場合) は、この観測からは満たされる見込みが薄い。
- worker がクラッシュしても broker 側 view から部分結果・診断を回収できる
  余地がある。セッションプロトコルの異常系設計に使えると思われる。
- 段階1 の制御チャネル設計 (HANDLE_LIST + 環境変数での handle 番号伝達 +
  専用イベント/パイプ) はこの spike の形をそのまま昇格させれば足りる。

### 段階1 PR-C: broker RenderSession + バッチ動画レンダー CLI (2026-07-19 追記)

実装は `broker/crates/broker/src/render_session.rs` (RenderSession
open/render_frame/close + `render-video-batch` CLI)、launch 分離は
`windows_process.rs` (`run_isolated_impl` を launch (`launch_isolated_impl`) と
回収 (`LaunchedIsolatedProcess::wait_and_collect`) に分割、one-shot 経路は
分割後の合成で挙動不変)、sealed staging のセッション生存期間保持は
`secure_launch.rs` (`SecureSessionProcess`) と `secure_image_dispatch.rs`
(`dispatch_secure_image_session`)。

観察 (実 worker との相互運用、開発機、frozen evidence ではない):

- PR-B (#115, b11129d) の worker を worktree でビルドし、broker CLI
  `render-video-batch` で PNG 3 フレームを 1 worker プロセスに通した。
  `passed: true`、`persistent_sequence_setup_error/setdown_error: 0`、
  `render_error: 0`、`guard_bytes_intact: true`、suite/handle lifetimes
  balanced。同一入力 3 フレームの slot checksum は一致し、フレーム転送は
  決定的だった (fixture: pf_sampling_probe)。
- broker 統合テスト (`broker/crates/broker/tests/render_session.rs`) は
  プロトコル忠実な dummy worker
  (`dummy-workers/src/bin/session_protocol_worker.rs`) を実プロセス
  (restricted token + sealed tree + Job Object) で駆動し、正常系・
  frame-local エラー継続・watchdog (deadline 超過で TerminateJobObject)・
  クラッシュ無効化・generation 不一致・静的ヘッダ改変・checksum 不一致の
  fail-closed を検証した (9 本、いずれも pass)。

逸脱 (プロトコル §8 からの):

- §8 は「per-frame 出力検証は `render_with_artifact` 内の検証列
  (image_render.rs の selector/bounds/size/pixel/guard 検証) を純粋関数に
  切り出して one-shot とセッションで共用する」とした。実装では共用せず、
  セッション専用の per-frame 検証 (`validate_ok_frame` + slot checksum
  再計算) を書いた。理由: one-shot の検証列は one-shot worker report の
  フィールド (spatial echo、custom UI、GPU 系) に強く結合しており、
  per-frame の正本は `frame_done` + slot バイト列でフィールド集合が重なら
  ない。共有済みの純粋関数 `validate_image_buffer_layout` は両経路で共用
  している。one-shot 側の検証列の純粋関数化は PR-D (長さ1セッション
  wrapper 化) で改めて評価する。
- §8 の API スケッチでは `close()` が `io::Result<SessionReport>` だが、
  実装は診断を常に返すため `Value` (セッション要約 JSON) を返す。無効化
  済みセッションの close でも「frame N で死亡、N 以降未レンダー」を
  構造化して返すため。

### 段階0 項目1・2・6: AE 実機のフレーム列 selector 観測 (2026-07-19 追記)

観測手段: 新規 instruments `pf-selector-timeline-probe` (Classic/Smart 2 flavor、
全 selector を JSONL sidecar へ記録、sequence data 内カウンターで継続性を追跡、
アニメーション可能な "Drive" slider と挙動選択の "Probe Mode" slider を搭載)。
`tools/capture-selector-timeline.ps1` が probe を
`MediaCore\AEXCompatOracle` 配下へ一時インストールし、AfterFX.com で
24 フレーム comp (640x360, 30fps) + Drive keyframe (0→100) の project を構築、
aerender で全フレームレンダーして sidecar を回収する。環境: After Effects
25.3.1x3 (aerender)、Windows 11。数値は開発機の観察で frozen evidence では
ない。生ログは `target/selector-timeline/` (untracked)。

#### 観察 (Classic flavor、MFR 宣言なし、24 フレーム、Drive アニメーション)

- render engine プロセスは 1 つ。GLOBAL_SETUP / PARAMS_SETUP /
  GLOBAL_SETDOWN は各 1 回。
- **render engine で SEQUENCE_SETUP は 1 回も呼ばれない**。project 構築側の
  AE セッションが SETUP → (FLATTEN → RESETUP)×3 → SETDOWN を発行し
  (project 保存のたびに FLATTEN。`SEQUENCE_DATA_NEEDS_FLATTENING` を宣言して
  いなくても呼ばれる)、render engine は保存済み project のロードから
  **SEQUENCE_RESETUP (11 回)** で復元する。
- **sequence data はレンダーコンテキストごとにクローンされ、全フレームを
  通した単一の連続 handle は存在しない**。render engine には 12 の thread が
  現れ、RESETUP ごとに「保存時点の counter 値」へ戻った独立クローンが生まれ、
  クローン内でのみ render counter が継続する (1→2→3)。SETDOWN はクローン数
  と同数。
- **フレーム順序は非順次**。RENDER 発行順 (frame): 4, 7, 2, 0, 6, 5, 3, 1,
  8, 9, ..., 20, 16, 17, 19, 18, 15, 21, 22, 23。atomic 通し番号で見る限り
  selector 呼び出し自体は直列 (ブロック交差なし)。
- フレームごとの列は FRAME_SETUP → RENDER → FRAME_SETDOWN で固定 (各 24 回)。
- 時刻表現: time_scale=30720 (30fps×1024)、time_step=1024。
- **項目2**: アニメーションする Drive の値変化は selector を誘発しない
  (USER_CHANGED_PARAM / UPDATE_PARAMS_UI / RESETUP のいずれも来ない)。
  per-frame の補間値が FRAME_SETUP / RENDER の params にそのまま届く。
  RESETUP はコンテキスト管理 (クローン生成) にのみ紐付く。

#### 観察 (Smart flavor、SUPPORTS_SMART_RENDER、24 フレーム、Drive アニメーション)

- ライフサイクル構造は Classic と同一 (RESETUP×11 クローン、順不同分配、
  SETUP なし)。フレームごとに SMART_PRE_RENDER → SMART_RENDER が各 1 回。
- **項目6 (checkout パターン)**: host が effect へ渡す output_request は毎
  フレーム同一で、レイヤー範囲より外側へ拡張されていた
  ([-64,-36,704,396] = 640x360 の各辺 10% 拡張)。probe の full-frame
  checkout への応答 (result/max_result) も毎フレーム同一の full-frame。
  この単純 comp ではフレーム間で checkout 要求・応答は変化せず、v1.1 の
  単一入力スロット割当で足りると思われる。
- **フレームキャッシュ**: 静止入力 + 静的パラメーターの 8 フレーム comp
  (smart-inset run) では SMART_PRE_RENDER / SMART_RENDER が **1 回だけ**
  発行され、8 フレーム分の出力 PNG が同一レンダーから複製された。
  「1 出力フレーム = 1 RENDER」は AE では成立しない (時間変化がなければ
  キャッシュされる)。oracle の複数フレーム比較 (#10 の temporal 拡張) は
  時間依存性のある comp でのみ per-frame レンダーを強制できる。

#### 仮説・含意 (セッション設計への反映)

- AE 自身が aerender で全フレーム連続の sequence data を保証していない
  (コンテキストクローン単位の継続のみ)。セッション v1 の「1 worker = 1
  sequence handle 連続」は AE の 1 レンダーコンテキストと同型で、AE 等価性の
  観点で過剰でも不足でもないと思われる。
- v1 が SEQUENCE_RESETUP を発行しない設計は「単一コンテキストの生存期間」に
  一致する。per-frame パラメーター変更 (#107) を将来入れる場合も、AE は値
  変化で RESETUP しないため、param_epoch 相当を RESETUP に結び付ける必要は
  ないと思われる (メッセージフィールド追加で足りる)。
- 非順次アクセス (シーク・逆再生) はセッション仕様が既に「順序仮定なし」で
  あり、AE 実機の分配挙動 (順不同) と整合。

### 段階1-3 (one-shot の長さ1セッション wrapper 化) の実現可能性調査 (観察、2026-07-19 追記)

PR-C (#116) merge 後のコードに対して、段階1-3「既存 one-shot 経路の
『長さ 1 セッション』wrapper 化 (挙動不変)」が成立するかを調査した。

- `SessionOpenRequest` (render_session.rs:307-322) が受けられる構成は
  plugin + parameters (payload) + 寸法 + 深度 + timing + dependencies のみ。
  one-shot の argv trailer 群のうち secondary/timed layer、audio sidecar、
  mask/spatial/render-environment context、custom UI action (click/draw)、
  alpha-as-coverage、aux manifest、parameter animation、world dump、
  output-checksum-detail、GPU backend/runtime policy はいずれも渡せない
  (parameter animation は worker のセッションモード側は対応済みで、broker
  側だけが欠けている → issue #132)。SmartFX も worker_kind が Render 固定で
  非対応 (プロトコル v1 のスコープどおり)。
- 戻り値スキーマが非互換。one-shot 公開エントリは worker report を平坦化
  した `interactive_image_render` Value を返し、harness GUI はそのうち
  `render_path` / `worker_classification` / `worker_diagnostics.stage_events`
  / `gpu_*` を診断表示に消費する (harness/src/main.rs:1729-1758,
  4506-4542)。`RenderSession::close` の Value は別スキーマ
  (`render_session_close` + `final_report`) でこれらを提供しない。
- 検証コードは共有されていない。#116 は image_render.rs に可視性変更のみを
  加え、per-frame 検証はセッション側 (`validate_ok_frame` 等) の独立実装。
  検証対象自体も異なる (one-shot = 出力ファイル、セッション = 共有メモリ
  header/slot)。

### 段階1-3 の方針再提案: 段階分割によるゴール到達計画 (提案、訂正 2026-07-19)

(訂正: 当初この節は「見送り」を提案したが、段階1-3 のゴール自体は維持し、
一発の挙動不変置換ではなく段階分割で到達する計画に改める。)

観察から、「今すぐ一発で挙動不変置換」は成立しないが、ギャップはすべて
埋められる種類のものである。以下の段階で到達する:

- **W1: セッションの静的構成受容を one-shot と同等にする** (独立小 PR 群)
  - parameter animation (#132): worker 対応済み、`SessionOpenRequest` +
    sidecar + argv 付与のみ。最小。
  - aux manifest / alpha-as-coverage / dump-worlds / checksum-detail:
    auxiliary option 群は worker のセッション argv が既に受容している
    (`strip_auxiliary_options` はセッションでも走る)。broker 側 open に
    足すだけ。最小。
  - mask / spatial / render-environment context: セッション argv (固定
    argc=10) の拡張 + プロトコル文書 §3 改訂 + broker 側。中。
  - layer スロット: プロトコル §6 にレイアウト定義済み。worker の受容 +
    per-frame スロット転送 + broker 側。中〜大。
- **W2: wrapper 本体** (classic CPU 経路の切替)。worker のセッション最終
  レポートは one-shot と同じ ClassicReport 出力コードを使っているため、
  `final_report` + `frame_done` から `interactive_image_render` Value を
  合成する変換層は既存転記ロジックの流用で作れる。「挙動不変」の定義は
  「report バイト一致」ではなく公開契約不変 (harness が消費するフィールド
  + PNG バイト + pass/fail 判定) に置き直す (owner 合意事項)。custom UI
  (click/draw) と audio は当面 one-shot fallback に残す。
- **W3: SmartFX/GPU セッション (v1.1)**: `smart_render_runtime` の
  manage_sequence 相当 + broker の WorkerKind::Smart/GPU policy。W1/W2 と
  並行可。
- **W4: one-shot argv モードの縮退・削除**: 契約テストをセッション経由に
  移行してから。

クリティカルパスは W1 (context/layer) → W2。W1 の「最小」2 件は即着手
可能。custom UI のセッション化は #107 の per-frame parameters (v2) と
同時期に扱う。

## 追記 (W1-4b: timed layer をセッションで運ぶ)

観察 (実装):

- timed layer の worker 側消費ロジック (`l2_main.cpp` の
  `same_rational_time` 選択と `classic_context->add_timed_layer`) は既に
  render_once に存在する。W1-4b は transport の拡張のみで、`LayerInput` は
  `time`/`time_scale`/`timed` フィールドを既に持つ。
- `session-layers:v1|` trailer を 5 フィールド形式 `slot,w,h,time,scale`
  に拡張 (3 フィールドは従来の static secondary)。worker parser
  (`worker_request_parser.cpp`) はエントリ内カンマ数で 3/5 を分岐し、5 なら
  `timed=true` + `time`/`time_scale` を設定。dedup は one-shot の
  layered_image_mode と同一ラムダに揃えた (同一 slot は両方 timed かつ
  異なる有理時刻のみ許可)。
- broker `SessionLayer` に `timed: Option<(i32,u32)>` を追加。open の
  slot 一意性検証を「同一 slot は両方 timed かつ有理時刻相違のみ許可」に
  変更 (worker parse と一致、fail-closed を open 時点に前倒し)。trailer
  生成は `timed` が Some なら 5 フィールドを emit。物理スロットは layer
  index ごとなので同一 semantic slot の timed 複数もそれぞれ独立領域を占有。
- wrapper (`image_render.rs`) の適格条件から `timed_secondaries.is_empty()`
  を除去。static secondary と timed secondary を連結して `session_layers`
  を構築。session ルートの `secondary_layers` 診断は one-shot と揃えるため
  `timed.is_none()` で static のみ列挙 (両ルート同一集合)。

観察 (検証):

- broker 統合テスト `render_session` 26 件パス。追加した
  `timed_layers_travel_the_session_trailer_into_their_slots` (同一 slot に
  2 つの異なる時刻の timed + 別 slot の static、fixture worker が
  `layer_slot_count` と各スロット先頭バイトを検証) と
  `open_rejects_two_timed_layers_at_the_same_slot_and_time` (2/60 == 1/30
  の同時刻衝突を open が拒否) を含む。
- 実 worker 再ビルド後、wrapper A/B 等価テスト
  (`render_session_wrapper`) パス。plain classic 経路の回帰なし。

仮説 (残作業):

- 実 AEX での timed layer 消費の等価性 (A/B PNG バイト一致) は layer
  parameter を宣言する probe fixture (#195) を要する。それまでは fixture
  worker 経由の transport 検証が interim。
- alpha-as-coverage は W1-4b と別コミットで扱う (適格条件の
  `alpha_as_coverage_params.is_empty()` 緩和 + aux option 追加)。

### 訂正 (W1-4b dedup: static+timed 混在は拒否でなく許可)

上の W1-4b 追記で「同一 slot は両方 timed かつ異なる有理時刻のみ許可」と
書いたが、これは誤り。Codex レビュー (#204) の2指摘が逆方向を突いて真の
規則が判明した:

- 1回目: worker の session parse が static+timed 混在を受理するのに broker
  open が拒否する不整合を指摘 → 私は worker を拒否側に寄せた (誤り)。
- 2回目: one-shot の layered_image_mode parser
  (`worker_request_parser.cpp` の該当ラムダ) は static+timed 同一 slot を
  受理しており、それは layer parameter を current_time (static) と他時刻
  (timed) でサンプルする正当な構成。broker open が拒否すると wrapper が
  適格な render で無言 one-shot fallback する、と指摘。

観察: 正しい規則は one-shot と完全一致。同一 slot は (a) static 同士 →
拒否、(b) timed 同士同時刻 → 拒否、(c) static + timed の混在 → **許可**、
(d) timed 同士異時刻 → 許可。W1-4b の目的は one-shot 等価なので、session
の worker parse・broker open の双方をこの canonical 規則に揃えた
(worker は最初の実装 = one-shot ラムダに revert、broker open は
`(None,None)=>拒否, (Some,Some)=>同時刻拒否, _=>許可`)。テストは
`open_admits_a_static_and_timed_layer_at_the_same_slot` (受理) と
`open_rejects_two_static_layers_at_the_same_slot` (static 同士拒否) に
差し替え、`open_rejects_two_timed_layers_at_the_same_slot_and_time` は
維持。

教訓: 「両ルートの整合」を取る方向は2つあり (両方拒否 / 両方許可)、
canonical な基準 (= 既存の one-shot 挙動、AE 等価の真値) に合わせる方を
選ぶべきだった。1回目の指摘に literal に従って拒否側に倒したのが誤り。

## 追記 (W1-4c: alpha-as-coverage をセッションで運ぶ)

観察 (実装):

- alpha-as-coverage は one-shot で auxiliary option
  `--alpha-as-coverage-v1 <slot,...>` として送られる
  (`image_render.rs`)。worker (Render entry) は one-shot と共有の auxiliary
  フック `parse_l2_alpha_coverage` (`l2_main.cpp:2457`) でこれを parse し、
  `parse_alpha_coverage_params` がグローバル `g_alpha_as_coverage_params` に
  格納、classic render runtime が毎フレーム
  `publish_alpha_coverage_provider` で読む。
- auxiliary option は `classify_worker_mode` の前に
  `strip_auxiliary_options` で tail から剥がされる
  (`l2_cli_dispatch.cpp:76-77` のコメント)。session mode も同じ経路を通る
  ため、**worker 側の変更は不要**。設定は launch 時一度・全フレーム再利用で、
  session ライフタイムに一致。
- broker のみ変更: `SessionOpenRequest.alpha_as_coverage_params: &[u32]` を
  追加、open で one-shot と同一検証 (sort・重複禁止・slot <= 1024) 後に
  `--alpha-as-coverage-v1` を emit。`VideoBatchRequest` にも
  `alpha_as_coverage_params` を追加 (CLI 経路も対応)。wrapper の
  `session_representable_context` から `alpha_as_coverage_params.is_empty()`
  を除去し (aux_channels のみ残す)、host_context の slot を
  `SessionWrapperRequest` 経由で open に渡す。

観察 (検証):

- broker 統合テスト `render_session` 30 件パス。追加した
  `alpha_as_coverage_params_travel_the_session_launch` (open + fixture
  render 成功) と `open_rejects_an_out_of_range_alpha_as_coverage_slot`
  (slot 1025 を open が拒否) を含む。
- 実 worker + pf_sampling_probe の wrapper A/B (`render_session_wrapper`)
  に alpha-as-coverage ペア (`alpha_as_coverage_params:[0]`) を追加、
  session/one-shot の report 全フィールド + PNG byte 一致を確認。C++ 変更が
  ないため worker 再ビルド不要 (#204 マージ済み main と同一バイナリ)。

仮説 (残作業):

- pf_sampling_probe は alpha-coverage provider を消費しないため、byte 差
  としての意味的効果は現れない。alpha-coverage を実際に読む effect での
  検証は #195 の probe fixture 系の作業。それでも「両ルートが同一オプションを
  同一 worker に送る」等価性は wrapper A/B で確認済み。
- これで goal 条件1 (W1-3 context + alpha-as-coverage が session を通り適格
  条件に入る) が充足。残る one-shot 専用は audio / custom UI (#201 で確定) と
  aux channels (session transport 未対応、別途)。
## 追記 (2026-07-20): W3 SmartFX セッション v1.1 の実装

観察と実装記録 (issue #98 W3、プロトコル文書 §9.1 が正本):

- **観察 (worker 構造)**: smart 経路の SEQUENCE は `begin_render_lifecycle`
  が張っており、`smart_render_runtime` 自体に manage_sequence は無かった。
  拡張は「begin/end を frame-only lifecycle に差し替える」1 点に集約でき、
  `smart_execution::SessionFrame` (manage_sequence 相当 + 出力 ARGB 捕捉 +
  guard 判定) を 1 ポインタで通した。one-shot 経路は SessionFrame=nullptr
  で挙動不変。
- **実装 (worker)**: classic の `run_render_session` をフレームループ共通部
  (`run_session_frame_loop`) と renderer コールバックに分離し、smart 版
  (`run_smart_render_session`) は `smart_render_once(session)` を
  per-frame で呼ぶ。SEQUENCE_SETUP の遅延ホイスト (-47) と §4 のメッセージ
  仕様・fail-closed 判定は classic と完全共通。フレーム局所エラーは
  one-shot の優先順位 (GPU setup → PreRender → render → GPU setdown) で
  frame_done.render_error に畳む。寸法契約は classic と同じ「全フレーム =
  launch 寸法」で、SmartFX の partial/empty result も -44 で無効化する
  (部分レンダー受容はレイヤースロットと同時期の拡張)。
- **実装 (report/exit)**: smart 最終レポートに session_* フィールドを追加
  (`append_smart_session`)、session の clean 判定はセッション機構のみで
  決める (最終フレームの selector エラーで clean close を落とさない)。
  exit 23/24 契約を smart worker にも配線。broker `final_report_clean` は
  classic/smart でキー集合を分けて fail-closed。
- **実装 (broker)**: `SessionOpenRequest` に smart / gpu_backend /
  gpu_runtime_policy。GPU 起動は one-shot と同じ認証列を
  `dispatch_secure_gpu_image_session` として通す。セッションは飛行中
  リトライ不可のため Auto の CPU fallback は open 時に畳む (Auto+policy
  なし→CPU コマンド、明示 GPU+policy なし→open で拒否)。
- **検証**: worker 直接駆動の behavioral self-test 5 本
  (tests/test_smart_session_worker.py、pf_smart_geometry_probe) と broker
  統合テスト (fixture、smart close 契約 + GPU policy 拒否 + Auto 縮退) が
  pass。cargo workspace / pytest 全体も pass (既存の環境依存 ERROR 2 件
  (vswhere 応答空) は main でも再現し無関係)。
- **観察 (E2E 阻害、#185 に切り出し)**: 実 worker での smart batch E2E
  (render-video-batch smart:true) はこのマシンでは module audit 失敗で
  不成立。原因は W3 ではなく、smart dispatch が CPU レンダーでも
  `begin_backend_context(3)` (CUDA) を無条件初期化し、NVIDIA driver store
  DLL が audit の unknown (2 件) になる既存問題。sealed 経路の one-shot
  smart (`render_experimental_image_at_time_with_format(smart=true)`) でも
  同一失敗を確認済み (＝W3 回帰ではない)。classic batch は同一 sandbox で
  成功。audit なしの直接駆動では smart session は全シナリオ成功。

## 2026-07-20

### issue #107: プロトコル v:2 (per-frame parameters) の設計判断 (合意方針の実装着手)

GUI ハーネスの「パラメーター操作のライブ再レンダーでセッションを維持する」
には、レンダー間でパラメーター値を更新する手段が要る。v1 では launch argv
payload で固定のため、選択肢は (a) パラメーター変更ごとに open し直す、
(b) §4.2 予約どおり render_frame の v 増分でフィールドを足す、の 2 つ。
(a) は固定費 (warm 35〜40ms + broker staging/hash) を最頻操作で毎回払う
ことになり issue の目的を満たさない。(b) を採る。

観察 (worker 側実装調査):

- `render_once` は呼び出しごとに definitions をローカル再初期化し、
  requested (`apply_requested_assignments`, l2_main.cpp:4054) →
  parameter animation (同:4063) の順に毎回適用している。セッションループ
  (`run_render_session`, l2_main.cpp:4249) は launch 時の
  `RequestedAssignments*` を全フレームに渡しているだけで、per-frame の
  適用器は既に存在する。
- payload 符号化 (`encode_interactive_payload` ⇔ `parse_parameter_payload`)
  は両側に strict 検証込みで実装済み。メッセージに同じ符号化文字列を
  載せれば新形式の発明が不要。

設計 (プロトコル文書 §4.2.1 に反映):

- v:2 render_frame は `parameters` 必須・そのフレーム限りの完全置換。
  stateful な「セッションに sticky なパラメーター状態」は持たない
  (GUI も AviUtl2 も毎呼び出しで現在値を持っているため不要で、状態を
  持たない方が診断が単純)。
- 応答スキーマ・ヘッダレイアウト・launch 構成は不変。close は v:1 のみ。
- 不正 `parameters` はフレーム局所エラーではなくプロトコル違反 (broker が
  送信前検証済みのため、届いたら broker 欠陥か改竄)。宣言パラメーターとの
  render 時型不一致はフレーム局所のまま。

検証用 fixture: 既存 instruments にパラメーター値が出力画素へ反映される
probe が無い (pf-param-utils-animation-probe は suite 検証で値を色に
落とさない) ため、スライダー値を出力色に反映する `pf-parameter-echo-probe`
を追加し、Python behavioral self-test で「v:2 で値を変えたフレームの
出力バイトが自己計算した期待値と一致する」ことを確認する。

GUI アダプタ (issue #107 コメントの合意どおり): 別プロセス常駐 worker +
GUI 内非同期セッションスレッド。broker に公開セッション API
(open / render / close、interactive_image_render 型レポート合成) を足し、
harness はセッションスレッド 1 本がそれを所有する。セッション基盤失敗は
one-shot spawn_native へ fallback し、次レンダーで新セッションを開き直す
(SEQUENCE_SETUP からのやり直しであることは診断に明示する)。

### issue #107: before/after 実測 (観察、2026-07-20)

計測方法: `broker/crates/broker/tests/resident_session_live.rs` の
`resident_session_latency_versus_one_shot` (--release、--ignored 手動実行)。
fixture は `pf_parameter_echo_probe.aex` (float slider 1 本、値を出力色に
反映)、FHD 1920x1080 8bpc、N=12、パラメーター値を毎回変更。broker の公開
エントリ呼び出し時間 (staging・検証・PNG 変換込み) を計測。機材依存の
参考値で frozen evidence ではない。

| 経路 | median | min | max |
|---|---|---|---|
| one-shot (現行 GUI: パラメーター変更ごとに worker 起動) | 304.4ms | 283.1ms | 2680.4ms |
| 常駐セッション open (初回のみ) | 177.9ms | - | - |
| 常駐セッション render_frame (v:2 パラメーター更新込み) | 66.7ms | 58.1ms | 129.1ms |

- パラメーター操作 1 回あたり約 4.6 倍の改善 (304ms → 67ms)。echo probe の
  レンダー本体はほぼゼロなので、67ms の大半は FHD の入力転送 + PNG
  エンコード + 検証と思われる (内訳分離は未実施)。
- max 2680ms は one-shot 初回の AV スキャン系 cold 効果と思われる
  (段階0 項目4 と同じパターン)。常駐セッションはこの再発自体が起きない。

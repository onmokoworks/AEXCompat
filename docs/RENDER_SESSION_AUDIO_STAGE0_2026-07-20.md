# 常駐セッション audio 段階0 調査ノート (#98 W4 / #239)

status: 段階0 (AE 実機観測) 完了。設計方針が確定した (audio は image frame loop
に相乗りさせず別チャネル/別セッションで期間一括 audio を運ぶ)。実装は次段階。
時系列追記。観察 (事実) と仮説 (推論) を分けて書く。frozen evidence 化する値は
`analysis/` + refresh script 経由 (`docs/EVIDENCE_POLICY_2026-07-18.md`)。

## 位置づけ

#201 owner 判断 B により、classic レンダーに残る one-shot 専用構成 (audio /
custom UI) を session 化して 0 にするのがゴール (#98 W4 = one-shot argv モード
削除 の前提)。custom UI は v:2 一般化で実装済み (PR #241)。audio は「フレーム列
image session に別 selector 系列 (AUDIO_SETUP/RENDER/SETDOWN) の audio をどう
混ぜるか」の意味論が未決のため、実装前に AE 実機観測を挟む (推測で仕様化しない)。

## 観察 (SDK 机上、2026-07-20)

AE SDK (`ae25.2_20.64bit`) の `Examples/Headers/AE_Effect.h` および
`Examples/Effect/SDK_Backwards` から確定した audio ABI:

- audio selector: `PF_Cmd_AUDIO_SETUP` / `PF_Cmd_AUDIO_RENDER` /
  `PF_Cmd_AUDIO_SETDOWN` が存在する。
- audio effect の宣言には `PF_OutFlag_AUDIO_EFFECT_TOO` (1<<30, video+audio) か
  `PF_OutFlag_AUDIO_EFFECT_ONLY` (1<<31, audio のみ) が **必須**。
  `PF_OutFlag_I_USE_AUDIO` だけでは audio を「読む」宣言に留まり、AUDIO selector
  は届かない (既存 `pf-visual-audio-probe` は I_USE_AUDIO のみで timeline 記録も
  無かった)。
- audio 時間フィールドは `PF_InData` にある: `start_sampL` / `dur_sampL` /
  `total_sampL` (audio command 時のみ有効)、`src_snd` (PF_SoundWorld)。
  `PF_OutData` に `start_sampL` / `dur_sampL` / `dest_snd`。
- `PF_SoundWorld` = `{ PF_SoundFormatInfo fi { rateF, num_channels, format,
  sample_size }, A_long num_samples, void* dataP }`。`sample_size` はバイト数
  として直接ポインタ算術に使われる (SDK_Backwards、PF_SSS_1/2/4 = 1/2/4)。
- AUDIO_RENDER では host が `src_snd` (入力) と `dest_snd` (割当済み出力) の両方を
  渡し、effect は `dest_snd.dataP` に書く。pass-through は同一フォーマットなら
  `memcpy(src.dataP -> dst.dataP)`。

## 実装 (段階0 tooling、2026-07-20)

`instruments/pf-selector-timeline-probe` (#122 の selector timeline probe) を
audio 対応に拡張した (commit: "instrument: record AUDIO selectors ..."):

- classic flavor の GLOBAL_SETUP で `PF_OutFlag_AUDIO_EFFECT_TOO` を宣言
  (PiPL OutFlags も 1024 -> 1073742848 に合わせないと host が out_flags 不一致で
  拒否する)。smart flavor は不変 (audio 観測は classic のみ)。
- AUDIO_SETUP/RENDER/SETDOWN を sequence data の counter
  (audio_setup/render/setdown) で数え、sample range (start/dur/total sampL) と
  交渉済みフォーマット (rate/channels/sample_size) を JSONL に記録。
- AUDIO_RENDER は入力 audio を pass-through (host 割当 dest_snd に同一フォーマット
  copy) して render を有効に保つ。
- **ビルド成功**: `tools/build-pf-selector-timeline-probe.ps1 -Configuration
  Release` で classic/smart 両 .aex を生成。

## 観測 (AE 実機、2026-07-20)

After Effects 25.3.1x3 (Windows 11)。拡張した classic probe を audio-only WAV
footage (mono 44100、tone) レイヤーに適用し、30fps / 30 フレーム (1 秒) comp を
"Lossless with Alpha" (audio 付き movie) で in-app レンダー。probe は
`AEXCompatOracle` (ユーザー書込可の MediaCore サブフォルダ、UAC 不要) へ一時
install、実行後 cleanup。生ログは scratchpad の `audio-selector-timeline.jsonl`
(開発機観察、frozen evidence ではない)。

**観察 (事実):**

1. **AUDIO selector はフレーム単位ではなく、レンダー全体で 1 回だけ発行された。**
   30 フレーム comp に対し AUDIO_SETUP ×1 / AUDIO_RENDER ×1 / AUDIO_SETDOWN ×1
   (image の per-frame とは無関係)。
2. **AUDIO_RENDER は全期間を 1 チャンクで要求した。** `start_samp=0`,
   `dur_samp=48000`, `total_samp=48000` (48kHz × 1 秒 = 48000 サンプルを一括)。
   image の time_step (1024/30720) とは別単位の sample 時間軸。
3. **audio render は非 audio の SEQUENCE 系とは別スレッドで走り、レンダー中に
   独立した 2 つの sequence_data 履歴が存在した。** AUDIO 系は tid=2676、
   非 audio の SEQUENCE 系 (SETUP/FLATTEN/RESETUP) は主に tid=32732。
   SEQUENCE_SETDOWN が 2 回発行され、2 つの独立した counter 履歴
   (audio 側 resetup=1/flatten=1 と、もう一方の resetup=3/flatten=3) が
   観測された。**ただし本観測は audio-only レイヤーで image RENDER が発火して
   いない (#5) ため、2 つ目の履歴を「image のもの」と断定できない**。audio と
   image が sequence 状態を共有するか分離するかは、映像+音声レイヤーでの
   追加観測 (次アクション参照) が要る。ここで確定なのは「audio が別スレッドで
   独立した sequence 履歴を持つ」ことまで。
4. host が rate/channels/format を正規化した: src は 48000Hz / stereo /
   sample_size=4 / **format=2 (PF_SIGNED_FLOAT)** で届いた (入力 WAV は
   44100Hz mono)。format enum を trace に記録して float であることを確認済み
   (sample_size=4 だけでは 4byte PCM と区別できないため。probe 更新後の
   映像+音声再観測で `src_format=2`)。session でも audio フォーマットは
   host 交渉値であり、入力そのままではない。
5. image RENDER は発火しなかった (render=0)。probe が audio-only レイヤーに
   載っていたため。image と audio を同一レイヤーで同時観測するには映像+音声を
   持つ footage が要る → **観測2 で実施済み** (下記)。

## 観測2 (映像+音声レイヤー、2026-07-20)

先の audio-only 観測の限界 (image RENDER 不発で audio/image の sequence 分離を
断定できない) を埋めるため、映像+音声を持つ footage (ffmpeg 生成の 64x32 30fps
1 秒 MP4、H.264 映像 + AAC 440Hz 音声) に同じ probe を適用して再観測した。

**観察 (事実):**

1. **同一 effect が image RENDER と AUDIO 両方を受けた。** RENDER ×30 /
   FRAME_SETUP ×30 / FRAME_SETDOWN ×30 (フレーム毎) と、AUDIO_SETUP ×1 /
   AUDIO_RENDER ×1 / AUDIO_SETDOWN ×1。
2. **AUDIO は映像+音声 comp でも全期間 1 回・1 チャンク**: AUDIO_RENDER は
   `start_samp=0, dur_samp=48000, total_samp=48000` (audio-only 観測と一致)。
3. **image と audio は別スレッドで並行し、sequence インスタンスも分離していた
   (今回は断定できる)**: image の FRAME/RENDER は約 10 個の worker スレッドに
   分散 (MFR)、AUDIO 系は単一の専用スレッド tid=14184 で、image RENDER の
   どのスレッドとも重ならない。SEQUENCE_SETDOWN が 11 回 (各 render スレッド +
   audio スレッド) 発行され、audio は自前の sequence インスタンスを持つ。
4. **interleave は非同期**: AUDIO_SETUP はフレーム描画の途中 (約 8 フレーム後)
   に発行され、AUDIO_RENDER・AUDIO_SETDOWN も image フレームが他スレッドで
   描画され続ける中で並行して起きた。image の per-frame と audio の期間一括は
   時間的にインターリーブするが、別スレッド・別 sequence で独立している。

これで観測1 の未確定 (audio/image の sequence 分離) が確認された: **両者は
別スレッド・別 sequence インスタンスで、audio は per-frame image とは独立した
期間一括処理**。設計方針 (別チャネル) が sequence 分離の面からも裏付けられた。

## 結論 (観測に基づく設計方針)

**audio は image frame loop に相乗りさせない。** 観測1・2 で確定:
(a) audio は per-frame ではなく期間一括 (1 AUDIO_SETUP → 1 AUDIO_RENDER が
全期間 → 1 AUDIO_SETDOWN)、(b) 映像+音声 comp でも audio は image RENDER とは
別スレッド・別 sequence インスタンスで独立して並行する (観測2 #3)。期間一括
かつ独立処理なので、image の per-frame RGBA スロットモデルには構造的に合わない。
設計方針としての #239 の audio session 化は:

- 共有メモリの per-frame RGBA スロット (§6) には載せない。audio 専用の sample
  バッファ (float, host 交渉の rate/channels) を別チャネルで運ぶ。
- image の `render_frame` メッセージではなく、期間指定の audio 要求
  (start_samp/dur_samp) を運ぶ別メッセージ (または audio 専用セッション) にする。
- 大きな dur を 1 チャンクで要求されうるため、audio バッファ上限は image スロット
  とは別に見積もる (48kHz stereo float で 1 秒 ≒ 384KB、長尺は分割チャンクの
  可能性 — 長尺 comp での AUDIO_RENDER 分割有無は追加観測の候補)。

これは custom UI (v:2 の per-frame `ui_action` で image frame loop に相乗り) とは
対照的で、audio は別経路が適切。§4.2 の「per-frame 動的属性」一般化には乗らない。

## 残作業 (段階0 の観測実行) — 完了 (上記観測で解決)

観測にはフレーム列 comp に **audio が載っている** 必要がある (probe は
AUDIO_EFFECT_TOO なので、適用レイヤーに audio が無いと AUDIO_RENDER が発火しない)。
既存 capture 基盤 (`tools/capture-selector-timeline.ps1` +
`ae-selector-timeline-project.jsx`) は静止画 comp + PNG 連番出力で、audio を
持たない。以下が要る:

1. audio footage (WAV mono 44100 等) を生成し、それをレイヤーとして持つ comp を
   組む audio 版 JSX (probe を audio レイヤーに適用)。
2. audio をレンダーする出力モジュール (WAV or audio 付き movie)。PNG 連番は
   audio を出さないため AUDIO_RENDER が発火しない可能性が高い (要検証)。
3. aerender 実行 → JSONL の `cmd_name` が AUDIO_* の行を収集。
4. 分析: フレーム列における AUDIO_SETUP/RENDER/SETDOWN の発行回数・順序と、
   image RENDER (FRAME_SETUP/RENDER) との時間軸関係。sample range が
   frame と同期するか、別レート・別まとめか。sequence data を image と共有するか。

## 仮説 (2026-07-20 観測で確認済み)

- ~~audio render は image フレームとは別の時間軸・別まとめ~~ → **確認**
  (観測1・2 参照)。audio は per-frame ではなく期間一括で、別スレッドで独立した
  sequence 履歴を持つ。「区間 audio を別チャネルで運ぶ / audio 専用セッション」で
  image frame loop に相乗りさせない、という設計方針が裏付けられた。
- (訂正 2026-07-20) 当初、観測1 (audio-only) では audio と image の sequence
  状態の分離/共有が未確認と記していたが、**観測2 (映像+音声) で分離を確認済み
  (別スレッド・別 sequence インスタンス)。この caveat は解消**。実装確定前の
  追加観測は不要になった。

## 次アクション (段階0 完了後)

段階0 観測 (観測1: audio-only、観測2: 映像+音声) は完了。設計方針 (audio は
期間一括かつ image と別スレッド・別 sequence で独立するので、image frame loop
に相乗りさせず別チャネルで運ぶ) が確定した。以降:

1. audio session プロトコルの設計: 期間指定 (start_samp/dur_samp) の audio 要求
   メッセージと audio sample バッファチャネル (host 交渉の rate/channels/float)。
   image session とは独立 (別 sequence インスタンスの観測に整合)。プロトコル
   文書に §10 (audio) 等として追記。
2. worker: audio-only レンダー経路 (`--render-audio` / `render_experimental_audio`)
   をセッション化。AUDIO_SETUP → AUDIO_RENDER(区間) → AUDIO_SETDOWN を
   session lifecycle に載せる。
3. broker: audio session の open/render/close 配線、wrapper 適格条件から
   `audio.is_none()` を外す (または audio 専用ルート)。
4. 実 worker A/B: audio バイト一致で session ≡ one-shot 等価。
5. (任意) 長尺 comp で AUDIO_RENDER が複数チャンクに分割されるかの追加観測。
   分割される場合、audio バッファのチャンク境界設計に反映。

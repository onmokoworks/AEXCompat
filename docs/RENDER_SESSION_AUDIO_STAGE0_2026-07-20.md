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
4. host が rate/channels を正規化した: src は 48000Hz / stereo / sample_size=4
   (float) で届いた (入力 WAV は 44100Hz mono)。session でも audio フォーマットは
   host 交渉値であり、入力そのままではない。
5. image RENDER は発火しなかった (render=0)。probe が audio-only レイヤーに
   載っていたため。image と audio を同一レイヤーで同時観測するには映像+音声を
   持つ footage が要る (interleave 詳細の追加観測は任意)。

## 結論 (観測に基づく設計方針)

**audio は image frame loop に相乗りさせない。** これは観測で確定した
「audio は per-frame ではなく期間一括 (1 AUDIO_SETUP → 1 AUDIO_RENDER が全期間
→ 1 AUDIO_SETDOWN)」という事実だけで導ける: 期間一括の audio は image の
per-frame RGBA スロットモデルに構造的に合わない。加えて audio は別スレッドで
独立した sequence 履歴を持つ (観測 #3)。**ただし audio と image の sequence
状態が分離しているか (共有か) は本観測では未確定** (audio-only レイヤーで
image RENDER が出ていない)。よってプロトコルを「image/audio の sequence 分離」
前提で固定はしない。映像+音声レイヤーでの interleave 観測を実装確定前に行う
(次アクション)。設計方針としての #239 の audio session 化は:

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
  (上記「観測」参照)。audio は per-frame ではなく期間一括で、別スレッドで独立した
  sequence 履歴を持つ。「区間 audio を別チャネルで運ぶ / audio 専用セッション」で
  image frame loop に相乗りさせない、という設計方針は期間一括の事実で裏付けられた。
  ただし audio と image の sequence 状態の分離/共有は未確認 (audio-only 観測、
  image RENDER 不発)。映像+音声観測を実装確定前に行う (次アクション)。

## 次アクション (段階0 完了後)

段階0 の主要観測は完了し、設計方針の核 (audio は期間一括なので image frame loop
に相乗りさせず別チャネルで運ぶ) は期間一括の事実で確定した。ただし sequence 状態の
分離/共有は未確定なので、プロトコルを sequence 分離前提で固定する前に以下を行う:

0. **(実装確定の前提) 映像+音声レイヤーでの interleave 観測**: 映像も音声も持つ
   footage に probe を適用し、image RENDER と AUDIO_RENDER の相対順序、sequence
   状態の分離/共有を観測する。これで audio と image が sequence_data を共有するか
   独立かを確定してからプロトコルを固める。
1. audio session プロトコルの設計: 期間指定 (start_samp/dur_samp) の audio 要求
   メッセージと audio sample バッファチャネル (host 交渉の rate/channels/float)。
   image session とは独立。プロトコル文書に §10 (audio) 等として追記。
   sequence 状態の扱いは item 0 の結果を反映する。
2. worker: audio-only レンダー経路 (`--render-audio` / `render_experimental_audio`)
   をセッション化。AUDIO_SETUP → AUDIO_RENDER(区間) → AUDIO_SETDOWN を
   session lifecycle に載せる。
3. broker: audio session の open/render/close 配線、wrapper 適格条件から
   `audio.is_none()` を外す (または audio 専用ルート)。
4. 実 worker A/B: audio バイト一致で session ≡ one-shot 等価。
5. (任意) 長尺 comp で AUDIO_RENDER が複数チャンクに分割されるかの追加観測。
   分割される場合、audio バッファのチャンク境界設計に反映。

# 常駐セッション audio 段階0 調査ノート (#98 W4 / #239)

status: 進行中。custom UI (#238/PR #241) と並行して、audio を常駐レンダー
セッションに載せる (#239) ための段階0 = AE 実機観測を進める。時系列追記。
観察 (事実) と仮説 (推論) を分けて書く。frozen evidence 化する値は
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

## 残作業 (段階0 の観測実行)

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

## 仮説 (未検証、観測で確定する)

- audio render は image フレームとは別の時間軸・別まとめ (AE の audio は
  image render とは独立した audio-render パスで、フレーム単位ではなく
  区間単位でまとめて要求される) の可能性が高い。もしそうなら、session の
  audio 意味論は「per-frame audio スロット」ではなく「区間 audio を別チャネルで
  まとめて運ぶ」または「audio 専用セッション」になり、image frame loop への
  相乗りは適さない。→ 観測で確定する。

## 判断待ち / 次アクション

- 観測実行 (audio 版 capture の構築 + aerender) は AE 実機の排他利用。
  段階0 観測結果が出るまで §6 共有メモリ audio スロットは設計しない。
- 観測後、結果を本ノートに追記し、design を固めてから実装 (worker/broker) と
  実 worker A/B に進む。

# 残り 5 本 (mocha 3 / Upscale / rendered_empty 2) の観測 (2026-08-18)

issue #1285 の調査記録。母集団は AE 2026
`Support Files\Plug-ins\Effects` を `render_sweep` に引数で渡した 304 AEX、
`--depth` 既定 (8)、base は `origin/main` の `997d3b7d`。
観測 (trace / 逆アセンブル / AE 実機) と推論を分けて書く。

## 1. 手順

- trace: worktree で worker 3 exe を build、`AEXCOMPAT_EXTENDED_DIAG=1` +
  `render_sweep --filter <name> --close-report`
  (`docs/SWEEP_INVESTIGATION_WORKFLOW_2026-08-17.md` §2)。
- 静的: Ghidra `AEXCompat.gpr` に `Set_Channels.aex` / `Grow_Bounds.aex` /
  `mochashape4ae_adobe.aex` を import (headless、
  `tools/ghidra/DumpDecomp.java` の `sym:` セレクタ)。
- AE oracle: `tools/capture-ae-reference.ps1`。入力は sweep が使うのと同じ
  256x144 の単色 RGBA (32,64,128,255) PNG
  (file sha256 `4264b64e96c511afc24a6baa053ea608c57bad79ac1089be03da1eda08c91a4b`、
  decoded RGBA sha256
  `8d030dced8f899e66504e8ee0ae9e5fff91379468085c994c2d9cff06c224445`)、
  AE 26.3x87、bpc 8、default param、frame 0、`-RequireLoadedAexIdentity`。
- host 出力: `render_sweep --dump-frames <dir>` の raw pixel を AE の PNG の
  decoded RGBA と比較。

この調査で trace に足したもの:

- `extended_diag:pre_checkout_request` / `extended_diag:pre_checkout_answer`
  — SmartFX PreRender の checkout が「どの rect を要求し、host が何を答えたか」。
  それまで record 側には `input_checkout_request` /
  `input_checkout_result_rect` しか無く、しかも index 0 の**最後の 1 回**しか
  残らないので、複数回 checkout する PreRender は読めなかった。
- `extended_diag:add_param ... layer_dephault=<n>` — layer parameter の
  `PF_LayerDefault` (SDK `AE_Effect.h`: MYSELF = -1、NONE = 0)。

## 2. 観測と結果

| plug-in | 着手時 | 原因 | 行き先 |
| --- | --- | --- | --- |
| `mochaAE/MochaAE.bundle/…/MochaAE.aex` | `render_frame_failed:worker_invariant_failure` (-47) | `AEGP Effect Suite@1` と `AEGP Keyframe Suite@3` が未提供 | `rendered` |
| `Upscale.aex` | `not_discovered:cluster_session_invalidated` | GLOBAL_SETUP の size 0 extended alloc を host が `*out` 未設定で拒否 → plug-in が未初期化 pointer を release → host が所有権未確認のまま `free()` | `rendered` |
| `Grow_Bounds.aex` | `rendered_empty` | SmartFX の空 `result_rect` を host が空フレーム扱い | `rendered` (AE と byte 一致) |
| `Set_Channels.aex` | `rendered_empty` | 同上 | `rendered` (AE と byte 一致) |
| `mochaAE/mochashape4ae_adobe.aex` | `render_frame_failed:worker_invariant_failure` (-47) | `PF AE Private Effect Suite@3` が未実装 | 未解決 (#1286) |
| `mochaAE/MochaAEAEGP.bundle/…/MochaAEAEGP.aex` | `not_discovered:exit_12` | AEGP 専用 plug-in。PF worker が仕様通り拒否 | 仕様通り (下記 §2.5) |

### 2.1 MochaAE.aex — 古い版の AEGP suite (#1073 の残り 2 本のうち 1 本)

SMART_PRE_RENDER の trace:

```
stage:smart_render_begin
extended_diag:acquire_suite name="PF Handle Suite" version=2 -> 0
stage:suite_acquire_failed name=AEGP Keyframe Suite version=3
extended_diag:acquire_suite name="AEGP Keyframe Suite" version=3 -> 1
extended_diag:acquire_suite name="AEGP Dynamic Stream Suite" version=5 -> 0
...
stage:suite_acquire_failed name=AEGP Effect Suite version=1
extended_diag:acquire_suite name="AEGP Effect Suite" version=1 -> 1
stage:smart_render_end pre_error=-1 render_error=-1
```

host は `AEGP Effect Suite` を 2/3/4、`AEGP Keyframe Suite` を 4/5 で提供して
いて、plug-in が要求する v1 / v3 だけが無かった。SDK ヘッダで両 suite の
全 version の member を機械的に突き合わせた結果:

- `AEGP_EffectSuite1` (16 member) は `AEGP_EffectSuite2` (17 member) の先頭
  16 member と**シグネチャまで完全一致**で、v2 が末尾に
  `AEGP_DuplicateEffect` を足しただけ。version 間でシグネチャが変わる唯一の
  member は `AEGP_EffectCallGeneric` (v3 で `PF_Cmd` 引数が増える) だが、
  これは v1〜v4 のどの版でも host 側は診断付き未対応 stub のままなので、
  v1 を足しても触らない。
- `AEGP_KeyframeSuite3` と `AEGP_KeyframeSuite4` は 20 member で名前も順序も
  同じ。差は 5 member の引数が `AEGP_StreamValue*` (v3) か
  `AEGP_StreamValue2*` (v4) かだけで、この 2 つの struct は
  `{AEGP_StreamRefH; union}` で union の差は `AEGP_MarkerValH markerH` と
  `AEGP_MarkerValP markerP` のみ。どちらも pointer size なので **x64 では
  layout が同一**。host の keyframe 値は mask outline 専用で marker member に
  触らないので、v3 は v4 の table の複製で足りる。

実装後、MochaAE は 256x144 / `result_rect [0,0,256,144]` /
`output_pixels_valid` / clean close で `rendered`。

### 2.2 Upscale.aex — size 0 の extended alloc と所有権未確認の free

one-shot l2 worker (`--l2-params-only`) の末尾:

```
stage:global_setup_begin
extended_diag:addr ext_alloc=... ext_free=...
extended_diag:alloc out=000000717954D8F0 size=0x0
(exit -1073740940 = 0xC0000374 STATUS_HEAP_CORRUPTION)
```

`l2_main_support.inc` の `host_extended_alloc` は `size == 0` を error 4 で
拒否しつつ **`*out` を書かなかった**ので、plug-in の local pointer は
未初期化のまま残る。続く `host_extended_free` は
`observe_extended_free()` (foreign / double free を**数えるだけ**の診断) を
呼んだあと、所有権に関係なく `std::free(*ptr)` していた。

同じファイルの `extended_inter_memory.cpp` には、この 2 点を最初から正しく
実装した `aexcompat::extended_inter::allocate` / `release` があり
(`*out` を必ず先に nullptr で埋める、size 0 は 1 byte の解放可能 token を
返す、所有していない pointer は `free()` に渡さない)、ヘッダにも
「plug-in が host に任意の static / foreign pointer を free させられない
ようにする」と書かれていたが、**呼ばれていなかった** (参照は selftest のみ)。
live 側をそこへ配線した。

### 2.3 / 2.4 Grow_Bounds.aex と Set_Channels.aex — 空 `result_rect` の意味

どちらも SMART_PRE_RENDER が `PF_Err_NONE` で空の `result_rect` を返し、
host はそれを「空のフレーム」として `rendered_empty` にしていた。

**AE oracle (2 本とも、identity verified)**: 出力は入力と byte 一致
(decoded RGBA sha256 `8d030dced8f899e6…`)。

- `Grow_Bounds.aex` (`ADBE GROW BOUNDS`) は
  `PF_OutFlag2_SUPPORTS_SMART_RENDER | FLOAT_COLOR_AWARE |
  SUPPORTS_THREADED_RENDERING` を出しているのに、唯一の entry export
  `FilterMain` (`0x180002f10`) の `switch (cmd)` には **`PF_Cmd_SMART_PRE_RENDER`
  (23) も `PF_Cmd_SMART_RENDER` (24) も case が無い**。両方 `default` に落ちて
  `extra` に触れずに 0 を返す (`PF_Cmd_RENDER` (11) も no-op)。
  PARAMS_SETUP が足す唯一の param は `Pixels` (FLOAT_SLIDER、default 10.0)。
- `Set_Channels.aex` (`ADBE Set Channels`) は 4 本の layer param
  (`layer_dephault=0` = `PF_LayerDefault_NONE`) を宣言し、PreRender で
  未接続 slot の checkout が空 rect を返すのを受けて自分で
  `result_rect` を空に畳む:

```
pre_checkout_layer index=0 id=1000   request rect=[0,0,0,0]     -> result=[0,0,0,0] max=[0,0,256,144]
pre_checkout_layer index=7 id=7      request rect=[0,0,256,144] -> result=[0,0,0,0] max=[0,0,0,0]
pre_checkout_layer index=1 id=1      request rect=[0,0,0,0]     -> result=[0,0,0,0] max=[0,0,256,144]
...
pre_checkout_layer index=0 id=0      request rect=[0,0,0,0]     -> result=[0,0,0,0] max=[0,0,256,144]
最終 result_rect=[0,0,0,0] max_result_rect=[0,0,256,144]
```

Grow_Bounds の方が情報量が多い。**plug-in が書いた値では AE のフレームを
説明できない** (観測)。AE が SMART_RENDER を回したとも考えにくい (推論):
この plug-in は output world に一切書かないので、dispatch されていれば
未書き込みの buffer が出るはずで、入力と byte 一致にはならない。ただしこの
議論は「AE が selector の前に output world を入力で埋めている」可能性までは
潰していない (§3)。以上から AE 側の contract は次のように読める (推論):

> SmartFX の PreRender が `PF_Err_NONE` かつ空の `result_rect` で終わったら、
> その effect はこのフレームに何も寄与しない。AE は render selector を回さず、
> **effect の入力をそのまま出す**。

host をこれに合わせた (`worker_smart_dispatch.cpp` の
`empty_result_passthrough`)。selector を回さないのは従来通りで、変わったのは
「その後に何を出すか」だけ。**Classic fallback ではない** (別経路を走らせない、
selector を再 dispatch しない)。複製できないと分かった dispatch は
従来の空のままで、フレームを捏造しない: 入力の無い dispatch
(`plan.missing_input`)、GPU negotiation 中 (pixel が device 側にある)、
入力 world が register 時の layout と一致しなくなっているとき
(`resolve_registered_dispatch_world` の fail-closed。PreRender は既に走って
いて、plug-in に渡した layer ParamDef はこの world の前半を alias している
ので、pixel pointer だけは `plan` では保証できない)、要求 rect が入力の外に
出るとき、rect が空/負のとき。output world を組めなかったときは複製せず
`render_error = -6` を返す (空フレームの成功にはしない)。

記録は 3 か所:

- worker / session report の `empty_result_passthrough` (true のときだけ
  `empty_result_rect` も true)。
- worker stderr の `stage:smart_empty_result_passthrough_begin` /
  `_end reason=<identifier>`。broker の stage parser が拾うので
  `discovery_diagnostics.stage_events` に載る。`input_copied` が複製した、
  それ以外は host が降りてフレームは空のまま。
- `render_sweep` の record の `worker.empty_result_passthrough_reason`
  (上の reason)。report 側の同名 bool と型が違うので key を分けてある。
  `--close-report` を付けなくても出るので、full-corpus の record だけで
  「複製されたフレーム」を数えられる。

これがあるので「host が複製したフレーム」を「plug-in が描いたフレーム」と
読み違えることはできない。

#1195 (Grow_Bounds) の「Smart は合法な 0x0 を返している」という観測自体は
正しく、否定していない。否定したのはその先の「だから host 側にできることは
無い」という判断で、AE は同じ 0x0 を受けて入力を出している。
#1055 (Set_Channels) の `checkout_pixels empty_result` も同じ 1 本の規則で
説明がつく。

`pf_smart_geometry_probe` の mode 3 (`EmptyResult`) がこの contract を
pin している (`tests/test_pf_smart_geometry_probe.py`)。出力が入力と
**byte 一致**であることまで見る
(`test_empty_result_emits_the_input_unchanged`)。1 pixel でも変えた
passthrough は、置き換えた空フレームより悪い silent-wrong なので。

### 2.5 MochaAEAEGP.aex — `not_discovered:exit_12` は仕様通り

exit 12 は `worker_runtime_admission.cpp` の
`is_aegp_candidate_without_execution()` による拒否で、
「この worker は PF effect 専用。AEGP 候補は image-only preflight から弾いて、
その DllMain と delay-load 依存を PF inspection process で走らせない」
(issue #377) という既存の意図的な fail-closed。MochaAEAEGP.aex は AEGP 専用
plug-in なので render 対象の effect ではなく、discovery されないのが正しい。
host 側に直すものは無い。sweep の母集団が Effects folder の全 `.aex` である
以上、AEGP がこの bucket に出るのは避けられない。

### 2.6 mochashape4ae_adobe.aex — `PF AE Private Effect Suite@3` (未解決)

```
stage:smart_render_begin
stage:suite_acquire_failed name=PF AE Private Effect Suite version=3
extended_diag:acquire_suite name="PF AE Private Effect Suite" version=3 -> 1
stage:smart_render_end pre_error=-1 render_error=-1
```

host にはこの名前の v3 / v5 が
`worker_suite_call_slot_probe.cpp` の **opt-in 診断 probe としてしか無い**
(`AEXCOMPAT_SUITE_CALL_SLOT_PROBE` を立てたときだけ trampoline table を返す)。
実装 (table の member 数、各 slot の signature と意味) は未確定で、#1210 の
BEE facade と同型の private ABI の RE が要る。#1286 に分離した。

## 2.7 出力の妥当性 (silent-wrong を成功扱いにしていないこと)

`render_sweep --dump-frames` の raw pixel と AE の PNG の decoded RGBA を比較した
(4 本とも AE 26.3x87、同じ入力、default param、frame 0):

| plug-in | host 出力 | AE 出力 | 判定 |
| --- | --- | --- | --- |
| `Grow_Bounds.aex` | 256x144、全 pixel (32,64,128,255) | 同じ | byte 一致 |
| `Set_Channels.aex` | 256x144、全 pixel (32,64,128,255) | 同じ | byte 一致 |
| `MochaAE.aex` | 256x144、全 pixel (32,64,128,255) | 同じ | byte 一致 |
| `Upscale.aex` | 260x148、内側 256x144 が (32,64,128,255)、外周 2px が (0,0,0,0) | 256x144 全 pixel (32,64,128,255) | comp 領域は一致。外周は host の expand buffer (`result_rect [-2,-2,258,146]`、`output_origin [2,2]`) |

Upscale の 2px 外周は host が expand buffer をそのまま返すためで、AE の comp
座標に写すと一致する (`docs/SWEEP_INVESTIGATION_WORKFLOW_2026-08-17.md` §2 の
extent 注記と同じ話)。単色入力なので「effect が何か描いたのか、入力が
素通りしたのか」は区別できていない — 4 本とも default param では AE 自身も
入力と同じものを出す、というのがここで確かめたこと。

## 2.8 計測 (full corpus)

母集団: AE 2026 `Support Files\Plug-ins\Effects` を `render_sweep` の引数に
渡した 304 AEX、`--depth` 既定 (8)、size/time/frames も既定。baseline は
`origin/main` の `9804ae6f` (#1279 merge 後) を専用の worktree で同じ手順で
build して測ったもの。sweep CLI は両者同一バイナリで、worker 3 exe だけが違う。

| bucket | baseline (9804ae6f) | 変更後 |
| --- | --- | --- |
| rendered | 289 | **293** |
| rendered_empty | 2 | **0** |
| not_discovered:cluster_session_invalidated | 1 | **0** |
| render_frame_failed:worker_invariant_failure | 2 | **1** |
| frame_error:512 | 5 | 5 |
| frame_error:4 | 1 | 1 |
| not_discovered:exit_20_params_setup:13 | 2 | 2 |
| not_discovered:exit_20_global_setup:2 | 1 | 1 |
| not_discovered:exit_12 | 1 | 1 |

bucket が動いたのは 4 本だけで、他の 300 本は baseline と同じ bucket
(突き合わせの key は `plugin_relative_path`)。

silent-wrong の確認として、両方の run で `detail.pixel_sha256` を持つ record を
突き合わせた (baseline 291、変更後 293、baseline で hash を持つ record が変更後に
落ちたものは 0)。hash が変わったのは Grow_Bounds と Set_Channels の 2 本だけで、
どちらも `e3b0c442…b855` (空バイト列の SHA-256、= 0 byte 出力) から
`8d030dce…4445` (入力の RGBA、= AE oracle と同一) への変化。残りの 289 本は
hash 一致。

## 3. 残っている不確かさ

- AE が `PF_PreRenderOutput` を selector 前に何で埋めているかは直接観測して
  いない。§2.3 の推論は「Grow_Bounds の AE 出力が入力と byte 一致」から
  逆算したもので、AE が (a) 0 で埋めてから空を「寄与なし」と解釈するのか、
  (b) 別の初期値を入れて別の理由で入力を通すのかは分けられていない。この 2 本の
  plug-in については host の振る舞いはどちらでも同じになるが、一般には同じでは
  ない: (b) なら AE は SMART_RENDER を回していることになり、output world の
  一部しか書かない plug-in ではこの host と挙動が分かれる。分けるには
  `PF_PreRenderOutput` の入力値を報告する probe AEX を AE に入れる必要が
  あり、AE の `Support Files\Plug-ins` への書き込みは昇格が要るので
  このセッションでは実施していない。
- `AEGP Keyframe Suite@3` は mask outline 以外の stream 値では v4 と挙動が
  分かれうる (marker stream)。host が marker keyframe 値を扱うようになったら
  v3 を別 table に分ける必要がある。
- `analysis/SMARTFX_GEOMETRY_CONTRACT_RESULT_2026-07-19.json` は §2.3 の訂正前の
  mode 3 (空 result = 0 byte 出力) を記録したままになっている。refresh には
  採取時の独立 SmartFX AEX (`target/ntsc-rs-ae.aex`) が要り、この環境には無い。
  evidence 文書は refresh script 経由でしか更新しない
  (`docs/EVIDENCE_POLICY_2026-07-18.md` §5.2) ので手で直していない。
- MochaAE.aex の AE capture は `loaded_aex_identity: module_loaded`
  (sha256 は tested と一致) だが `effect_provenance: unverified`
  (`effect_match_name_not_uniquely_bound_to_loaded_module`)。この AEX は複数の
  effect を出すので matchName から provider を一意に決められない。
  Grow_Bounds / Set_Channels / Upscale の 3 本は provenance も verified。

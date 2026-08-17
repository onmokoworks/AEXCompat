# mask / audio 無しで frame_error:4 だった 4 本の観測 (2026-08-17)

issue #1253 の観測記録。AE 2026 `Support Files\Plug-ins\Effects` (304 AEX)
の render sweep で、host の拒否痕跡 (`-> 4` / `callback_denied` /
`suite_slot_unsupported` / `selector_seh`) が一切出ないまま
PF_Err_OUT_OF_MEMORY (4) を返していた AudWave / Scribble / Inner-Outer-Key と、
PR #1267 で discovered になった直後に同じ bucket に入った Reshape_New。
4 本とも「host が AE と違う答えを返していた callback」を直すと default
(mask 無し / audio layer 無し) の render で AE と同じ出力になった (AudWave は
alpha の表現 (AE の PNG は premultiplied、host は straight) を揃えたうえで
一致。原因の判定は §3、根拠は §2)。観測 (実測) と推論を分けて書く。

## 1. 手順 (再現可能)

- trace: worktree で 3 worker exe を build、`AEXCOMPAT_EXTENDED_DIAG=1` +
  `render_sweep --filter <name> --close-report` (`docs/SWEEP_INVESTIGATION_WORKFLOW_2026-08-17.md` §2)。
  今回、`PF Path Query Suite` の enumeration / checkout / checkin と
  `PF_CHECKOUT_LAYER_AUDIO` の引数と結果が trace に出るようにした
  (`extended_diag:path_*` / `extended_diag:checkout_layer_audio`)。
  以前は path 系 callback が呼ばれたかどうかが trace から読めず、issue #1253
  本文の「num_paths 等は一切呼ばれていない」は checkout の観測漏れだった。
- 静的: Ghidra `AEXCompat.gpr` に 4 本の .aex と FLO.dll を import
  (headless、`tools/ghidra/DumpDecomp.java` に `sym:` / `ref:` / `mk:`
  セレクタ、`tools/ghidra/DumpListing.java` を追加。使い方は
  `tools/ghidra/README.md`)。Scribble の C++ 例外 catch は
  FH4 (`__CxxFrameHandler4`) の funclet で Ghidra が関数を作らないので、
  `.pdata` → UNWIND_INFO → FuncInfo4 → TryBlockMap → HandlerType を
  読んで catch funclet のアドレスを出し、`mk:` で関数化して読んだ。
- 動的: `cdb -o` で `render_sweep --filter` を子プロセスごと debug し、
  `sxe -c "..." cpr` で子 (worker) に deferred bp を張る。C++ 例外の
  throw site (`sxe eh` + `k`)、Adobe DLL 内部関数の戻り値
  (`bp /1 @$ra "r rax"`) を取った。
- AE oracle: `tools/capture-ae-reference.ps1` (`tools/ae-reference-capture.jsx`
  が result JSON の `params` に effect の全 property の
  index|name|matchName|type|value を書くようにした) で 4 effect を default の
  まま 1 frame render。入力は
  `tools/generate-oracle-rgba-input.py --width 256 --height 144`
  (decoded_rgba_sha256 `d609f80d…9e973`)。AE 26.3x87、
  loaded_aex_identity verified。
- host 出力: `render_sweep --dump-frames <dir>` (今回追加) で raw pixel を落とし、
  AE の PNG と比較。

## 2. 観測

### 2.1 Scribble.aex (classic、`ADBE Scribble Fill`)

- RENDER: `acquire_suite "PF Path Query Suite" v1 -> 0` → 直後に
  `PF_CheckoutPath(effect_ref, unique_id=0, current_time, time_step,
  time_scale, &path)` (host trace `path_checkout arg=0 -> 4`) → 私的
  lookup id 25 "Not enough memory to execute Scribble" → 4。
- cdb: `_CxxThrowException` の stack は `Scribble+0x6cd88`
  (FUN_18006ccb0、"Mask" path param の遅延 checkout ヘルパ) →
  `Scribble+0x10558` (RENDER FUN_180010370 の `iVar6 == 3` 分岐)。
  FUN_18006ccb0 は `PF_CheckoutPath` の戻りが非 0 なら
  `AbortException(err)` を throw する。FilterMain の catch (FH4 funclet
  `Scribble+0x70160`) は `err == 4` なら id 25、`512` なら id 26 を引いて
  `out_data->return_msg` に入れ、`PF_OutFlag_DISPLAY_ERROR_MESSAGE` を立てる。
  `AllocFailed` の catch (`+0x70220`) は無条件に 4 + id 25。
- 同じ Scribble の別分岐 (FUN_1800030f0、全 mask 走査) と Inner-Outer-Key /
  Reshape は checkout の戻り値を見ずに、返った pointer が NULL かどうかで
  「mask 無し」を判定している。
- SDK (`AE_EffectSuites.h` PF_PathQuerySuite1): `PF_CheckoutPath ... can
  return NULL ptr if path doesn't exist`。`PF_PathDef.path_id` は mask 未選択で
  `PF_PathID_NONE` (0)。
- AE oracle (mask 無し default): status captured、出力 PNG は全 pixel
  (0,0,0,0) (Composite の default "On Transparent")。ExtendScript の値:
  Scribble=3、Mask=0、Fill Type=1、…
- host 修正後 (`PF_CheckoutPath` の未解決 id → 0 + NULL、対応する NULL
  checkin → 0): rendered、出力 36864 pixel 全部 (0,0,0,0)。AE と一致。

### 2.2 Inner-Outer-Key.aex (classic、`ADBE ATG Extract`)

- RENDER: 7 suite acquire → `path_checkout arg=0 -> 4` / `path_checkin
  arg=0 value=0 -> 4` (id 0 の NULL path を checkin) → iterate → 4。
  path 修正後は 512 に変わり、`selector_seh ... access=execute fault=null
  stack0=plugin:Inner-Outer-Key.aex+0x9a60` (FUN_180009990: `in_data->utils`
  +0x50 = `PF_UtilCallbacks.gaussian_kernel` を呼ぶ。生成 contract に
  `utils.gaussian_kernel` が無く host が slot を null のまま渡していた。
  #777 / #981 / #1252 と同型)。
- PF.dll `PF_GaussianKernel` (export ordinal 337、0x52bd0) の RE:
  `r = (short)ceil(kRadius)`、`diameter = 2r+1`、1D なら 1 行、2D なら
  y∈[-r,r]、各 x∈[-r,r] で `d = hypot(x,y)/(kRadius+1)`、
  `g = d<=1 ? 1-(1-exp(-2.378 d²))*1.102 : 0` (= `PFp_GaussianValue`)、
  `v = g*255 (*multiplier if != 1)`、格納値 `clamp((int)(v+0.5),0,255)`、
  sum は格納したエントリごとに v を加算。NORMALIZED は各エントリを
  `(int)(entry * ((y_extent*0x1fe+0xff)*(2r+1) / sum))` (再 clamp 無し:
  radius 1 の 1D は 192,380,192)。型フラグは無視して常に A_long。
  radius < 0 / diameter NULL / kernel NULL は 3 (A_Err_PARAMETER)。
- AE oracle (mask 無し default): 出力は入力と全行一致 (passthrough)。
- host 修正後 (gaussian_kernel を wire): rendered、出力は入力と一致。

### 2.3 Reshape_New.aex (classic、`ADBE RESHAPE`)

- RENDER: Path Query / Path Data acquire → checkout_param 4 → 4。
  cdb: `FLO!FLO_DoDistortion` が 4 を返す (Reshape+0x513c で rax=4)。
  FLO 内部 `FUN_180010850` の mode 1 分岐で `FUN_18001c340` が 1/2 以外を
  返す → 4。Reshape の grid generator callback (`Reshape_New+0x6fa0`) が
  呼ばれた最初の 1 回で 3 を返している (`*sequence == mode` が 0/1/2 の
  どれでもない)。mode は PARAMS_SETUP 後 `params[7]->u.pd.value - 1`
  ("Interpolation Method" popup) で、host は value 0 を渡していた
  (`dd` で `value=0 num_choices=3 dephault=0`、Elasticity も
  `value=0 num_choices=9 dephault=0`)。Reshape の PARAMS_SETUP
  (FUN_1800047d0) は両 popup の `u.pd` を `value=0, dephault=0` で
  `PF_ADD_PARAM` している (SDK の PF_ADD_POPUP マクロを使わない)。
- AE oracle: ExtendScript で `Elasticity` = 1、`Interpolation Method` = 1
  (user 操作なし)。出力は入力と全行一致 (passthrough)。AE が RENDER の
  `u.pd.value` に 1 を渡していること自体は捕まえていない (推論、§3)。
- host 修正後 (popup の declared default が 1 未満なら 1): rendered、
  出力は入力と一致。dephault > num_choices の扱いは未観測なので触っていない。

### 2.4 AudWave.aex (smart、`ADBE AudWave`)

- SMART_RENDER: `ansi(sqrt) -> 0` の直後に 4。close report では
  `audio_checkout_calls=0` だが `invalid_audio_operations=1`、
  `last_audio_checkout_index=1 start=-2 duration=4 scale=30`。新しい trace 行:
  `checkout_layer_audio index=1 start=-2 duration=4 scale=30
  rate=2890137600 bytes=2 channels=2 format=1 audio_in=00003FE020BBAEC3 -> 4`。
  host は `*audio != NULL` (out ポインタの入口値) を invalid として拒否して
  いた。AudWave は out スロットを初期化せずに渡す (値は毎回違う stack ごみ)。
  index 1 は "Audio Layer" (issue 本文の「index 11」は Random Seed で
  取り違え)。start_time -2 は current_time - Audio Offset の窓。
- SDK 上 `*audio` は出力専用で、AE は入口値を見ない (AudWave が AE で
  動く以上、見ていない)。host も入口値を一切参照しないので、この検査は
  host 状態を守っていなかった。
- AE oracle (Audio Layer = None、default): PNG は行 70..74 のみ有色
  (207/209/209/209/207 pixel、x 24..232)、色 (premultiplied RGBA)
  (64,11,41,64) ×410 / (191,23,142,191) ×410 / (255,6,242,255) ×205。
- host 修正後 (入口値の検査を外す): rendered、同じ行 70..74、同じ pixel 数
  (207/209/209/209/207、x 24..232)、色は straight (255,44,163,A64) /
  (255,31,190,A191) / (255,6,242,A255) で、AE の premultiplied 値と一致
  (64 = 255*64/255、11 ≈ 44*64/255、41 ≈ 163*64/255)。

## 3. 推論

- 4 本とも「plug-in 自身の仕様で 4」ではなく、host が AE と違う答えを
  返した callback (PF_CheckoutPath の未解決 id、PF_UtilCallbacks の未配線
  slot、popup default の materialize、audio checkout の入口値検査) が原因
  と判断した。根拠は §2 の AE oracle (同じ default で AE は render する) と
  host 修正後の出力一致。
- Scribble が checkout の戻り値で abort するのに対し、同じ Adobe 製でも
  Inner-Outer-Key / Reshape は pointer NULL で判定しており、AE の
  PF_CheckoutPath は「未解決 id は err 0 + NULL」で両者を満たす。err 非 0 で
  返す実装は Scribble のような plug-in をすべて落とす。
- Reshape の dephault 0 は AE 側で 1 として materialize されている
  (ExtendScript の値が 1、render が passthrough = mode 0 で通る、の 2 点から
  の推論。観測は 0→1 のみ)。num_choices を超える default の扱いは観測して
  いないので host も触っていない。

## 4. 未解決 / 記録のみ

- mask を 1 つ以上与えたときの Scribble / Inner-Outer-Key / Reshape の
  出力を AE と比較する経路 (`render_sweep --mask ...`) は作っていない。
  session の mask trailer (`v2|...`) 自体は broker にあるので、比較したく
  なったら sweep のオプションとして足せる。
- Inner-Outer-Key の gaussian_kernel 数値は PF.dll の RE から実装し、
  selftest で AE 値 (192,380,192 など) を固定した。実機 AE の
  `PF_GaussianKernel` を Frida で叩いた動的キャプチャはしていない。

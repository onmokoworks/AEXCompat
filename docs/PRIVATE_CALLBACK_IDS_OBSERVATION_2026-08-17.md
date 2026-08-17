# AE の私的 get_callback_addr id (-5 / -2) の動的キャプチャ (2026-08-17)

issue #985 の観測記録。Bulge が `PF_UtilCallbacks.get_callback_addr` に id -5
を、Compound Blur / CC Cross Blur / Matte Choker が id -2 を要求して 516 で
止まっていた。id の実体は AE 側にあり (プラグインは返る関数ポインタを呼ぶ
だけ)、静的な定数スキャンでは dispatcher を特定できなかったので、実機 AE を
Frida でフックして「AE が実際に返す関数」と「その関数の入出力」を取った。
観測 (実測) と推論を分けて書く。

## 1. 手順 (再現可能)

- ツール: `tools/capture_ae_private_callbacks.py` (driver) +
  `tools/frida/ae_private_callback_probe.js` (hook) +
  `tools/ae-private-callback-capture.jsx` (AE 側)。
  driver は AfterFX / aerender / aerendercore が 1 つでも動いていれば起動を
  拒否する (AE は排他リソース)。
- 起動: `AfterFX.com -m -noui -r <jsx>`。この形では AfterFX.com 自身が
  AfterFXLib.dll をロードしてホストになる (AfterFX.exe の子は生えない)。
  Effects フォルダのプラグインは起動時には読まれず、初回使用時に遅延
  ロードされるので、hook は `LdrLoadDll` の戻りと 200ms ポーリングで
  モジュール出現を待ってから当てる。
- hook の当て方: 各対象 AEX の entry export (`EffectMain` /
  `EffectMainExtra` / `MainEntry` / `FilterMain`) を Interceptor で受け、
  引数の `PF_InData` から `in_data->utils` (+0xb0) →
  `utils->get_callback_addr` (+0xc0) を読み、その関数を hook して
  (effect_ref, quality, mode, id, *out) と `*out` の関数ポインタ
  (module + RVA) を記録する。
- 返った関数の中身: id -5 は `double(double)` として入力表で直接呼ぶ。
  id -2 は 65x65 の PF_LayerDef を Frida 側で組んで (8-bit と 16-bit)、
  インパルス / 半透明インパルス / ステップ / ランプの 4 種 × 半径 26 種 ×
  flags 数種 × `in_data->quality` 0/1 × mode 0/1 で AE の関数を呼び、
  出力ピクセルを回収する。プラグイン自身が -2 を呼ぶときの引数と
  ワールド前後のピクセルも全部記録する。
- dispatcher の列挙: 最初の応答時に (quality 0..1) × (mode 0..2) ×
  (id -12..40) を AE の get_callback_addr にそのまま投げて表にする。
- 入力画像: `tools/generate-oracle-rgba-input.py --width 256 --height 144`
  (decoded_rgba_sha256 `d609f80dce558abf4659df34bcfca58afd7a54440a15aa7b2946b6229529e973`)。
- 環境: AE 2026 (26.3x87)、Windows 11、Frida 17.8.0。
- 返る関数の本体は Ghidra (`AEXCompat.gpr` に FLT.dll / PF.dll / 対象 AEX 4 本を
  import) で読んだ。

## 2. 観測 (AE 26.3)

### 2.1 dispatcher

`get_callback_addr` の実体は PF.dll+0x17c30 (jmp thunk)。列挙結果のうち
本件に関わる行:

| id | quality / mode | 返る関数 |
| --- | --- | --- |
| -5 | 全 6 通り同じ | PF.dll+0x52f70 = export `PFp_GaussianValue` |
| -2 | mode == 1 = PF_MF_Alpha_STRAIGHT (quality 0/1) | FLT.dll+0x311f0 |
| -2 | mode 0 = PF_MF_Alpha_PREMUL, mode 2 (quality 0/1) | FLT.dll+0x30ff0 |
| 9 (COPY) | q1 m1 / q1 m0,2 / q0 | PF.dll+0x37fa0 / +0x3e000 / +0x37d10 |

id -1, -3, -4, -6, -8, -9, -11, -12 にも PF.dll 内の関数が返る (未解読、
corpus に要求元なし)。-7 / -10 / 0 は AE が 515 (PF_Err_INVALID_CALLBACK)
を返し、1 と 5 は err 0 のまま null を書く。host は未知 id を従来どおり
516 で拒否している (AE の 515 とは違うが、どちらも呼び出し側では
エラー扱い。本 PR では変えていない)。

### 2.2 呼び出し元 (プラグイン側) と実引数

| plug-in | 呼び出し点 | get_callback_addr 引数 | 返った関数の呼び方 (実測) |
| --- | --- | --- | --- |
| Bulge.aex | +0x30d9, +0x26e3 | (q=1, m=0, -5) | `double f(double)`。falloff の lerp 係数に使う |
| Compound_Blur.aex | +0x4722 (直前 +0x46ee で id 9) | (q=1, m=1, -2) | `f(in_data, ?, 1.0, ?, 0x6f, world)` を 128x72 → 4x2 のピラミッド各段に 1 回ずつ |
| CrossBlur.aex | +0x3a95, +0x46d4 | (q=in_data->quality, m=1, -2) | `f(in_data, ?, RadiusX, ?, 0x5f, world)` と `f(in_data, ?, RadiusY, ?, 0x3f, world)` |
| Matte_Choker.aex | +0x5667 | (q=0, m=0, -2) | `f(in_data, ?, softness*0.5, int*, 0x71, world)` を 2 回。呼ぶ前に in_data->quality を 1 に書き換える |

id -2 の第 4 引数はプラグイン側の進捗カウンタ (Matte Choker は
`*p += world->width + world->height` を自分で加算) で、AE 側は読まない
(FLT.dll の FUN_180030f90 が捨てる)。第 2 引数も AE は使わない。

### 2.3 id -5: PFp_GaussianValue

Ghidra (PF.dll 0x52f70) と Frida の入力表が一致:

```
double PFp_GaussianValue(double x) {
  if (1.0 < x) return 0.0;
  return 1.0 - (1.0 - exp((x * -2.378) * x)) * 1.102;
}
```

実測値: f(0)=1, f(0.5)=0.5061259345505494, f(1.0)=0.00019492200675952365,
f(1.1)=0, f(-0.5)=f(0.5) (負は対称)。定数 1.0 / -2.378 / 1.102 は PF.dll の
rdata から読んだ double。

### 2.4 id -2: FLT.dll のインプレース blur

FLT.dll 0x311f0 / 0x30ff0 は同型で、`PFp_WorldDepth(world)` で 8/16/32 を
分岐し、`FUN_180030f90(in_data->effect_ref, radius, ?, flags,
in_data->quality, alpha_type, world)` を呼ぶ。alpha_type だけが違い、
0x311f0 が 0、0x30ff0 が 1。そこから RenderGraph (FUN_180031a50 →
FUN_1800313f0) が PF_BoxBlurNode / PF_GaussianBlurNode を組み、PF.dll の
`PF_BoxBlur1D` / `PF_GaussianBlur1D` が 1D 処理をする。

シグネチャ (実測 + 逆アセンブル):

```
PF_Err fn(PF_InData* in_data, void* unused, double radius,
          void* unused_progress, int32 flags, PF_LayerDef* world);   // in place
```

flags は FLT Blur Suite と同じ語: 0x0f = チャンネル (A=1,R=2,G=4,B=8)、
0x10 = repeat edge、0x20 = vertical、0x40 = horizontal、0x100 = box 近似を
使わない。

軸ごとのカーネル選択 (FUN_1800313f0):

- scale = quality==0 ? 1.4 : 1.0、iterations = quality==0 ? 1 : 3。
- rho = scale * radius / 2.71 (float)。rho > 1 かつ !(flags & 0x100) なら
  box、それ以外は gaussian。
- gaussian (PF_GaussianBlur1D): n = ceil(radius)、整数重み w[0]=255、
  w[i]=(int)(PFp_GaussianValue(i/(radius+1))*255)、正規化は
  255+2Σw[i]。8-bit も 16-bit も同じ整数重み (16-bit の r=1 の実測
  8240/16288/8240 がこの重みそのもの)。
- box (PF_BoxBlur1D): 1 pass の半幅 n = ceil(rho)、両端タップの重みは
  1-(n-rho)。8-bit の span はこれを 1/1024 単位で切り捨て、正規化は
  (2n+1)*1024-2*deficit (16-bit のキャプチャは切り捨てない正確な重みで
  一致し、1/1024 で切り捨てると r=6.3 の 4 タップが 1 段階ずれる)。
  quality 1 は同じ box を 3 pass。8-bit は pass ごとに 8-bit に丸まる
  (16-bit も pass ごとに丸めると r=6.3 の 19 タップが完全一致。推論)。
- alpha type 0 (mode 1 の関数) = ワールドを straight alpha として扱う:
  色チャンネルが選ばれていれば入力を premultiply (8-bit は
  `(c*a+0x80)`, `(t+(t>>8))>>8`) → blur → 最後の pass の出力で
  `min(255, (Σc*255 + Σa/2) / Σa)` で unpremultiply。alpha type 1
  (mode 0/2 の関数) = premultiplied 扱い: 各チャンネルをそのまま blur。
  半透明インパルス (a=127, r=204) で確認: mode 1 は隣接画素の r が 205 に
  なり (色が保たれる)、mode 0 は 51/101/51 (値そのものが広がる)。
- 端: repeat 無しでは中間バッファがゼロ詰めで拡張される (RenderGraph の
  ROI が node の extent 分広がる) ので、多 pass の結果は無限ゼロ詰めと
  等しい。repeat あり (0x10) は gaussian が「範囲内の重みで再正規化」
  (FUN_1800450b0 に明示)、box は端画素の複製に近い挙動 (§3 の残差参照)。
- 2 軸のとき X 軸 → Y 軸の順。straight のときは X の出力が premul の
  中間、Y の最後で unpremultiply。

### 2.5 実効果での一致度 (host 実装後、同一入力・同一パラメータ)

host = 本 PR の worker、AE = 上のキャプチャの PNG。入力は §1 の 256x144。

| effect | params | 結果 |
| --- | --- | --- |
| Bulge | 既定 | host と AE の出力が **byte 一致** (7728 px が入力から変化、差 0) |
| CC Cross Blur | Radius X 6.3 / Y 3.7 | max 1 LSB、mean 0.004、差 >1 の画素 0 |
| CC Cross Blur / Matte Choker | 既定 | 両者とも入力と同一 (半径 0 / 不透明入力なので効果なし) |
| Compound Blur | Maximum Blur 7.5、blur layer = 自身 | 大差 (mean 66)。原因は -2 ではなく id 9 (PF COPY) の resample 差 (§4) |

## 3. host 実装 (`minihost/src/worker_pf_private_callbacks.cpp`) と残差

- id -5: 上の式をそのまま。
- id -2: §2.4 の構造を 8-bit は整数演算で AE と同じ丸め、16-bit は double
  で計算して pass ごとに 16-bit 段階へ丸め (premultiply は切り捨て、
  unpremultiply は最大値で飽和)、32f は丸めもクリップもしない double。`in_data->effect_ref` の一致・quality 0/1・
  半径 [0, 4096]・既知 flags のみ・チャンネル指定あり・ワールドが解決
  できる (FLT Blur Suite と同じ admission) を満たさない呼び出しは 516 と
  `stage:callback_denied callback=private_blur_* reason=...` で拒否し、
  ワールドは触らない。
- 8-bit 合成ワールド 228 ケース (run_r7、上のツールの前身) を Python の
  同モデルと比較した残差: alpha は全ケース 0、gaussian 経路は全チャンネル
  0、box の premul 経路も 0。box の straight 経路の色は unpremultiply の
  丸めで低 alpha 画素に ±1〜5、repeat あり + 高周波の合成ランプでは
  ワールド端の行列に ±2 (角で最大 9)。これは端の扱いが「pass ごとの
  端画素複製」でも「1 回の複製詰め」でも「再正規化」でも完全には
  一致しないため (推論: RenderGraph の中間バッファの詰め方が違う)。
  内部 (端から extent 以上離れた画素) は 0。
- 16-bit はカーネル形状と straight/premul の半透明インパルスを確認。
  pass ごとに 16-bit 段階へ丸め、premultiply を切り捨てにすると、
  キャプチャした 16-bit の値 (r=6.3 の 3-pass box 19 タップ、半透明
  インパルスの緑 13106) と完全一致する (推論: AE の 16-bit 経路も整数演算。
  8-bit と同じ構造から自然だが、16-bit の逆アセンブルは読んでいない)。
  32f は手組みのワールドを AE が例外で拒んだので未確認で、host の float
  経路は丸めもクリップもしない。16-bit の unpremultiply の最大値飽和は
  AE で観測していない (host は 8-bit と同様に飽和させる)。
- worker self-test `--self-test-pf-private-callbacks` が、installed
  `in_data->utils` 経由で取った関数に対して §2.3 の値、§2.4 の 8-bit
  インパルス応答 (r=1 → [64,127,64]、r=2 → [23,62,84,62,23]、q1 r=6.3 の
  15 タップ、q0 r=6.3 → [9,34×7,9])、straight/premul の半透明インパルス、
  alpha-only、AE の 65x65 ステップでの 2 軸 straight (端はゼロ詰め)、
  16-bit の r=1 / r=6.3 / 半透明 straight、fail-closed 群を検査する。
  期待値はすべて AE の関数が書いた値。

## 4. 未解決 / 別 issue

- Compound Blur が id 9 (COPY) で 256x144 → 128x72 → … とサイズの違う
  ワールド間コピーを要求し、AE の PF.dll+0x37fa0 は面積平均で resample
  する (level 0 は 2x2 平均の四捨五入、16x9 → 8x4 のような非整数比も
  平均) のに対し host の copy_world8 は左上をコピーする。-2 の入力が
  既にずれるので出力が AE と一致しない。別 issue として記録
  (issue #1262)。
- 私的 id -1 / -3 / -4 / -6 / -8 / -9 / -11 / -12 の中身は未読。要求元が
  出たときに同じ手順で取る。

## 5. 追記

(観測が覆ったら消さずにここに追記する)

# #997 — Classic extent/origin の AE oracle

2026-11-06。AE 25.1x68 / CPU Software / Best / Full、作業色空間なし、
linear blendingなし。Classic専用SDK probe（8/16 bpc）で測定した。
入力67×49、opaque/透明端、寸法不変/83×61へ拡大/51×37へ縮小、
origin (0,0)/(5,3)/(-5,-3) の36セル。入力はAEが実際に受け取った
packed rawをnativeへ再投入し、row paddingと入力world extentも記録値を渡した。
時刻は両hostとも current=0 / step=1024 / scale=24576。

## 観測と変更

実測されたRENDER契約は以下。出力worldのextentとin_dataのhintはいずれも
入力座標であり、メモリの添字として使う前にoutput_originを足す必要がある。

```
output.extent_hint = [-ox, -oy, output_width-ox, output_height-oy]
in_data.extent_hint = intersection(input.extent_hint, output.extent_hint)
```

AEは寸法不変でもFRAME_SETUPで返したoriginを反映した。例えば67×49、
origin=(5,3)ならhint=[0,0,62,46]。51×37、origin=(-5,-3)なら
hint=[5,3,56,40]で、右下は出力寸法を超えるが、出力座標へ変換すれば
[0,0,51,37]に収まる。出力幅で直接clampする旧処理はこの座標系を混同していた。

修正は一般の交差計算、同寸法originの伝達、出力world extentの更新。
resize/shift後のcoverageは新しい出力バッファ全体を検査する。旧入力diagnostic
extentを縮小済みbufferのcoverageに渡して起きた誤拒否も解消する。
寸法/flag/origin overlapの拒否、guard、padding、未書込み拒否は維持する。

## 結果

|比較|修正前|修正後|
|---|---:|---:|
|host受理|24/36|36/36|
|RENDER geometry・入力・出力exact|8/36|36/36|
|入力差数|0|0|
|出力差数|53,226|0|
|出力最大差|32,768|0|

比較出力は491,184 sample。最初の出力差は8_opaque_1の(x=1,y=0,R)、
AE=0 / native=17。修正前の縮小12セルはselector成功後にcoverage error -6で
拒否され、成功に数えていない。AEをもう一度採取した36セルも、入力・出力raw、
RENDER geometryすべてexactだった。許容誤差なし。

FRAME_SETUPの提供寸法・返却寸法/origin、入力寸法、時間、downsample、
pre-effect originも一致した。AEは各セルでFRAME_SETUPを2回、RENDERを1回呼んだ。
nativeは各1回。selector回数の同値性は本件の主張ではない。

FRAME_SETUPのin_data hintには `[177,-469060063,32766,0]` のような画像範囲外の値を
観測した（再採取でも同値）。SDK AE_Effect.hは全in_data fieldが有効になる点を
RENDERとする。初期化状態が不明なFRAME_SETUP hintを矩形の期待値にしない。
raw観測は保存し、未初期化との解釈は推論に留める。RENDERのout_data寸法は
AEでは0で、FRAME_SETUPの提供・返却値とは区別する。これらのfieldをexactと称していない。
出力rowbytesとwriteable flagはhostのallocation属性で異なり得る。
pixel比較はpaddingを除く全packed sample、extent/origin比較は数値完全一致。
32 bpcは未比較（probeはClassic 8/16のみ）。本件をnative32認証に流用しない。

## 証拠と再現

[数値packet](ISSUE997_EXTENT_ORACLE.json) に36セルのAE geometry、native geometry、
修正前後の差数/最大差/first mismatch、raw等509ファイルの相対名・size・SHA-256を保存。
private絶対path、SDK、AEX、画像内容は含めない。
probe: 54,272 bytes / SHA-256
`14fd420e01a5b4ce136902cf41b38fe0188bf2797f5cf4a42480c547d4499c0e`。
probe sourceとAE採取/replay/hash生成はolm-win-checksの
`capture/extent_probe.cpp`、`tools/t40_capture.jsx`、`tools/t40_compare.py`、
`tools/t40_evidence.py`。rawはそのT40 durable dataに保持。

公開repoだけで動くbehavioral検証（SDK/AEX不要）:

```powershell
pwsh -File tools/build-native.ps1 -Target worker_render_lifecycle_selftest,worker_classic_runtime_selftest,worker_classic_execution_selftest,worker_output_coverage_selftest
target/minihost-build/worker_render_lifecycle_selftest.exe
target/minihost-build/worker_classic_runtime_selftest.exe
target/minihost-build/worker_classic_execution_selftest.exe
target/minihost-build/worker_output_coverage_selftest.exe
```

geometry helperはoracle9点を検査。production render_onceも9点×8/16 bpcを通り、
同寸法のorigin伝達、拡大/縮小、padded input/output、報告origin、全出力を検査する。
各点で最後の1画素を意図的に書き残す18 mutationは、coverage1画素・error -6で拒否。
既存の不正extent・origin拒否、lifecycle/ownership、coverage境界テストも成功。
Release worker build、broker integration10件、focused pytest3件、Rust workspace lib
（395+33+8 passed、既存ignore3）を確認した。

レビューは別工程の慎重な自己レビューと修正後再レビュー。別担当によるreviewではない。

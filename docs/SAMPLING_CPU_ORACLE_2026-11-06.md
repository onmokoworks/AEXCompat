# Sampling CPU oracle — #1039

AE 2025 25.1x68、CPU/Software、Best/Full の保存返値に基づき、
nearest/subpixel/area の8/16/32 bpcを検証する。
mode 0/1/2 は全観測で同値。画像外は0として扱う。
入力別・plug-in別の分岐、許容差は使用しない。

## 観測と一般式

- nearest は `floor(x+0.5), floor(y+0.5)` の元word。
- subpixel の整数座標は元wordを直接返す。透明RGBも保持する。
- 整数subpixelは bilinear weight と入力alphaによるRGB加重平均。
  出力alphaが整数丸めで0ならRGBも0。RGBの分母は丸め前のalpha積和。
  16 bitの積和もuint64範囲内。整数座標以外の深度最大値は255/32768。
- float subpixel は右下累積面積のbinary32差分から4係数を作る。
  alphaとの積はbinary32、alpha積和はdouble、RGB積和は行順のbinary32。
  binary32 alpha分母の逆数をRGBへ掛ける。全alpha0ならstraight RGBの積和。
- area は各軸のradiusが0.5以下なら、その軸の中心をsubpixelで標本化する。
  0.5より大きい軸はpixel squareとの交差長で積分する。
  両軸が広ければ元画素を積分する。狭い軸を単に0.5へclampする式ではない。
  subpixelの返値をalphaで再加重し、積和を正規化する。
  8 bpcはbinary32積和とRGB直接除算、16/32 bpcはdouble積和とdouble逆数乗算。
  areaの全alpha0は全成分0。出力alphaは深度最大値を上限とする。
- `area` と `samp_behave` は計算に使用しない。
  引数/world検証、正のradiusかつ128未満という既存の実行範囲制限は維持する。

これは観測範囲に一致する一般実装であり、別AE版・Draft・非有限入力の同値証明ではない。
CPU実装の丸め境界を保持し、FMAで積和を融合しない。

## 保存証拠とテスト

T25/T34/T43 の176セル、16,641 callback、66,564 wordと、
T41 Compound Blurの12 subpixel callback、48 wordを使用する。
T41は観測された4tapを2×2ローカル座標へ平行移動して検査する。
whole Compound Blurの一致をこの12点だけから主張しない。

`tests/native/sampling_oracle_fixture.hpp` の期待値はAEの返値を直接exportしたもの。
重複callbackは同じ入力・引数・返値の組だけ統合し、観測回数を保持する。
41入力、14,655 unique callback、16,653観測、66,612 word。
合成3×2入力のbasis・半透明・全透明・相殺RGB、半径0.5前後1 fixed単位、
非単位半径、等面積で異なる縦横比、四辺・画像外・負座標を含む。

`sampling_oracle_provenance.json` に保存capture identityとfixture hashを記録する。
export元は olm-win-checks の `tools/t43_export_sampling.py`。
binary、SDK、DLLの逆アセンブル内容は含めない。

native self-testは実callbackを呼び、全wordを比較する。
未整列source/destination、row padding、guard保持も各観測で検査する。
旧hostのstraight RGB補間との等価性テストは互換性を示さないため置換した。
ランダム3200ケースでは非alias実行との比較により、aliasとguardを別途検証する。
実AEXのsampling probeに埋め込まれていた旧hostモデルと許容差判定は撤去し、
callback返値をそのまま出力する測定器にした。callback errorは引き続き伝播する。
実AEXテストは不透明な一定basis場の厳密な出力、suite lease、guard、SEHを検査し、
非一様入力の精度は保存AE word corpusで検査する。runtimeの保護検証は変更しない。

## ローカル検証

- native self-test: 66,612 word exact、guard/alias/拒否条件も成功。
- Release worker build、Python関連32件、追加artifact/session/batch 19件成功
  （未buildの別fixtureに依存する3件はskip）。
- Rust workspace lib: 395/33/8件成功、3件ignored。broker integration: 10件成功。
- 新workerでcapture AEXを実行: 162セル・15,471 callback、全error0。
  AEと61,884 word比較、差0・最大0・first=null。Tag0はcompiled corpusで検査する。
- 既存25画像ケース（各2回）: 24 exact / 1 mismatch / 未比較0。
  残存3/238,944 wordの不一致は本件の完了に含めず、許容差へ変更しない。
- 全tracked Rust format check、diff check成功。

```powershell
pwsh -NoProfile -File tools/build-native.ps1 -Target worker_pf_sampling_wiring_selftest
target/minihost-build/worker_pf_sampling_wiring_selftest.exe
```

期待: `passed`、`oracle_callbacks=16653`、`oracle_words=66612`。
失敗時はdepth/method/mode/座標/radius/channel/期待word/実wordを出力する。

# #1263 / AE 25.1 Timecode の同一 font/style 縮小描画

status: partial

同じfont/styleで比較したARGB8の16条件は、一般的な軸平行縮小フィルタの変更で
実AE CPU rawと全値一致した。日本語LStrの選択、16/32bpcの変換、元2026版は未完了で、
#1263はOPENに保持する。以下は2025版の証拠であり、旧16%比率を補正値にしていない。

## 実体と観測条件

- AE25.1x68、CPU/Software、Best/Full、PAR1、30fps、色管理なし。
- Timecode: 531400 bytes、SHA-256 `801780f7d891eac6549eed097601623781338fb1dac44414f972ccdb4d5057f8`。
  過去2025版531912 bytes / `41dabdd8…`、旧2026版とは別物。
- BEE25.1.0.73: SizeOfImage `0x1627000` / TimeDateStamp `0x6740074a`、
  SHA-256 `1c00a8c32715bd07e30944e71323a3dbf9e77ebeaf02ddf3137dbc4ca5c38c68`。
  source-item offset0x27c8 / const-source slot178 / parent-comp offset0x338は既存2025 layoutに一致。
  metadata追加後も既存export/vtable/実呼出し検証を維持する。
- Oracle adapter: 62464 bytes、SHA-256 `4fd93782ac5302aa746641c64f9f7b510f7cd5ad628a9bfd0c7be2fd598ca85f`。
  元AEXへselectorを転送しfont/styleのLStr入力のみ固定。pixel処理とpreferencesは変更しない。
  MS Gothic/Regularの4条件は元AEの入力・出力ともexact、native Arial4条件も元nativeとexact。
- Arial Regular/Bold/Italic、MS Gothic Regularを選択したことをread-only観測で確認。
  16件すべてfound1/error0。traceあり・なしの入力/出力32 captureもexact。

## Raw比較（ARGB8）

表は変更前の差数/最大差。変更後は全16セルで差0、最大0、firstなし、glyph/box位置も一致。

|寸法 / size / frame / 表示|Arial|Bold|Italic|MS Gothic|
|---|---|---|---|---|
|256×144 / 36 / 0 / SMPTE|4806/44|4488/54|5127/53|5331/53|
|320×180 / 18 / 17 / SMPTE|2112/82|2199/59|2325/63|2178/26|
|512×288 / 48 / 65 / SMPTE|6681/105|6141/69|6339/81|6321/48|
|321×181 / 27 / 39 / frame番号|2691/44|2256/26|2334/54|2094/28|

最初の変更前差はArial/size36の(37,25,R)、AE6 / host5。
元nativeのArialと日本語AEのMS Gothicを混ぜた初回比較とは分離している。

## 原因と修正境界

事実: Timecodeの各描画の最初のglyphについて、PF transform_worldの入力、出力、
行列、composite設定を保存した。全16件がquality1/mode1/source-over/倍率0.5。
中心bilinearのモデルは各件267～774値の差を示す一方、pixel footprintの面積積分は
16件とも差0となる。保存trace SHA-256は
`bb6e2f567ee3c6e49e8f2899e5b659a185cfd92ed22091b07debf65e8b7c1fc3`（22574118 bytes）。
推論: 今回の同一fontの残差は、縮小時にsource範囲を積分せず中心だけ補間するhost処理に起因する。
実装後の全フレーム一致がこれを裏付ける。

修正は軸平行affine縮小のsource footprintを積分し、範囲外を透明として正規化する。
非縮小軸はunit幅、straight RGBはalphaで重み付けしてから戻す。font名、plugin名、
特定倍率、寸法による分岐はない。非軸平行変換・mask評価の既存経路は保持する。
8/16/floatの解析的self-testで、非線形色、半透明、透明RGB、3:1縮小、fractional translation、
縦横、反転、外側coverage、padding、draftを検査する。これら解析的テストは深度別AE oracleの代用ではない。

## 再検証

```powershell
pwsh -File tools/build-native.ps1 -Target aex_worker
$env:AEXCOMPAT_RENDER_WORKER=(Resolve-Path target/minihost-build/aex_worker.exe).Path
python -m pytest -q --run-built-artifact-tests --run-local-artifact-tests tests/test_pf_world_transform_affine.py tests/test_pf_world_transform_blend.py tests/test_pf_world_transform_composite_rect.py tests/test_pf_world_transform_convolve.py tests/test_pf_world_transform_transfer_mask.py
& $env:AEXCOMPAT_RENDER_WORKER --kind classic --self-test-bee-scene-facade
```

focused pytestは6 passed。機械固有のraw/probe/module manifestはolm-win-checks T48のdurable dataに保持。
同repoの`tools/t48_resume_evidence.py`が16セル、AE透過性4、trace反復32 capture、209 manifest entryを再計算する。
Adobeバイナリ、SDK、raw画像を本repoへ含めない。

残件: 未翻訳LStrにより元hostはArial、日本語AEはMS Gothicとなる入力選択。
16/32bpc projectの8bit AEX dispatch前後の変換、rotation/shear/maskを含む一般transform oracle、
元2026版と同実体のfont/style検証。今回のARGB8一致だけでこれらを完了扱いにしない。

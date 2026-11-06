# #1262 COPY CPU oracle — 未完のローカルcheckpoint

status: blocked

AE 25.1x68 / CPU Software / Best / Full で専用SDK probeの2,352条件を2回採取した。
一般COPY候補は全logical sampleと追加確保列に一致したが、whole Compound Blurは未一致。
この変更はローカル保存のみ。#1262をcloseできる状態ではなく、PR/CI/mergeは未実施。

## 観測と変更

- private q0/q1 × mode0/1/2、public utility COPY対照、8/16/32 bpc。
  opaque、混合alpha、premultiplied、impulse、隣接丸め、alpha impulseの8入力。
  null/明示rect、2×2→1×1、16×9→8×4、odd、非整数縮小/拡大、alias、paddedの14形状。
- q0は各bin末端のnearest、q1/m1はstraight-alphaのarea/bilinear。
  fixed16 step、半径の切捨て、floatの係数・積和順を別に扱う。
  q1/m0,2はdeep worldでも4-byte単位のsampler。全寸法同一時のcopyは型幅を使う。
  runtimeにはplug-in名・入力pattern・期待値による分岐を置かない。
- public utility COPYは観測されたBestのstraight-alpha経路へ接続。
  WorldTransformSuiteのcopyとout-of-world rectの既存clipped-overlap契約は保持する。
  未観測quality/modeは既存overlap経路を保持。public Draftとout-of-world resamplingの
  AE同値性は未確認であり、今回の2,352条件の主張に含めない。
- 所有権・world format・rowbytes・4096 boundsを保持し、paddingをコピーしない。
  in-placeの前方向feedbackはAE観測に合わせる。

## 結果

|比較|logical/storage word一致|strict exact|
|---|---:|---:|
|main 34ddf50c対AE|609/2352|572/2352|
|候補native対AE|2352/2352|2315/2352|
|AE反復|2352/2352|2306/2352|
|候補native反復|2352/2352|2352/2352|

各runはsource 362,208値、destination 148,512値。候補は全深度で差数0、最大0、
floatはraw word一致・0 ULP。alias source/destinationは同一storageの重複を含む。
AE初回の37条件、反復のいずれか46条件でpadding変更があるためstrictとは区別した。
nativeはpadding変更0、guards保持、未書込み0、suite/world収支均衡。
AEのpadding挙動を模倣するための境界保護緩和は行わない。

`worker_pf_copy_oracle_fixtures.hpp` は独立したAE記録から抽出した99条件の合成画素。
期待値は実装モデルから生成していない。SDK/plug-in bytesを含まない。
異なるstrideとguardを加え、全bufferをbyte比較するbehavioral self-testを追加。
Release build、bad_callback self-test、private callback self-test、broker
`trusted_worker_stage` 10 testsが成功した。

## whole Compound Blur — 未完受入

同一AE 2025 Compound_Blur.aex、256×144、8 bpc、self map、最大ブラー7.5で比較。
baselineは14,053値、候補は13,161値が不一致。両者の最大差168、firstは(3,0,G)、
AE=10/native=9。20pixel内側の領域は差0だが、部分一致を合格にはしない。
AEの通常反復とinstrumentation中の反復は、入力・出力とも全word一致。

実render中の6 COPYは256×144→128×72→64×36→32×18→16×9→8×4→4×2。
全q1/m1・null rect・error0で、Python候補model v3のreplayと差0。
これはtraced inputのnative replayではない。

同じrenderでsubpixel callback 5,120回を観測し、先頭12 edge点を保存した。
12点の計48値中32値が既存native整数式と異なり最大差109。
firstのfixed座標(-16383,311297)ではAE ARGB=[143,1,54,29]、
既存式=[143,1,40,22]。sampling自体は本変更で修正しない。

推論: 残差には既存 #1039 のalpha/edge sampling gapが関係する。
全frame残差をそれだけで説明できると証明したものではない。
#1039の一般式と全深度受入を別scopeで完成させてから再比較が必要。

## identityと再検証

- probe: 81,920 bytes / SHA-256
  `69a1cbc59b13f0a4d10d07c3bede38bdef73e31ef36176e9f520ddfe5fbd158e`
- Compound Blur: 56,264 bytes / SHA-256
  `20318d2d3020f7cc8db7123210abfa94477f3ab9a037f75f1328d8eebb9d9c93`
- PF.dll SHA-256:
  `ec710b8eaacae5b232afdb76367d5ba61f1f61a7f2276815b79f249ae1ddd190`
- private q0: PF.dll RVA0x53b90、q1/m1とpublic:0x53e20、q1/m0,2:0x5a910。
  observed direct subpixel:0xa4f70。アドレスをruntimeに埋め込まない。

```powershell
& tools/build-native.ps1 -Target @('aex_worker','worker_pf_bad_callback_param_selftest')
& target/minihost-build/worker_pf_bad_callback_param_selftest.exe
& target/minihost-build/aex_worker.exe --kind classic --self-test-pf-private-callbacks
cargo test --offline --manifest-path broker/Cargo.toml -p aexcompat-broker --jobs 2 --test trusted_worker_stage
```

外部oracleと全証拠の再計算はolm-win-checksの `tools/t41_resume3_audit.py --verify`。
外部保存データを必要とし、検証成功は未一致を許容する意味ではない。
実装と分けた慎重な自己レビューを行い、未知quality/mode fallbackとdiagnostic経路の
退行を修正した。修正後のdiffを再レビューした。別担当レビューではない。

# #1262 — private COPY の CPU oracle と一般実装

AE 25.1x68 CPU/Software の private COPY id9、q0/q1 × mode0/1/2 × 8/16/32 bpcを実測した。
**全2,352条件のlogical/storage wordがnativeと一致し、同一Compound Blurの全147,456 sampleも一致した。**
AEのpadding変更は別記し、hostの境界保護を維持する。許容差は使用しない。

## 一般規則と保持する契約

- q0: 各bin末端のnearest。
- q1/m1: straight-alphaのarea/bilinear。16.16 step、半径の切捨て、floatの積和順・丸め位置を保持。
- q1/m0,2: deep worldでも4-byte単位のsampler。同寸法full-world copyは型幅を使う。
- public utility COPYのBest経路は実測したstraight-alpha経路へ接続する。
- WorldTransformSuite copy、未知quality/mode、out-of-world rectangleのclipped-overlap処理を維持する。
- world所有権、登録後geometry、format、rowbytes、4096 boundsを検証し、paddingを書き換えない。
  aliasは実測した前方向feedback。plug-in名・入力pattern・期待値によるruntime分岐なし。

#1039 / PR #1811（main f170b257）の一般sampling修正に依存する。
COPYと直接samplingはfloat係数・accumulatorの丸めが異なるため、数値処理を同じ関数へ置換していない。

## 独立AE oracleと結果

専用SDK probeのprivate6/public1 × 3深度 × 8入力 × 14形状 = 2,352条件。
opaque、混合alpha、premultiplied、横/縦/点impulse、隣接丸め、alpha impulseを含む。
null/明示rect、2×2→1×1、16×9→8×4、odd、非整数縮小/拡大、alias、padded viewを含む。
各runはsource362,208値・destination148,512値。aliasは同じstorageの重複を含む。

|比較|全logical/storage word同値|padding不変も要求するstrict exact|
|---|---:|---:|
|修正前 34ddf50c 対保存AE|609/2352|572/2352|
|最終native 対保存AE|2352/2352|2315/2352|
|最終native 対新規AE|2352/2352|2321/2352|
|前候補native 対最終native|2352/2352|2352/2352|

nativeの差数0、最大0、float raw word同値・有限値0 ULP、first mismatchなし。
get/copy/lifecycle error0、distinct source変更0、native padding変更0、guard保持、未書込み0、suite/world収支均衡。

保存AEは37条件、新規AEは31条件でpaddingを書き換えた。q1/m0,2の同寸法fast copyが該当する。
AE反復でもpadding変更数は同じにならない。これをstrict一致に数えず、active sampleとは別に全該当cellを記録した。
padding書込みの再現のためにhost-protectionを緩めていない。

public utility COPYのBest336条件は全word同値。public Draft、out-of-world resampling、任意の非有限入力は
今回のoracle範囲外であり、同値性を主張しない。今回の受入は宣言したprivate quality/mode/depthとpublic Best対照。

## whole Compound Blur

同一AE 2025実体、256×144、8 bpc、Best/Full、CPU、self map、最大ブラー7.5、サイズpopup1、invert0。
AEが実際に受け取った入力rawをそのままnativeへ渡す。色管理なし、linear=false。

|native|異なる値数|最大差|first mismatch|
|---|---:|---:|---|
|COPY修正前|14053|168|(3,0,G), AE10/native9|
|COPY候補・#1039修正前|13161|168|(3,0,G), AE10/native9|
|COPY修正・#1039修正後|0|0|なし|

全147,456 sampleを比較し、外周を除外していない。新規AE採取も保存AEのinput/outputと差0。
実renderのCOPY6段（256×144→128×72→64×36→32×18→16×9→8×4→4×2）はq1/m1/null rect。
全画面一致の実native検証と、前工程のPythonモデルreplayは区別する。

## behavioral testsとレビュー

独立AE記録から99条件を抽出した `worker_pf_copy_oracle_fixtures.hpp` を使用する。
SDK/plug-in bytesは含めない。異なるstrideと前後guardを付けて全allocationをbyte比較し、
全経路/深度で登録後geometry改変を拒否してallocationを保持することも確認する。

- Release worker / bad_callback self-test / private callback self-test: 成功。
- portable compiled `test_pf_bad_callback_param.py`: 1 passed。
- broker integration `trusted_worker_stage`: 10 passed。
- Rust workspace lib: 437 passed / 3 ignored（内訳1+395+33+8）、deep chain: 2 passed。
- focused Python worker/color/parameter: 39 passed。全tracked Rustのrustfmt成功。
- 実native COPY全2,352条件、Compound全画面を修正後に再実行して一致。
- 比較器の7 mutation（3深度pixel、error、padding、2 allocation追加列）を検出。

実装とは別工程の慎重な自己レビューを実施した。別担当レビューではない。
未知quality/modeとdiagnostic routingの前工程の修正を再確認し、今回float積和の非contract化と
登録後geometry拒否のbehavioral testを追加した。修正後の全diffを再レビューし、受理済み未対応指摘0。

## identityと再現

- CopyProbe: 81,920 bytes / SHA-256 `69a1cbc59b13f0a4d10d07c3bede38bdef73e31ef36176e9f520ddfe5fbd158e`
- Compound Blur: 56,264 bytes / SHA-256 `20318d2d3020f7cc8db7123210abfa94477f3ab9a037f75f1328d8eebb9d9c93`
- PF.dll: SHA-256 `ec710b8eaacae5b232afdb76367d5ba61f1f61a7f2276815b79f249ae1ddd190`

実loaded identity、raw、各padding cellの差数・最大差・firstは外部台帳に保持する。
公開文書へprivate絶対pathや実画像を格納しない。

```powershell
& tools/build-native.ps1 -Target @('aex_worker','worker_pf_bad_callback_param_selftest')
& target/minihost-build/worker_pf_bad_callback_param_selftest.exe
& target/minihost-build/aex_worker.exe --kind classic --self-test-pf-private-callbacks
cargo test --offline --manifest-path broker/Cargo.toml -p aexcompat-broker --jobs 2 --test trusted_worker_stage
```

外部oracle再計算: olm-win-checks `tools/t41_resume4_audit.py --output <保存audit> --verify`。
期待: 9,464証拠、word2352/strict2315（保存AE）・word2352/strict2321（新規AE）、
native回帰2352 strict、Compound差0/最大0。padding差はそのまま再現する。

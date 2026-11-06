# #98 resident CPU oracle — T50限定修正

## 保存AE証拠

AE25.1x68、CPU/Software、Best/Full、64×32、24fps、色管理なし、linear false、
MFR false要求。Classic8/16・Smart8/16/32の5セルで、作成直後と保存/reopen後に
0→2→1→1、Drive0→0→0→2を要求した。40要求DONE、250 selector終了記録error0。
各phaseの実入力hashは3種類。同時刻の最後2要求は入力同一・出力相違。
静止素材やcache反復の比較ではない。追加AE測定はownerの停止指示により実施しない。

同一プローブの実loaded identityを採取前後に確認:

|probe|bytes|SHA-256|
|---|---:|---|
|olm_resident_classic.aex|58368|3df34cb54aede98eceb5764c137a9e01468784ce45c6e9d1df3a7ed99ca4fbf7|
|olm_resident_smart.aex|60416|1a3158c138dfe0ecf5e5f3c5372161651e9b066db623d3f3d465f52249a7fa03|

原本と採取コードはolm-win-checksのT50/P06記録。AE側は
`tools/t50_capture.jsx`、`capture/resident_probe.cpp`、比較は
`tools/t50_compare_ae.py`。保存データ名は`T50/ae-oracle`と`T50/ae-comparison`。
原本610 identityのmanifest SHA-256:
`7176cd2a028b90d3ff2c1c27ff70fbe2bc2ac75754e3de7d8f9fed00c9f7619e`。
binary、SDK、raw画像、private absolute pathは本repoへ含めない。

## 事実と修正根拠

旧residentではdeep入力・出力に同じ差があった。Classic16/Smart16各182値、
Smart32は24072値、計24436値。整数最大1、float最大1 ULP。
最初の16bpc出力差は(30,0,R) AE32640/native32639、floatは(3,0,R)
AE word1045220558/native1045220557。8bpcは差0。

保存rawのRGBに全256入力値があり、各値の出力wordは全24 deep要求で一意。
Classic16/Smart16の写像も同一。`tools/t50_depth_audit.py`が保存rawから写像を抽出し、
`tests/native/resident_depth_oracle.hpp`を生成する。期待値は実装式から生成しない。
抽出結果`depth-audit.json` SHA-256:
`15c1651c50fed18e0eada397423d6379d91ce94e6cd09b9c3490a57bdaf17049`。

推論: float32逆数乗算 `v * (1.0f/255.0f)` と、16bpcではさらにfloat32の
`*32768.0f + 0.5f`を整数化する一般式が、全256値を厳密に再現する。
AE内部実装を逆解析した主張ではない。旧整数有理丸めは254で差1、旧float除算は
複数値で1 ULP異なる。共通のnative RGBA8入力変換をこの式に修正した。
素材は不透明なので非不透明alphaや色管理付きimportを実測済みとはしない。
same-depth raw transportは変換しない。Rust画像decoderの別入口は今回の対象外。

AE Smartの全24要求はSMART_PRE_RENDER→SMART_RENDER、FRAME_SETUP/SETDOWNは0。
旧nativeでは各要求にClassic frame pairが付いていた。Smart layoutでそのpairを省き、
sequence所有権・aux cleanup・各frameのorigin初期化は維持した。
Classic layoutのframe pair・geometry・エラー処理は維持する。

## native検証

修正workerと既存probeで5セル×4要求を各1 resident PIDで再実行。
保存AE両phaseに対応した40比較組、入力/出力各327680 sampleは差0、最大0、
float 0 ULP、first mismatchなし。各phaseの入力hash3種類、同時刻parameter変更の
出力hash相違も確認。`tools/t50_replay_saved.py`はAEに接続しない。
Smart各frameはPRE_RENDER→SMART_RENDER、ClassicはFRAME_SETUP→RENDER→FRAME_SETDOWN。

behavioral self-testでは全256入力値の参照word、strided world/padding/guard、
SmartのClassic frame selector不発行、sequence cleanupとprimary error優先、
origin初期化、Classic geometryを検証する。broker integrationは10 passed、
Rust resident unitは20 passed。Python基本suiteは962 passed/409 skipped/1 failed
（既存Pillow11.3.0にget_flattened_dataなし）。共有venvを12.3.0へ更新して当該moduleは
5 passed。失敗を成功として丸めていない。AE automation moduleは追加測定禁止のため除外。
focused native wrapperは1 passed、Smart geometry fixture未配置の9件はskip。
その代わり既存T50 probeで実residentを上記5セル実行し、SDK fixture群はCIで検証する。

実装後、別工程の慎重な自己レビューで全diffを確認した（別担当reviewではない）。
origin初期化の保持、cleanupのエラー優先、同深度rawの無変換、期待wordの独立性、
full lifecycle未完の表示を確認し、古いSmart frame説明を修正して再確認した。
受理した未対応指摘は0。

最初のCI run42248484951はPython後段5 failed/360 passed/14 skipped。
内訳は診断側の旧float変換1、layer receiptテストの旧float期待式1、
SmartFXでFRAME_SETUPのseed検査を期待した既存fixture利用3。
診断側をnativeと同じ深度式へそろえ、独立AE wordと1-bit mutationで厳密判定を検証した。
receipt期待式も同じ一般変換へ更新。Smartのtile比較は全byte比較を維持し、
初期seed/fingerprint/padding検査はClassic frame selectorを通る3深度の独立テストへ分離した。
seed生成/coverage/guardの製品コードを緩和していない。probeソース変更や追加build/installはない。
ローカル再確認: Rust diagnostics6 passed、Smart session9 passed、tile/Classic seed6 passed。
CI artifact再利用のreceipt3件はmtime freshness guardで実行前に拒否されたため、最新head CIへ委ねる。
CI修正全diffを旧reviewed-headから再レビューし、未対応指摘0。
次のrun42250374526のearly partitionでは、新規Classic seed3件のbuilt-artifact manifest登録漏れで
ビルド前のharness不存在を検出した。3 nodeを既存manifestへ登録し、全collectionの整合と
early partitionで3件が除外されることを確認。検査をskip追加で消したのではなく、既存の
native build後partitionで必ず実行する分類へ直した。これはrunner障害ではない。

```powershell
$env:PYTHONUTF8='1'
# TEMP/TMP/TMPDIRは利用者指定scratchへ設定して実行する。
& .\tools\build-native.ps1 -Target @('aex_worker','worker_render_lifecycle_selftest')
& .\target\minihost-build\worker_render_lifecycle_selftest.exe
cargo test --offline --manifest-path broker/Cargo.toml -p aexcompat-broker --test trusted_worker_stage --jobs 2
python -m pytest -q --run-built-artifact-tests tests/test_render_lifecycle_frame_setup_extent.py
```

## 未達とIssue状態

**#98全体の受入は未達。OPEN維持。** full selector列を削って一致とはしない。
AE render sequenceはphase0 setup/resetup/flatten=1/1/1、保存reopen後=1/3/3、
resident=1/0/0。AEのUI側とrender側は別handle。residentでphase1 restoreは未実行で、
同じ4要求の出力を保存AEの両phaseへ比較しただけである。
probe出力は時間とparameterに依存するがsequence回数には依存しない。
UI/render clone・flatten/RESETUPの意味論、sequence依存effect、MFR、GPUは未完/対象外。
回数を合わせるだけのRESETUP追加やpluginごとの特例は実装しない。

owner指示の部分修正として#98を参照し、完了を表すclosing keywordは使用しない。
別subissueは作成しない。

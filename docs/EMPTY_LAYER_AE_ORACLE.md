# 未割当mapのcurrent-time shape（#962 / T39 P03）

## 観測

2026-11-06、Windows AE 25.1x68、CPU/Software、64×32、24 fps、time=0、
8/16/32 bpc、working spaceなし、linear blendingなし、MFR無効で取得した。
primaryは不透明RGB=(25,43,61)、secondaryは不透明RGB=(137,71,223)のPNG。
同じprobe effectのOptional MapをNoneまたはsecondaryへ設定し、直後の
WorldCaptureでpacked ARGBを採取した。6件ともRQ DONE、32-word復号成功。
PNG/PSD previewではなく、元深度のraw channel wordを比較した。

未割当は全深度でPreRender error=0、result/max rect=[0,0,0,0]、
SmartRender checkout error=0、world=null、dataなし。worldがないためprobeは
format queryを呼ばず、local sentinelの516とformat=0を記録する。
width/height/rowbytes/extent/first pixelは未観測を表す0xffffffff。
**516はAE callbackから返ったエラーではない。**

割当済みはerror=0、result/max/extent=[0,0,64,32]、world/dataあり、
width=64、height=32、rowbytes=256/512/1024。
probeが8-bitへ正規化した先頭ARGBは[255,137,71,223]。
formatはARGB32=0x62677261、ARGB64=0x36316561、ARGB128=0x32336561。
全32 wordと画像全8192 channel sample/ケースを照合した。
このprobeの先頭pixel欄はdeep raw sample自体の精度検証ではない。

## identity

- probe: 16,896 bytes、SHA-256 `7e4f8e96d81ff26a956cd229d5dcab6ec94b47c4490bf13ff79b15d42179db88`
- probe build SDK: After Effects SDK 25.2_20、Windows x64、MSVC Release。
- capture: 68,096 bytes、SHA-256 `25c6dbffc00a13c5e16c37704d39d899cb5c646041ae6c7fd7db52c44de5367c`
- primary PNG: 138 bytes、SHA-256 `6fc5eaa6efccf4619aba6098e01a8d7195a133624cf599c4254cd5a71921178e`
- secondary PNG: 137 bytes、SHA-256 `067dfd6ae91fa39ca6318a1d2d837d1d4aa560029abfb8ed8eacb185a1a3594f`

|セル|AE packed ARGB SHA-256|
|---|---|
|8 unassigned|`4d9025eb0c61a2c19f22be13775ce7876335b2c0094e304333c1c9147976496e`|
|8 assigned|`01fb273698840e62e494bd8b437aa1a66fc6ed3950ea7463804beff290f7e062`|
|16 unassigned|`7cfe1ec68e73974fadd3e1870a969442424a6b279d3294a86bf031bf475d9b50`|
|16 assigned|`a20d7d75c75b2d55ea701ead9b92bc7155c95a76dd457a11b7902e2d4c52a12d`|
|32 unassigned|`ec5e88ff45ce5e20ccfa5e263accaff51fcdce875b2966e17d7afd933b16d1c4`|
|32 assigned|`de9f86db1041c2dedab54ae10a3869499d427bde9eb829de13a92dafdcc36025`|

元raw/header、MCP job、loaded-module identity、全word、実行引数、before/after
worker snapshotsはWindows検証環境のT39 durable evidenceに保管。
公開テストは観測した32 wordと全channel sampleを同じSDK probeで再生成・比較する。

## before / after

base `f4481676c60af8121f797f0f479ea4c4098d77c9` のhostは未割当をfull透明worldで返す。
修正前はunassignedで各15/32 word差。8 bpcは1098/8192 sample差（最大255）、
16 bpcは1095/8192（最大32768）、32 bpcは1095/8192（最大1065353216 ordered-word ULP）。
全深度のfirst mismatchはsample1789、R(x63,y6)、native=white、AE=0。
assignedの3セルは既に差0。

一般callbackをnull返却へ変更後、6セルの192 word・49152 channel sampleは差0、
最大差0、float 0 ULP。checkout/checkinの追跡は維持し、不要なworld allocationを削除した。
configured-missing、unknown/stale checkout、null destination、実入力のempty-result拒否は維持する。
PiPLのversion/out_flags/out_flags2もGLOBAL_SETUPと一致させた。旧PiPLにはSmart Render bitがなく、
旧AE取得は停止していたため契約証拠に含めない。

## 再検証と範囲

SDK probe、worker、harnessをbuildし、`PYTHONUTF8=1`と現在のworker環境変数を設定:

```powershell
$env:PYTHONUTF8 = '1'
$env:AEXCOMPAT_CARGO_PROFILE = 'debug'
pwsh -File tools/build-native.ps1 -Target aex_worker
$env:AEXCOMPAT_CANONICAL_WORKER = (Resolve-Path target/minihost-build/aex_worker.exe).Path
$env:AEXCOMPAT_RENDER_WORKER = $env:AEXCOMPAT_CANONICAL_WORKER
cargo test --manifest-path broker/Cargo.toml -p aexcompat-broker --test trusted_worker_stage
cargo build --manifest-path broker/Cargo.toml -p aexcompat-harness
pwsh -File tools/build-pf-empty-layer-contract-probe.ps1
python -m pytest -q --run-built-artifact-tests --run-local-artifact-tests tests/test_pf_empty_layer_contract_probe.py
target/minihost-build/aex_worker.exe --kind smart --self-test-pf-checkout-intersection
```

current-timeのprobe子scopeのみ。非current-time（#1748）、GPU、原影響plugin
DeepGlow2/3DGlasses/Starglowの同一実体によるAE画像比較は未検証。親#962はopenを維持する。
8/16/32 bpcのnull契約がこれらの残件をすべて解決するという主張ではない。

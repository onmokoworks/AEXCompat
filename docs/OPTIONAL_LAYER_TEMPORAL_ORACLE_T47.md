# #1748 / T47 — 未割当secondaryの時間契約

実AE 25.1x68 CPU/Software、8/16/32 bpc、64×32、30 fps、作業色空間なし、
linear blend/linearizeなし、MFRなしで120セルを採取した。
未割当secondaryはcurrent0、future+1/+5、past−5すべてでPreRender error0、
空result/max rect、pixel checkout error0、null worldを返す。
保存checkpoint d2a5cbca6e23da31140e87cd2b33b1c62ba298f3 の時間計算を再利用した
単一probeで測定。今回の契約修正と親issue完了は別であり、#1748はOPENを維持する。

## 実測

同一probe: 17,920 bytes / SHA-256
`2dbff6dc3627a5d9abe87e7a38e36dd5f4e2b9e58db81352c96901756c01596c`。
AEの実loaded moduleとnativeで照合した。プローブの再buildは初回の1回のみ。
runtime/PiPL flagsは33555458/5120で一致。

32-word payloadの未割当期待値（M=0xffffffff、F=深度のPF format）:

```text
[0x554c5031,1,32,0, 0,0,0,0,0,0,0,0, 0,0,0,
 M,M,M,M,M,M,M, 516, M,M,M,M, 0,0,F,1,1]
F: 8=0x62677261, 16=0x36316561, 32=0x32336561
```

|観測|結果|
|---|---|
|AE全120セル|正常完了、各job後items/queue0。57契約/36画素/27baseline|
|AE画素36セル対独立baseline|294,912 sample、差0、32 bpc 0 ULP|
|修正前native対AE 81セル|72 exact / 9 mismatch、891 sample差|
|修正後native対AE 81セル|81 exact / 0 mismatch、663,552 sample差0|
|configured/timed missing等18セル|missing拒否12、供給成功6、契約確認|
|streamed primary 9セル|current成功3、未供給future拒否6、session clean|

修正前不一致は全深度の未割当offset−5/+1/+5のみ。
各セルword3がAE0/native4、word12がAE0/native0xffffffff、99 sample差。
first sample501（native白/AE0）、最大255/32768/1,065,353,216 ordered-word ULP。
これは契約を白黒画像へ符号化した距離。修正後は全word・全sampleが一致し、許容差なし。

AEはprimary/secondaryの時変sourceにも要求時刻の別patternを返した。
hostはstreamed primaryの未供給futureを拒否するため、primary-sequenceのfuture12セルを
host一致の81セルへ含めない。secondaryは時刻0/1/5の実画像を明示供給して比較した。
outside-spanのAE secondaryはこの条件では非空画像を返した。hostでは同じ静止sourceの
契約を比較し、hostがAE layer spanを実装したという主張には使わない。

元のPNGを直接両hostへ読むと16 bpcではprimary23/secondary26 sampleが最大1、
32 bpcでは3004/3049 sampleが最大1 ULP違った。これを合格へ丸めず記録した。
比較用primaryはAE raw RGBAを直接渡し、secondaryはPNG16の可逆符号化または
float32 TIFFで全sampleを保存。EXRは既存decoder非対応で一度失敗し、その記録も保持した。

## 一般修正

各dispatchのparameter型・NONE default・external bindingから未割当slot集合を作る。
primary、MYSELF default、設定済みsecondary、どこかの時刻に実sourceを持つslotを除外する。
temporal guardを抜けられるのはこの集合のslotだけ。currentのempty world形状は変更しない。
runtime側でもhosted bindingを再確認し、stale分類をmissing-frame成功へ変えない。
malformed request、時間scale0、null destination、容量上限、double checkin等の保護を保持。
static/timed入力のnative精度比較用に、既存native image入口と同じsession/containmentを
使う `render_native_image_with_timed_layers` を追加。decoderやfloat変換式は変更しない。

## 検証と再現

Release worker build成功。`--self-test-pf-checkout-intersection` にpast/future未割当、
primary/非layer/範囲外/configured missing拒否、timed supply、所有権を追加した。
既存probeテストは各深度assigned/unassigned×offset0/+1/+5/−5の24条件で3 passed。
workerを再buildしても独立DLLのprobeは古くならないため、probeのfreshnessは自身のsource/resourceに照合する。
関連self-test/PiPLは67 passed / 1 skipped。broker trusted_worker_stageは10 passed。
Rust workspace libは395+33+8 passed / 3 ignored（子実行1件を重複加算しない）。
上記skipはSDK ABI検査で、外部SDKを明示した別実行で1 passedを確認した。

実装とは別工程で全diffを慎重に自己レビューした（独立担当レビューではない）。
型・binding・defaultの分類、guardとownership、深度変換、raw比較と未比較範囲を確認。
assigned pastのAE観測を示唆するテスト説明を修正し、再読後の受理未対応0、却下なし。

```powershell
# TEMP/TMP/TMPDIRを専用scratchへ向ける。AEを追加起動しない。
pwsh -NoProfile -File tools/build-native.ps1 -Target aex_worker
target/minihost-build/aex_worker.exe --kind smart --self-test-pf-checkout-intersection
cargo test --offline --manifest-path broker/Cargo.toml -p aexcompat-broker --test trusted_worker_stage --jobs 2
# AEXCOMPAT_CARGO_PROFILE=debug、同profileのharnessをbuild済みにする
python -m pytest -q --run-built-artifact-tests --run-local-artifact-tests tests/test_pf_empty_layer_contract_probe.py
```

機械固有のraw/logと実loaded identityはWindows検証リポジトリのT47 durable dataに保存。
再採取・比較・再検証は同リポジトリ `tools/t47_capture.jsx`, `tools/t47_compare.py`,
`tools/t47_extra_checks.py`、詳細コマンドは `docs/T47_P04_RESUME.md`。

## 残件

指定5root/3,817 AEXでHarris Shutter/Motion Blur ML候補0、探索error0。
合法な同一binaryによる通常非空画像のexact検証は未完。本機全域の不在は断定しない。
親issueを自動closeする参照は付けず、原影響plugin検証の残件を保持する。
primaryの任意future供給やAE layer spanの追加実装は本変更の範囲外。

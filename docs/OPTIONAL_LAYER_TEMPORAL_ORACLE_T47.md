# #1748 / T47 temporal oracle 準備

status: blocked（導入担当によるprobe差替え・AE再起動待ち）

保存済みcheckpoint `d2a5cbca6e23da31140e87cd2b33b1c62ba298f3` の
64-bit中間値による時間計算と32-word画像payloadを再利用する。
最新main `ed29ab4933f71b48dcec7c02b630aa6191d9ed70` から開始し、
host本体のtemporal guard・world shapeは変更していない。

## 単一binaryの観測範囲

既存Optional Map（slot1）に次の制御を追加した。

|slot|制御|値|
|---|---|---|
|2|Checkout Target|1=secondary、2=primary|
|3|Time Step Offset|-5〜5、既定0|
|4|Probe Output|1=32-word契約値、2=checkoutされた全pixelのコピー|

primary currentをcheckout id0で確保して出力を非空に保ち、観測対象を別id1でcheckoutする。
観測時刻は `current_time + offset * time_step`。範囲外のA_longを拒否する。
全controlのparameter checkout/checkinはSMART_PRE_RENDERで収支を合わせる。
コピーmodeはerror/null/別寸法・別formatを拒否し、欠落入力を透明画像に変換しない。
失敗の契約値はmode1で取得する。従来のschema1/32-wordは維持する。

SDKの時間依存宣言に合わせてWIDE_TIME_INPUTをruntimeとPiPL双方へ追加。
flags=33555458、flags2=5120。default current-timeでも同じ単一binaryを使う。
深い先頭pixelのword23:27は従来どおり8-bit正規化値なので、deep精度は別途mode2の全sampleで検証する。

## native事前検証の事実

MSVC Release probeを1回build。17,920 bytes、SHA-256:
`2dbff6dc3627a5d9abe87e7a38e36dd5f4e2b9e58db81352c96901756c01596c`。
workerは上記mainのT41 postmerge artifact（SHA-256
`03b30cb000304ffcd1fea442368523fdc2976a1aa3bddef5fcde3168ff2cec39`）を再利用。
新たなworker buildやmerge後回帰の結果とはしない。

`olm-win-checks` の `tools/t47_prepare.ps1` / `tools/t47_probe.py` で
8/16/32 bpc、64×32、current0/step1/scale30/total300を実行した。
90セル=契約値66＋pixel24。pixel196,608値はauthored opaque PNGからのnative-depth変換と
完全一致、差0、最大差0（32bpcは0 ULP）、first mismatchなし。
PNGは画素ごとに異なるpatternで、current/futureには別patternを供給した。

|入力|current|future|
|---|---|---|
|未割当secondary|error0、null world|error4、pixel checkoutなし|
|configured-missing（time2だけ供給）|error4|time1要求はerror4|
|timed currentだけ供給|供給pixel|time1要求はerror4|
|timed futureだけ供給|error4|供給pixel|
|timed current/futureを別patternで供給|current pattern|future pattern|
|static secondary / explicit static primary|供給pixel|供給pixel|
|streamed primary（履歴未供給）|成功|error4、pixel checkoutなし|

±5はstatic primary/secondaryとunassigned secondaryの契約値も検証した。
suite/world/handle/parameter収支、guard、output、GPU未dispatchを確認。
既存checkout-intersection/pre-checkout-result self-testもpassed。
最初の検証器は画像APIをstreamed primaryと誤認して失敗した。実reportのstatic宣言と
API実装を確認し、streamed sessionを別入口として追加。失敗証拠も保持している。

## 未達・再開条件

これはnativeの観測でありAE同等性ではない。AE 25.1x68は稼働中でMCP応答を確認したが、
導入済みprobeは旧current-time版である。担当者による差替え・再起動後に
`olm-win-checks/tools/t47_capture.jsx` の120セルをMCPで取得する予定。
outside-spanのAE layerはconfigured-missingのhost transportとは別状態として記録する。

AE raw比較、一般修正の是非、全focused tests、PR/最新head CI/merge、postmergeの
bridge buildとT17 extraは未実施。AE証拠なしでguardを移動しない。
指定5 rootの3,817 AEXを再探索したがHarris Shutter/Motion Blur ML候補0、探索error0。
合法な同一実体による通常非空画像の比較がないため、親#1748はOPEN維持。
本機全域での不在やライセンス状態を断定しない。

probe build（scratch環境変数は担当ticketのdata配下へ設定）:

```powershell
pwsh -NoProfile -File tools/build-pf-empty-layer-contract-probe.ps1 -AfterEffectsSdk <external-sdk-root>
```

ローカルの別工程として全diffを慎重に自己レビューした。別担当reviewではない。
primary入口の区別と全sampleをauthored入力から照合する検証を確認し、受理未対応0。
未完scopeのためPR/CI/merge/issue closeは行わない。

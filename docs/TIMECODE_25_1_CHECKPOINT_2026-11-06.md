# #1263 / AE 25.1 の途中記録

status: blocked

この変更は font/style/pixel parity の修正完了ではない。未pushの作業checkpoint。

実測した Timecode は531400 bytes、SHA-256
`801780f7d891eac6549eed097601623781338fb1dac44414f972ccdb4d5057f8`。
過去の2025版531912 bytes / `41dabdd8…` とも、旧2026版とも異なる。
BEE 25.1.0.73 は SizeOfImage `0x1627000` / TimeDateStamp `0x6740074a`、
SHA-256 `1c00a8c32715bd07e30944e71323a3dbf9e77ebeaf02ddf3137dbc4ca5c38c68`。

既存mainではこのBEEを未対応として拒否し、元TimecodeのRENDERはerror512、画像なし。
静的観測で `pGetConstSourceItem` は `layer+0x27c8`、非constからconstへのslotは178、
`BEE_GetSourceTimeFormat(true)` は `layer+0x338` を読む。既存Ae2025レイアウトに一致する。
そのcandidate metadataだけ追加し、既存のexport/vtable/実呼出し検証は維持した。

Release worker build、broker integration10件、bee-scene-facade self-test成功。
元Timecodeの4つの8bpc画像を得たがAEと全て不一致。差数は19464 / 5544 / 32521 / 8043、
最大差は全て255。先頭の既定256×144/size36/t0は (223,14,R) AE0 / host51。
日本語AEのfont選択入力はMS Gothic/Regular、hostの未翻訳LStrはArial/Regularであり、
この比較を同一fontの一致判定に用いていない。

フォント入力を制御する外部oracle adapterを作成し、元nativeのArial4条件とraw一致を確認。
MS Gothicへ揃えても残差があり、font差だけでは説明できない。
adapterの実AE設置・元AE出力との透過性検証・非既定styleのoracleが必要。
16/32bpc AE captureは保存済みだが本AEXは8bit dispatchであり、深度変換を含む比較は未完了。

issueはopenを維持する。一般化したフォント/描画修正、全受入条件、PR/CI/merge、
merge後OLM回帰は未実施。このmetadata追加のみを完了PRとして提出しない。

# Persistent Data: 保存21セルに基づく部分修正

Refs #886。全変換表・永続化は未解決で、issueを閉じる変更ではない。

## 事実と対象範囲

AE 2025 25.1x68、Software、8 bpc transportでPersistentDataSuite3を測定した。
API自体はbpcを引数に取らず、16/32 bpcの画像互換を証明するものではない。
各測定はGUID付き専用sectionを使用し、GetPrefsDirectoryの返却先には書いていない。

計画2,128セル中、保存された返値はcell0–19とcell26の21セルのみ。
14セルはmissing→Data、6セルはmissing→DataHandle、1セルはmissing→String。
cross-kind setter/getterのAE値は未測定。旧prefixの最終section cleanupも未確認。

|観測|旧hostとの差|修正|
|---|---|---|
|missing→DataHandle、NULL defaultから空値、再読出しもerror3/NULL|cell14/15/16/18のfirst/second/readback、12 error差（最大3）|空の結果はerror3/NULL。欠落keyへ空defaultを書き込む副作用を保持|
|cell26、capacity1でDEFAULT_Aを保存、capacity64のreadback|NUL後54 byteがAE00/hostA5、最大byte差165|収まる文字列の出力は未使用領域もゼロで埋める|
|GetPrefsDirectory|AEは末尾separatorなし、hostはあり|host専用Preferencesディレクトリの末尾separatorを除去|

保存21セルは修正前16 exact / 5 mismatch、13 field差・54 byte差。
修正後21 exact / 0 mismatch、field差0・byte差0・最大差0・first mismatchなし。
float値のAE対象は0件であり、ULP0はfloat互換の証明ではない。
prefs文字列はhost固有。パス自体の同一性を要求せず、UTF-16のNUL終端・正確な
handle長・末尾separatorなしを検証する。実AEのprefs treeはhostの書込先にしない。

## クラッシュと未確定事項

cell20（missing→String、capacity0、NULL default、actual-size pointerあり）は
AEプロセスが消失。最後のflushはcall14 / first / AEGP_GetString / before。
after・返値・cleanupはない。API内部かログ直前の停止かは区別できない。
この条件はAE-crashとして保持し、数値比較へ含めない。

cell26は24 APIがすべてerror0でreturnし、key cleanup・Software render完了後に
「After Effects 続行できません: 内部構造破損(seq) ( 25 ､ 8 )」を観測した。
最後はcall24 / final_cleanup / ReleaseSuite / after / error0、end/calls24/cells1。
GetString内クラッシュとは分類しない。原因は未確定で、API保存値とセッション正常性を分ける。
owner指示により以降のAEセル実行を停止した。未実行2,106セル。

hostはゼロ容量Stringをerror3で、副作用・出力変更前に拒否する。これは一般的な
引数形状の安全方針であり、未測定条件のAE返値を捏造するものではない。
zero-fillによる無制限の書込を防ぐため、String出力容量は既存value上限1 MiB + NULまで。
超過はerror4で副作用前に拒否する。非NULL容量、空結果、通常文字列に対する一般実装であり、
fixture名やcell番号による分岐はない。空DataHandleとString余白の一般化は保存観測からの推論。
容量不足で複数byteある出力、cross-kind変換、永続化のAE契約は未確定のまま。

## 証拠identity

生ログはT46担当のdurable dataに保持。公開文書には私有絶対path・SDK・binaryを含めない。

|artifact|SHA-256|
|---|---|
|実loaded probe-v3（123904 bytes）|`1a4ed19fa626df1c4519276da9122a14eeea28eed63f82022f7e548b3b717701`|
|cell20 calls.pending|`f4666bad036821563e564ea7dca1018dcb154eacb1eed6cbc9930de130c31e83`|
|cell26 calls|`f09be9fdb904587c947dd72450995be748871202834eadc11f54db856b631fe4`|
|cell26 jsonl|`2377bf6e98109db9120d726238486f3e7ceb5d6069416f2a6dc604775fcac975`|
|修正後native manifest|`948b8642eda7c48e15a629d00fc3077a1156db31f2e5f528baf4ce492413ec78`|
|保存21セル比較summary|`8885d4c078981b5aec503b8b9c55c4f8221b21e7f1dcf33f1105258b0180f420`|

## 検証

- Release worker build成功。native全2,128セル完走、key残存0、guard/suite/world収支保持。
- `tests/test_aegp_persistent_data_suite3.py` とsuite4: 2 passed（各discovery/classic/smart）。
  self-testはerrorだけでなくempty default後のkey存在と再読出し、全出力byte、guard、
  拒否時の非書込、PAGE_NOACCESSの拒否、UTF-16形状を実table経由で検査する。
- broker `trusted_worker_stage`: 10 passed。
- Rust workspace lib: broker395 passed/3 ignored、host-core33 passed、FFI8 passed。
  broker内の子テスト実行1 passedもログにある（重複集計しない）。
- T46比較/trace/scheduler tests: 18 passed。保存証拠・native manifest再検証成功。
- 実装とは別工程の慎重な自己レビューで、default副作用、bound、SEH、所有権、
  未測定条件の表示を再確認。独立担当のレビューではない。

再実行時はTEMP/TMP/TMPDIRとpytest --basetempを担当のscratchへ設定する。

```powershell
pwsh -NoProfile -File tools/build-native.ps1 -Target aex_worker
python -B -m pytest -q -p no:cacheprovider --basetemp <scratch>/pytest --run-built-artifact-tests tests/test_aegp_persistent_data_suite3.py tests/test_aegp_persistent_data_suite4.py
cargo test --offline --manifest-path broker/Cargo.toml -p aexcompat-broker --jobs 2 --test trusted_worker_stage
cargo test --offline --manifest-path broker/Cargo.toml --workspace --lib --jobs 2
```

PR/latest-head CI/merge/postmerge T17 extraはまだ未実施。全P07受入・#886全体の完了を主張しない。

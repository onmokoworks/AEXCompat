# AEX移植解析ガイド

AEX execution porting dossierは、変更していないWindows x64 AEXをUnicorn
workerで実行し、実際に通った経路、Win64引数、戻り値、メモリの変化を
module-relative RVA付きJSONとして記録する機能です。通常renderではtrace hookを
導入しないため、解析が必要なときだけ明示的に使用します。

このdossierは「AEXの全コードを静的に説明するもの」ではありません。指定した
画像とパラメーターで実際に通った経路の観測結果です。

## 最短の使い方

setup selectorだけを観測する場合:

```sh
aex-guest-worker trace-selector plugin.aex PARAMS_SETUP > params-trace.json
```

実画像をrenderしながら観測する場合:

```sh
aex-guest-worker render-trace-png \
  plugin.aex input.png output.png \
  Amount=25 > render-trace.json
```

`render-trace-png`は通常のsetup後、Classic `RENDER`または
`SMART_RENDER`を記録します。Smart Renderでは通常、`SEQUENCE_SETUP`、
`FRAME_SETUP`、`SMART_PRE_RENDER`、`SMART_RENDER`、`FRAME_SETDOWN`、
`SEQUENCE_SETDOWN`ごとの記録が`execution_traces`へ入ります。生成画像は
`output.png`、通常のrender reportとtraceは標準出力のJSONへ入ります。

`--watch`を1件以上指定したrenderは、全basic-block/branch censusを行わない
低オーバーヘッドの`checkpoint` captureになります。指定したdirect call-siteと
そのcall-return直後だけをhookし、通常のSmart Render経路を維持したまま
entry/return snapshotを取得します。JSONの
`trace_configuration.capture_mode`は`checkpoint`になり、`basic_blocks`と
`branch_edges`は空です。watchなしの従来traceは`full_trace`です。

## 特定の値を追う

関数の入口とreturnで、引数ポインター先の変化を保存できます。

```sh
aex-guest-worker render-trace-png \
  plugin.aex input.png output.png \
  --watch function=0xcce0,arg=rcx,size=16,when=entry+return \
  --watch rva=0x350b,register=r9,size=64 \
  --watch-output-pixel 92,841 \
  Amount=25 > render-trace.json
```

### `--watch`構文

```text
--watch function=<function-rva>,arg=<register>,size=<bytes>[,when=entry+return][,occurrence=<n>]
--watch rva=<call-site-rva>,register=<register>,size=<bytes>[,when=both][,occurrence=<n>]
```

- `function`は監視対象関数の入口RVAです。trace対象selector自身も指定できます。
- `rva`はcallまたはtail-callを行うinstructionのRVAです。
- `function`と`rva`はどちらか一方だけを指定します。
- `occurrence=<n>`はn回目だけを取得するため、実質的なhit上限として使えます。
- `size`は1..4096 bytesに制限されます。`deref=<offset>`を指定すると、register
  またはstack引数の`+offset`に格納されたpointer先を取得します。
- checkpointも通常のguest selector timeout内でfail-closeします。
- `arg`と`register`は同義です。`rcx`、`rdx`、`r8`、`r9`、`rax`、
  `stack5`～`stack8`（または`5`～`8`）を指定できます。
- `size`は1～4096 byteです。
- `deref=<offset>`を指定すると、register/stack引数そのものではなく、
  `引数 + offset`に格納された64-bit little-endian pointerの参照先を監視します。
  offsetは0～4096 byteです。OpenCV `Mat`のdata fieldなど、headerと実データが
  分離した構造を同じentry/return境界で追う用途を想定しています。
- `when`は省略可能です。指定する場合は`entry+return`または`both`です。
- `occurrence`は省略可能な1始まりの呼び出し番号です。例えば
  `occurrence=2113`は、同じwatchに一致する2113回目だけを記録します。
  番号はselectorごとのexecution trace内で数え、選択前の呼び出しはmemory
  witness上限を消費しません。
- RVAは`0x`付き16進数または10進数で指定できます。
- `--watch`は複数回指定できます。
- `--watch-output-pixel x,y`は出力pixelの前後値と画像内位置を記録します。

`rva=`はメモリアドレスではなくinstructionのRVAです。監視するメモリのアドレスは、
その時点の指定registerまたはstack引数からworkerが取得します。`deref`指定時は、
そこからpointer fieldを1段だけ安全に読み取ります。fieldまたは参照先が未mapなら
snapshotはunreadableとして記録され、workerをクラッシュさせません。

## JSONの読み方

最初は次の順に見ると、巨大なJSONを頭から読む必要がありません。

1. top-levelの`render_error`が成功を示すことと、各
   `execution_traces[].truncation`が空であることを確認する。
2. `timeline`と`functions`で、通った関数とhost callbackの大枠を見る。
3. `events`の`pc_rva`、`target_rva`、`call_kind`でcall siteを特定する。
4. `exemplars.numeric_ranges`で変動する整数・float・double引数を探す。
5. 必要な関数へ`--watch`を追加し、`memory_witnesses`の`before`、`after`、
   `changed_ranges`を確認する。

主要フィールド:

- `image_sha256`、`preferred_image_base`、`entry_export`: AEX本体の識別情報。
- `events`: 初回観測順のcall、return、import、callback、定数アクセス。
  反復callは`observed_count`へ畳まれますが、先頭・末尾・distinct exemplar、
  fingerprint、数値範囲がbounded形式で残ります。
- `arguments`: `rcx`、`rdx`、`r8`、`r9`。
- `xmm_arguments`: `xmm0`～`xmm3`のraw値、float32 lane、float64 lane。
- `stack_arguments`: 第5～第8引数。`call_id`がcallと`rax`／`xmm0` returnを
  結びます。
- `memory_witnesses`: entry/return時のbounded hex、SHA-256、整数・浮動小数点
  解釈、pointer chain、変更範囲、読取失敗理由。
- `basic_blocks`、`branch_edges`: 実行されたcontrol flow。
- `modules`: AEXとemulated import-stub module。DLLを実際にロードしたという
  意味ではありません。
- `worker_build_identity`、`trace_configuration`、入力PNG SHA、パラメーター:
  観測条件の再現情報。
- `truncation`: budgetを超えて欠落したcategoryと件数。空でなければ完全な
  traceとして扱わないでください。
- `state_changes`: selectorによるhost-visible ABI fieldの変化。

たとえばcallだけをざっと見るには:

```sh
jq '.execution_traces[] |
  {selector, calls: [.events[] |
    select(.kind == "guest_call" or .kind == "tail_call") |
    {pc_rva, target_rva, call_kind, observed_count}]}' render-trace.json
```

memory witnessを確認するには:

```sh
jq '.execution_traces[] |
  {selector, witnesses: [.memory_witnesses[] |
    {watch_id, function_rva, pc_rva, changed_ranges,
     before_sha: .before.sha256, after_sha: .after.sha256}]}' render-trace.json
```

## GhidraでRVAへ移動する

dossierの`pc_rva`や`target_rva`は、AEX image先頭からのRVAです。Ghidraで
AEXをPEとしてimportし、dossierの`preferred_image_base`とGhidraのimage baseが
一致していることを確認してから、次のアドレスへ移動します。

```text
Ghidra address = preferred_image_base + RVA
```

例としてimage baseが`0x180000000`、`target_rva`が`0xcce0`なら、
Ghidra上のアドレスは`0x18000cce0`です。dossierの`functions[].entry_bytes`と
Ghidra上の先頭16 byteも照合すると、別buildやbaseの取り違えを検出できます。

1. `timeline`または`functions`から対象RVAを選ぶ。
2. Ghidraで上式のアドレスへ移動する。
3. `call_id`でcallとreturnを対応させ、引数register、XMM、stack値を見る。
4. 値の意味が不明なら、その関数またはcall siteへ`--watch`を設定して再実行する。
5. `changed_ranges`とtyped valuesから、移植側で再現すべき入出力契約を絞る。

## 条件を変えて比較する

画像やAmountを変えた2つのdossierを、巨大なJSON全体を表示せず比較できます。

```sh
python3 tools/diff_aex_dossiers.py \
  before.json after.json -o dossier-diff.json
```

`trace_diffs`はselector単位で、return、event観測回数、memory witness内容、
truncationの差を報告します。差分自体もboundedであり、上限を超えた場合は
`*_truncation.truncated`と`dropped_count`へ明示されます。

## English summary

The Unicorn worker can record an opt-in, module-relative execution dossier for
an unchanged Windows x64 AEX. Use `render-trace-png` for a real render,
`--watch` for bounded entry/return memory snapshots, and
`tools/diff_aex_dossiers.py` to compare two cases. RVAs map into Ghidra as
`preferred_image_base + RVA`. The dossier describes only the path executed by
the supplied image and parameters; use multiple representative cases for
branch-heavy effects.

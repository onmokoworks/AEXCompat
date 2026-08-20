# 残り 2 本 (PSL_Adjustments / Particle_Playground) の観測 (2026-08-19)

issue #1302 (PSL_Adjustments の `frame_error:14`) と #1289
(Particle_Playground の `frame_error:516`) の調査記録。母集団は AE 2026
`Support Files\Plug-ins\Effects` を `render_sweep` に引数で渡した 304 AEX、
`--depth` 既定 (8)、size/time/frames も既定。base は `origin/main` の
`b7486cab`。観測 (trace / cdb / 逆アセンブル / AE 実機) と推論を分けて書く。

## 1. 手順

- trace: worktree で worker 3 exe を build、`AEXCOMPAT_EXTENDED_DIAG=1` +
  `render_sweep --filter <name>`
  (`docs/SWEEP_INVESTIGATION_WORKFLOW_2026-08-17.md` §2)。
- 動的: `cdb -o -cf <script> render_sweep.exe --filter <name> ...` で worker を
  子プロセスごと debug し、`sxe -c "... bu VCRUNTIME140!_CxxThrowException ..." cpr`
  で C++ throw を全部拾う。throw の第 1 引数 (`rcx`) が投げられたオブジェクト、
  第 2 引数が `ThrowInfo`。
- 静的: Ghidra `AEXCompat.gpr` (`PSL_Adjustments.aex` / `Particle_Playground.aex` /
  `BEE.dll` は import 済み、headless + `tools/ghidra/DumpDecomp.java`)。
- AE oracle: `tools/capture-ae-reference.ps1`。入力は sweep が使うのと同じ
  256x144 の単色 RGBA (32,64,128,255) PNG (file sha256
  `b90064f380218150b09114abd5c38cb687bb54dea49d985cd84663da73e96a59`)、
  AE 26.3x87、bpc 8、default param、frame 0。
- host 出力: `render_sweep --dump-frames <dir>` の raw pixel を AE の PNG の
  decoded RGBA と比較 (host は straight alpha、AE の PNG は premultiplied)。

## 2. Particle_Playground — `frame_error:516` (#1289)

### 2.1 観測

`stderr_tail` が実際には先頭 64 KB だったため (#1290、下記 §4)、まずそちらを
直してから trace の末尾を読んだ。この調査のために足した operand-world trace
(当時の marker 名は `extended_diag:unresolved_world`。最終形は §2.2) が出す
world の形そのまま:

```
extended_diag:fill -> 0
extended_diag:unresolved_world transform_world_source world=0000000B728FB450
  flags=0 data=000001E9EBE041C0 rowbytes=416 width=4 height=4
  reserved_long4=0000000000000000 registry_known=0
extended_diag:unresolved_world transform_world_destination world=0000000B728FD768
  flags=0 data=000001E9ED301040 rowbytes=1024 width=256 height=144
  reserved_long4=0000000B728FD760 registry_known=1
stage:callback_denied callback=transform_world reason=source_world_unresolved
extended_diag:transform_world -> 516 (callback_error)
stage:classic_render_end error=516
```

(`unresolved_world` の 2 本は読みやすさのため折り返してある。実際は 1 行。
これは調査時点の build の出力で、最終形とは 3 点違う: marker 名は
`extended_diag:operand_world`、`reserved_long4` は出さない、そして
always-on の `stage:callback_denied` が**先**に出る — §2.2 の理由による。)

- destination は host が渡した frame output world (256x144、rowbytes 1024、
  registry 既知、`reserved_long4` に host の `PF_World` facade がある)。
- source は **4x4 / rowbytes 416** の `PF_EffectWorld`。416 = 104 px * 4 なので、
  幅 104 px の別バッファの一部を切り出した sub-world。pixel base は host が
  発行したものではなく (`registry_known=0`)、`reserved_long4` も 0。
  session 全体で `new_world` / `dispose_world` の trace は 0 件。
- つまり Particle_Playground は自前の sprite バッファの上に stack 上の
  `PF_EffectWorld` を組み立てて TRANSFORM_WORLD に渡している。#1289 本文の
  「BM (`BM_NWorld`) 経由で確保している」という当たりは、**BM 経由かどうかは
  この観測からは言えない** (host が発行していないことだけが言える)。sprite の
  出所自体は未確定のまま。

### 2.2 実装

`worker_pf_world_transform_runtime.cpp` の copy callback には既に
foreign-operand admission (#1037 / #1069) がある: registry が知らない world を
「宣言された stride / 高さで境界を切る」ことで受け、(a) registry が知っている
参照が解決に失敗した場合と (b) pixel base が host 発行の allocation の場合は
引き続き拒否する。AE の `PF_COPY` が「呼び手が記述できる任意の
`PF_EffectWorld`」を受けるのと同じ理由で、`PF_TRANSFORM_WORLD` も同じ。

そこで `resolve_copy_worlds` / `resolve_foreign_copy_world` を
`resolve_operand_worlds` / `resolve_foreign_operand_world` に改名し、
TRANSFORM_WORLD からも同じ経路を通した。fail-closed な性質は変えていない:

- 片側は必ず registry で解決できていなければならない (pixel format の anchor)。
- registry 既知の参照が解決に失敗したもの
  (`world_safety::dispatch_world_reference_known`)、host 発行 allocation の base
  (`world_pixels_owned`) は今までどおり拒否
  (`resolve_foreign_operand_world` の入口の gate)。
- 受理後も `extent_over_4096` / `rowbytes_underrun` / pixel format 一致は従来どおり。
- source の読み出しは従来から「宣言された rowbytes で 1 行ずつ `packed_row`
  だけ copy」なので、宣言を越えて読む経路は増えていない。

admission は**対称**で、destination 側が foreign になることもありうる。そちらは
host が**読んで書く**側 (blend が既存の destination pixel を混ぜる。copy
callback は読まない) だが、読み書きとも同じ宣言値で切られている:
`resolve_world` (= `bounded_typed_world`) が null base / 1..4096 外の
width・height / `width * pixel_bytes` 未満の rowbytes を先に落としており、
書き込みは `clip_legacy_rect(destination_rect, width, height)` の範囲を
その stride で歩くだけ。`copy_world8` が既に持っている latitude と同じで、
広げたわけではない。

一方で「source の宣言 extent が host 側の allocation を決める」ことになる:
`source_copy` は `width * height * pixel_bytes` (4096 上限・float で最大
256 MiB) で、matrix が実際に舐める範囲に関わらず宣言された extent 全体を
1 度 copy する。誤った宣言は「間違った絵」ではなく `bad_alloc` → 4、または
Job Object による kill になる。

`PF_TRANSFER_RECT` は registry-only のままにした。TRANSFORM_WORLD の
無変換版で同じ議論が当てはまるが、要求している plug-in を観測していないため
(計測せずに admission を広げない)。

診断のために足した trace は `extended_diag:operand_world` で、そのまま残した
(拒否理由の identifier だけでは「4x4 / rowbytes 416」は伝わらない)。
`AEXCOMPAT_EXTENDED_DIAG` のときだけ出る。always-on の
`stage:callback_denied` を**先に**出してからこの行を出すので、mis-declare
された world の walk が落ちても拒否の帰属は残る。読む offset は 16..44 に
限ってある: `read_world_layout` が全経路で 24..44 を deref 済みなので、
足しているのは 16 の flags だけ — 既に読んだ offset より**手前**であって
先ではない (`bounded_typed_world` も 16 を読むが、それは foreign fallback に
到達した経路だけで、`both_worlds_unresolved` はその手前で返る)。
`reserved_long4` = offset 0x50 は 44 の 36 byte 先なので出していない。
なお同じ header の `diag_probe_arg` は deref 前に `VirtualQuery` している。
こちらは既読 offset より手前で同じ allocation 内なので付けていない
(同じ page とまでは言えない。offset 17..24 に page 境界が来る = offset 16 と
offset 24 が別 page になると、flags は誰も触っていない page に載る。実際の `PF_EffectWorld` は 120 byte 1 個なので
起こらず、起きても always-on marker を出した後の contained fault で済む)。

どちら側が置けなかったかは resolver 自身が返す (`OperandRefusal`)。もう一度
registry に聞き直す実装にしていたが、registry は意図的に cross-thread
(#1299) なので 2 回の問い合わせの間に別 thread の scope 出入りで答えが変わり
うる。両方置けなかった場合の理由は `both_worlds_unresolved`。

self-test: `verify_transform_world_foreign_operand(admitted)` を追加し、
`tests/native/worker_pf_bad_callback_param_selftest.cpp` から 3 構成
(`world_pixels_owned` が false / true / null) で回す。各構成で
(a) atlas の上に載せた foreign source (登録済み destination と組) が受理され、
    宣言 stride どおりの pixel が書かれること (packed stride で読んでいたら
    2 行目の値が変わるので、bound 自体もここで固定される)、
(b) 逆向き (登録済み source + foreign destination、host が**書く**側)、
(c) 両側とも未登録なら anchor が無いので拒否されること、
を確認する。gate が閉じている構成では (a)(b) が拒否になる。**どの構成でも**
`stage:callback_denied` の reason まで読む: 開いている構成では (c) の
`both_worlds_unresolved`、閉じている構成では加えて (a) の
`source_world_unresolved` と (b) の `destination_world_unresolved`。516 だけを
見ていると side を名指しする ternary の腕を入れ替えても test が通ってしまう
(実際に review で入れ替えて落ちることを確認した)。gate が閉じている構成では
`extended_diag:operand_world` の行 (`rowbytes=104 width=2 height=2
registry_known=0`) も読む。harness は `AEXCOMPAT_EXTENDED_DIAG` を自分で
立てる。copy 側の `verify_copy_foreign_world_gate` と同型。

### 2.3 結果 (AE oracle)

`Particle_Playground.aex`: `frame_error:516:PF_Err_BAD_CALLBACK_PARAM` →
`rendered` (256x144 @ origin (0,0))。

AE 26.3x87 の参照出力 (`ADBE Playgnd`、default param、frame 0、8 bpc、
`loaded_aex_identity` = verified、`effect_provenance` = verified、
output PNG sha256 `be9535f05a8048391b50c712c4cd041f330753e5c13f14dbddf32e369fa8f92a`)
と比較して、**36864 pixel すべて一致 (max channel delta 0)**。host は straight
alpha、AE の PNG は premultiplied なので host 値を premultiply
(`round(c*a/255)`) してから比較した。赤い particle 42 px + 透明 36822 px で、
alpha のヒストグラムまで一致する。

## 3. PSL_Adjustments — `frame_error:14` (#1302、~~未解決~~ → **解決済み**、§5 と §3.2 の追記を参照)

### 3.1 観測

sweep が instantiate しているのは PiPL 6 entry のうち `EffectMainPHOTO_FILTER`
(cdb の stack で確認、param 4 本も Photo Filter と一致)。

`cdb` で worker の C++ throw を全部拾うと、frame が失敗する直前の throw は
次の 1 本 (thread は plug-in 所有の PSL async executor):

```
VCRUNTIME140!_CxxThrowException
BEE!BEE_WorkQueue_RegisterListener+0x46e617        (= BEE.dll RVA 0xc8c2f7)
BEE!PSLWorldAdapter<PF_Pixel8>::MakeEmptyPSLImageWH+0x102
PSL_Adjustments+0x4787
PSL_Adjustments+0x611f
PSL_Adjustments+0xc6d5
PSL_Adjustments+0xca0d
U!U_SuspendContext::ExecuteStatic+0xa9
dvacore!dvacore::config::ErrorManager::ExecuteFunctionWithTopLevelExceptionHandler+0x21
dvacore!dvacore::config::ExecuteTopLevelFunction+0x99
dvacore!dvacore::threads::CreateAsyncThreadedExecutor+0x1a75
```

投げられたオブジェクトは `BRVException` で、メッセージ文字列は
**`couldn't init CACE`** (`da poi(@rcx+18)` で実測)。

その直後に dispatch thread 側で `int 14` が投げられる:

```
VCRUNTIME140!_CxxThrowException      (rcx -> 0000000e)
PSL_Adjustments+0xc417
PSL_Adjustments+0x9232
PSL_Adjustments!EffectMainPHOTO_FILTER+0x2c
```

`PSL_Adjustments+0xc417` は `FUN_18000c010` (SMART_RENDER の本体) の中の
`if (CallOnThreadedExecutor(...) != 0) throw` で、14 は
`U_SuspendContext::CallOnThreadedExecutor` の戻り値。つまり
**14 は dvacore の top-level exception handler が上の BEE の throw を写した値**
(推論: handler の内部は読んでいない。throw と 14 の対応は 2 回の run で一貫)。

`couldn't init CACE` を投げているのは BEE.dll の `FUN_180c8c2a0` (RVA
0xc8c2a0)。この関数の throw はこれ 1 本しかない:

```c
if (DAT_1816a51a8 == 0) {                 // BEE 自身の ACE dispatch table
  cVar1 = FUN_180c8bde0();                // 遅延解決
  DAT_1816a51a8 = &DAT_1816a4c60;
  if (cVar1 == 0) DAT_1816a51a8 = 0;
  if (DAT_1816a51a8 == 0) throw BRVException("couldn't init CACE");
}
```

解決の実体は `FUN_180c8ab60` → `FUN_180c56f10(names, 0xa9, "ACEInterface2", table)`
で、169 (0xa9) 本の ACE proc を **BEE 自身が持つ BIB resolver**
`DAT_1816a13e0` (RVA 0x16a13e0) 経由で引く。resolver が null なら 1 本も引かずに
0 を返す。

`DAT_1816a13e0` に書く関数は BEE.dll 内に 1 つだけ (`FUN_180c568a0`) で、
**それを呼ぶのは `BEE_Birth` だけ** (Ghidra の callers 検索。BEE.dll は
`SetBIBProcAddress` 系の export を持たない — dumpbin /EXPORTS で確認):

```c
uVar8 = COR_GetBIBAddressProc(&local_178);
if (uVar8 == 0) {
  FUN_180c568a0(local_178);   // BEE の BIB resolver をここで初めて立てる
  ...
}
```

一方 PSL_Adjustments.aex **自身**の同型の CACE glue
(`FUN_1800268a0` / `FUN_180025160`、同じ 169 本の `ACEInterface2` proc 表) は
成功している: host の `AEFX Text BIB Suite` から resolver を受け取っているため。
issue #1302 本文の「169 本の `bib_resolve` が全部 non-null」はこの plug-in 側の
解決で、**BEE 側は 1 本も引いていない** (trace に 169 本ぶんしか出ない)。

### 3.2 原因の見立てと、~~なぜ host 内で閉じないか~~ (閉じた。下の追記を参照)

以下の連鎖のうち **観測**は「host が `BEE_Birth` を呼んでいない」「BEE の BIB
resolver を書くのは `BEE_Birth` だけ」「投げられた例外は
`BRVException("couldn't init CACE")`」の 3 つで、「だから `frame_error:14` に
なる」は**推論**。反証 (BEE の resolver を立てて 14 が消えることの確認) は
行っていない。

> **追記 (2026-08-20、#1439)**: この反証は実施済み。resolver を立てると 14 は
> 消える (推論は当たり)。ただし「host が `BEE_Birth` を呼んでいない」ことが
> 原因という部分は**外れ**で、実際は `BEE_Birth` を呼んでも resolver 設置に
> 到達しない。§5 と
> `docs/SUPPORT_LIBRARY_BIRTH_SEQUENCE_2026-08-18.md` §9 を参照。

- 観測 (逆アセンブルと cdb で確かめられる範囲): このホストは `BEE_Birth` を
  呼んでいない
  (`initialize_process_support_libraries` の birth 表は `BEZ_Birth` /
  `FILE_Birth` / `M_Birth` / `RND_Birth` / `VAL_Birth` / `PLUG_Birth` /
  `COR_Conception`、加えて `U_Birth` と `PF_Birth`。
  `docs/SUPPORT_LIBRARY_BIRTH_SEQUENCE_2026-08-18.md` §1 の AE の列では
  `BEE_Birth` は tail 側にある)。そのため BEE の BIB resolver が立たず、
  BEE の CACE 表が解決できず、`MakeEmptyPSLImageWH` が投げる。
- `BEE_Birth` の signature は
  `int BEE_Birth(unsigned char const*, std::map<std::string, dvacore ustring, ...> const&, unsigned int, int, bool)`
  で、resolver を立てる行に到達するまでに `SND_InstallProcs` x2 /
  `FLT_InstallProcs` x18 / `PIN_SetCallbacks` / `BEE_Globals::ProjectBirth` /
  dvacore の class 登録などが全部成功している必要がある。AE の列では
  `PLUG` / `TDB` / `PF` / `P` / `TXT` / `TDL` / `PREM` / `PIN` / `SND` / `OM` /
  `MSK` / `FLT` / `PR` の birth の**後**に来る。
- さらにこのホストは BEE object の **facade** を自前で持っている
  (`docs/BEE_SCENE_OBJECT_ABI_2026-08-17.md`)。実 BEE の global を起こすことと
  facade の共存は測っていない。
- したがってこの枝は「birth 列の tail をまとめて実装する」規模の作業で、
  この unit では閉じない。#1302 に上記を記録して open のまま残す。
  推測で `BEE_Birth` の引数 (MSVC `std::map` の内部表現) を組み立てて呼ぶことは
  していない (CLAUDE.md の「推測実装はしない」)。

### 3.3 併せて観測 (未解決、実害不明)

同じ cdb run で、frame が失敗する前に次の 2 本の first-chance throw がある:

```
PREF!PREF_Birth+0x643            <- PREF!PREF_GetPrefHandle+0x27
  <- COR!COR_IsPSLInitialized+0x9d2 <- AdobePIE!pie::version_0::initialize
AdobePIE!PSLSetImageSlices+0x97af (AdobePIE 内部)
```

`PREF_Birth` は AE の birth 列 step 7 にあり、このホストは呼んでいない。
export 名だけの symbol 解決なので `+0x643` が本当に `PREF_Birth` の中かは
確定していない (推論)。この 2 本は AdobePIE の PSL 初期化中に出て、その後
`MakeEmptyPSLImageWH` まで到達しているので、14 の直接原因ではない。

## 4. 併せて直したもの — `stderr_tail` が先頭だった件 (#1290)

`broker/crates/broker/src/windows_process.rs` の `STDERR_CAPTURE_LIMIT`
(64 KiB) が `diagnostics.rs` の `MAX_STDERR_TAIL_BYTES` (64 KiB) と同値で、
worker stderr の**先頭** 64 KB しか取り込んでいなかったため、64 KB を超える
trace では `stderr_tail` に末尾が入らない。§2.1 の観測はこれを直すまで
取れなかった。

- reader に retention を持たせ、stdout は従来どおり先頭保持
  (bounded JSON report なので末尾を落とす方が読める)、stderr は**末尾保持**に。
- stderr の取り込み上限を 128 KiB (= `MAX_STDERR_TAIL_BYTES` の 2 倍) にした。
  両方 64 KiB のままだと 2 つの上限が打ち消し合い、`diagnostics.rs` 側の
  行境界での切り出しと `[truncated to the last N bytes]` マーカーが一度も
  効かない。**大きく上げてはいない**: stdout と違って stderr は plug-in 自身が
  書ける channel (`AEXCOMPAT_EXTENDED_DIAG` では plug-in の stdout もここに
  合流する) で、`redact_windows_paths` は敵対的入力に対して capture 長の
  superlinear (実質 O(n^2)) なので、この上限はその作業量の上限でもある。
  → その superlinear 自体は範囲外として #1306 に起票した。
- redaction (`redact_windows_paths`) は**先頭**を残して切るので、末尾保持の
  capture をそのまま通すと 1 段下で同じバグが再発する: `from_utf8_lossy` は
  不正 byte を 3 byte の置換文字にするため、非 UTF-8 を吐く worker では
  本文が上限の最大 3 倍に膨らみ、先頭から切ると「末尾の最初の 1/3」だけが
  残る。retention が Tail のときは redaction を非トリムで回し、そのあと
  **末尾**を残して切るようにした (`redact_capture`)。redaction が歩く本文は
  最大で上限の 3 倍に収まり、ループ自体は `char` 単位で回るので試行回数は
  capture の byte 数 (= 上限) 以下。これは**入力**の上限であって作業量の上限では
  ない (redaction 自体は #1306 の superlinear)。上限を 64 KiB から 128 KiB に
  したことで、その worst case は約 4 倍になる — 定数倍であって青天井ではない、
  というのがここでの主張。膨張分の U+FFFD は superlinear 項そのものにも寄与し
  ない (quote escape の backslash 再走査にも marker 検証の drive letter にも
  当たらない)。
- **末尾保持の front-drop は行境界に揃える**。`redact_windows_paths` は path を
  `X:\` の頭でしか認識しないので、drop が path の途中に落ちると drive letter が
  消え、残りの private path が redaction を素通りして report に載る
  (`selftest.rs` と `l2.rs` と `render_session/discovery.rs` の stderr は
  `AEXCOMPAT_EXTENDED_DIAG` に関係なく report に入る)。Windows path は改行を
  含まないので、行頭から始まる capture は必ず完全な path だけを含む。cut が
  既に改行の直後なら何も余分に捨てない。cut より先に改行が 1 つも無い capture は
  丸ごと捨てる (`extended_diagnostics_stderr_tail` が巨大 1 行に対して空を
  返すのと同じ判断)。この「丸ごと捨てた」状態は**持ち越す**必要がある:
  捨てた時点で行の途中なので、次に読んだ分も行の途中から始まる。残りが上限に
  収まると以後 drop が走らず、整列の機会が二度と来ない (review で実際に
  private path が素通りする例を作って確認した)。`Capture` の
  `awaiting_line_start` がその債務で、read loop の最後に必ず精算する。
  前を捨てる箇所は 2 つ (`drop_front_to_bound` と `resume_at_line_start`) あり、
  どちらも「行頭に揃える」か「債務を立てる」のどちらかで終わる。精算点は
  `finish_capture` 1 箇所だけで、capture が `reader` を出る前に必ず通るので、
  債務が read loop より長生きすることはない。整列を外すと unit test 3 本と
  integration test 1 本が落ちることを実際に確認した。
- 末尾保持は「上限の 2 倍を超えたら上限まで前を捨てる」ので、front-drop の
  memmove は読み込み量に対して定数倍。ただしこれは Vec の**長さ**の上限で
  あってメモリの上限ではない (capacity は倍々で伸びて縮まず、続く
  `redact_windows_paths` が同じ本文の `Vec<char>` を作る)。実際の transient
  peak は上限の数倍で、これも上限を控えめに保つ理由。
- 併せて記録 (trade-off): 先頭にしか出ない情報は、stderr が 128 KiB を超えた
  run では落ちるようになった。具体的には `plugin_kind:*`
  (`l2_main_entry.inc` / `worker_runtime_admission.cpp`、load 時に 1 回)、
  `first_failure_stage` (最初にエラーを載せた `stage:*_end`)、`active_stage`
  (`stage:*_begin` 側が落ちるようになったため)、`stage_events` (先頭から
  埋まるので run の冒頭ではなく window の冒頭になる)。`failure_stage` は
  両側にまたがる: `stage:*_end` が error を載せている経路では取れるように
  なり、`active_stage` を使う fallback 経路では逆に取れなくなる。代わりに
  `last_completed_stage` / callback denial marker — 失敗した selector が
  残すもの — が取れるようになった。失われる側が入れ替わった
  ということで、既定 (extended diag なし) の stderr は桁違いに小さく、
  304 AEX の sweep でも record の差は出ていない。report に載る stderr の
  最悪サイズは倍になる (`l2.rs` / `selftest.rs` は capture 全体を埋め込む)。
- 取り込みの 1 チャンク分を `absorb` / `finish_capture` に切り出し、
  上限未満 / 上限〜2倍 / 繰り返し drain / 先頭保持 / 上限 0 を broker の
  unit test で固定した。加えて「上限を超える stderr が report に
  **stream の末尾として**、行境界で切られ marker 付きで届く」ところまでを
  1 本の test で通しで見る (`extended_diagnostics_stderr_tail` から環境変数の
  gate を剥がして `stderr_tail` を切り出した)。path が front-drop を跨ぐ場合、
  cut が改行直後のとき行を余分に捨てないこと、改行の無い capture、先頭保持は
  整列しないこと、も test にした。2 つの上限の大小関係自体は
  `const _: () = assert!(...)` でコンパイル時に固定した (実行時 assert では
  定義式と同語反復になる)。
- **production の配線** (stdout=先頭 / stderr=末尾) は unit test では固定でき
  ない (どの test も retention を自分で指定するため) ので、`dummy_large_stderr`
  worker を足して `run_isolated` 越しに「stream の末尾が届く / 先頭は落ちる /
  path が redact される」を見る integration test を
  `broker/crates/dummy-workers/tests/isolation.rs` に置いた (stdout 側の
  `production_stdout_capture_preserves_large_bounded_worker_reports` と同型)。

## 5. 残件

- ~~PSL_Adjustments の `frame_error:14` (§3)~~ → **解決 (2026-08-20、#1439)**。
  §3.2 の「反証 (BEE の resolver を立てて 14 が消えることの確認) は行っていない」
  はその後実施され、**14 は消えた**。ただし §3 の見立てのうち「`BEE_Birth` を
  呼んでいないことが原因」は外れで、正しくは「`BEE_Birth` は resolver 設置に
  到達していない」だった。詳細と実測は
  `docs/SUPPORT_LIBRARY_BIRTH_SEQUENCE_2026-08-18.md` §9。
  §3 以下の記述は当時の観測として残す。
- Particle_Playground の `ext_lookup` id 234..335 が空の件は #1289 本文の
  記録どおり未解明。render 出力が AE と一致したので、少なくとも frame 0 の
  pixel には影響していない (観察)。
- source sprite バッファの出所 (BM か plug-in 自前か) は未確定 (§2.1)。

# AE の process-wide support library birth 列 (issue #1279) 2026-08-18

対象: #1279 の discovery コホート 5 本
(3D Camera Tracker / Stabilizer = `params_setup:13`、Particle_Playground =
`global_setup:11`、ProfileToProfile = `global_setup:14`、PSL_Adjustments =
`global_setup:21249`)。#1266 (PSL_Adjustments) と #362 の残課題を含む。

観察 (実測) と推論を分けて記録する。以前の結論を覆した箇所は「訂正」として
明記し、古い記述は消さない。

## 1. 観察: AE 自身の birth 列 (aelib.dll)

`aelib.dll` の起動シーケンス関数 (step 番号でスイッチする形) に、AE が
process 起動時に呼ぶ support library の birth が並んでいる。Ghidra で読み出した
順と引数 (AE 2026):

| step | 呼び出し | export 元 (dumpbin /EXPORTS で確認) |
| --- | --- | --- |
| 3 | `BEZ_Birth()` | BEZ.dll |
| 4 | `FILE_AddExtensionMap(...)` x9 → `FILE_Birth()` | FILE.dll |
| 5 | `M_Birth(3, 0)` | **U.dll** |
| 6 | `RND_Birth()` | **U.dll** |
| 7 | `PREF_Birth(flags>>1 & 1, false, "Adobe", "After Effects", "26.3", <path>, false)` | PREF.dll |
| 8 | `VAL_Birth()` | VAL.dll |
| 9 | `MC_Birth(mode != 0, 0xf118657, mode == 3)` | MC.dll |
| 10 | `COR_Conception(flags & 1)` | COR.dll |
| 12 | `COR_Birth(mode == 1 \|\| (flags>>3 & 1), flags & 1, mode == 3)` | COR.dll |
| … | `PLUG_Birth` / `TDB_Birth` / `PF_Birth(0)` / `P_Birth` / `TXT_Birth` / `TDL_Birth` / `PREM_Birth` / `PIN_Birth` / `SND_Birth` / `OM_Birth` / `MSK_Birth` / `FLT_Birth` / `PR_Birth` / `BEE_Birth` / `MEE_Birth` / `FIM_Birth` / `FLO_Birth` / **`U_SP_Birth()`** / `MC_SP_Birth` / `PT_Birth` | 同名の各 DLL |

この表の「export 元」は各 DLL の `dumpbin /EXPORTS` で確認した (`M_Birth` と
`RND_Birth` が U.dll から出ている点はこのホストの実装が依存しているので特に
重要)。`PLUG_Birth` 以降の tail は同名の DLL (PLUG.dll、TXT.dll、BEE.dll …)
から出ているが、この issue では触っていないので個別確認はしていない。
**追記 (2026-08-18、§6)**: そのうち `PLUG_Birth` は #1280 で確認・実装した
(export 元は PLUG.dll、RVA 0x19810)。残りの tail は依然未確認。

この表は aelib.dll の step 3 以降で、`U_Birth` は含まれていない (より前段で
別の経路から呼ばれる)。したがって「U を最初に birth する」根拠はこの表ではなく、
各 library 側の依存 (`VAL_Birth` は `U_CopyString` と LIST を通る、`PF_Birth` は
U.dll の CPU feature block を読む) の観察による。

このホストは従来 `U_Birth` だけを呼んでいた (#362 / #1063)。`PF_Birth` は
この作業と並行して PR #1284 (issue #1212) が入れた
(`docs/PF_HOST_ABI_SHAPES_2026-08-18.md`)。列の他の birth は
どれもプラグインの closure からは呼ばれない (closure が import するのは working
entry point だけ) ので、呼ぶ責任はホストにある。

## 2. 観察: 各 discovery 失敗の分岐条件

### Particle_Playground — `global_setup:11`

- `Particle_Playground.aex` が import する U.dll シンボルは
  `U_SP_GetSPBasicSuite` **1 本だけ** (dumpbin /IMPORTS)。
- `U_SP_GetSPBasicSuite(out)` (U.dll 0x37d0) は
  `*out = &DAT_180145318; return DAT_180145310 == 0 ? 0xb : 0;`。
  **非 0 の戻り値は 0xb = 11 しか無い**。
- `DAT_180145310` (実 SPBasicSuite*) を立てるのは U.dll 内の
  `FUN_1800054a0`、すなわち `("SP Interface", "Startup")` メッセージを受けた
  ときに `*(msg + 0x18)` を保存する host plug-in entry。この entry を
  Sweet Pea に登録するのは U.dll の `FUN_180005560` で、それを呼ぶのは
  export された **`?U_SP_Birth@@YAHXZ`**:
  U 自身の `SPHostProcs` を組み立て → `ae_sweetpea::SPInit(<その procs>, 0, 0)` →
  `SPAddHostPlugin(0, FUN_1800054a0, 0, "Sweet Pea 2 Adapter", ...)` →
  `AS ZString Suite` / `AS ZString Dictionary Suite` を `SPAddSuite` →
  `SPStartupPlugins()`。
- 従来のホストは `ae_sweetpea::SPInit(nullptr,nullptr,0)` +
  `SPStartupPlugins()` を直接呼んでいた。SP は起動するが adapter を登録しない
  ので Startup メッセージが届かず、`U_SP_GetSPBasicSuite` は 11 のまま。
  → **#362 の「実 AE の PICA ブートストラップが要る、host 内で閉じない」は
  訂正**。U.dll の export だけで閉じる。
- `?U_SP_Death@@YAHXZ` は `SPShutdownPlugins()` + `SPTerm()`。teardown は
  起動した層と同じ層で行う (両方呼ぶと二重 shutdown)。

- U の Sweet Pea gate を越えたあと、Particle_Playground は GLOBAL_SETUP の
  途中で VAL.dll 内の null read で落ちる (実測:
  `stage:selector_seh selector=GLOBAL_SETUP code=0xc0000005
  site=other_module module=VAL.dll access=read fault=null`)。`VAL_Birth` を
  呼ぶようにすると落ちなくなる。落ちていたのが VAL の値型テーブルだという
  読みは `VAL_Birth` の逆アセンブル (`LIST_New` でリストを作り、32 種の
  値型を `LIST_Insert` する) によるもので、trace が直接示しているわけでは
  ない (推論)。

### ProfileToProfile — `global_setup:14`

- GLOBAL_SETUP は BIB/ACE の解決を通ったあと ACE を使う段で 14 を返していた
  (#362 の「ACE CMM 環境が要る」)。
- `COR.dll` の `COR_Conception(bool quiet_bib_errors)` が
  `U_Context::RegisterState(BIBState)` と `U_Context::RegisterState(ACEState)`
  を行い、`DAT_180104600 = dvabravoinitializer::InitBravoComponents(nullptr)`
  を設定し、ACE の profile ディレクトリ (CommonFiles/Adobe/Color/Profiles) を
  組み立てる。AE は step 10 でこれを呼ぶ。
- ホストがこの birth を呼ぶようにしたところ、ProfileToProfile は
  `global_setup_error=0` / `params_setup_error=0` / 8 params で
  `parameters_inspected` になった (実測)。
  → **#362 の「実 AE の CMM 環境が必要で fail-closed を緩めずには進めない」は
  訂正**。

### PSL_Adjustments — `global_setup:21249` (#1266)

- `EffectMainBLACK_WHITE` の `case 1` (GLOBAL_SETUP) 末尾は
  `return COR_InitPSL();`。
- `COR_InitPSL` は COR PSL thread を起こし、その先頭で
  `COR_GetBIBAddressProc(&resolver)` を呼ぶ。この関数 (COR.dll 0xc7d0) は
  `DAT_180104600 == 0` のとき `Up_ReportErrString(... "BIB is uninitialized.")`
  して **`return 0x5301` = 21249** を返す。`*param_3 = iVar8` でその値が
  `COR_InitPSL` の戻り値になる。
  → **21249 の出所は確定**。`DAT_180104600` を立てるのは上記
  `COR_Conception`。
  → #1266 本文の推論「`Premiere Memory Manager Suite` v4 が取れないことが
  原因」は**否定**。one-shot trace には `PF File Registration Suite` /
  `Premiere Memory Manager Suite` の acquire 自体が現れない (record の
  `missing_suites` は cluster session 内の別 member の分)。
- `COR_Conception` を呼ぶようにしたところ、PSL_Adjustments は
  `global_setup_error=0` / `params_setup_error=0` / 5 params で
  `parameters_inspected` になり、sweep の bucket は
  `not_discovered:exit_20_global_setup:21249` → `frame_error:4` に移った (実測)。

### 3D Camera Tracker / Stabilizer — `params_setup:13`

- one-shot trace: GLOBAL_SETUP は 0。PARAMS_SETUP の冒頭で
  `PF AE Private Effect Suite` v3 (2 回) と v5 (1 回) を acquire しようとして
  失敗し、13 を返す。
- `AEXCOMPAT_SUITE_CALL_SLOT_PROBE="PF AE Private Effect Suite@3;PF AE Private
  Effect Suite@5"` で 32 slot の trampoline を渡すと、acquire は成功するが
  **どの slot も呼ばれないまま** `3D_Camera_Tracker.aex+0xc1cef8` で null read の
  access violation になる (caller `+0xd15bd7`)。つまりこの suite は関数
  テーブルとしてだけでなくデータとしても読まれている可能性が高い (推論)。
  suite の実体は `AfterFXLib.dll` / `VideoFilterHost.dll` / `FLT.dll` に文字列が
  ある。
- 13 は 3D Camera Tracker / Stabilizer 固有ではない: ProfileToProfile も
  「必要な suite が取れない」経路で `_CxxThrowException(0xd)` を投げる
  (`FUN_180016140` / `FUN_180015e00`)。AE 同梱 effect が共有する
  「suite が取れなかった」コードと思われる (推論)。
- ここは本 issue では閉じていない。
  **訂正 (2026-08-18、§7)**: 「slot が呼ばれないまま AV になるので suite は
  データとしても読まれている」という推論は **否定**。throw は slot 呼び出しの
  前に `Localizer::Get()` で起きており、AV は catch handler 側の二次 fault。

## 3. ホスト側の実装 (この issue で入れたもの)

- `minihost/src/worker_sweetpea_bootstrap.hpp` +
  `worker_host_suite_wiring.cpp`: Sweet Pea の起動を U.dll の `U_SP_Birth`
  優先にした。U.dll が無い closure では従来どおり ae_sweetpea を直接起動する。
  latch のキーは U 経路が U.dll の mapping、direct 経路が
  (ae_sweetpea の mapping, admitted plug-in directory) の組。cluster session の
  最初の member が後続 member の分まで決めないため (#1267 の U_Birth latch と
  同型)。suite resolver は cache されず、この判断は `AcquireSuite` ごとに
  聞かれるので、失敗を「毎回やり直す」判断にすると LoadLibraryEx と SPInit が
  acquire ごとに走ってしまう。それを避けつつ、member が変われば聞き直す。
- `minihost/src/l2_main_support.inc` の
  `initialize_process_support_libraries()`: mapping されている support library を
  AE と同じ順・同じ引数形で birth する
  (`BEZ_Birth` / `FILE_Birth` / `M_Birth(3,0)` / `RND_Birth` / `VAL_Birth` /
  `COR_Conception(false)`)。各 library ごとに独立の latch。
- teardown は起動した層だけを通し、起動していなければどちらも通さない
  (ae_sweetpea は closure が map しているだけのことがあり、それを shutdown
  するのは所有していない層を畳むことになる)。実測では **one-shot 直叩きの時点で
  U.dll は既に unmap されている**。最終 build (`ProfileToProfile` の one-shot、
  `AEXCOMPAT_EXTENDED_DIAG=1`) が出す 2 行そのまま:

```
extended_diag:pica_component stage=teardown_begin u=remapped_or_unmapped
extended_diag:pica_component stage=teardown u=remapped_or_unmapped sweetpea=none
```

  (この観察自体は最初に teardown の trace を足した時点で取れており、そのときの
  label は `u=unmapped` だった。label はその後 review で
  `remapped_or_unmapped` に変わったので、上の 2 行は最終 build で取り直した
  もの。) 記録した HMODULE をそのまま使うと解放済み image を
  GetProcAddress で読むことになるので、teardown 時に `GetModuleHandleW` で
  同じ mapping であることを確かめてから呼ぶ。結果として one-shot では
  `U_SP_Death` は走らない。unmap の出所は `WorkerSession::unload_module` の
  `FreeLibrary` で、plug-in closure (U.dll を含む) は atexit より前に
  解放される。U 経路では ae_sweetpea もホスト自身の参照を持たない
  (U.dll の依存として入ってくるだけ) ので、両層とも unmap 済みになる。
- **走らせようとした結果 (実測、採用せず)**: `GetModuleHandleExW(FLAG_PIN)` で
  U.dll と ae_sweetpea を process 寿命まで pin すると `U_SP_Death` は実際に
  走るようになるが、**3 本とも process 終了時に access violation**
  (exit 0xC0000005) になった (pin 無しでは Particle_Playground exit 20 /
  ProfileToProfile exit 0)。atexit 時点の SP/dvacore 状態では
  `SPShutdownPlugins`+`SPTerm` は安全に呼べない、という観察。したがって
  pin は入れず、U 経路では teardown が no-op になることを受け入れて記録に
  留める。#362 の teardown が防いでいた dvacore shutdown fast-fail が
  この経路で再発しないことは corpus 304 本の sweep で確認している
  (session 分類に変化なし)。teardown を安全に走らせる場所 (session の
  unload 境界など) の検討は #1282。
- Sweet Pea の `SPInit` はこの process では最大 1 回しか走らせない
  (`decide` が `direct_started` で止める)。`SPInit` は refcount なので、
  2 回目を走らせると必要な `SPTerm` の回数が数えられなくなるため。
  既知の穴 (未解決、記録のみ): `U_SP_Birth` が内部の `SPInit` を終えたあとで
  throw / fault した場合、その init は host からは観測できないので teardown
  の債務として記録されない。teardown 自体が U 経路では no-op なので現状の
  実害は無いが、teardown を走らせるようにするなら一緒に解く必要がある
  (→ #1282)。
- Sweet Pea の起動は `dvabravoinitializer.dll` が解決できたときにしか走らない
  (`ensure_pica_components_initialized` の early return がその前にある)。
  この結合は #1279 より前からのもので、どちらも `provide_bib_suite` 経由
  すなわち BIB.dll がある closure でしか到達しないため実害は観測していないが、
  `U_SP_Birth` 経路がこれに依存するようになったので明記しておく。
  review では「Sweet Pea の起動を Bravo の early return より前に出す」案も
  出たが、計測していない挙動変更になるので入れていない (記録のみ)。
- `pica_component_mutex()` を LoadLibraryEx を跨いで保持しているため、loader
  lock との AB/BA が理論上成立する。観測している再入は同一スレッドのみで、
  DllMain から host suite を acquire する形は観測していないが、構造的に
  排除されてはいない。受け入れて記録する扱いで、追跡は #1287。
- 呼び出し順は `initialize_u_dll_allocator()` (U_Birth、#362 / #1063) →
  `initialize_process_support_libraries()` (この issue) →
  `initialize_pf_dll_host_layer()` (PF_Birth、#1212 / PR #1284) で、AE の
  相対順 (step 3〜10 → tail の PF_Birth) と一致する。
- `COR_Birth` は**呼んでいない**: この worker では戻ってこない (60 秒待って
  返らないことを実測。`COR_Conception` は 0 を返す)。21249 の原因は
  `COR_Conception` 側なので、そこまでで止めている。その結果このホストは
  「Conception 済み・Birth 未了の COR」という、実 AE には存在しない状態で
  以降 (PF_Birth を含む) を走らせている。順序は AE と同じだが状態は同じでは
  ない、という限定つきの一致であることを明記しておく。
- 副作用として `PF_Birth()` の戻り値が変わった: PR #1284 の計測時は
  21249 (COR の BIB resolver が null) だったが、`COR_Conception` が先に走る
  ようになったので 0 を返す (実測、
  `stage:pf_host_layer_init status=called result=0`)。#1212 の修正対象である
  dispatch table の書き込みはどちらの戻り値でも完了している。
- AE と初期化順が違う点 (観察): AE は BIB / Bravo のハンドシェイクを済ませて
  から step 10 で `COR_Conception` を呼ぶ。このホストの
  `initialize_process_support_libraries` はプラグイン load 直後に走るので、
  `COR_Conception` の中の `dvabravoinitializer::InitBravoComponents(nullptr)` が
  **ホスト自身の `SetBIBProcAddress` + `InitBravoComponents` より先**に走る
  (ホスト側は最初の `AcquireSuite` が BIB suite を要求したときに走る)。
  そのため `InitBravoComponents` は 1 プロセスで 2 回呼ばれる。実測では
  ホスト側の 2 回目は非 0 の resolver を返し (extended diag の
  `dll=dvabravoinitializer.dll status=called result=0 resolver=<ptr>` に出る。
  Particle_Playground では `0x18000E480` で BIB.dll の mapping 内)、
  ProfileToProfile / PSL_Adjustments の discovery も通る。COR が受け取った
  resolver と同一ポインタかどうかはこの trace からは見えない (未確定)。
  なお返る resolver はプラグインによって違う: Particle_Playground /
  ProfileToProfile では `0x18000E480` (BIB.dll の mapping 内)、
  PSL_Adjustments では heap 上のアドレス。どちらでも discovery は通っている
  (観察のみ、意味は未確定)。

## 4. 計測 (full corpus)

母集団: AE 2026 `Support Files\Plug-ins\Effects` を `render_sweep` の引数に
渡した 304 AEX、`--depth` 既定 (8)、size/time/frames も既定。baseline は
origin/main `997d3b7d` を専用の worktree で同じ build 手順で測ったもの
(sweep CLI は両者同一バイナリ、worker 3 exe だけが違う)。

| bucket | baseline (997d3b7d) | 変更後 |
| --- | --- | --- |
| rendered | 289 | 289 |
| frame_error:512 | 4 | 5 |
| frame_error:4 | 0 | 1 |
| not_discovered:exit_20_params_setup:13 | 2 | 2 |
| not_discovered:exit_20_global_setup:11 | 1 | 0 |
| not_discovered:exit_20_global_setup:14 | 1 | 0 |
| not_discovered:exit_20_global_setup:21249 | 1 | 0 |
| not_discovered:exit_20_global_setup:2 | 0 | 1 |
| not_discovered:exit_12 | 1 | 1 |
| not_discovered:cluster_session_invalidated | 1 | 1 |
| render_frame_failed:worker_invariant_failure | 2 | 2 |
| rendered_empty | 2 | 2 |

bucket が動いたのは 3 本だけで、他の 301 本は baseline と同じ bucket
(突き合わせの key は `plugin_relative_path`。ファイル名だけで引くと
`Threshold.aex` が別ディレクトリに 2 本あるため 1 本落ちる):

- `PSL_Adjustments.aex`: `not_discovered:exit_20_global_setup:21249` → `frame_error:4`
- `ProfileToProfile.aex`: `not_discovered:exit_20_global_setup:14` → `frame_error:512`
- `Particle_Playground.aex`: `not_discovered:exit_20_global_setup:11` → `not_discovered:exit_20_global_setup:2`

512 が増えたのは ProfileToProfile が render 段まで進んだ分で、baseline の 4 本
(ColorAndContrast / Curl_Noise / ShapeBlur / VRSphereToPlane) は変わっていない。

silent-wrong の確認として、baseline で `detail.pixel_sha256` を持つ 291 record
すべてについて変更後の hash と突き合わせ、差分ゼロ・欠落ゼロを確認した
(変更後も 291 record で同数)。

計測に使った build fingerprint (report JSON の `build`):

| | baseline | 変更後 |
| --- | --- | --- |
| l2_worker | `78ee4bd6…` | `c7c37291…` |
| classic_worker | `f2db99da…` | `472ac121…` |
| smart_worker | `d6cc3774…` | `18e333df…` |
| cli | `ca0ea4ca…` | `ca0ea4ca…` (同一) |

report JSON の SHA-256: baseline
`6FA292BBD4F5AFADC73EFACF62A9589378DF65C682DEF75ED5C982FD9AB78EC1`、変更後
`3BB0CC171565072F9E8D01AEF5B3677814DD637C05215AF3D9F103B84854D40D`。

この表は review ループを抜けた最終 build で測り直したもの。途中の build でも
同じ 3 本だけが動き、pixel hash 差分ゼロで、結果は変わらなかった。

## 5. 残件 (別 issue)

- Particle_Playground: `global_setup:11` → `global_setup:2` に移ったが未 discovery。
  `FUN_18000e900` の `U_SP_GetSPBasicSuite` は 0 を返すようになり、次の
  `FUN_180016a10` (string table を引きながら particle layer type を登録する
  ループ) 以降で 2 になる。trace 上、ホストの string table が返せた id は
  79 回の lookup 中 1 件 (`id=22 -> "AllMask"`) だけで、`id=234..335` は
  選択された LStr group (`$$$/AE/Playground/LStr/` は 0..231) に存在しない。
  Particle_Playground は `ext_alloc(out, 0x101b)` のように **resource id で
  string table を取得**しており、ホストはその id を無視して 1 group だけを
  返している。ここが次の境界 (観察)。→ #1280
  **訂正 (2026-08-18、§6)**: この当たりは否定された。2 の出所は
  `PLUG_RegisterStaticRoutine` の throw で、string table とは無関係。
- PSL_Adjustments (`frame_error:4`) と ProfileToProfile (`frame_error:512`) の
  render 段。→ #1281
- PSL_Adjustments を `aex_l2_worker.exe --l2-params-only` で直接叩くと、
  report (`status=parameters_inspected`、5 params) を書き終えたあと process
  終了時に access violation で落ちる (exit 0xC0000005)。COR PSL thread /
  AdobePIE が生きたまま atexit の `teardown_pica_components` に入るためと
  思われる (推論)。**この exit code の汚れは one-shot 直叩き経路だけで、
  multifilter の discovery 経路には出ていない**: cluster session (full sweep) でも
  singleton (`render_sweep --filter PSL_Adjustments`、member 1 本) でも
  `frame_error:4` = discovery 成功で、`worker failed` 系の分類にはならない
  (どちらも実測)。→ #1282
- `COR_Birth` がこの worker で返らない件。→ #1282
- Sweet Pea の teardown を安全に走らせられる場所。現状は U 経路で no-op で、
  atexit で無理に走らせると落ちる (§3 の pin 実験)。→ #1282
- 3D Camera Tracker / Stabilizer の `PF AE Private Effect Suite` v3/v5。→ #1283
  **訂正 (2026-08-18、§7)**: suite と Localizer の**両方**が要る。suite が
  無いうちは acquire の失敗そのものが 13 の出所で、suite を publish して
  初めて Localizer の throw が表に出る (その状態だと 13 は AV に変わる)。
  「suite だけ入れれば通る」でも「Localizer だけ入れれば通る」でもない。

## 6. 追記 (2026-08-18、issue #1280 の作業)

### 訂正: Particle_Playground の `global_setup:2` は string table ではなく PLUG が未 birth

§5 の 1 項目め (と #1280 本文) が立てた「`ext_alloc` の resource id を無視して
1 group しか返していないのが 2 の原因」という当たりは **否定**。実測は次のとおり。

観察 (cdb、`aex_l2_worker.exe --l2-params-only Particle_Playground.aex` を
子プロセス無しで直接 debug、`sxe eh` で first-chance C++ throw の stack):

```
C++ EH exception - code e06d7363 (first chance)
  KERNELBASE!RaiseException
  VCRUNTIME140!_CxxThrowException
  PLUG!PLUG_RegisterStaticRoutine+0x211
  Particle_Playground!PluginDataEntryFunction+0x14448
  Particle_Playground+0x16d6c
  Particle_Playground+0xe9ed
```

同じ run で `Particle_Playground+0x2e3e4` (最初の登録前の照会) は `eax=0` で
通っており、throw の直後に `+0x2e50f` の FH4 catch が `[rsp+0x40]` から 2 を
読み、`+0x2e7be` が `esi` = 2 をそのまま返している (いずれも breakpoint で実測)。

- `?PLUG_RegisterStaticRoutine@@...` (PLUG.dll+0x1ca80) は
  `PLUG.dll+0x6df90` と `PLUG.dll+0x6e098` の 2 語がどちらも `0x910ba1` の
  ときだけ登録し、そうでなければ `Up_ReportError(0x30002a, 0x10)` して
  **C++ の整数 2 を throw** する。この 2 語に `0x910ba1` を書く命令は
  `?PLUG_Birth@@YAHAEBV?$function@...@Z@boost@@@Z` (PLUG.dll+0x19810) の中の
  2 つの `mov` だけ (下の即値 scan の項を参照)。
- Particle_Playground の GLOBAL_SETUP は `FUN_180016a10` の中で 14 個の
  particle action type (`ADBE AllMask` … `ADBE Wall`) を
  `FUN_18002e3a0` → `PLUG_RegisterStaticRoutine` で登録する。1 本目
  (`ADBE AllMask`) の throw を FH4 catch が受けて選択子の戻り値 2 になる。
- lookup が返す文字列は関係なかった: `id=22 -> "AllMask"` は正しく引けており、
  `id=23` が空文字列なのは `$$$/AE/Playground/LStr/0023=` が実際に空値だから
  (AEX のバイト列で確認)。`id=234..335` が空なのは事実だが 2 の原因ではない
  (→ #1289 に観察として引き継ぎ)。

ホスト側の実装: `initialize_process_support_libraries()` の表に
PLUG.dll の birth を追加した。AE の列では `COR_Conception`/`COR_Birth` の後、
`PF_Birth` の前なので、表の末尾 (= `initialize_pf_dll_host_layer` の前) が
AE と同じ相対順になる。引数は
`const boost::function<bool(const StdString16&)>&` (AE がスキャン対象の
plug-in ファイルを絞るための述語) で、このホストは PLUG のファイルスキャンを
使わないので**空の function** を渡す。`PLUG_Birth` が最初に読むのは function
object の vtable ポインタ (offset 0) で、`+0x19832` の `test rax,rax / jz` が
0 のとき empty 経路に落ち、それ以降のバイトを読まない。渡すのはゼロ埋めした
64 バイトの静的バッファだが、**安全なのはこの早期脱出が取られるからであって
バッファサイズによるものではない**: 非空側は offset 0x28 までコピーするか、
vtable 自身の manager (`+0x19864` の `and rax,~1` → `+0x19870` の
`call qword ptr [rax]`) を呼ぶかで、後者がどれだけ読むかは呼び出し側の
manager が決める。空の object は PLUG の global skip predicate
(`PLUG.dll+0x6e0a0`) に install され、その唯一の consumer は同じ word を
テストして「何もスキップしない」と答える。

`0x910ba1` という即値は PLUG.dll 中に 26 箇所あり (観察)、`mov` は `+0x19940`
(`→ +0x6e098`) と `+0x1994a` (`→ +0x6df90`) の 2 箇所だけで、残り 24 は
`cmp`、つまり 12 対の guard。この scan が示すのは「その**即値**を書く命令が
他に無い」ことまでで、計算結果やコピーによる書き込みは scan に現れない。
「gate を開けるのは `PLUG_Birth` だけ」「12 対がそれぞれ別の entry point に
ある」はこの scan と各 site 周辺の逆アセンブルからの推論 (実測の範囲では
反例なし)。いずれにせよ gate は `PLUG_RegisterStaticRoutine` 固有ではなく
module 全体にかかっている。
`PLUG_Birth` は sentinel を**無条件に**書く (確保に失敗しても書く) ので、
`result=` が非 0 の場合は「gate は開いているが scan cache が NULL」という
状態になる。ホストは記録するだけで判断には使わない。実測は
`stage:legacy_support_init library=PLUG.dll entry=?PLUG_Birth@@... status=called
result=0`。

`PLUG_Birth` は冪等ではない (2 回目は handle 1 個と LIST 2 本を作り直し、
前のものを解放しない。解放するのは `PLUG_Death` だけで、このホストは呼ばない
= §3 の teardown 方針どおり) ので、mapping 単位の latch は最適化ではなく
必須。依存は U.dll (`ALOG_Log` / `U_ZLookupLStrCache` /
`U_AllocateHandleClear`) と LIST.dll (`LIST_New`) だけで、どちらも PLUG.dll が
import しているので PLUG.dll が map されていれば map 済み。file / registry I/O、
thread 生成、TLS、独自の atexit は無い。

### `PF_InfoDrawText` / `PF_InfoDrawText3` の `Z0` 引数

PLUG_Birth を入れると Particle_Playground は discovery を通り (126 params、
名前も AE の UI と一致)、render 段で `frame_error:4:PF_Err_OUT_OF_MEMORY` に
なった。trace の末尾は
`acquire_suite name="PF AE Adv App Suite" version=1 -> 0` →
`lookup id=222 -> "Number of particles: %d"` → `classic_render_end error=4` で、
host 側の拒否痕跡は出ていなかった。

`extended_diag:info_draw_text` の行を足して測ると
`slot=PF_InfoDrawText args=22,null -> 0` (修正後) / 修正前は 4 で、
**第 2 引数が null** だった。`AE_AdvEffectSuites.h` の
`PF_AdvAppSuite1/2::PF_InfoDrawText(const A_char *line1Z0, const A_char *line2Z0)`
は両引数とも `Z0` = 省略可能なので、null を 4 で撥ねていたホストが誤り
(#1055 が `PF_InfoDrawText3Plus` に適用したのと同じ読み)。`PF_InfoDrawText3` の
3 引数も同じ。長さ 256 以上の (= 終端が見つからない) 文字列は引き続き 4 で
撥ねる。

`extended_diag:info_draw_text` の行自体にも 2 つ規則を入れた:

- 引数は左から順に分類し、**終端が見つからない引数で止める**。その後ろは
  検査も join も trace も読まない (`args=...,unread`)。従来の `||` 連鎖が
  短絡でそうなっていたので、trace がそれを崩すと「診断を付けたときだけ
  wild pointer を読んで落ちる」ことになる。**これは新しい pytest
  `test_info_text_trace_stops_at_the_first_unterminated_argument` が固定して
  いる**: self-test 側が拒否より後ろの引数に `PAGE_NOACCESS` の page を渡す
  ので、読めば process ごと落ちる。
- 1 呼び出し 1 行で、受理と拒否を別々に 64 行で打ち切る
  (`status=suppressed`)。broker が持ち出す stderr は末尾固定長なので、
  毎フレーム info 行を描く effect があると fault site が押し出される。
  **こちらは regression test が無い**: self-test route が出す info-text 呼び出しは
  受理 10 件 / 拒否 9 件程度で上限 64 に届かず、`status=suppressed` の分岐に
  入らない。上限そのものは定数の読みと上の算術 (`fetch_add` の戻り値だけで
  判定するので suppressed 行はちょうど 1 回) による。

この 2 つを入れると Particle_Playground は
`not_discovered:exit_20_global_setup:2` → `frame_error:516` に移り、次の境界は
`transform_world` の `source_world_unresolved` (→ #1289)。

### 計測 (full corpus、#1280 の変更)

母集団: AE 2026 `Support Files\Plug-ins\Effects` を `render_sweep` の引数に
渡した 304 AEX、`--depth` 既定 (8)、size/time/frames も既定。baseline は
origin/main `9804ae6f`。sweep CLI は両者同一バイナリで、worker 3 exe だけが
違う。

| bucket | baseline (9804ae6f) | 変更後 |
| --- | --- | --- |
| rendered | 289 | 289 |
| frame_error:512 | 5 | 5 |
| frame_error:4 | 1 | 1 |
| frame_error:516 | 0 | 1 |
| not_discovered:exit_20_params_setup:13 | 2 | 2 |
| not_discovered:exit_20_global_setup:2 | 1 | 0 |
| not_discovered:exit_12 | 1 | 1 |
| not_discovered:cluster_session_invalidated | 1 | 1 |
| render_frame_failed:worker_invariant_failure | 2 | 2 |
| rendered_empty | 2 | 2 |

bucket が動いたのは 1 本だけで、他の 303 本は baseline と同じ bucket
(突き合わせの key は `plugin_relative_path`):

- `Particle_Playground.aex`: `not_discovered:exit_20_global_setup:2` →
  `frame_error:516:PF_Err_BAD_CALLBACK_PARAM`

silent-wrong の確認として、baseline で `detail.pixel_sha256` を持つ 291 record
すべてについて変更後の hash と突き合わせ、差分ゼロ・欠落ゼロを確認した
(変更後も 291 record で同数)。

計測に使った build fingerprint (report JSON の `build`) と report の SHA-256:

| | baseline (9804ae6f) | 変更後 |
| --- | --- | --- |
| l2_worker | `c04d476d…` | `cfa98ebc…` |
| classic_worker | `83c014da…` | `4f18b869…` |
| smart_worker | `aaa714fd…` | `05c50fad…` |
| cli | `d5d12042…` | `d5d12042…` (同一) |

report JSON の SHA-256: baseline `2A2B1AE7EC13528BFC2E2E541BBA63BCD3B4A40B7913CAEE982A25C4C67B5B20`、変更後 `543DAAECA068A5CD8943B29B73F59C4D10F4EE8717C137925A472A3AD6FB4CED`。

birth 列の trace は変更後の build でこう出る (Particle_Playground の
one-shot):

```
stage:legacy_support_init library=COR.dll entry=?COR_Conception@@YAH_N@Z status=called result=0
stage:legacy_support_init library=PLUG.dll entry=?PLUG_Birth@@YAHAEBV?$function@$$A6A_NAEBV?$basic_string@_WU?$char_traits@_W@std@@U?$STLAllocator@_W@allocator@dvacore@@@std@@@Z@boost@@@Z status=called result=0
stage:pf_host_layer_init status=called result=0
```

## 7. 追記 (2026-08-18、issue #1283 の作業)

### 訂正: `params_setup:13` は suite 単独では説明できない (Localizer と両方が要る)

§2 の 4 項目め (と #1283 本文) は「`PF AE Private Effect Suite` が未実装なので
13 になる」「32 slot の trampoline を渡すと slot が 1 つも呼ばれないまま AV に
なるので、この suite は関数表としてだけでなくデータとしても読まれている
可能性が高い」と書いた。前者は必要条件としては正しいが**十分条件ではなく**、
後者は **否定**。

`3D Camera Tracker.aex` / `Stabilizer.aex` は parameter 名を毎回

```
Localizer::Get()->GetLocalizedString(&wstr, "$$$/apd/analysis/parameter/name/...")
```

で作り、その UTF-16 を suite の slot 2 に渡す。**throw は slot 呼び出しの前**に
起きる。`dvacore.dll` の
`?Get@Localizer@config@dvacore@@SAPEAV123@XZ` (dvacore+0x105640) は
`Localizer::S_localizerP` が 0 なら
`"dvacore::config::Localizer not initialized. Check dvacore/config"` を throw
する。条件はこの global 1 個だけ。

観察 (cdb `sxe eh`、3D Camera Tracker):

```
KERNELBASE!RaiseException
VCRUNTIME140!_CxxThrowException
dvacore!dvacore::config::Localizer::Get+0x61
3D_Camera_Tracker + 0xd10755     (call [Localizer::Get] の戻り)
3D_Camera_Tracker + 0xc14460
3D_Camera_Tracker + 0xd211ae
3D_Camera_Tracker + 0xc1f118
3D_Camera_Tracker + 0xaf0580     (selector dispatcher、try/catch を持つ)
```

`+0xc1cef8` の null read (#1283 本文の AV) は **catch handler 側の二次 fault**。
`Tracker3dEffect` の vtable slot 0x1a0 (index 52) が
`out_data->sequence_data`(= `[this+0x18]+0x38`) を deref しており、AV 時の実測で
そこは 0。PARAMS_SETUP で `sequence_data` が NULL なのは AE でも正常なので、
これは Localizer の throw が起きて初めて到達する plug-in 側の潜在バグで、
host の契約違反ではない。throw を無くせば到達しない (実測: 到達しない)。

### 観察: `PF AE Private Effect Suite` の実体

register するのは `VideoFilterHost.dll` の `RegisterPrivateEffectSuite()`
(`0x1800443d0`)。**v3 / v5 / v6 が同一の 10 entry 関数ポインタ表 `0x1801aa030` を
共有**する (v4 は無い)。data member は無い。`AfterFXLib.dll` は consumer (v3 を
acquire するだけ)。表の直後 `0x1801aa080` から `.rdata` の文字列プールが始まり、
10 個の `__FUNCTION__` 文字列が順に並ぶ:

| slot | offset | 名前 | impl |
| --- | --- | --- | --- |
| 0 | +0x00 | `RegisterForIdleEvents` | 0x180042640 (0x203 stub) |
| 1 | +0x08 | `UnRegisterForIdleEvents` | 0x180042660 |
| 2 | +0x10 | `HostUTF16ToMultibyteString` | 0x180042680 |
| 3 | +0x18 | `HostZStringToUTF16String` | 0x180042860 |
| 4 | +0x20 | `PushSingleIdleEvent` | 0x180042b40 |
| 5 | +0x28 | `GetEffectName` | 0x180042da0 (0x203 stub) |
| 6 | +0x30 | `PF_GetCurrentState_Async` | 0x180042a10 |
| 7 | +0x38 | `SetSequenceDataNeedsSerialization` | 0x180042dc0 |
| 8 | +0x40 | `GetEffectNodeID` | 0x180042e00 |
| 9 | +0x48 | `GetEffectRef` | 0x180043200 |

slot 8 / 9 は実装が自分の名前文字列を `lea` するので位置に依らず確定
(`0x180042e39 -> 0x1801aa210`、`0x180043234 -> 0x1801aa2e0`)。slot 2 は caller 側で
確定 (両 plug-in が `[suite+0x10]` を `(const wchar_t*, 0x100, char*)` で呼ぶ)。
slot 5 と 6 だけは文字列順による位置決めのみで、実装アドレスが文字列順と
単調でない唯一の組 (→ #1291)。

PARAMS_SETUP 全体 (suite timeline 86 entry) で実際に呼ばれる slot は **2 だけ**
(v3 / v5 とも)。この host の実装後も `unsupported_suite_calls` は空のまま
(実測)。

### ホスト側の実装

1. `initialize_dvacore_localizer()` (`minihost/src/l2_main_support.inc`)。
   dvacore の `DummyLocalizerImpl` を ctor
   (`??0DummyLocalizerImpl@config@dvacore@@QEAA@XZ`) で静的バッファ上に構築し、
   `?Set@Localizer@config@dvacore@@SAXPEAV123@PEBVDir@filesupport@3@@Z` に
   null `Dir*` で渡す。`Initialized()` が既に true なら触らない。
   `DummyLocalizerImpl::GetLocalizedString` は ZString key に埋め込まれた
   default 文字列 (`"$$$/path=Default"` → `"Default"`) を返すので、英語 AE の
   表示と一致する。実 `.dat` 辞書の読み込み (`LoadDVADictionaries`) は別。
   AE は dvacore の設定として process 起動時に install するので、この host も
   birth 列の先頭 (U_Birth より前) で行う。
2. `minihost/src/worker_pf_private_effect_suite.{hpp,cpp}`: v3 / v5 / v6 に
   同一の表を publish する。実体があるのは **slot 2 だけ**で、残りは
   `record_unsupported_suite_call` の診断付き stub
   (`UnsupportedSuiteId::pf_ae_private_effect`)。VideoFilterHost 自身も slot 0 と
   5 は `0x203` を返す stub なので、「表として存在すること」が要件。
   publish する slot 数は AE の 10 ではなく 32 (slot probe と同数) で、10 を
   越えて読んだ caller が `.rdata` の続きではなく診断付き stub に当たるように
   してある。
   AE の slot 2 (`VideoFilterHost.dll+0x42680`) は `wcslen` で長さを取り、
   `MediaFoundation.dll!MF::UTF16ToCodePageMultiByte` (= platform の code page)
   で変換し、`destination_bytes - 1` に**切り詰めて**コピーし、終端して 0 を
   返す。この host の意図的な相違は 2 点で、どちらも AE oracle 作業から見える
   ように記録する:
   - 変換先が **UTF-8**。どちらの encoding も worker の JSON report には届かない
     (`worker_report.cpp` の `bounded_diagnostic_text` が 0x20..0x7e 以外の
     byte を落とす) ので report 上は観測できない。効くのは plug-in が
     その後使う buffer で、UTF-8 はどの名前も無損失に表せるのに対し code page は
     持たない文字を置換する。観測した caller は ASCII しか渡さない。
   - `destination_bytes` の上限 (`kMaxDestinationBytes`) を設けている。AE は
     どんな値でも受ける。caller の自己申告以外に手掛かりが無いので、これは
     「申告が出鱈目なときの 1 回の書き込み量を縛る」だけで、嘘をつく caller を
     安全にはしない。
   切り詰めについては AE に合わせた: 拒否しても安全にはならず (どちらも
   caller の申告範囲内にしか書かない)、AE なら短く表示されるだけの名前を
   PARAMS_SETUP の失敗に変えるだけだった。
   なお AE の表は 0 / `0x203` (未実装。slot 0 と 5 がこれ) / `0x200` (失敗) を
   返す。この host の診断付き stub は house convention の 4 を返すので、
   #1295 の境界が `frame_error:4` として出る。
3. `worker_render_session.cpp` の Premiere GPU-filter fallback (#1271) の判定を
   512 だけから **512 と 516** に広げた。理由は下記。併せて、fallback で
   route に入ったときは `stage:pr_gpu_route_begin` に
   `reason=cpu_internal_struct_damaged` / `reason=cpu_bad_callback_param` を
   載せるようにした。これが無いと `rendered` になった record から
   「CPU path が host に断られたのを GPU route が肩代わりした」ことが読めず、
   host の拒否を隠す方向の変更になる。broker 既存の parser がそのまま
   拾うので Rust 側の変更は無い (実測: `pr_gpu_route` の `begin` event に
   `reason: cpu_bad_callback_param` が入る)。
   なお `render_sweep` が record の `worker.pr_gpu_route` に持ち上げるのは
   `_end` の reason だけなので、`begin` の reason は stage event
   (`--close-report`) にはあるが top-level には出ない。

### 副作用: VR family の CPU path が一歩進む

Localizer を入れると、`xGPUFilterEntry` を export する VR 12 本の
**PF CPU path の戻り値が 512 から 516 に変わる** (実測)。#1271 の fallback は
「selector 自身の 512」だけを合図にしていたので、この 11 本が
`rendered` → `frame_error:516` に落ちた (localizer だけを外した A/B で確認)。

trace の差分 (VRSharpen、`AEXCOMPAT_EXTENDED_DIAG=1`):

```
-stage:smart_render_cpu_end error=512        ← localizer 無し。ここで pr-gpu retry に入る
-stage:pr_gpu_route_begin … stage:pr_gpu_route_end reason=committed
+extended_diag:new_world -> 0
+extended_diag:fill -> 0
+stage:callback_denied callback=transform_world reason=matrix_not_finite
+extended_diag:transform_world -> 516 (callback_error)
+stage:smart_render_cpu_end error=516
```

つまり **VR family はもともと PF CPU path では render できておらず**、512 を
合図に pr-gpu 経路へ落ちて `rendered` になっていた。Localizer が入ると CPU path が
一歩進んで、`AE VR Effects Video Attributes Suite` (未実装) から取れなかった
field of view で行列を組み、非有限の行列を host に渡して 516 を受け取り、それを
自分の戻り値として返す。どちらの code で諦めるかは付随的で、fallback が見たい
性質は「CPU path がこのフレームを出せなかった」こと。判定を 512/516 に広げると
11 本は `rendered` に戻り、`pr_gpu_route` は `committed` のまま、
`detail.pixel_sha256` も一致する (実測)。

`VRSphereToPlane.aex` だけは元から失敗しており (`pr_gpu_route=render_fault`)、
報告される code が 512 → 516 に変わる。GPU 経路の fault は変わっていない。

### 計測 (full corpus、#1283 の変更)

母集団: AE 2026 `Support Files\Plug-ins\Effects` を `render_sweep` の引数に
渡した 304 AEX、`--depth` 既定 (8)、size/time/frames も既定。baseline は
main `7f88d210` (= #1285 の PR #1296 をマージした後。この branch も同じ commit を
マージ済み) を別 worktree で同じ手順で build し、**同一の sweep CLI バイナリ**で
測ったもの (worker 3 exe だけが違う)。

| bucket | baseline (7f88d210) | 変更後 |
| --- | --- | --- |
| rendered | 293 | 295 |
| frame_error:512 | 5 | 2 |
| frame_error:4 | 1 | 3 |
| frame_error:516 | 1 | 2 |
| not_discovered:exit_20_params_setup:13 | 2 | 0 |
| not_discovered:exit_12 | 1 | 1 |
| render_frame_failed:worker_invariant_failure | 1 | 1 |

bucket が動いたのは 5 本だけで、他の 299 本は baseline と同じ bucket
(突き合わせの key は `plugin_relative_path`):

- `3D Camera Tracker.aex`: `not_discovered:exit_20_params_setup:13` → `frame_error:4`
- `Stabilizer.aex`: `not_discovered:exit_20_params_setup:13` → `frame_error:4`
- `ColorAndContrast.aex`: `frame_error:512` → **`rendered`**
- `Curl_Noise.aex`: `frame_error:512` → **`rendered`**
  (この 2 本が動いたのは **Localizer** の効果。この commit の変更は 3 つ
  独立にあるので 3 つとも潰した: fallback 判定の変更は `xGPUFilterEntry` を
  export しない 2 本には届かず (`worker.pr_gpu_route` も記録されていない)、
  `PF AE Private Effect Suite` は `AEXCOMPAT_EXTENDED_DIAG=1` の trace で
  どちらも 1 度も acquire していない (`"PF AE Private Effect Suite"` の出現
  0 件)。残るのは Localizer だけ — いずれも観察)
- `VRSphereToPlane.aex`: `frame_error:512` → `frame_error:516` (元から失敗、上記参照)

silent-wrong の確認として、baseline で `detail.pixel_sha256` を持つ 293 record
すべてについて変更後の hash と突き合わせ、差分ゼロ・欠落ゼロを確認した
(変更後は新たに render できた 2 本が増えて 295 record)。`rendered` に留まった
VR 11 本は `worker.pr_gpu_route` が `committed` のままで、pixel hash も一致。
その 12 本 (VRSphereToPlane を含む) には新しい
`worker.pr_gpu_route_entered_from = cpu_bad_callback_param` が付く。

計測に使った build fingerprint (report JSON の `build`) と report の SHA-256:

| | baseline (7f88d210) | 変更後 |
| --- | --- | --- |
| l2_worker | `ec6d7fe4…` | `1d0b1ea7…` |
| classic_worker | `bf6d8946…` | `5287f57e…` |
| smart_worker | `f030fac1…` | `11a1fd6a…` |
| cli | `5d8279de…` | `5d8279de…` (同一) |

report JSON の SHA-256: baseline `454BAC68FFAE4C7B6385CADE7F1A289C93F8340BCB62719C392D5915D44C4B6A`、変更後 `1C7C87E0A2A64D0396DE10C3BEC889E199E88A8FA26EFE71BDB785E72C323E1B`。

### 残件

- ~~3D Camera Tracker / Stabilizer の render 段 (`frame_error:4`)~~ →
  slot 3 (`HostZStringToUTF16String`) を実装して両方 `rendered` になった
  (#1295)。`Analysis Host Suite` v2 は未実装のままだが、両 plug-in はその
  acquire 失敗を受け取ったうえで render を完了しており、「次はそこで止まる」
  という当時の推論は外れだった。
- suite の残り slot (0/1/4/5/6/7) の prototype、slot 5/6 の曖昧性、
  非英語 parity のための `LoadDVADictionaries`。→ #1291
  (slot 3 は #1295 で契約を確定して実装した)
- VR family の CPU path が非有限行列を作る件そのもの
  (`AE VR Effects Video Attributes Suite` 未実装)。fallback で `rendered` には
  なるが、CPU path は依然 render できていない。→ #1271
---

方針の正本は `CLAUDE.md`。計測手順は
`docs/SWEEP_INVESTIGATION_WORKFLOW_2026-08-17.md`。

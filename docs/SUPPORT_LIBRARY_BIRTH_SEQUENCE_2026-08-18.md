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

| step | 呼び出し |
| --- | --- |
| 3 | `BEZ_Birth()` |
| 4 | `FILE_AddExtensionMap(...)` x9 → `FILE_Birth()` |
| 5 | `M_Birth(3, 0)` |
| 6 | `RND_Birth()` |
| 7 | `PREF_Birth(flags>>1 & 1, false, "Adobe", "After Effects", "26.3", <path>, false)` |
| 8 | `VAL_Birth()` |
| 9 | `MC_Birth(mode != 0, 0xf118657, mode == 3)` |
| 10 | `COR_Conception(flags & 1)` |
| 12 | `COR_Birth(mode == 1 \|\| (flags>>3 & 1), flags & 1, mode == 3)` |
| … | `PLUG_Birth` / `TDB_Birth` / `PF_Birth(0)` / `P_Birth` / `TXT_Birth` / `TDL_Birth` / `PREM_Birth` / `PIN_Birth` / `SND_Birth` / `OM_Birth` / `MSK_Birth` / `FLT_Birth` / `PR_Birth` / `BEE_Birth` / `MEE_Birth` / `FIM_Birth` / `FLO_Birth` / **`U_SP_Birth()`** / `MC_SP_Birth` / `PT_Birth` |

このホストは従来 `U_Birth` だけを呼んでいた (#362 / #1063)。`PF_Birth` は
この作業と並行して PR #1284 (issue #1212) が入れた (`docs/PF_HOST_ABI_SHAPES_2026-08-18.md`)。
列の他の birth は
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

## 3. ホスト側の実装 (この issue で入れたもの)

- `minihost/src/worker_sweetpea_bootstrap.hpp` +
  `worker_host_suite_wiring.cpp`: Sweet Pea の起動を U.dll の `U_SP_Birth`
  優先にした。U.dll が無い closure では従来どおり ae_sweetpea を直接起動する。
  latch は U.dll の mapping をキーにするので、cluster session の最初の member に
  U.dll が無くても後続 member で birth される (#1267 の U_Birth latch と同型)。
  teardown は起動した層を通す (`U_SP_Death` か、ae_sweetpea の
  `SPShutdownPlugins`+`SPTerm` のどちらか一方)。
- `minihost/src/l2_main_support.inc` の
  `initialize_pin_support_libraries()`: mapping されている support library を
  AE と同じ順・同じ引数形で birth する
  (`BEZ_Birth` / `FILE_Birth` / `M_Birth(3,0)` / `RND_Birth` / `VAL_Birth` /
  `COR_Conception(false)`)。各 library ごとに独立の latch。
- `COR_Birth` は**呼んでいない**: この worker では戻ってこない (60 秒待って
  返らないことを実測。`COR_Conception` は 0 を返す)。21249 の原因は
  `COR_Conception` 側なので、そこまでで止めている。
- AE と初期化順が違う点 (観察): AE は BIB / Bravo のハンドシェイクを済ませて
  から step 10 で `COR_Conception` を呼ぶ。このホストの
  `initialize_pin_support_libraries` はプラグイン load 直後に走るので、
  `COR_Conception` の中の `dvabravoinitializer::InitBravoComponents(nullptr)` が
  **ホスト自身の `SetBIBProcAddress` + `InitBravoComponents` より先**に走る
  (ホスト側は最初の `AcquireSuite` が BIB suite を要求したときに走る)。
  そのため `InitBravoComponents` は 1 プロセスで 2 回呼ばれる。実測では
  ホスト側の 2 回目は非 0 の resolver を返し (extended diag の
  `dll=dvabravoinitializer.dll status=called result=0 resolver=<ptr>` に出る。
  Particle_Playground では `0x18000E480` で BIB.dll の mapping 内)、
  ProfileToProfile / PSL_Adjustments の discovery も通る。COR が受け取った
  resolver と同一ポインタかどうかはこの trace からは見えない (未確定)。

## 4. 計測 (full corpus)

母集団: AE 2026 `Support Files\Plug-ins\Effects` を `render_sweep` の引数に
渡した 304 AEX、`--depth` 既定 (8)、size/time/frames も既定。baseline は
origin/main `175c4844` を同じ worktree・同じ build 手順で測ったもの。

| bucket | baseline | 変更後 |
| --- | --- | --- |
| rendered | 284 | 284 |
| frame_error:512 | 9 | 10 |
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
(plug-in 単位で突き合わせ済み):

- `PSL_Adjustments.aex`: `not_discovered:exit_20_global_setup:21249` → `frame_error:4`
- `ProfileToProfile.aex`: `not_discovered:exit_20_global_setup:14` → `frame_error:512`
- `Particle_Playground.aex`: `not_discovered:exit_20_global_setup:11` → `not_discovered:exit_20_global_setup:2`

512 が 9 → 10 になったのは ProfileToProfile が render 段まで進んだ分で、
既存の 9 本は変わっていない。

## 5. 残件 (別 issue)

- Particle_Playground: `global_setup:11` → `global_setup:2` に移ったが未 discovery。
  `FUN_18000e900` の `U_SP_GetSPBasicSuite` は 0 を返すようになり、次の
  `FUN_180016a10` (string table を引きながら particle layer type を登録する
  ループ) 以降で 2 になる。trace 上、ホストの string table が返せた id は
  79 回の lookup 中 1 件 (`id=22 -> "AllMask"`) だけで、`id=234..335` は
  選択された LStr group (`$$$/AE/Playground/LStr/` は 0..231) に存在しない。
  Particle_Playground は `ext_alloc(out, 0x101b)` のように **resource id で
  string table を取得**しており、ホストはその id を無視して 1 group だけを
  返している。ここが次の境界 (観察)。
- PSL_Adjustments: discovery は通るが render で `frame_error:4`。
- PSL_Adjustments を `aex_l2_worker.exe --l2-params-only` で直接叩くと、
  report (`status=parameters_inspected`、5 params) を書き終えたあと process
  終了時に access violation で落ちる (exit 0xC0000005)。COR PSL thread /
  AdobePIE が生きたまま atexit の `teardown_pica_components` に入るためと
  思われる (推論)。**この exit code の汚れは one-shot 直叩き経路だけで、
  multifilter の discovery 経路には出ていない**: cluster session (full sweep) でも
  singleton (`render_sweep --filter PSL_Adjustments`、member 1 本) でも
  `frame_error:4` = discovery 成功で、`worker failed` 系の分類にはならない
  (どちらも実測)。
- `COR_Birth` がこの worker で返らない件。
- 3D Camera Tracker / Stabilizer の `PF AE Private Effect Suite` v3/v5。

---

方針の正本は `CLAUDE.md`。計測手順は
`docs/SWEEP_INVESTIGATION_WORKFLOW_2026-08-17.md`。

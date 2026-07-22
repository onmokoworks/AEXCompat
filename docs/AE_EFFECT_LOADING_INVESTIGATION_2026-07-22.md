# AE エフェクトの互換ホストロード調査 (issue #304)

2026-07-22。AviUtl2 multi-filter (#295/#303) で「After Effects のプラグインフォルダを
既定スキャンし、AE の全エフェクトを AviUtl2 のキーフレーム可能フィルタにする」を目指した際、
**本物の AE エフェクトの大半が互換ホスト (AEXCompat) でロードできない**理由を根本まで
掘った記録。将来この解錠に着手する際の一次情報。観察 (事実) と仮説 (推論) を分けて記す。

## 症状 (観察)

- AE 2025 `Support Files\Plug-ins` (再帰 353) + `Common\Plug-ins\7.0\MediaCore` (再帰 210)
  ≈ 563 の `.aex` を discovery すると、**ロード成功は ~25 のみ**。残りは失敗。
- 失敗の diagnostics: `"classification":"nonzero_exit","exit_code":11`、`stage_events:[]`、
  `peak_process_memory_bytes` ≈ 1.4MB (worker がほぼ起動直後に終了)。
- 当初 8 並列 discovery では 563 中 380 が `timeout` に見えたが、これは並列競合の偽陽性
  (後述)。

## 誤った初期仮説と否定

1. **「非effect だから」** → 否定。`Support Files\Plug-ins\Effects\` にある 3D Camera Tracker /
   AddGrain / Bilateral 等は紛れもない本物のエフェクト。
2. **「並列タイムアウトが本質」** → 部分否定。直列 (1件ずつ) discovery では各 ~1.5〜3.4s で完了し、
   タイムアウトは 30 件中 1 件 (Arithmetic ≈ 3%) のみ。8 並列 (68%) のタイムアウトは
   **資源競合の偽陽性** (8つの重い AE DLL 同時ロードでディスク/メモリ/CPU 競合 → 各 discovery が
   5s の worker 期限を超える)。5s 期限は discovery 1件ごとで、全体の実行時間の予算ではない。
3. **「依存 DLL が PATH に無いから」** → 否定。AE の Support Files を PATH に入れても exit 11 のまま
   (worker のロードフラグが PATH を探索対象に含まない)。

## 根本原因 (確定)

### exit 11 = LoadLibrary 失敗

`minihost/src/worker_runtime_admission.cpp:38-40`:
```cpp
SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32 | LOAD_LIBRARY_SEARCH_USER_DIRS);
HMODULE module = LoadLibraryExW(plugin_path, nullptr,
    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
if (!module) return 11;
```
依存探索先は「プラグイン自身のフォルダ + System32」のみ。

### broker が aex を依存から引き剥がしてステージングする

`broker/crates/broker/src/secure_image_dispatch.rs:107` の
**`SealedLoadTree::create(main, dependencies)`** が aex を隔離ツリー (temp のランダム root) に
ステージし、worker はそこからロードする (`sealed_load_tree.rs` の `populate` は main と依存を
**同一 root フォルダ**に配置)。封入されるのは**呼び出し側が渡した `dependencies` だけ**。

multi-filter は `inspect_experimental_with_diagnostics(repo, plugin, sha)` を**依存空**で呼ぶ
→ sealed tree に aex 単体のみ → AE エフェクトが要する `dvacore.dll` 等の Adobe runtime DLL が
隔離先に無い → `LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR` (=依存の無い隔離先) で依存解決できず失敗。

### 証明

- 失敗エフェクトの import に `dvacore.dll` 等。`dvacore.dll` は AE の `Support Files\`
  (Effects\ の親、worker の探索対象外) にある。
- **AddGrain.aex + AE の Support Files 全 DLL (397) を1フォルダに置き、worker と同一フラグ
  (`SEARCH_DLL_LOAD_DIR | SEARCH_SYSTEM32`) で PowerShell から `LoadLibraryExW` → 成功**
  (`LOADED ok`)。DLL 自体は正常で、依存が隣接すればロードできる。worker が失敗するのは依存を
  剥がした隔離先からロードするため。
- ロードできた ~25 は Adobe runtime 依存が無く自己完結 (ntsc-rs / probe / 一部 AE)。

## 解錠の試み: 依存クロージャ封入 (実装・検証済み)

`bridges/aviutl2-multifilter/examples/discover_with_deps.rs` (診断用 example):
aex の PE import を再帰的に辿り、AE の Support Files で解決した DLL 群 (依存クロージャ) を
`ApprovedImageArtifact` (path + sha256 + size) として
`inspect_experimental_with_approved_dependencies_and_diagnostics(repo, plugin, sha, deps)`
に渡す。

- 依存クロージャは管理可能な数: **AddGrain → 15 DLL、Bilateral → 28 DLL**
  (dvacore/dvaui/ae_sweetpea/vulcan/boost/dynamiclink 等)。397 全部ではない。
- 封入すると worker メモリが 1.4MB → **7〜10MB** に増え、**exit 11 (LoadLibrary 失敗) を突破**。

## 層別のブロッカー (2026-07-22 午前時点の整理)

| 層 | 内容 | 状態 |
|---|---|---|
| **L1** | LoadLibrary 失敗 (exit 11) | **解決** — 依存クロージャを sealed tree に封入 |
| **L2** | sealed tree の module audit (evidence-tier) が未認証モジュールを拒否 (`module_audit_failure`, `unknown_count:2`, exit 14) | 未 — runtime module authorization policy (`inspect_experimental_with_runtime_policy` 系 / `authorize_runtime_modules` / `parse_runtime_module_authorization`) が要る |
| **L3** | Adobe runtime のスタンドアロン初期化ハング (dvacore→VulcanMessage5/dvanet の IPC/ネットワークが実 AE 環境を待つ; AddGrain は L2 を越えても `timeout_killed`) | 未 — 実 AE の IPC 環境の模倣/スタブが必要。**最難関** |

> **訂正 (2026-07-22 午後)**: L2 の欄の「runtime module authorization policy が要る」は
> 仮説であり、後述の実測で**否定**された。unknown な 2 モジュールは Adobe のモジュールでは
> なく、WinSxS 上の OS side-by-side assembly (`COMCTL32.dll` / `gdiplus.dll`) だった。
> 詳細は「実測 (2026-07-22)」節を参照。

## 関連コード / API

- 失敗判定: `minihost/src/worker_runtime_admission.cpp` (exit 11 = `!module`, 15 = 認証パース失敗,
  14 = module audit / timeout)。
- ステージング: `broker/crates/broker/src/sealed_load_tree.rs` (`create`/`populate`、main+deps を
  同一 root に配置、basename 一意・reparse 拒否・manifest ハッシュ)。
- 依存付き discovery: `image_render.rs` の
  `inspect_experimental_with_approved_dependencies_and_diagnostics` /
  `inspect_experimental_with_runtime_policy`。
- `ApprovedImageArtifact { path, expected_sha256:[u8;32], expected_size:u64 }`
  (`secure_image_dispatch.rs`)。
- 診断用 example: `discover_timing.rs` (直列/所要時間)、`discover_with_deps.rs` (依存クロージャ+封入)。

## 将来の実装方針 (仮説)

1. **依存解決**: aex の import を再帰クロージャで辿り、AE の `Support Files\` 等で解決
   (実装容易、検証済み)。
2. **L2 突破**: 解決した依存モジュールを runtime authorization policy として渡し module audit を
   通す。デフォルト tier での監査要否は CLAUDE.md の tier 方針と要整合。
3. **L3 突破**: dvacore→vulcan/dvanet の IPC 初期化を、スタブ/ダミーの Vulcan メッセージ層で
   満たすか、初期化を回避する。ここが最難関で、動く保証は無い。
4. **収穫の見積り**: 自己完結エフェクト (~25) は解錠不要。dvacore 系は L2+L3 次第。L3 の壁ゆえ
   「数百が動く」筋は現時点で不確実。**まず vulcan/dvanet 非依存だが Adobe DLL は引く中間層の
   エフェクトが L2 のみで動くか**を測るのが次の低コストな検証。

## スコープ整理

- multi-filter (#295/#299/#303) 自体は正しく動作 — 互換ホストがロードできるものを公開している。
- 本件 (dvacore 系 AE エフェクトの解錠) は互換ホストのコア機能 (依存封入 + runtime policy +
  Adobe IPC 互換) に触れる**多層プロジェクト**であり、multi-filter とは別。

---

# 実測 (2026-07-22 午後、依存クロージャの本実装後)

依存クロージャ解決を example から broker の機能
(`broker/crates/broker/src/plugin_dependency_closure.rs`) に移し、multi-filter の discovery /
render session に配線したうえで、AE 2025 の全プラグインフォルダを 2 回 (封入あり / なし)
sweep した実測。ツールは `bridges/aviutl2-multifilter/examples/discover_sweep.rs`。

## 実行環境 (観察の前提)

- After Effects 2025 (`Support Files\Plug-ins` 再帰 353、`Common\Plug-ins\7.0\MediaCore` 210)。
- worker はこの checkout を Ninja/Release でビルドしたもの。inspect の worker 期限は 5s、
  sweep は直列 (並列競合による偽タイムアウトを避けるため)。
- 依存探索フォルダは「AEX 自身のフォルダ → AE の `Support Files`」の順。

## 結果 1: exit 11 (LoadLibrary 失敗) は消えた

`Support Files\Plug-ins` 353 件:

| bucket | 封入なし | 封入あり |
|---|---|---|
| `exit_11` (LoadLibrary 失敗) | **222** | **0** |
| `exit_12` (Effect ではない: AEGP / 不正 PiPL / entrypoint 無し) | 32 | 87 |
| `exit_20` (ロード・dispatch はできたが lifecycle 契約で fail) | 94 | 94 |
| `module_audit_failure` (exit 14) | 5 | 88 |
| `closure_error` (クロージャが封入上限超過) | 0 | 84 |
| `loaded` (パラメーター discovery 成功) | **0** | **0** |

プラグイン単位の遷移 (352 件で突合。1 件は basename 重複で突合対象外):
`exit_11 → closure_error` 84、`exit_11 → module_audit_failure` 83、`exit_11 → exit_12` 55、
`exit_20 → exit_20` 93、`exit_12 → exit_12` 32、`module_audit_failure → module_audit_failure` 5。
**退行 (封入で悪化した件) は 0**。

`Common\Plug-ins\7.0\MediaCore` 210 件は封入あり/なしで完全に同一 (`exit_20` 207、
`module_audit_failure` 1、`unparsed_error` 2)。MediaCore の `.aex` は AE の `Support Files` から
何も import しておらず、解決されたクロージャは全件 0 モジュールだった。**#304 は MediaCore には
効かない**。

重要: **「exit 11 が消えた」= 「エフェクトが使えるようになった」ではない**。353 件中
`loaded` は封入前後とも 0。exit 11 だった 222 件の行き先は、Effect ですらなかった 55 件を除くと
L2 (module audit) と封入上限であり、その先に L3 が残っている。加えて、この sweep を走らせた
worker ビルドは selector dispatch 自体が落ちる状態 (#318、結果 4 を参照) なので、
`exit_20` と `loaded` の数字は互換性の指標として読めない。読めるのは dispatch より前の層
(`exit_11` / `exit_12` / `module_audit_failure` / `closure_error`) だけ。

## 結果 2: L2 の unknown モジュールは Adobe ではなく WinSxS の OS assembly (旧仮説の否定)

worker の module audit は unknown の**個数**しか報告しない。そこで
`bridges/aviutl2-multifilter/examples/stage_closure.rs` でクロージャを 1 フォルダに展開し、
`tools/observe-staged-aex-modules.ps1` が worker と同じフラグでロードして、読み込まれた
モジュールの出所を worker と同じ規則で分類した。

AddGrain / Bilateral / AutoLevels / Basic_3D の 4 件すべてで、unknown はちょうど 2 件、
しかも**同一の 2 モジュール**だった:

```
C:\WINDOWS\WinSxS\amd64_microsoft.windows.common-controls_..._none_...\COMCTL32.dll
C:\WINDOWS\WinSxS\amd64_microsoft.windows.gdiplus_..._none_...\gdiplus.dll
```

- これは worker の audit が報告する `unknown_count: 2` と一致する。
- **仮説の否定**: 当初の「Adobe の依存モジュールを runtime module authorization policy で
  認証する必要がある」は、少なくともこのクラスの failure については誤り。封入した Adobe DLL は
  sealed root 直下にあるので `plugin` に分類され、監査を通っている。落ちているのは
  Windows の side-by-side assembly (activation context 経由で WinSxS から解決される
  COMCTL32 / GDI+) であり、audit が System32 直下しか OS モジュールとして認めていないため。
- したがって L2 は「Adobe 用の policy 機構」ではなく「**audit に WinSxS の OS assembly クラスを
  足す**」問題。フォローアップとして別 issue に切り出した (#315)。

## 結果 3: 封入コストと現実性 (クロージャの大きさ)

`--survey-only` (クロージャの大きさだけを数える。worker は起動しない) で 353 件を計測:

- クロージャのモジュール数: 中央値 **2**、224 件が **0** (自己完結)、p90 は 181。
- 封入できた 141 件は 1〜61 モジュール / 最大 **220 MB**。sealed tree のコピー + ハッシュ込みで
  discovery 1 件あたりの中央値は 0.8s (依存なしは 0.13s)、最大 3.2s。**5s の worker 期限には
  影響しない** (封入は launch 前に完了する)。
- 一方 84 件は 96〜225 モジュール / 最大 **約 1.0 GB** を要求する。これらは
  `Scripting.aex` / `MediaBrowser.aex` / `EssentialGraphics.aex` / `CEPManager.aex` など AE 自身の
  UI・拡張プラグインで、実質 Adobe ランタイムほぼ全体を引く。
- 実装は 64 モジュール (`MAX_SESSION_DEPENDENCIES`) / 1 GiB で **fail-closed** にした。
  上限を 128 に上げてもこの層は救えない (大半が 96 以上) 一方、1 件の discovery ごとに
  1 GB をコピー + 3 回ハッシュすることになる。**sealed copy 方式は Adobe ランタイム全体を
  引くプラグインには構造的に合わない**というのがこの計測の結論で、そこを解錠したければ
  「コピーしない封入」(共有 sealed runtime tree、あるいは default tier での
  `AddDllDirectory` + audit のクラス拡張) の設計が要る。

## 結果 4: `exit_20` は AE エフェクト固有ではなく worker 側の条件だった (この節の当初の結論を訂正)

封入前後で変わらない `exit_20` (AE 94 件 + MediaCore 207 件) を、当初は「ロード・dispatch は
できているが AE エフェクトの lifecycle 契約を満たせない互換性ギャップ」と読んだ。これは**誤り**
だった。

リポジトリ自身の probe (`instruments/pf-param-utils-animation-probe`。GLOBAL_SETUP は 2 行
代入して `PF_Err_NONE` を返すだけ) を同じ params-only 経路に通すと、AE エフェクトと**まったく
同じ** `global_setup error=512` / `params_setup -1` / `out_flags 0` になる。worker のレポートは
`last_seh_selector: GLOBAL_SETUP` / `last_seh_error: 512` を出しており、512 は
`minihost/src/worker_selector_dispatch.cpp` の `kAuditFailure` = `invoke_entry_seh` の SEH 例外
経路の値。`out_flags` が 0 のままなので、**プラグイン本体は一度も実行されていない**。

つまりこの sweep を走らせた worker ビルドでは、どのプラグインでも最初の selector 呼び出しが
例外で落ちる。`loaded` が 0 件だったのもこれで説明がつく。sealed tree の有無 (module audit の
要否) にも、作業中の変更にも依存しない (clean な main でも同一)。切り分けと再現手順は #318。

**この計測から読み取ってよいのは、selector dispatch より前の層だけ** — すなわち
`LoadLibraryExW` (結果 1) と admission 時の module audit (結果 2)、およびクロージャのサイズと
封入コスト (結果 3)。dispatch 以降 (`exit_20` / `loaded`) の数字は #318 の条件下のものなので、
AE エフェクトの互換性の指標としては使えない。#318 の解決後に再測が要る。

## セキュリティ整合 (CLAUDE.md の tier 方針との突き合わせ)

依存クロージャの解決は「何を封入するか」を決めるだけで、**認証の緩和はしていない**。

- 解決したモジュールは `session_dependency_manifest::validate` にそのまま通す。手書きで
  dependencies を渡した場合とまったく同じ検証 (絶対パス、Windows-safe basename、basename の
  大文字小文字を含む衝突拒否、reparse point 拒否、single-link 検証、サイズ + SHA-256 の再照合)
  を受け、その後 `SealedLoadTree` がコピー・再ハッシュ・ハンドル保持で固定する。
- **worker の DLL 探索パスは広げていない**。探索フォルダはブローカー側 (この解決器) の入力で
  あって、worker には渡らない。worker のロードフラグは従来どおり「sealed root + System32」。
- プラグイン内の import 名は攻撃者制御データとして扱う。ディレクトリに結合する前に
  「区切り文字なし・ドライブレターなし・制御文字なし・予約デバイス名でない ASCII の basename」
  を要求し、外れたものは解決対象外にして**名前自体もレポートに出さない** (件数のみ)。
  `..\` 形式の import 名でルート外に出られないことは単体テストで固定した。
- 上限超過は fail-closed。黙って切り詰めると、この機構が消したはずの不透明なロード失敗として
  再び現れるため。
- tier との関係: これは crash-containment tier でも evidence tier でも成立する。前者では
  「同梱した依存の識別子を記録する」provenance、後者では sealed tree の manifest ハッシュに
  依存が含まれるので、どのバイトが同居してロードされたかが receipt に紐づく。**識別子の固定を
  緩めて通す方向の変更は入れていない**。

## 実測を踏まえた層別の更新

| 層 | 内容 | 状態 |
|---|---|---|
| **L1** | LoadLibrary 失敗 (exit 11) | **解決** (222 → 0)。依存クロージャの解決・封入は broker の機能として実装済み |
| **L1'** | クロージャが封入上限 (64 モジュール / 1 GiB) を超える | 未。353 件中 84 件。sealed copy 方式の構造的限界 (結果 3) |
| **L2** | module audit が WinSxS の OS assembly (COMCTL32 / gdiplus) を unknown 扱い | 未 (#315)。旧仮説「Adobe モジュールの runtime policy が要る」は否定 |
| **L3** | Adobe runtime のスタンドアロン初期化ハング (dvacore→VulcanMessage5/dvanet) | 未検証のまま。L2 を越えないと再測できない。最難関という評価は変えていない |
| **別軸** | selector dispatch が SEH 例外で落ちる (`last_seh_error: 512`)。プラグイン非依存 | 未 (#318)。#304 の解錠とは独立。これが解けるまで dispatch 以降の再測はできない |

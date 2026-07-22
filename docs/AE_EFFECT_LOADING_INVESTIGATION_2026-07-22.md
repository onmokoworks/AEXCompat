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

## 層別のブロッカー (現状)

| 層 | 内容 | 状態 |
|---|---|---|
| **L1** | LoadLibrary 失敗 (exit 11) | **解決** — 依存クロージャを sealed tree に封入 |
| **L2** | sealed tree の module audit (evidence-tier) が未認証モジュールを拒否 (`module_audit_failure`, `unknown_count:2`, exit 14) | 未 — runtime module authorization policy (`inspect_experimental_with_runtime_policy` 系 / `authorize_runtime_modules` / `parse_runtime_module_authorization`) が要る |
| **L3** | Adobe runtime のスタンドアロン初期化ハング (dvacore→VulcanMessage5/dvanet の IPC/ネットワークが実 AE 環境を待つ; AddGrain は L2 を越えても `timeout_killed`) | 未 — 実 AE の IPC 環境の模倣/スタブが必要。**最難関** |

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

## #315 対応 (2026-07-22)

WinSxS の OS side-by-side assembly は、`WinSxS\\<assembly>\\<module.dll>` の2段だけを
`winsxs` 分類として扱うようにした。worker の `module_audit` JSON と broker validator、
GPU module policy validator の分類を同期し、canonical path、reparse point、basename、
hash/size の既存 fail-closed gate は維持する。その他の場所は引き続き `unknown` として拒否する。

focused pytest、broker focused Rust tests、native worker Release build、L2/render/smart の
native self-test は確認済み。実 AE 83件の再走査と GitHub Actions の最終判定は外部環境/CIの
確認範囲として残る。

# ブランチ集約ログ (2026-09-03 開始)

リポジトリオーナー `onmokoworks` の GitHub アカウントが停止され、Issue / PR /
CI が使えなくなった期間に、未マージのブランチを PR を経由せずローカルで
main に取り込む作業の記録。時系列で追記する。結論が覆った場合も古い項目は
消さず「訂正」を追記する。観察 (事実) と仮説 (推論) は分けて書く。

Issue の claim コメントと `Closes #N` が書けないので、CLAUDE.md の
「Issue Claim and PR Linking」の代替としてこのファイルに作業単位を記録する。
レビューは CLAUDE.md どおり、マージごとに別エージェントの adversarial review
を回し、CI 相当の検証をローカルで通してから main を進める。

## 2026-09-03 観察: 停止時の状態

- API (`gh api repos/onmokoworks/AEXCompat`, `users/onmokoworks`) は 404。
  GraphQL も "Could not resolve to a Repository"。Issue / PR 本文 / レビュー /
  Actions の結果はどこからも読めない。
- git の SSH 経路は生きている。`git fetch` / `ls-remote` は成功し、
  `refs/pull/*` が 737 本広告されている。使い捨てブランチ
  `zz-access-probe-20260903` を push → `ls-remote` で確認 → 削除、まで通ったので
  書き込みもできる (実測、2026-09-03)。
- origin/main の最終更新は 2026-08-21 20:20 JST の `4912b28f` (PR #1536 の
  マージ)。ローカル main は `1a56b7e4` (PR #1209、08-12) で止まっていて、
  worker desktop 周りの未コミット変更 5 ファイルが乗っていた。
- mirror バックアップ:
  `C:\Users\naari\src\github.com\onmokoworks\AEXCompat-mirror-20260903.git`
  (`git clone --mirror`、refs 841 本 = heads 104 + pull 737)。Issue / PR の
  コメント本文は git に含まれないので、これでは復元できない。
- 仮説: SSH 経路が開いているのは停止処理の反映遅れの可能性があり、いつ
  閉じるか分からない。mirror は `git -C <mirror> remote update` で随時更新する。

## 2026-09-03 処置: ローカル main の整理

- 未コミット変更を `wip/worker-desktop-20260903` (`f76fc9e2`) に退避してから
  main を `4912b28f` に fast-forward した。

## 2026-09-03 棚卸し: main 未マージのブランチ 42 本

分類の根拠: `git cherry main <branch>` で patch-id が main に無いコミット数
(novel)、`git merge-tree --write-tree` の conflict 有無、mirror に tip が
あるか (GitHub 上に存在するか)、main のログでの issue 番号言及回数。

### 層 0: 中身が既に main にある (novel=0)。マージ不要 (9 本)

`fix-duplicate-env-key`, `issue674-dynamic-layer`,
`enforcement-audit-2026-08-05`, `issue728-single-floor-docs`,
`issue657-header-dep-tracking`, `issue668-unique-filenames`,
`issue680-noload-retirement`, `issue681-suite-cleanup`,
`issue691-drop-source-text`

### 層 1: 直近 (08-20〜21)、乖離が小さい (6 本)

| branch | novel | merge | 備考 |
|---|---|---|---|
| `issue1537-ffi-prebuilt` | 2 | clean | レビュー対応済みの状態で停止 |
| `codex/issue1446-remove-workflow-string-test` | 1 | clean | 削除対象ファイルは main に既に無い (PR #885)。実質 no-op |
| `codex/issue1475-rust-test-shards` | 1 | conflict | CI workflow + `run-broker-rust-tests.py` |
| `issue1134-load-library-w` | 2 | conflict | guest x64 imports |
| `bee-scope` | 1 | clean | WIP、#1264 レビュー 3 周目。この機にしかない |
| `wip/worker-desktop-20260903` | 1 | conflict | 退避した未コミット分、WIP |

### 層 2: 08-06〜12 の codex 系、900 commits 以上遅れ、conflict (9 本)

`codex/integrate-1162-world-io` (novel 17)、
`codex/issue851-smart-primary-checkout` (novel 10、1162 と大半重複)、
`codex/issue1135-typed-iterate-suites`、`codex/issue1113-macos-exr`、
`codex/issue983-seh-diagnostics`、`codex/olm-win64-stack-args`、
`codex/issue388-resolve-ofx` (clean だが Resolve OFX という別ホスト面の追加)、
`multifilter-ae-builtin-categories-876` / `multifilter-menu-label-871` (同一 commit)

### 層 3: 7 月〜08-05、1000〜2200 commits 遅れ、conflict (18 本)

issue 番号が main でその後何度も言及されていて、別実装で解決済みの可能性が
高い (仮説)。個別に superseded かどうかを判定して報告する。

`issue335-ci-greenable`, `issue339-session-audio-sidecar`,
`issue353-layered-argb32f`, `worktree-issue351-worker-desktop`,
`backup-issue351-full-impl`, `issue664-ci-speedup`, `issue660-scan-depth`,
`issue671-ninja-worker`, `issue686-simd-unpack`,
`codex/issue404-sweep-parallel`, `codex/olm-suite-followup`,
`codex/pointparam-suite`, `codex/docs-refresh`,
`issue725-classic-setup-error` (実体は issue777 の WIP),
`worktree-claim-convention` (#21 WIP), `issue8-smartfx-geometry-probe`,
`issue743-linux-pytest-probe` (「一時 workflow」と本人が記述)

### 決定 (2026-09-03、オーナー naari3)

- 層 1 + 層 2 と `wip/worker-desktop-20260903` をマージ対象にする。
- マージごとに origin/main へ push する (SSH で通ることは実測済み)。
- 層 3 は個別判定して報告し、残す価値のある差分だけ後で提案する。
- 検証は CI (`.github/workflows/windows-clean-clone.yml`) の全 job を
  ローカルで再現したスクリプトで行う。内容: rustfmt 全木チェック、
  broker `cargo build`/`cargo test --workspace --locked`、bridges 3 workspace の
  `cargo test`、`tools/build-native.ps1` + `verify-minihost-build-deps.ps1` +
  instruments (trace_writer_selftest, abi_layout_probe)、probe AEX 26 本の
  ビルド、SDK fixture (Backwards / GrabbA / AEGP Render Options / PF Adv Time)、
  `pytest --collect-only --validate-local-artifact-manifest`、
  `tools/run-python-ci-tests.py main` + skip 漏れ検査、
  `run-python-ci-tests.py classic-evidence`、`generate-third-party-licenses.py`。
  CI と違うのは sccache の GHA バックエンドと nextest の分割 (ローカルは
  `cargo test` 一括) の 2 点。

## マージ記録

(マージごとに、対象ブランチ、conflict 解決の要点、ローカル CI 結果、
review で受けた指摘と採否、マージ commit と push 先 SHA を追記する)

### 2026-09-03 判定: `codex/issue1446-remove-workflow-string-test` は取り込み不要

観察: ブランチの唯一の変更は `tests/test_windows_clean_clone_workflow.py` の
削除で、そのファイルは main に既に無い (PR #885 で削除済み)。マージしても
差分ゼロ。ブランチ削除候補。

### 2026-09-03 判定: `codex/issue1475-rust-test-shards` は superseded

観察: ブランチは `tools/run-broker-rust-tests.py` の `run_independent` を
`cargo test` 呼び出し単位で core / integrations に分割し、CI job を matrix
化する。main はその翌日 (08-21、`04d510bb` "Reuse a shared nextest archive in
CI"、`c85330c7` "Split native-dependent Rust CI tests") に runner を nextest
archive + filterset 方式へ置き換えていて、分割対象の関数自体が無い。
3 ファイルすべてが conflict。
推論: 取り込むには nextest filterset の上で分割を設計し直す必要があり、
それは新規作業であってマージではない。取り込まない。archive 方式でも
independent partition の実行時間が問題なら別途検討する。

### 2026-09-03 判定: `wip/worker-desktop-20260903` はこのままでは取り込まない

観察: 退避した `worker_dialog.rs` と `windows_process.rs` の blob は、PR #1196
(`578bf83f`、issue #1194 の「worker desktop を broker プロセスで 1 枚共有」)
の直前の版と byte 単位で一致する (`git log --find-object`)。削除されていた
`dummy_desktop_probe.rs` / `tests/shared_desktop.rs` も #1196 が足した
ファイル。docs 側は #1194 の根拠段落 (per-worker desktop で DWM composition
state がリークする) を削っている。
推論: #1196 の機械的な revert を作業ツリーに展開した状態で、#1194 の決定を
覆す新しい根拠は書かれていない。共有 desktop が原因かどうかを切り分ける
実験だった可能性が高い。決定済みの変更を理由なしに戻すことになるので、
オーナーが意図を示すまでマージ対象から外す。ブランチは残す。

### 2026-09-03 マージ 1: `issue1537-ffi-prebuilt` → `a078a290` (consolidate/main 上)

- conflict なし。
- review 指摘: (1) should-fix: `AEXCOMPAT_HOST_CORE_FFI_DLL` に相対パスを
  渡すと存在確認は呼び出し元 cwd、gate exe は `Push-Location $buildRoot`
  後の cwd で解決するので食い違う。採用、`ef056d2f` で 4 スクリプトとも
  絶対パスに正規化し、env 指定時のエラー文言を分けた。(2) nit: 同じ文言が
  env 指定時に誤解を招く。(1) と一緒に採用。(3) nit: `rust-release-ffi` job
  の `save-cache: 'false'` 固定。却下: 兄弟 job (`rust-test-archive`,
  `rust-doctest`) と同じ扱いで、実際の再利用は sccache 側。CI 自体が今は
  走らないので現状維持。
- ローカル CI: main への ff 後にまとめて実施 (下記)。

### 2026-09-03 ローカル CI スクリプトの訂正

ベースライン実行で `gpu-input-fixture` step が exit 2 で落ちた。原因は
`generate-oracle-rgba-input.py` が既存ファイルの上書きを拒否する仕様で、
CI は clean clone なので当たらない。スクリプト側で生成前に前回の出力を
消すよう直した。コード側の不具合ではない。

### 2026-09-03 ベースライン: main `4912b28f` のローカル CI 結果

- Rust (fmt / broker build+test / bridges 3 つ)、native (workers, deps
  verify, instruments)、probe AEX 26 本、SDK fixture 4 種、classic-evidence:
  すべて PASS。
- pytest `main` partition: 初回は 32 失敗したが、CI の「Mirror workers into
  expected build layouts」「Restore built-artifact freshness ordering」を
  スクリプトが省いていたのが原因 (worker exe が期待レイアウトに無い)。
  両 step を足した再実行で 1 失敗、その 1 件
  (`test_the_committed_observation_is_what_the_probe_prints`) は freshness
  step が target/ 配下の全 exe を同じ mtime にした結果、この checkout に
  残っていた古い `abi_layout_probe.exe` (target/abi-layout-probe-build) が
  「最新」に選ばれたもの。clean clone の CI には無い状況。
  `AEXCOMPAT_ABI_LAYOUT_PROBE` で instruments ビルドの probe を指した
  再実行で 896 passed / 0 failed / 22 skipped (skip 漏れ検査も PASS)。
- license-audit: `generate-third-party-licenses.py` が
  「snapshot differs」で失敗。packages / licenses は一致していて、差は
  `cargo_lock_sha256` のみ。snapshot の値は `broker/Cargo.lock` を CRLF に
  変換したバイト列の SHA-256 と一致した (実測)。つまり snapshot は
  autocrlf=true の checkout で生成されていて、この機 (autocrlf=input、LF)
  では一致しない。hosted runner の Git for Windows は autocrlf=true が既定
  なので CI では通っていたと思われる (仮説)。スクリプトが Cargo.lock の
  生バイトを hash しているのが原因で、改行正規化してから hash すべき。
  マージ作業とは無関係の既存問題なので follow-up として記録し、ローカル
  CI ではこの step の失敗を既知として扱う。

### 2026-09-03 マージ 2: `origin/issue1134-load-library-w` → `93b6b2c8`

- conflict: `guest/.../x64/types.rs` の `GuestState` に main が `crt_errno`、
  ブランチが `windows_module_refcounts` を足していたので両方残した
  (`GuestState` は `#[derive(Default)]` なので初期化箇所の追加は不要)。
- guest workspace `cargo test`: 400 passed。
- review: blocking なし。follow-up として記録 (Issue が立てられないため):
  (1) `LoadLibraryW` (完全一致のみ) / `LoadLibraryExW` (basename 一致) /
  `GetModuleHandleW` (basename + ntdll) で名前解決ポリシーが 3 通りある。
  (2) `windows_module_refcounts` は `LoadLibraryW` だけが増やし、
  `FreeLibrary` 相当も他の loader も触らない部分モデル。
  (3) 未知モジュール拒否時に拒否した名前を診断に残していない
  (main の `LoadLibraryA` と同じ挙動ではある)。
  nit: `ERROR_NOT_ENOUGH_MEMORY` が x64.rs に追加され imports.rs の関数
  ローカル定義 4 箇所が死んでいる。いずれも設計判断を伴うので今回は
  触らない。

### 2026-09-03 マージ 3: `bee-scope` → `4d82796a` + 修正 2 commit

- conflict なし。broker `cargo test` 612 → 修正後 627 passed。
- review 判定: 「未完成の痕跡なし、指摘を直せば landable」。
  採用: (1) render session の swap で attribution window を swap hook の
  後に開いていたため、incoming plug-in の GLOBAL_SETUP / PARAMS_SETUP での
  facade 呼び出しが前の plug-in に付いていた → hook の前に移動
  (`62add92d`)。(4) `layer_vtable_calls` キー欠落を non-array と同じ
  malformed 扱いにし、slot index に上限 (1024) を付けた (同 commit)。
  (2)(3) はコメントで明示する形で採用: cluster close の block は close
  時点の plug-in を指すこと、その経路では `unsupported_suite_calls` が
  伝播されないこと。
  却下/保留: (5) `Counters counters()` が selftest 以外に消費者なし
  (API 面の整理は別件)。(6) `docs/BEE_SCENE_OBJECT_ABI_2026-08-17.md` の
  Timecode 証跡に `bee_facade` への参照を足す提案 (文書更新は別件)。
- ブランチ側の `diagnostics.rs` が rustfmt 未適用だったので整形 commit を
  追加。全木 rustfmt check PASS。
- 修正 commit の再レビューを実施中 (結果は追記)。

### 2026-09-03 判定: `codex/integrate-1162-world-io` はマージせず cherry-pick 候補を切り出す

commit 単位で main と照合 (別エージェントの調査、根拠は main の commit /
file:line で確認済み):
- ALREADY-ON-MAIN 11 本: UCRT tolower/toupper、GetModuleHandleExA、
  GetModuleFileNameW (08-16 の x64 import 系列)、typed PF iterate suites
  (`1970f589`)、Apple Silicon FLOAT32 EXR (`afe3f2c6`)、declarative render
  checkpoints (`f9e0e16c`)、declarative fixtures on Apple Silicon /
  macOS fixture world identity / lround (PR #1184)。
- SUPERSEDED 1 本: `d76075aa` LoadLibraryW (basename 一致) は main の
  `b8f229dd` (完全一致、マージ 2) の方が厳しく、取り込むと退行。
- MISSING 5 本 (約 550 行): `94210e51`+`40c3348c` (guest classic の
  `PARAM_ARBITRARY_DATA` default materialization と ARBITRARY_CALLBACK
  disposal)、`3ea396a1` (smart layer checkout を disk id で解決; main は
  まだ positional)、`37bdeae9`+`6561b11b` (trace の pointee watch と
  checkpoint capture mode; #1160 とは層が違い重複しない)、`642b3c7b`
  (harness macos.rs で render error と cleanup error を両方報告)。
- 推論: 955 commits 遅れの統合ブランチを 15 ファイル conflict で
  マージするより、MISSING 5 本を main の現在の形に合わせて cherry-pick
  する方が確度が高い。これにより `codex/issue1135-typed-iterate-suites`
  と `codex/issue1113-macos-exr` も ALREADY-ON-MAIN として不要になる。
  `codex/issue851-smart-primary-checkout` は 1162 との差分 1 commit
  (`28d53546` typed angle overrides) のみ要判定。

### 2026-09-03 bee-scope 修正の再レビュー

- 指摘 A2 (採用): 「discovery route も同じ trade をしている」というコメント
  が誤り。discovery は outgoing の GLOBAL_SETDOWN の後に window を開いて
  いる。正確な位置は swap hook 内の unload 後・bootstrap 前なので、window
  の open を `worker_render_session.cpp` の frame loop から
  `l2_main_entry.inc` の `cluster_swap_invoke` (swap_release_module の後、
  hash 確認 / LoadLibrary の前) に移した。
- 指摘 A1 (採用、コメントで明示): swap が load 段階で失敗した場合 (exit 25)
  の terminal report の block は失敗した load 試行しか覆わない。outgoing
  plug-in の render 期の活動はどこにも報告されない。
- 指摘 (coverage): キー欠落 = truncated と slot 上限の新セマンティクスが
  単体テストで固定されていなかったので、`tests.rs` の既存テストに 2 ケース
  追加。
- この修正 commit を再度レビューしてから main に進める。

### 2026-09-03 層 2 の残りの判定 (別エージェント調査、根拠は main の file:line で確認)

- `codex/issue983-seh-diagnostics` (`13d57f95`): PARTIAL。main の #983 系
  (#1212、`stage:selector_seh` stderr 行と close report の `last_seh_*`) は
  補完関係で、frame 単位の `selector_crash` プロトコル項目、broker 側の
  fail-closed 検証 (`malformed_selector_crash`)、sweep の
  `frame_crashed:<SELECTOR>` bucket は main に無い (`render_sweep.rs` は
  今も全部 `frame_error:512`)。→ main の現状に合わせて cherry-pick
  (worktree `AEXCompat-pick-983`、branch `pick/983-seh`)。
  併せて 1162 の `642b3c7b` (harness macos.rs の render error と cleanup
  error の両報告) も同じ worktree で取り込む。
- `codex/olm-win64-stack-args` (`6332ab87`, `f3407f1a`): MISSING。main の
  `engine.rs` は今も 4〜16 引数固定・0x108 固定フレーム・ゼロ埋めなし。
  実害のある bug fix ではないが自己完結なので cherry-pick
  (worktree `AEXCompat-pick-olm`、branch `pick/olm-win64`)。
- `codex/issue388-resolve-ofx`: DROP。新規 8 ファイルのみで conflict は無い
  が、`docs/PROJECT_DIRECTION.md` (main) が「AEXCompat is not an OFX host」と
  明記しており、`tests/test_resolve_ofx_surface.py` はソース文字列を assert
  する (CLAUDE.md で禁止、#691/#780/#885 で全廃済み)。OFX 面が要るなら
  scope を切った issue から始める。
- `multifilter-ae-builtin-categories-876` / `multifilter-menu-label-871`:
  ALREADY-ON-MAIN。`4c84a62f` (PR #1507 "issue876-main-reapply") 以降、
  `ae_builtin_categories.rs` は byte 単位で同一。DROP。
- `codex/issue851-smart-primary-checkout` の差分 1 commit (`28d53546`
  typed angle overrides): SUPERSEDED。main は `adea00f5` で
  `ParameterValue.angle: Option<f64>` (degrees) を採用済み。ブランチの raw
  `angle_fixed: Option<i32>` と CLI 構文は入っていないが、設計が違うので
  取り込むなら main の設計に対する新規変更として。DROP。
- `codex/issue1135-typed-iterate-suites` / `codex/issue1113-macos-exr`:
  1162 の調査で同内容が main (`1970f589`, `afe3f2c6`) にあると確認。DROP。
- 1162 の MISSING 5 commit は worktree `AEXCompat-pick-1162`、branch
  `pick/1162-guest` で cherry-pick 中。

### 2026-09-03 逸脱の記録

決定は「層 1 + 層 2 をマージ」だったが、層 2 の codex 系 (900 commits 以上
遅れ) は、同内容が別 PR で main に入っているものが大半で、残りも
15 ファイル conflict の統合ブランチをそのまま merge すると main の後続
設計 (LoadLibraryW の厳格化など) を退行させる。そのため層 2 は
「ブランチの merge」ではなく「main に無い commit だけを main の現状に合わせて
cherry-pick」に切り替えた。取り込まれる内容は同じで、merge commit の
形だけが変わる。

### 2026-09-03 bee-scope 修正 (`ad9e3103`) の再々レビュー

blocking なし。swap hook 内の位置 (early return 3 箇所より後、hash 確認 /
LoadLibrary / run_bootstrap より前)、include、他の swap 経路の不在、exit 25
時の completion report 経路をいずれもコード上で確認済み。唯一の指摘は
Rust 側コメントが「exit 25 = load 失敗」と一般化しすぎている点 (SETDOWN や
unload 段階の失敗でも exit 25 になり、その場合は outgoing の window が
残る)。コメントのみなので次バッチに載せる commit で直した
(consolidate/main 上、CI 中の main `ad9e3103` には含めない)。

### 2026-09-03 バッチ 1 の main 反映

main を `ad9e3103` に ff (1537 + 1134 + bee-scope とそのレビュー修正、
計 24 ファイル +845/-51)。この head でローカル CI 全 job を実行中。
PASS したら origin/main へ push する。

### 2026-09-03 バッチ 1 を origin/main へ push

- `ad9e3103` を push (4912b28f..ad9e3103)。続けてこのログを `b6662f00` で
  commit / push。mirror も更新済み。

### 2026-09-03 層 3 の個別判定 (別エージェント調査、diff-of-diffs と main の squash commit で確認)

| branch | 判定 | 根拠 |
|---|---|---|
| `issue335-ci-greenable` | ALREADY-ON-MAIN | `162238f4` (#346) に squash 済み。後に #731 で restricted token 自体が消えた |
| `issue339-session-audio-sidecar` | ALREADY-ON-MAIN | `ee3d88ac` (#343)、diff-of-diffs 空 |
| `issue353-layered-argb32f` | ALREADY-ON-MAIN | `c2fc6725` (#355) |
| `worktree-issue351-worker-desktop` | ALREADY-ON-MAIN | `c7817e5f` (#359) |
| `backup-issue351-full-impl` | SUPERSEDED | #359 → #1196。未反映の案は `DESKTOP_SWITCHDESKTOP` を worker に渡さない点だけで、同一トークンの worker には効かない (推論) |
| `issue664-ci-speedup` | ALREADY-ON-MAIN | `d23c586c` (#669) |
| `issue660-scan-depth` | ALREADY-ON-MAIN | `d2bac24d` (#666) |
| `issue671-ninja-worker` | ALREADY-ON-MAIN | `dfd08d8b` (#673) |
| `issue686-simd-unpack` | ALREADY-ON-MAIN | `030ed09e` (#687) |
| `codex/issue404-sweep-parallel` (ローカルのみ) | ALREADY-ON-MAIN | `287dee60` (#406) |
| `origin/codex/olm-suite-followup` | SUPERSEDED | 各トピックが #509 / #516 / #518 / #525 / #526 / #528 / #531 / #756 として個別に main 入り。guest crate はその後モジュール分割されていて適用不能 |
| `origin/codex/pointparam-suite` | SUPERSEDED | 上の部分集合 |
| `origin/codex/docs-refresh` | RESIDUAL (小) | README の headless CLI 段落と docs/README.md の索引 3 件 (`CONFORMANCE_BUNDLE_CONTRACT.md` 等) が main に無い。README は 08-20 に再構成済みで hunk は当たらないので、要るなら手で書き直す (約 26 行) |
| `issue725-classic-setup-error` | ALREADY-ON-MAIN | 実体は #777 の WIP で、`f42289d8` (#785) に全行含まれる |
| `worktree-claim-convention` | ALREADY-ON-MAIN | `0854865b` (#21)。main は `MEMORY_LIMIT_DETECTION_SLACK` を足した上位互換 |
| `issue8-smartfx-geometry-probe` | ALREADY-ON-MAIN | `e3278416` (同題)。probe と refresh script は main にあり後に進化 |
| `issue743-linux-pytest-probe` | 一時 workflow | 本人が「本採用時に削除」と明記。main に #743 の痕跡なし。Issue #743 の open/closed は git からは判定不能 |

推奨: `codex/docs-refresh` の索引 3 件をオーナー判断で手書きする以外、
層 3 は全部 DROP (ブランチ削除候補)。今回はブランチを削除しない。

### 2026-09-03 バッチ 2 の準備 (consolidate/main 上)

- `9ee15933` bee-scope の close コメント精度 (再々レビュー指摘)。
- `d41ca02e` license audit の lockfile hash を改行正規化してから計算する
  ように修正し、snapshot の `cargo_lock_sha256` を LF 版に更新。packages /
  licenses / HTML / TXT は変化なし。ローカルで `generate-third-party-licenses.py`
  (check) が通ることを確認。ベースラインで見つけた既存問題の修正で、
  マージ作業の範囲外だが、CI 相当の検証を全 job 揃えるために取り込む
  (逸脱として記録)。
- `pick/1162-guest` (5 commit、+528/-60、guest のみ) を merge、conflict なし。
- `pick/olm-win64` (3 commit、+209/-13、guest のみ) を merge、conflict なし。
- 両 pick のレビューを別エージェントで実施中。`pick/983-seh` は作業中。

### 2026-09-03 バッチ 2 のレビュー結果 (1 周目)

- `pick/olm-win64` (3 commit): 承認。frame 計算 (RSP ≡ 8 mod 16、shadow
  0x20、stack arg 位置)、上限検査 (`STACK_SIZE/8` 引数は拒否)、ゼロ埋め範囲、
  main の scheduler-deferred-thread 処理の保持、テスト helper の一致を独立に
  検算済み。非 blocking: `imports.rs` / `opencl_imports.rs` の callback
  frame は旧式 `(top-0x108)|8` のまま (元 commit も同様、別件)。
- `pick/1162-guest` (5 commit): 指摘あり、修正中。
  - F5 blocking: `resolve_layer_parameter_offset` が disk-id 照合を param
    type で絞らずに曖昧判定するため、`[Slider id=3, Layer id=1, Popup id=2]`
    で `checkout_layer(2)` (positional、AE と minihost `pre_checkout_layer`
    はこれ) が「ambiguous」で拒否される。→ positional slot が layer なら
    即それを返し、disk id は layer 以外のときだけ layer 同士で照合。
  - F6: disk-id fallback は minihost にも AE 文書にも対応物が無く、発火
    しても記録が残らない → 記録を残す (fallback 自体はオーナーのブランチ
    由来なので残す)。
  - F1: arbitrary default の value が default handle の alias。minihost は
    `PF_Arbitrary_COPY_FUNC` で私有コピーを作り同一 handle を拒否する →
    合わせる。F2: null default を error にしている (minihost は
    「value = null で続行」) → 合わせる。F3: dispose 失敗時に GLOBAL_SETDOWN
    の結果が捨てられる → 両方残す。F4: 最初の DISPOSE 失敗で中断 (minihost
    は残りも dispose) → 全部 dispose。
  - F8: `--watch` があると checkpoint mode が強制され、`function=` /
    jmp tail-call / indirect call の watch が黙って 0 件になる → entry
    hook を checkpoint mode でも残し、hook できない watch を
    `unhookable_watches` として出力。F9: 設定が sticky → 毎回リセット。
    F7: dossier の `size` / `deref` 重複記述。
- `pick/983-seh` (2 commit): レビュー中。
- consolidate/main (1162 + olm + 983 を merge、`cddb06fc`) のテスト: 全木
  rustfmt PASS、broker 629、multifilter 237、ymm4 3、guest 444 (983 merge
  前、guest は 983 で変化なし)、いずれも failed 0。

### 2026-09-03 バッチ 2 のレビュー結果 (2 周目以降)

- `pick/1162-guest` 修正 1 周目 (4 commit、F1〜F9 全部対応)。再レビューで
  追加指摘: (a) `native_x64.rs` (macOS 専用、この機ではコンパイル不能) に
  `SmartCheckoutDiskIdFallback` の二重 import で E0252 → 除去。(b) render
  成功後の value-copy DISPOSE 失敗で render 結果を捨てていた (minihost は
  `invalid_operations` を数えて結果を残す、AE は ARB callback の戻り値を
  無視) → 結果を残し `RenderReport.arbitrary_dispose_failures` に記録、
  `end_global` の失敗扱いは維持。(c) `Compound` で cleanup が primary に
  なる arm があり GLOBAL_SETDOWN の code が消えていた → 入れ替え。
  (d) fallback 記録が resident frame 間で累積する旨を doc に明記。
  (e) image 外を指す direct call の `rva=` watch を unhookable に分類。
  → 修正 2 commit を再々レビュー: 退行なし、承認。補足: resident の
  `frame_done` は `arbitrary_dispose_failures` や
  `smart_checkout_disk_id_fallbacks` を投影しない (既存の
  `unsupported_suite_calls` と同じ扱い、CLI `render` / `render-png` の
  report には出る)。
- `pick/983-seh` 1 周目: 承認 + should-fix 1 件 (`selector_crash` が
  「最後に fault した selector」を指し、precedence で 512 を勝ち取った
  selector と一致しない場合がある)。512 のみに attach する逸脱 (adapting
  agent の判断) はレビューも妥当と判定。修正 2 commit: per-frame の
  `SelectorFaultAttribution` (最初の fault + それ以前の非ゼロ結果の有無)
  を導入し、`frame_error == 512 && captured && !error_before_fault` の
  ときだけ attach。ARB probe と GPU_DEVICE_SETDOWN は attribution を pause。
  worker self-test route `--self-test-selector-fault-attribution` (8 シナ
  リオ) を追加し `test_worker_selftest_routes.py` から 3 route で実行。
  再レビュー実施中。
- guest テスト (1162 修正 2 周目 merge 後): 453 passed。

### 2026-09-03 バッチ 2 の main 反映

- `pick/983-seh` 修正 2 commit の再レビュー: blocking なし、承認
  (`frame_fault` と pause guard は selector dispatch が単一スレッドである
  前提で `seh_sequence` と同じ保管モデル、reset 点は全 route で frame 先頭、
  `!error_before_fault` 規則は under-attribution 側にしか倒れない、
  self-test は実 dispatch 経路で実 SEH を起こす)。
- consolidate/main `e06884b9` (1162 修正 2 周 + olm + 983 修正 + license
  audit 修正 + bee-scope コメント) の Rust suite: 全木 rustfmt PASS、broker
  629、multifilter 237 (+ `--all-targets` check)、ymm4 3、guest 453、failed 0。
- main にはログの commit `b6662f00` があり ff できないので、
  `git merge --no-ff consolidate/main` で `e22592bc` を作成 (conflict なし、
  27 ファイル +2607/-147)。この head でローカル CI 全 job を実行中。

### 2026-09-03 バッチ 2 を origin/main へ push

- main `e22592bc` のローカル CI: 全 25 step PASS (license-audit も修正で
  PASS)。pytest main partition は failed 0。origin/main へ push 済み。
- レビュー済み境界: `e22592bc` (バッチ 1 は `ad9e3103`)。以後の差分は
  `git diff e22592bc` で見る。

## 状態まとめ (2026-09-03 時点)

取り込み済み (origin/main):
- `issue1537-ffi-prebuilt` (merge + 相対パス正規化)
- `issue1134-load-library-w` (merge)
- `bee-scope` (merge + attribution window / parsing の修正 3 commit)
- `codex/integrate-1162-world-io` の MISSING 5 commit (cherry-pick + 修正 6 commit)
- `codex/issue983-seh-diagnostics` (cherry-pick + 修正 2 commit) と 1162 の
  `642b3c7b`
- `codex/olm-win64-stack-args` (cherry-pick 2 + 追加テスト 1)
- license audit の CRLF 依存修正 (逸脱として記録)

取り込まない (判定は上の各項):
- 層 0 の 9 本、`codex/issue1446`、`codex/issue1475`、`codex/issue1135`、
  `codex/issue1113`、`codex/issue851`、`codex/issue388-resolve-ofx`、
  `multifilter-*-876/871`、層 3 の 18 本 (`codex/docs-refresh` の索引 3 件は
  オーナー判断)。ブランチは削除していない。

保留 (オーナーの意図待ち):
- `wip/worker-desktop-20260903` (#1196 の機械的 revert)。

follow-up (Issue が立てられないためここに記録):
- LoadLibraryW / LoadLibraryExW / GetModuleHandleW の名前解決ポリシー
  統一、`windows_module_refcounts` の部分モデル、拒否モジュール名の診断
  (マージ 2 のレビュー)。
- `imports.rs` / `opencl_imports.rs` の callback frame が旧式
  `(top-0x108)|8` のまま (olm レビュー)。
- resident `frame_done` が `arbitrary_dispose_failures` /
  `smart_checkout_disk_id_fallbacks` を投影しない (既存の
  `unsupported_suite_calls` と同じ)。
- `docs/BEE_SCENE_OBJECT_ABI_2026-08-17.md` の Timecode 証跡に
  `bee_facade` の参照を足す。
- `test_worker_selftest_routes.py` は built worker 無しの素の `pytest -q`
  で落ちる (既存)。

ローカル CI スクリプト (CI workflow の再現) はこのセッションの scratchpad
にあり、リポジトリには入れていない。必要なら `tools/` へ移す。


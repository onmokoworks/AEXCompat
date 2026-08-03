# Worker Isolation Inventory (2026-08-04)

worker isolation の「実装されている機構」「対処している失敗」「対処していない領域」を
1 枚に固定する参照文書。エージェントセッションが isolation を security sandbox と
誤読する事故が繰り返されているため、その再発防止を目的とする (issue #641)。

規範 (何をしてよいか) は `CLAUDE.md` の Execution Tiers and Safety Rules が正本。
本書は実装状態の記述であり、方針を追加しない。実装が変わったらこの文書を更新する。

## TL;DR

このプロジェクトに「sandbox」という単一のセキュリティ機能は存在しない。実在するのは
性格の違う 2 系統:

1. **クラッシュ封じ込めの床 (常時オン)**: 別プロセス + kill-on-close Job Object +
   ホスト保護の実行時不変条件。対象は「壊れた・行儀の悪いプラグイン」であって攻撃者
   ではない。
2. **evidence tier (opt-in)**: sealed load tree、receipt でピン止めした identity、
   module audit。対象は「どのバイトが実行されたかの証明 (provenance)」であって、
   封じ込めの強化ではない。

`SECURITY.md`: "This is crash and integrity containment; it is not a
confidentiality sandbox." `docs/WINDOWS_NATIVE_HARDENING_PLAN_2026-07-16.md`:
"No mode may be labeled a security sandbox until its confidentiality, integrity,
network, child-process, and persistence boundaries have direct adversarial
evidence." どのモードもこの昇格基準を満たしていない。

## 1. 全 launch 共通の床

すべての worker 起動は `broker/crates/broker/src/windows_process.rs` の
`launch_isolated_impl` を通る。

- `CREATE_SUSPENDED` で起動 → Job Object へ割り当て → resume。この間に失敗したら
  job ごと terminate (`SuspendedProcessCleanup`)。
- Job Object の制限フラグは **2 つだけ**: `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` と
  `JOB_OBJECT_LIMIT_PROCESS_MEMORY` (既定 512 MiB、probe 経路は上限 2 GiB)。
  UI 制限・プロセス数制限・CPU 制限は設定していない (§6)。
- `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` で継承ハンドルを明示列挙。渡るのは
  stdout/stderr パイプの書き込み端、trace/minidump/session の各ハンドルのみ。
  sentinel テスト (`run_sentinel_check`) が非継承を検証している。
- 既定では専用 desktop (`CreateDesktopW` + 保護 DACL) 上で起動し、モーダル
  ダイアログは `worker_dialog.rs` が WM_CLOSE で掃除する (issue #351 の UI
  封じ込め)。例外として `_on_current_desktop` 系 (GUI ハーネス用の
  `WorkerDesktopPolicy::Current`) は呼び出し元 desktop で起動し、その場合
  dialog sweep は付かない。
- stdout 24 MiB / stderr 64 KiB の取得上限とパス redaction。
- 正常終了後も job を terminate し、継承パイプを握った子孫プロセスが収集を止め
  られないようにしている。

この床は identity 検査を含まない。プラグインのハッシュはクラッシュ封じ込めに何も
足さない (`CLAUDE.md` 参照)。

## 2. `secure_launch` 系のみの追加 (evidence 側の入口)

- **Restricted token**: `restricted_worker_token.rs` の
  `CreateRestrictedToken(DISABLE_MAX_PRIVILEGE, …)` + `CreateProcessAsUserW`。
  restricting SID には per-worker のランダム SID (`S-1-5-88-…`) と
  `S-1-5-12` (RESTRICTED) のほか、**互換目的で Everyone / Authenticated Users /
  Users / 現ユーザー SID を含む**。したがってユーザーの通常 ACL で読めるものは
  ほぼ読める。実際に守っているのは「ランダム SID への deny ACE が付いた staged
  tree」だけで、これは confinement ではなく staged tree の完全性保護。
- **Sealed load tree** (`sealed_load_tree.rs`): プラグイン + 依存 closure を
  `%TEMP%` 下のランダム root にハッシュ検証つきでコピーし、ハンドルを broker が
  worker 存続期間中保持し、DACL (`restricted_worker_acl.rs`) で worker には
  read+execute しか許さない。reparse point / 複数リンクを持つ source /
  basename 汚染 / 大文字小文字衝突を拒否し、staging 後に保持ハンドルを
  再ハッシュする (staging 自体は hard link を優先し、保護は write/delete 共有
  なしの保持ハンドルと再ハッシュが担う)。目的は「実行されたバイトの同定」。
  adversarial worker テスト (`sealed_acl_probe.rs`) が read 可 /
  write・create・delete・WRITE_DAC 不可を実証している。
- **Trusted worker stage** (`trusted_worker_stage.rs`): worker 実行ファイル自体も
  ハッシュ → staging → 再ハッシュ → 同じ保護 DACL で起動する。
- **Module audit** (`worker_module_audit.rs`): worker が実際にロードした
  モジュールを**事後に**分類・検証する (sealed root / worker stage / System32 /
  WinSxS / driverstore / policy 以外は unknown で fail-closed)。ロード自体を
  止める仕組みではない。
- worker 側 (C++) は `SetDefaultDllDirectories` + sealed root の
  `AddDllDirectory`、`LoadLibraryExW` 直前の SHA-256 再チェック。GPU backend は
  System32 のみからロードする。

## 3. ホスト保護の実行時不変条件 (常時オン)

「malformed なプラグインがホスト状態を壊さず、診断として観測される」ための層。
互換に見せる目的でこれらを外すことは禁止 (`CLAUDE.md`)。

- **出力バウンド**: 4096×4096 / 16M pixels、rowbytes・容量の checked-mul 検証
  (`render_request.rs` の `validate_image_buffer_layout` ほか)、aux channel /
  parameter animation / batch サイズの各上限。
- **フレーム出力検証** (`render_session.rs` の `validate_ok_frame`): guard byte、
  generation 不一致、ヘッダ改変、slot 超過、異常形状の resize などをすべて
  fail-closed でセッション無効化。worker の申告を信用しない設計。
- **suite/handle 所有権** (worker 側 C++, `worker_handle_runtime.cpp` /
  `worker_world_registry.cpp` ほか): unknown・stale・foreign・double-dispose・
  budget 超過を拒否して `invalid_operations` に計上。generation カウンタは
  枯渇時に fail-closed。
- **ハンドル・ケイパビリティ転送**: minidump (`minidump_policy.rs`)、trace
  (`trace_policy.rs`)、render session transport は、broker が検証した対象を
  開いたハンドルとして継承させ、worker にパス文字列を渡さない (#18 → #66)。

### ハンドル転送の正しい分類

ハンドル転送はしばしば「TOCTOU 対策 = セキュリティ機構」と語られるが、この分類は
不正確である。worker はプラグインと同じユーザートークンで動くため、悪意ある
プラグインはユーザーが書ける場所には最初から直接書ける。junction スワップを
防いでも「悪意あるコードの書き込み能力」はほぼ変わらない。

それでもこの機構が床に属するのは、守っている性質がセキュリティではなく次の 2 つ
だから:

1. **broker が回収する成果物の object identity**。minidump や trace は broker が
   後で読んで診断・evidence として扱う。パス再解決方式では「回収時にそのパスに
   あった何か」を読むことになり、診断と実行の結びつきがパス信頼に落ちる。これは
   `validate_ok_frame` が worker の申告を検証するのと同じ族のホスト保護不変条件。
2. **ホスト主導の書き込みチャネルの爆発半径**。ハンドル方式では worker が壊れて
   も (プラグインのメモリ破壊でパス文字列が化けても) 書き込み先は事前に開いた
   object 1 個に閉じ、budget (ファイル数・総量) の enforce 先も broker 所有
   object になる。パス方式では壊れた worker が broker の転送機構経由でユーザーの
   ファイルを巻き込める。

残存例外: `--dump-worlds-v1` と `--aux-manifest-v1` は今もパスを worker が
再オープンする (debug 用途。`resolve_managed_dump_dir` が `target/` 配下限定と
canonical 検査を行うが、re-open である事実は変わらない)。

## 4. デッドラインの現在地

タイムアウトは床の一部では**ない** (issue #354 で降格)。discovery の parameter
inspection は無期限に待つ (`wait_and_collect(None)`)。封じ込めはクロックではなく
Job Object と dialog sweep が担う。

残っているデッドライン: session の frame deadline (既定 30 s、1–600 s に
クランプ)、one-shot image dispatch の 30 s、`l1` の per-plug-in timeout、
`selftest` の probe deadline、終了処理の各 grace (5 s / 10 s / 30 s)。

## 5. 対処している / していない

| 対処している (実装あり) | 対処していない (docs が明言) |
|---|---|
| クラッシュ・abort の伝播 (別プロセス + Job) | **機密性**: ユーザートークンで動き、ユーザーが読めるものは読める |
| メモリ暴走 (512 MiB commit 上限) | **ネットワーク**: 制限するコードは一切ない |
| 孤児プロセス (kill-on-close) | レジストリ書き込み・永続化 |
| モーダルダイアログでのハング (#351) | 子プロセス生成 (job 内封じ込めのみ) |
| 不正ハンドル・world 操作によるホスト状態破壊 | UI オブジェクト生成 (private desktop 隔離のみ) |
| 出力の偽装・バッファ超過 | System32 DLL の自由なロード (audit は事後) |
| broker 検証後のファイル差し替え (ハンドル転送) | プラグインが安全であることの保証 |
| 「どのバイトが走ったか」の改ざん (evidence tier) | |

## 6. 実装されていないもの (誤解の頻出源)

以下の機構は **実装コードに存在しない**。リポジトリ内での言及は
`docs/WINDOWS_NATIVE_HARDENING_PLAN_2026-07-16.md` (計画) と
`docs/PROJECT_DESIGN_2026-07-03.md` (旧設計) の構想記述、および本書のような
「未実装である」という注記に限られる:

- `SetProcessMitigationPolicy` / `PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY`
  (mitigation 全般: CFG、dynamic-code、child-process、Win32k、signature policy)
- `JOB_OBJECT_UILIMIT_*` (clipboard / desktop / handles / atoms の UI 制限)
- `JOB_OBJECT_LIMIT_ACTIVE_PROCESS` / job メモリ総量 / CPU レート制限
- integrity level SID (low IL)
- AppContainer / LowBox token

これらを前提に isolation を語る記述を見たら、HARDENING_PLAN のモード表 (未実装の
計画) を実装と読み違えている。

## 7. ルート別の現状 (tier は均一ではない)

| ルート | 起動 | sealed tree | token | module audit |
|---|---|---|---|---|
| L2 launch transaction (`l2.rs`) | `secure_launch` | あり | restricted | 必須 |
| `render.rs` / `render_request.rs` 系 (#312 以降) | `secure_launch` | あり | restricted | 必須 |
| image dispatch / `RenderSession` / `AudioRenderSession` | `dispatch_secure_image*` | あり | restricted | 経路による (cluster session は close 時検証) |
| GPU probe 群 | `secure_launch` 系 | あり | restricted | CUDA/OpenCL probe はなし、wgpu DX12 PF probe は必須 (`dispatch_secure_image` 経由) |
| **`l1.rs`** | **`run_isolated`** | **なし** | **normal** | **なし** |
| `selftest.rs` | `run_isolated` | なし | normal | なし (プラグイン非ロードの自己 probe であり sealing gap ではない) |

`l1` が最後のプラグインロード normal-token 経路。receipt 不要の default tier を
標準経路として整備する作業は issue #36 で追跡されていたが、#36 は 2026-07-24 に
not-planned で close されており、2026-08-04 時点で生きている追跡 issue はない。
つまり「全 dispatch が sealed」も「軽量 tier が既に使える」もどちらも成立せず、
後者には実装計画の器すらない。

非 Windows では `secure_launch` / restricted token とも `Unsupported` で
fail-closed する。

## 8. 過大主張が訂正されてきた系譜

同種の誤解はエージェントに限らず、リポジトリ自身が繰り返し訂正してきた:

1. **frozen worker-trust constants**: セキュリティゲートとして導入されたが、
   「攻撃者が worker 実行ファイルは書き換えられるのに broker や定数は書き換え
   られない、という前提でしか意味がない (全部同じユーザー書き込み可能な checkout
   にある)」として削除 (`docs/EVIDENCE_POLICY_2026-07-18.md` §3)。同 §2 は
   "audit / gate / authenticate" という語が実態以上を約束していたとも記録する。
2. **「every dispatch is sealed」**: commit `bb32349b` で偽と撤回。その訂正も
   `063286e2` で再訂正 (`selftest` は sealing gap ではなく別カテゴリ)。
3. **timeout の invariant 降格**: 「常時オンの crash containment invariant」と
   されていたが、封入量依存の偽陰性を含む `timeout_killed` (26/353 件) を受けて
   #354 で降格。
4. **#67 → #66**: 「ディレクトリをハンドルでピンして worker が re-open」方式は
   「そもそもパスを渡さない」方式に劣ると判明し置き換え (#18 レビュー)。
5. **旧設計語彙**: `PROJECT_DESIGN_2026-07-03.md` の「sandbox broker」
   「ネットワーク遮断」は現行文書に引き継がれていない。旧設計を読んだ場合は
   本書と `CLAUDE.md` を優先すること。

## 一文で言うなら

worker isolation は、開発中・未知の AEX が**事故る**ことを前提にホストと診断結果を
守る床 (常時オン) と、観測が**どのバイトから出たか**を証明する evidence tier
(opt-in) の 2 つであり、悪意あるバイナリを安全に実行するための sandbox は存在
しないし、存在すると主張してはいけない (昇格基準未達)。

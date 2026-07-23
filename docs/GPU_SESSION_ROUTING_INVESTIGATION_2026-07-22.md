# GPU 単一画像 render を length-1 session 経由にする — 調査ノート (issue #290)

作業ブランチ: `issue290-w4-gpu-session` (base main)。時系列で追記する。観察(事実)と
仮説(推論)を分ける。古い項目は消さず、覆った場合は「訂正」を追記する。

## 2026-07-22 初期調査 (観察)

### 確定した事実

- **producer 不在**: `GpuRuntimePolicyInput { ... }` の構築箇所はリポジトリ全体でゼロ
  (`rg 'GpuRuntimePolicyInput\s*\{' *.rs` → no match)。`gpu_runtime_policy` フィールドは
  全 caller (`bridges/aviutl2`, broker tests, image_render, render_session の内部) が `None`
  固定。→ `dispatch_secure_gpu_image` / `dispatch_secure_gpu_image_session` は scaffold の
  みで end-to-end 実行実績なし。issue の「決定的事実」通り。

- **session 側は完全配線済み** (`broker/crates/broker/src/render_session.rs`):
  - `SessionOpenRequest.gpu_runtime_policy: Option<GpuRuntimePolicyInput<'a>>` (:511)。
  - `RenderSession::open` (:818-832) が gpu_capable / effective_backend / gpu_attempt を判定し、
    policy 無しの GPU を fail-closed、Auto+policy無を CPU session に degrade。
  - dispatch (:1089-1117) が `authenticate_gpu_worker_report` で認証し
    `dispatch_secure_gpu_image_session` に `GpuRuntimeAuthorization` を渡す。
  - `session_command` (:134-168) が Argb32f × Auto/Cuda/OpenCl/DirectX を smart-session の
    GPU command word にマップ済み。

- **routing 側の gap は wrapper のみ** (`broker/crates/broker/src/image_render.rs`):
  - `render_with_artifact` (:4425) が `session_eligible` を計算 (:4700-4733)。現状 smart は
    `secondaries空 && timed空 && host_context無 && ((Cpu&&Argb32f) || (Auto&&policy無))` のみ
    admit。GPU combo は :4721-4723 で「#290 の GPU session stage に defer」と明記して除外。
  - eligible なら `render_classic_via_length_one_session` (:5482) へ。この wrapper は
    `SessionWrapperRequest` (:5429) を受け、`SessionOpenRequest` 構築時 `gpu_runtime_policy: None`
    固定 (:5526)。`SessionWrapperRequest` に該当フィールドが無い。
  - public entry `render_experimental_image_with_approved_dependencies_and_gpu_runtime_policy`
    (:2062) は policy を `render_with_artifact` に forward するが、全 caller が `None` を渡す。
  - `InteractiveRenderSession::open` (:5780) は classic 非smart の別経路。#290 対象外。

- **one-shot GPU も policy 必須**: `render_with_artifact` の `gpu_initial_attempt` block
  (:5052-5102) は `gpu_runtime_policy` が `None` なら error (Auto なら CPU fallback)。全 caller が
  None なので one-shot GPU secure dispatch も一度も実行されていない。実 GPU engine は
  policy-less 経路からは到達不能で、`dispatch_secure_gpu_image[_session]` からのみ到達。

- **成功 path の report 等価は現状で成立**: 成功時 one-shot は
  gpu_fallback_used=false / gpu_fallback_reason=None / gpu_attempt=None。session wrapper も
  `flatten` facts を false/None/None 固定 (:5703-5705)。worker report 由来の gpu_* フィールド
  (gpu_render_dispatched 等) は両者同じ GPU command word を実 Cuda で走らせれば一致するはず。
  → 乖離は GPU **失敗**時のみ (one-shot は CPU 再試行して attempt を記録、session は fail-closed)。
  issue の「report 等価の分析」通り。item 3 は「等価を検証」であり追加実装ほぼ不要 (仮説)。

### producer の深い gap (手順1 の核) — 観察

- 認証関数 `authenticate_gpu_worker_report[_at]` (`runtime_module_policy.rs:236-270`) が食う
  DTO は `GpuWorkerModuleReportDto { session_identity: 64hex, backend, modules: [{classification:
  policy|system32|sealed|trusted, basename, path_token, sha256, size}] }` (:161-167)。
  各モジュールを classification 別の候補 (policy.modules / sealed / trusted / system32.join) と
  照合し、path_token + sha256 + size 一致 + **ディスク再ハッシュ** で認証。成功時
  `AuthenticatedGpuModuleReport { session_identity, backend, policy_expires }` を返す。

- **この DTO を emit するコードが production/worker のどこにも無い**
  (`minihost/**/*.cpp` を session_identity/path_token/classification で grep → no match)。
  テスト (`tests/runtime_module_policy.rs:24-45`) だけが手書き JSON でこの関数を叩いている。

- C++ worker が実際に出すのは **DLL-load module audit** (`minihost/src/runtime_module_audit.cpp:327
  module_audit_json`): `{schema:1, status, post_load, pre_unload, observed_union, ...}`、各 snapshot
  は `{status, unknown_count, worker[], plugin[], system32[], policy[]}` で **basename のみ**。
  path_token / sha256 / session_identity / per-backend binding を持たない。
  → 認証に必要な identity 情報を欠く。broker 側 validator は
  `worker_module_audit.rs::validate_required_worker_audit` (secure launch の
  `require_module_audit: true` で毎回強制)。

- 既存の runtime-policy 経路は **パラメータ inspection のみ**:
  `inspect_experimental_with_runtime_policy` (:2982) → `..._with_diagnostics_and_runtime_policy`
  (:3003) が `prepare_runtime_authorization_transport` (:146) で AEXRMA1 バイナリ manifest
  (`encode_runtime_module_authorization`, purpose=PfParameterInspect) を作り、
  `--runtime-module-authorization-v1 <basename>` で L2 worker (`--l2-params-only`) に sealed
  dependency として渡す。worker 側は `runtime_module_audit.cpp:217 parse_runtime_module_authorization`
  で magic/session/expiry 検証 + 各 DLL 再ハッシュ。**render 経路には未配線**。
  harness gate `tools/run-glator-runtime-policy-inspect-gate.ps1` がこの inspect 経路を OpenGL/
  GLator で回している (policy JSON = DriverStore の nvoglv64.dll/nvgpucomp64.dll を sha/size/version
  で認証して構築)。

- 実 GPU engine は実在・コンパイル対象 (`minihost/CMakeLists.txt:18-22`):
  `gpu_cuda_backend.cpp` (~187 LOC), `gpu_directx_backend.cpp` (~214), `gpu_opencl_backend.cpp`
  (~213), `gpu_memory_world_transport.cpp` (~551)。smart worker の render path から呼ばれ、broker は
  report フィールド (gpu_render_dispatched 等) で成否を推論。→ dead ではない。消すのは不適切。

- GPU render fixture: `tools/build-sdk-invert-cuda.ps1` が `SDK_Invert_ProcAmp_CUDA.aex`
  (CUDA/OpenCL/DirectX 対応 SmartFX サンプル) をビルド。A/B の被写体候補。

### 結論 (この時点の理解)

- 手順3 (routing) と 手順4 (A/B) の broker 側配線は tractable かつ小さい。
- **手順1 (producer) が本体工数**。しかも「既存部品の Rust 配線」ではなく、認証が要求する
  classified GPU module report (path_token + sha256 + session_identity binding) を emit する
  仕組みの新設が必要。既存 DLL-load audit は basename のみで不足。
- ここに設計 fork がある (次項で方針決定):
  - (A) 専用 preflight worker mode を新設し `GpuWorkerModuleReportDto` を直接 emit。
  - (B) 既存 module audit emission を path+sha256 付きに enrich し、broker が classify +
    session_identity binding + 認証。
  - (C) broker が policy から report を合成 (worker 変更なし)。self-certifying で
    「実際にロードされたモジュールを反映」しないため fail-closed 思想に反する。信頼性最低。

## 2026-07-22 方針決定

- スコープ: ユーザ判断で「手順1-4 全部を1 PR で完遂」。実 Cuda 検証は #296
  (issue294-smart-layers) の実 worker A/B とシリアライズ (排他 GPU、片方が走る間はもう片方待ち)。
  実装・レビューは並列。
- producer emit 方式: **(A) 専用 preflight emit** で確定。既存 audit の enrich (B) や broker
  合成 (C) ではなく、smart worker に preflight mode を新設し、plugin + 認証済み GPU DLL を
  AEXRMA1 manifest 下でロード → 実際にロードされたモジュールを classification 別に
  path_token+sha256+size で列挙 → broker が渡した session_identity を echo して
  `GpuWorkerModuleReportDto` を直接 emit。認証関数が食う形と 1:1、DLL-load audit (別 validator
  用) と責務分離、最も faithful で fail-closed。

## 実装計画 (二相、シリアライズ制約付き)

producer flow は本質的に二相 (認証は dispatch の前に済ませる必要があるため、報告を出す
preflight run と実 render run は別 launch):
1. broker が session_identity 生成 + policy JSON ロード (`parse_and_validate`) + AEXRMA1 manifest
   encode。
2. preflight worker 起動 (sealed, module-audit) → classified GPU module report を stdout に emit。
3. broker が report を読み `GpuRuntimePolicyInput` を組む (sealed_modules は SealedLoadTree、
   trusted_modules は trusted-worker-stage、system32 パス解決)。
4. render session / one-shot を `Some(policy)` で開き `authenticate_gpu_worker_report` → GPU dispatch。

### Phase 1 (実 GPU 不要、#296 と完全並列)
- C1. C++ preflight mode 新設 (`minihost`): AEXRMA1 manifest から session_identity/backend/
  authorized modules を取得 (既存 `parse_runtime_module_authorization` 再利用) → GPU backend init で
  実 DLL ロード → loaded module 列挙・分類 (既存 capture ロジック再利用) → 各モジュールを
  path_token+sha256+size+basename+classification (worker→trusted / plugin→sealed / system32 /
  policy) で emit。session_identity を echo。worker 再ビルド。
- C2. broker producer (Rust): preflight 起動 + report 捕捉 + `GpuRuntimePolicyInput` 組み立て。
  RuntimeModulePurpose に render/preflight 用 variant が要るか確認。
- C3. routing 配線 (#290 core, 小・決定的): `GpuRuntimePolicyInput` に `Clone, Copy`;
  `SessionWrapperRequest` に `gpu_runtime_policy` field; gate (:4700-4730) を
  `Argb32f && runtime_backend(gpu_backend).is_some() && policy.is_some()` に拡張;
  wrapper (:5526) の `None` を `request.gpu_runtime_policy` に置換; public entry から producer 配線。
- C4. harness command 新設 (inspect-runtime-policy を模す): policy JSON path を取り producer→render。
- C6. Rust/Python unit test: gate が GPU+policy を admit する検証、producer の DTO 組み立て/分類
  マッピング (合成 preflight stdout)、fail-closed 非回帰。source_owners.py 更新。

### Phase 2 (実 Cuda、#296 とシリアライズ)
- C5. gate script `run-invert-gpu-session-render-gate.ps1` (glator inspect gate を模す):
  cuda invert fixture + worker + harness 認証 → cuda policy JSON 構築 (nvcuda.dll 等を
  System32/DriverStore から sha/size/version 認証) → session と one-shot (wrapper bypass) で
  同一 policy render → 出力 byte (sha) + report (gpu_* / gpu_fallback_used) 等価検証 → evidence JSON。
- 実 Cuda A/B。成功 path で session==one-shot の byte + report 等価を確認。

### 検証順序の信頼性
Phase 1 は worker 再ビルドまで含め GPU を占有しない (実 preflight/render dispatch は Phase 2 の
gate でのみ実 GPU を叩く)。よって #296 と完全並列可能。Phase 2 のみ #296 の実 worker A/B と
シリアライズ。

## 2026-07-22 producer 契約の確定 (観察 + correctness 要件)

C3 (routing 配線) 実装済み・broker lib clean compile・非回帰 (Some を渡す caller はまだ無い)。

### authenticator が食う DTO と検証ロジック (`runtime_module_policy.rs`)
- `GpuWorkerModuleReportDto { session_identity: 64hex, backend: cuda|opencl|directx (cpu 不可),
  modules: [ClassifiedModuleReport] }` (:163)。serde `deny_unknown_fields`。
- `ClassifiedModuleReport { classification: policy|system32|sealed|trusted, basename, path_token,
  sha256, size }` (:138)。
- `authenticate_gpu_worker_report_at` (:236): session_identity nonzero + 期待値一致 (constant time)、
  backend 一致、policy 非 expired を確認後 `validate_worker_reports_at` (:286)。
- `validate_worker_reports_at`: 各 report を classification 別候補と照合:
  - Policy → `policy.modules` を backend で filter。
  - Sealed → broker が渡す `context.sealed` slice。
  - Trusted → broker が渡す `context.trusted` slice。
  - System32 → `system32.join(basename)` を canonical 化し System32 直下確認。
  - 照合条件 (:335-338): basename (ascii ci) + sha256 + size 一致 **かつ**
    `path_token(canonical_exact(candidate.path)) == report.path_token`。
  - 重複 (classification:path_token) 拒否。最後に `authenticate_file` でディスク再ハッシュ。
  - modules 数上限 `MAX_RUNTIME_MODULES=128`。

### path_token の完全仕様 (C++ preflight が byte 一致で再現すべき)
```
PATH_TOKEN_DOMAIN = b"AEXCompat runtime module path token\0v1\0"   // 末尾に NUL 2 つ
fold_path(p)   = to_string_lossy(p).replace('/', "\\").to_lowercase()   // Unicode lowercase
path_token(p)  = lower_hex(SHA256(PATH_TOKEN_DOMAIN || fold_path(p).as_bytes()))
```
- authenticator は `path_token(fs::canonicalize(candidate.path))` を照合対象にする。
- **Rust の `fs::canonicalize` は Windows で `\\?\` prefix 付き verbatim path を返す**
  (例 `\\?\C:\Windows\System32\nvcuda.dll`) → folded は `\\?\c:\windows\system32\nvcuda.dll`。
- 一方 C++ 既存 `canonical_path` (`runtime_module_audit.cpp:49`) は `\\?\` を **strip** する。
  → preflight emit では **strip せず** GetFinalPathNameByHandleW(FILE_NAME_NORMALIZED|VOLUME_NAME_DOS)
  の結果 (`\\?\C:\...`) をそのまま lowercase (ASCII path 前提) して token 化する。strip した path で
  計算すると Rust 側と不一致になり認証失敗。
  ⚠ System32/DriverStore の GPU DLL パスは ASCII 前提。非 ASCII パスが来た場合 Rust の Unicode
  lowercase と C++ towlower が乖離し得るが、その場合は認証が fail-closed に落ちる (安全側)。

### classification マッピング (C++ audit → authenticator)
既存 `audit_loaded_modules` (`runtime_module_audit.cpp:93`) の分類 → DTO classification:
- `worker` (実行ファイル、`aexcompat-trusted-worker-*` dir) → **trusted**
- `plugin` (plugin_root dir) → **sealed**
- `system32` (System32 dir) → **system32**
- `policy` (`authorized_runtime_module` = AEXRMA1 manifest 記載) → **policy**
- `unknown` → 拒否 (fail-closed、report を出さず error)

broker (C2) が渡す照合候補の出所:
- `context.sealed` = SealedLoadTree (plugin + deps) の ApprovedClassifiedModule 群。
- `context.trusted` = trusted-worker-stage (worker exe) の ApprovedClassifiedModule。
- `context.system32` = 正規 System32 パス。
- `policy` = policy JSON を `parse_and_validate` したもの。
→ C1 が emit する各モジュールの (basename, sha256, size, path_token) は、C2 が渡す候補と全て一致
  する必要がある。特に sha256/size は worker がディスクから実測、path_token は上記仕様で計算。

### 解決した設計上の緊張点: report scope = policy モジュールのみ (sealed/trusted は空)

観察: `SealedLoadTree::create` (`sealed_load_tree.rs:34-43`) は `create_random_root(temp_dir())` で
**dispatch ごとにランダムな temp root** に plugin/deps をコピーし、Drop で削除する。trusted-worker-stage
も同様に一時ステージング。よって:
- preflight dispatch と render dispatch は別々のランダム temp path を使う。
- preflight の sealed/trusted path は render 認証時 (render_session.rs:1095) に存在しない & 不一致。
- 認証は sealed/trusted 候補に対し `canonical_exact` (fs::canonicalize、実在必須) + `authenticate_file`
  (再ハッシュ) を行う → ephemeral path では必ず失敗。

決定: **GPU module report の scope は policy-classified モジュール (= AEXRMA1 manifest 記載で実際に
プロセスにロードされた GPU runtime DLL) のみ**。sealed_modules / trusted_modules は空 slice、
system32 も (少なくとも初版は) 報告しない。根拠:
- policy/system32 モジュールは System32 / DriverStore 等の**安定・永続パス**に在り、render 認証時も
  実在・一致する。plugin(sealed)/worker(trusted) の ephemeral path 問題を完全回避。
- 先例 `run-glator-runtime-policy-inspect-gate.ps1` も authorized_policy_modules = GPU ベンダ DLL
  (nvoglv64.dll / nvgpucomp64.dll) のみを認証しており、policy モジュール集合が意味のある単位。
- plugin/worker/unknown の網羅性は sealed load tree + trusted worker stage + 常時 on の
  require_module_audit (DLL-load audit、unknown_count==0 を強制) が別途担保する。authenticator は
  report の網羅性を要求せず、列挙された各モジュールを認証するだけ (validate_worker_reports_at)。
- ⚠ policy JSON (C5 の gate script が構築) は、実際にロードされる CUDA runtime DLL 群を過不足なく
  列挙する必要がある。過剰 (ロードされない DLL を policy に入れる) は report に出ないだけで無害、
  不足 (ロードされる GPU DLL が manifest 未記載) は DLL-load audit で unknown 判定され preflight が
  fail-closed する。glator と同様、実測して policy を組む。

→ C2 の `GpuRuntimePolicyInput`: `sealed_modules=&[]`, `trusted_modules=&[]`,
  `system32=<canonical System32>`, `policy=<parse_and_validate(policyJSON)>`,
  `module_report_json=<C1 preflight stdout>`, `session_identity=<broker 生成、manifest と共有>`。

### AEXRMA1 purpose
`RuntimeModulePurpose` は現状 `PfParameterInspect = 1` のみ。C++ `parse_runtime_module_authorization`
は `purpose != 1` を拒否。preflight を purpose 分離する (`PfGpuRenderPreflight = 2` を追加し C++ parse
を expected purpose 受け取りに拡張) 方が purpose-bound で faithful。worker map 確認後に確定。

## 2026-07-22 実装ログ (C1 + 逸脱)

### 逸脱: AEXRMA1 purpose を分離せず PfParameterInspect (=1) を preflight でも再利用
当初 `PfGpuRenderPreflight=2` の追加を計画したが、以下の理由で**分離せず purpose=1 を再利用**する
決定に逸脱:
- 分離は shared `parse_runtime_module_authorization` の signature 変更 (expected purpose 引数)、
  呼び出し側 `worker_runtime_admission.cpp`、Rust `RuntimeModulePurpose` enum、C2 の encode 呼び出しの
  4 箇所に波及し surface が増える。
- 認証の実効的な束縛 (どのモジュール群・どの session・どの backend・expiry) は manifest が既に担保。
  purpose の追加寄与は限定的で、session_identity が per-render random + expiry 短のため inspect
  manifest の preflight 転用は session 不一致で防がれる。
- CLAUDE.md「最も信頼でき確度の高い解決策」に沿い、C++ の共有 parse を無改変に保つ方が risk 小。
→ C++ `parse_runtime_module_authorization` は `purpose != 1` のまま。C2 の encode も
  `PfParameterInspect` を渡す。

### C1 実装済み (未ビルド検証)
- `minihost/src/runtime_module_audit.hpp`: `authorized_runtime_backend()` /
  `gpu_module_report_json()` 宣言追加。
- `minihost/src/runtime_module_audit.cpp`:
  - `#include <bcrypt.h>` + `#pragma comment(lib, "bcrypt.lib")`。
  - globals `g_authorized_session_identity[32]` / `g_authorized_backend` 追加。parse で捕捉
    (session は破棄されていた箇所、backend は検証後)。関数先頭で reset。
  - anon 名前空間に `kPathTokenDomain` (Rust の `PATH_TOKEN_DOMAIN` と byte 一致、NUL 2 個)、
    `hash_bytes_hex` (BCrypt raw-bytes SHA256、lower hex)、`path_token` (`\\?\` prefix 復元 →
    fold → domain 前置で hash)。
  - `gpu_module_report_json`: EnumProcessModulesEx でロード済み canonical path 集合を作り、
    `g_authorized_runtime_modules` のうちロード済みのものを classification=policy で emit。
    session_identity/backend を echo。
- `minihost/src/l2_main.cpp`: `worker_main_impl` の pipl self-test 直後に
  `--gpu-module-report-v1 <plugin> <sha> --runtime-module-authorization-v1 <manifest>` intercept
  追加。parse → backend id を framework (3/1/4) にマップ → `begin_backend_context` で GPU DLL ロード
  → `gpu_module_report_json` を stdout → `end_backend_context`。exit 74/75/76 が失敗。
- worker は `aex_worker_runtime_core` に入るため全 worker (l2/render/smart) が取得。
### C1/C2/C3 ビルド・テスト検証 (2026-07-22)
- **broker lib**: `cargo check -p aexcompat-broker` clean (C2 producer + C3 routing)。
- **worker**: `aex_smart_worker` を DevShell (Enter-VsDevShell) でビルド成功 (Ninja, [117/117] →
  rewrite 後 [14/14] Linking, exit 0)。C1 の BCrypt/path_token/gpu_module_report_json/l2_main
  intercept がコンパイル・リンク通過。
  - ⚠ ビルド手順の教訓 (逸脱ログ): 当初 Git Bash から `cmd.exe /c "batch"` + `| tail` で vcvars64
    を回したら **cmd が対話モードに落ちて stdin 待ちで hang** (quote 破損)。残留 cmd.exe 8 個。
    → PowerShell の `Import-Module Microsoft.VisualStudio.DevShell.dll` + `Enter-VsDevShell
    -DevCmdArguments '-arch=x64 -host_arch=x64' -SkipAutomaticLocation` 経由が確実。以後これを使う。
- **source-text テスト**:
  - `test_image_render_secure_dispatch.py`: `WorkerKind::Smart` count 2→3 に更新 (preflight route
    追加による正当な増。コメント付き)。pass。
  - `test_native_worker_module_audit.py::test_report_exposes_..._basenames_not_paths`: 当初失敗。
    原因は serializer slice が `module_audit_snapshot_json`〜EOF で、末尾に足した `gpu_module_report_json`
    が `module_path.wstring()` を含んだため。テストを弱めず、**関数を単一パス形に書き換えて
    `module_path.wstring()` を排除** (loaded set を作らず各ロード済みモジュールを authorized entry に
    直接照合、path_token/basename のみ emit)。私の関数も privacy テスト対象に入ったまま正当に pass。
  - フル pytest: 私の変更起因の失敗はこの1件のみ (修正済み)。他 3 件
    (`test_aegp_projector_levels`, `test_pf_visual_audio_admission_result`,
    `test_world_format_registry_composite16`) は **stash して clean base でも同様に失敗** →
    worktree に全 worker 未ビルド等の環境要因で、私の回帰ではない。
- **未実施**: C4 (harness command) が無いため producer→render の end-to-end 経路はまだ誰も叩かない。
  実 preflight/GPU 検証は Phase 2 (C5 gate, 実 Cuda, #296 とシリアライズ)。全 worker ビルド +
  broker 統合テストも Phase 2 で。

## 2026-07-22 C4 + preflight スモークテスト + 決定的 finding

### C4 / C6 / inspect command 実装済み・コンパイル済み
- **C4**: harness `--render-experimental-smart-32-gpu-policy <plugin> <input> <output> <backend>
  <policyPath>`。policy 構築 → `prepare_gpu_runtime_policy` → GPU-policy render entry を `Some` で。
  A/B は同コマンドを `AEXCOMPAT_DISABLE_RENDER_SESSION_WRAPPER` env の on/off で session/one-shot 切替
  (`session_eligible` at image_render.rs:4824 が env を見る、確認済み)。
- **inspect command** (preflight 単体): harness `--inspect-gpu-module-report <plugin> <backend>
  <policyPath>` + `PreparedGpuRuntimePolicy::report_json()`。preflight を単独で叩き report を出す。
- **C6**: `test_gpu_policy_render_routes_through_the_session_and_a_preflight_producer` (gate tripwire)。
  broker workspace (harness 含む) clean compile、source-text 8 passed。

### 決定的 finding: single-link 認証 vs System32 GPU loader の非互換 (手順4 のブロッカー)
preflight スモークテスト (`--inspect-gpu-module-report` + nvcuda policy + synthetic AEX) を実 GPU で
実行 → **broker 側 policy 検証で `runtime module must be regular and single-link` により拒否**
(`runtime_module_policy.rs:472 validate_regular_unique`, `nNumberOfLinks != 1`)。preflight worker は
起動前。

原因 (fsutil hardlink list で確認、観察):
- `C:\Windows\System32\nvcuda.dll` は **2 ハードリンク**: System32 ↔
  `System32\DriverStore\FileRepository\nvmdsi.inf_amd64_.../nvcuda_loader64.dll` (同一 inode)。
  → どちらのパスからも `nNumberOfLinks==2` で single-link 不可。
- 他 GPU loader も全て multi-link (System32 ↔ WinSxS/DriverStore):
  OpenCL.dll=2, d3d12.dll=2, d3d11.dll=2, vulkan-1.dll=3。
- glator の OpenGL が通った理由: ICD `nvoglv64.dll` は System32 に無く DriverStore 経由で registry
  ロード = **single-link**。System32 標準 loader (CUDA/OpenCl/DirectX) は Windows が WinSxS/DriverStore
  に正規ハードリンクを張るため multi-link。

意味 (仮説→ほぼ確定):
- **GPU-render-via-authenticated-policy 設計全体 (one-shot `dispatch_secure_gpu_image` も
  session `dispatch_secure_gpu_image_session` も) が、単一 link 要件で System32 の実 GPU runtime
  loader を認証できない**。producer が今まで無く一度も end-to-end 実行されなかった root cause が
  これ (認証を通せる GPU モジュールが無い)。
- C1-C4 のコードはバグではない。認証設計 (evidence-tier integrity) の single-link 不変条件が、
  System32 GPU loader という現実の対象に初めて exposed された。
- CLAUDE.md より single-link (hardlink TOCTOU 防止の host-protection 不変条件) は weaken 不可。
  手順4 (実 GPU A/B) は設計判断が要る: (a) system-managed DLL (System32↔WinSxS/DriverStore の
  正規ハードリンク) を署名/DriverStore 帰属等の補償で multi-link 許容する認証拡張、(b) 実際に
  ロードされる single-link な vendor driver DLL のみで policy を組めるか (nvwgf2umx.dll 等、要実測・
  不確実)、(c) 手順4 を defer。→ 別 issue 化して方針決定。
- 手順1-3 (producer + routing) の**コードは完成・コンパイル済み**。手順4 の実機検証のみブロック。

### 追加 finding: single-link は vendor DriverStore DLL で回避可、しかし require_module_audit が次の壁
方針(b) の実測 (fsutil hardlink list):
- vendor driver DLL は DriverStore で **single-link**: `nvopencl64.dll` (1 link, OpenCL ICD),
  `nvwgf2umx.dll` (1 link, D3D12/D3D11 UMD)。CUDA は nvcuda_loader64.dll も 2 link で不可のまま。
- → OpenCL/DirectX は vendor DLL を policy に載せれば single-link を通せる (CUDA=Auto は不可)。

DirectX vendor UMD (nvwgf2umx.dll) policy で preflight スモークテスト再実行 → **policy 検証は通過**。
次のエラー: `secure worker module audit is missing` (`worker_module_audit.rs:34
validate_required_worker_audit`)。secure dispatch は `require_module_audit: true` 固定で、worker が
標準 DLL-load module audit を stdout の `module_audit` フィールドで emit することを要求。要件:
- `module_audit` フィールド必須、schema==1, status=="passed", **unknown_count==0** (全 snapshot)。
- `phase_count >= 3` (3+ capture フェーズ)。
- `observed_union.worker` 非空 **かつ `observed_union.plugin` 非空** ("lacks required images")。

preflight にとっての壁 (観察 + 仮説):
- plugin 非空 → preflight が plugin をロードしていない (report のためには不要だった)。
- zero-unknown → `begin_backend_context` で GPU DLL をロードすると、その DriverStore 依存 DLL 群が
  policy 未記載なら unknown 分類され audit fail。
- **これは実 GPU render 本体も同じ制約**: render worker が GPU DLL + 推移的依存をロードし、
  require_module_audit が zero-unknown を要求する → policy に **GPU DriverStore DLL の推移的閉包を
  全列挙** (全て single-link, 全て manifest 記載) する必要。これが GPU policy 経路が producer 不在
  以前に一度も機能しなかった核心的理由。

### 手順4 の再評価 (重要)
手順4 (実 GPU A/B) は「実装済み経路の検証」ではなく、**never-executed な secure GPU dispatch を
初めて機能させる複数難題の R&D**:
1. #300 single-link (vendor DriverStore DLL で OpenCl/DirectX は回避可、CUDA 不可)。
2. require_module_audit: plugin ロード + 3+ フェーズ + zero-unknown → GPU DLL 推移的閉包の
   完全列挙 policy (driver バージョン依存, 大)。
手順1-3 (producer + routing) のコードは完成しているが、手順4 はこの2難題の解決を要し、#290 当初の
「Cuda 実機依存の gate 付きテスト」想定を大きく超える。→ 方針判断が必要 (ユーザに諮る)。

## 2026-07-22 レビュー結果 + ゴール変更

### ローカルレビュー + Codex
- ローカルエージェントレビュー: blocking なし。低 severity 2 件 (GPU report の enumeration overflow /
  空 report 認証) を fail-closed 強化で対応 (commit 7e34c82)。
- Codex P1 (l2_main.cpp:1782): 「preflight は require_module_audit を満たさず構造的に成功不能」。正確
  (smoke test で確認済みの #300 の壁)。Codex 提案 (a) module_audit 併記 / (b) 検証無効化 は
  それぞれ「動かないコード追加」/「不変条件 weaken」で原則と衝突。
- owner 判断: #301 を **hold (draft)、merge しない**。Codex thread は #300 tracker として未解決のまま。

### ゴール (2026-07-22 更新): #300 の解決 + W4 (#290 手順4) の完全解決
hold から方針転換。**#300 の2難題を実際に解き、GPU render A/B まで完遂する**。#296 マージ済みで
実 GPU は排他競合なし。

### #300 解決計画 (path a: nv DriverStore 閉包を policy に列挙)
module-audit の unknown 分類の実態 (再分析):
- 分類は worker(exe) / plugin(plugin dir) / system32(System32 dir) / policy(manifest 記載) / unknown。
- d3d12.dll 等の標準 loader は System32 からロード → **system32 分類 (unknown ではない)**。OK。
- unknown になるのは **DriverStore の nv 固有 DLL** (nvwgf2umx.dll 等、System32 外) で policy 未記載のもの。
  これらは single-link (nvwgf2umx.dll=1, nvopencl64.dll=1 実測済み)。
- → **policy に、GPU backend がロードする nv DriverStore DLL の推移的閉包を全列挙**すれば
  unknown_count==0 を満たせる。閉包は audit の unknown_keys から反復的に発見可能・driver 依存だが
  enumerable。d3d12/System32 deps は system32 分類で policy 不要。

同時に require_module_audit は observed_union.plugin 非空 + worker 非空 + phase_count>=3 を要求する
ため、preflight も **plugin をロードし 3+ フェーズ audit を回す**必要 (= render worker の audit
lifecycle を preflight でも踏む)。これは Codex 提案(a)の contract も満たす。

同じ閉包 policy が preflight と実 render 両方で機能する (両者とも同じ GPU DLL をロード)。

### 実装ステップ (#300 → W4)
1. preflight に plugin ロード + module_audit lifecycle (3+ フェーズ) を追加し、module_audit + gpu report
   の結合 JSON を emit。broker は module_audit を validate_required_worker_audit に通し、gpu report を
   抽出。→ これで preflight が走り、unknown_keys が **nv DriverStore 閉包を露出**する (診断兼 Codex fix)。
2. unknown_keys を見て single-link な nv DriverStore DLL を policy に反復追加、zero-unknown まで収束。
3. 収束した閉包 policy で preflight 成功 → producer が実 GPU で report を produce (C1/C2 初 e2e 検証)。
4. 実 render (DirectX/OpenCl) を session と one-shot で回し byte + report 等価 A/B (#290 手順4)。
5. gate script 化 (`run-invert-gpu-session-render-gate.ps1`) + evidence + 全 worker ビルド +
   broker 統合テスト。
6. #301 を完成 (Codex fix 含む) して Codex clean → merge。#300 を Closes。
CUDA (Auto) は nvcuda が multi-link で不可のまま; W4 の実 render 検証は DirectX/OpenCl で行う
(#290 スコープに明示 GPU backend 含む)。CUDA=Auto は #300 で別途 (署名/DriverStore 帰属認証拡張) 要検討。

### 決定的 breakthrough (2026-07-22): DirectX 閉包を実測、path (a) viable 確定
スタンドアロン probe (`scratchpad/gpu_closure_probe.cpp`: D3D12CreateDevice(default adapter) +
EnumProcessModulesEx) で D3D12 デバイス作成時のロード閉包を実測 (実 GPU, hr=0 成功):
- ロード総数 58。**非 System32 直下のモジュールは worker exe + 5 個の DriverStore nv DLL のみ**、
  かつ **5 個とも single-link (linkcount=1)**:
  - `nvldumdx.dll`, `nvgpucomp64.dll`, `NvMemMapStoragex.dll`, `nvwgf2umx.dll`, `nvppex.dll`
  - 全て `C:\Windows\System32\DriverStore\FileRepository\nvmdsi.inf_amd64_d39f1ab212fcacea\` 配下。
- 残り 51 個は System32 直下 → audit で system32 分類 (unknown ではない)。

→ **policy にこの 5 個 (backend=directx) を載せれば**: (1) 全て single-link で validate_regular_unique
を通り、(2) audit の unknown はゼロ (worker=exe / plugin=sealed / system32=51 / policy=5)。#300 の
2 難題が DirectX で同時解決。閉包は小さく安定 (同一 DriverStore dir)。CUDA が multi-link nvcuda で
不可なのと対照的に、DirectX の vendor UMD 群は single-link。

### 残実装 (#300 → W4、次段)
A. **preflight に module_audit lifecycle 追加** (Codex fix 兼 zero-unknown 充足): plugin を
   LoadLibraryExW + `audit.required=true/plugin_path=plugin` 設定 + post_load(capture) →
   begin_backend_context → capture_phase → gpu report 構築 → pre_unload(capture) → phase_count>=3。
   stdout を結合 JSON `{ "module_audit": module_audit_json(), "gpu_module_report": <report> }` に。
   (top-level は deny_unknown_fields 無し、validate_required_worker_audit は "module_audit" のみ見る)。
B. **broker**: `prepare_gpu_runtime_policy` は stdout 全体でなく `gpu_module_report` フィールドを
   抽出して authenticate に渡す (現状は stdout 全体を report bytes 扱い)。
C. **DirectX 5-DLL policy** を構築 (上記 5 個、`\\?\` path、実 sha/size、backend=directx)。
D. preflight 実行 → 成功 + report を確認 (C1/C2 初 e2e 検証)。
E. 実 render A/B: `--render-experimental-smart-32-gpu-policy` を session / one-shot (DISABLE env) で
   回し byte + report 等価。GPU-capable fixture (SDK_Invert_ProcAmp DirectX) が要 → build-sdk-invert
   系でビルド。
F. gate script + evidence + 全 worker ビルド + broker 統合テスト → #301 完成 (Codex clean) → merge。

## 2026-07-22 MILESTONE: preflight producer が実 GPU で e2e 成功

`--inspect-gpu-module-report <pf_sampling_probe.aex> directx <5-DLL closure policy>` が **exit 0** で
gpu_module_report を出力。確認できたこと (観察):
- DirectX 5-DLL closure policy 受理 (single-link 検証通過)。
- preflight worker が plugin (pf_sampling_probe.aex) を LoadLibraryExW + D3D12 デバイス作成
  (begin_backend_context) + module_audit lifecycle (post_load / capture_phase / pre_unload, 3 phase)。
- **module_audit PASS (zero-unknown)**: 5 個の nv DriverStore DLL が policy 分類、残りは system32。
  → validate_required_worker_audit を通過 (require_module_audit 充足)。
- gpu_module_report が 5 policy モジュールを path_token + sha256 + size で報告。
- broker が結合 JSON から gpu_module_report 抽出 + authenticate_gpu_worker_report 認証成功
  → **C++ path_token が Rust path_token と実データで byte 一致 (correctness 実証)**。

= GPU-runtime-policy producer の**初の end-to-end 実 GPU 実行成功**。C1 (C++ preflight) + C2
(broker producer) が実機で機能。#300 の2難題 (single-link + zero-unknown module audit) が DirectX で
解決。Codex #301 の module_audit contract 指摘も同時に解消 (preflight が module_audit を emit)。

残: E. 実 render A/B (DirectX invert fixture で session vs one-shot、byte + report 等価)。
render worker も require_module_audit を通すため同じ closure policy を使うが、実 SmartRender は追加
DLL (d3dcompiler 等) をロードし得るので unknown を実測して policy に追記する可能性あり。

## 2026-07-22 W4 最終ブロッカー特定: render worker が manifest を parse しない

DirectX fixture (SDK_Invert_ProcAmp_DirectX.aex, PiPL .rc を .r から自前生成してビルド) で実 render
A/B を試行 → session/one-shot とも失敗。one-shot 診断:
- `first_failure_stage: gpu_device_setup`, error 512, exit_code 14 (module_audit_failed)。
- module_audit: `unknown_count: 5`, `policy: []` (空)。**5個の GPU DLL はロードされたが unknown 分類**。

根本原因: `parse_runtime_module_authorization` は `admit_runtime` (`worker_runtime_admission.cpp`) で
`request.authorize_runtime_modules` が真の時だけ呼ばれ、それは l2_main.cpp:1835 で
`!is_rendering_worker() && invocation.runtime_module_authorization_mode`。**render/smart worker は
`is_rendering_worker()==true` なので manifest を parse せず** g_authorized_runtime_modules が空 →
実 render 中にロードされる GPU DLL が policy でなく unknown に分類 → module audit fail →
gpu_device_setup が 512 で失敗 → render 失敗。preflight (私の --gpu-module-report-v1 mode) は明示的に
parse するので policy 分類され成功していた、という差。

= GPU render 経路が never-executed だった**第3の層**:
1. producer 不在 (解決: C1/C2)。
2. single-link 認証 vs System32 loader (解決: vendor DriverStore single-link closure)。
3. module audit zero-unknown (preflight は解決)。
4. **実 render dispatch が AEXRMA1 manifest を render worker に渡さず、render worker もそれを parse
   しない** ← 今ここ。

### 残実装 (W4 完遂の最終ピース)
- broker: `dispatch_secure_gpu_image` / `dispatch_secure_gpu_image_session` (または呼び出し元
  render_session.rs:1106 / image_render.rs one-shot) で、GpuRuntimePolicyInput.policy + session_identity
  から AEXRMA1 manifest を再 encode し、render worker に sealed dependency + `--runtime-module-authorization-v1`
  arg として渡す (preflight と同じ transport)。
- worker: render/smart path で manifest を parse し g_authorized_runtime_modules を populate
  (admit_runtime の `!is_rendering_worker()` gate を GPU render のケースで解除、または render 経路に
  manifest parse を追加)。これで実 render 中の GPU DLL が policy 分類され module audit 通過。
- 注意: security-sensitive (secure dispatch + worker admission)。manifest の session_identity は
  render の session と一致する必要 (preflight とは別 session_identity になる点に注意 — render 用に
  再生成した manifest の session を GpuRuntimePolicyInput.session_identity と揃えるか、render は
  manifest の session だけ使い report 認証と分離するか要設計)。

## 2026-07-22 W4 実 GPU A/B: routing 等価性を実証、GPU render エンジンの別バグを露出

4層目 (render worker が manifest を parse) を実装 (worker: strip_auxiliary_options に
`--runtime-module-authorization-v1` trailer 追加 + capture hook + prepare_runtime_request を render で
authorize; broker: render_session.rs / image_render.rs one-shot が policy から manifest 再 encode して
sealed dep + arg で render worker に配線) → **module audit 通過を実証**:
- one-shot DirectX で module_audit.observed_union.policy = [5 GPU DLL], status=passed (unknown 解消)。

### DirectX の追加ブロッカー: 外部 asset (sealed tree 非互換)
DirectX invert は `.cso` シェーダを `<plugin_dir>\DirectX_Assets\` から読む (SDK_Invert_ProcAmp.cpp:355)
が、flat な sealed load tree には .aex しか入らず asset 不在 → gpu_device_setup 512。
→ **OpenCL invert に切替** (カーネルを CreateCString で .aex に埋込、外部 asset 不要、sealed tree 互換)。

### OpenCL 実 GPU A/B: routing 等価性を実証
OpenCL 閉包を probe (context+queue+kernel build+run) で実測 → 4 個の DriverStore nv DLL、全 single-link:
nvopencl64 / nvdxgdmal64 / nvvm64 (clBuildProgram の NVVM) / nvptxJitCompiler64。nvcuda (multi-link) は
ロードされない。この 4-DLL policy で session / one-shot 両方を実行:

| | session | one-shot |
|-|-|-|
| module_audit | passed (4 DLL policy) | passed |
| gpu_device_setup_error | 0 | 0 |
| gpu_render_dispatched | **true** | **true** |
| smart_render_error | **4** | **4** |
| output_pixels_valid | false | false |

**session と one-shot が GPU dispatch まで完全に同一挙動 = #290 の routing 等価性を実証**。GPU デバイス
setup 成功 (error 0)、GPU render dispatched。同 plugin の **CPU render は正常** (smart_render_error=0,
output valid) なので plugin は健全。

### 残: smart_render_error=4 は GPU render エンジンの別バグ (両経路共通・orthogonal)
`what_gpu == PF_GPU_Framework_OPENCL` branch (SDK_Invert_ProcAmp.cpp:896) は走るが、その OpenCL render
内で error 4。= worker の **never-executed GPU render エンジン** (`gpu_memory_world_transport` が plugin に
渡す OpenCL GPU world data / PF_GPUDeviceSuite1) の問題。GPU render が今回初めて実行されて露出。
- #290 routing とは独立 (session/one-shot で同一)。#300 single-link/module-audit とも独立。
- → 別 issue 化。

### 到達点
- **#300 の2難題 (single-link + zero-unknown module audit) を解決・実 GPU で実証**。
- **#290 の routing 等価性 (session ≡ one-shot GPU dispatch) を実 GPU で実証**。producer が e2e 動作。
- **残る smart_render_error=4 は GPU render エンジン (worker GPU world transport) の別バグ**で、両経路
  共通・#290/#300 と orthogonal。これが解ければ byte 等価 A/B (valid pixels) まで到達する。

## 2026-07-22 #305 root cause 特定 (観察 → 確定)

### 診断手法
sandbox が stderr/file を封じるため、worker report の free-text JSON フィールド
(`world_debug_report_json`) に `gpu_diag_site` を追加し、error 4 を返す各経路に
site コードを埋めて特定した (TEMP 計装、確定後に revert)。

### 段階的な絞り込み (観察)
1. `CL2Err` は CL エラーを `PF_Err_INTERNAL_STRUCT_DAMAGED`(=512) に写像。
   `PF_Err_OUT_OF_MEMORY`=4 (AE_Effect.h:470 で確認)。→ error 4 は CL 由来ではない。
2. checkout callback (checkout_pixels/checkout_output, site 101-107) → 発火せず (site 0)。
3. suite 関数 (gpu_get_device_info/gpu_create_world/gpu_get_world_data, site 200-321) →
   発火せず (site 0)。かつ `gpu_allocations_created:2` (input/output のみ、plugin の
   CreateGPUWorld による3個目なし) → SmartRenderGPU 本体が短絡され未実行と判明。
4. `get_pixel_format` (PF_GetPixelFormat, site 400/402) を計装 → **site 402 で確定**。

### 確定した root cause
`SmartRender` wrapper (SDK_Invert_ProcAmp.cpp:1120) が `PF_WorldSuite2::PF_GetPixelFormat(input_worldP)`
を呼ぶ。worker の `get_pixel_format` → `resolve_dispatch_world_format` が world+24 を
登録値と比較して不一致で false (worker_world_safety.cpp:71-73) → error 4。

原因は **worker_smart_dispatch.cpp の登録と transport の +24 上書きの順序**:
1. L185 `register_world(input_world, GPU_BGRA128)` が world+24 = **ホスト側ピクセル
   ポインタ** を entry.data に記録。
2. L201 `prepare_render_transport` が world+24 を **GPU device ポインタ (cl_mem)** に上書き
   (plugin の GetGPUWorldData が +24 を返すため必須)。
3. dispatch 中 `PF_GetPixelFormat` の resolve が world+24 = device ptr を読み、
   entry.data = host ptr と不一致 → false → error 4。

CPU 経路は checkout が `input_checkout_view_world` (swap されない別 world) を返すため
(`use_views = !gpu_render_dispatched`) 露出しなかった。GPU 固有。

### 修正方針
`prepare_render_transport` が +24 を device ptr に swap した**後**に input/output world を
再登録し、dispatch 中に plugin が見る layout (device ptr) と登録 entry を一致させる。
register_world は同一 world の旧 entry を除去して置換するため (worker_world_safety.cpp:45-47)、
stale な host-ptr entry は上書きされる。安全側検査 (layout 比較) は弱めない。

### 修正と検証 (確定)
`worker_smart_dispatch.cpp` の GPU 経路で `prepare_render_transport` が +24 を device ptr に
swap した後、input/output world を `register_world(..., GPU_BGRA128)` で再登録するよう変更。
診断計装は全て revert し、修正 (再登録) のみ残した。クリーンビルドで実 GPU 検証:

| 経路 | smart_render_error | output_pixels_valid | output_sha256 |
|------|-------------------|---------------------|---------------|
| session (default) | 0 | true | 17331e64...bb17 |
| one-shot (WRAPPER 無効) | 0 | true | 17331e64...bb17 |

- **output_sha256 一致 = byte 等価 A/B 達成** (W4 完全解決)。
- output != input (`d4b4ade8...`) = 実 GPU compute (invert+procamp) 実行。
- 2経路は観測上明確に異なる (one-shot は sequence_setup/setdown stage を持つ、
  elapsed_ms 3818 vs 6242、36行差分) が同一ピクセルを産む = routing 等価。

### #305 の位置づけ (訂正)
「GPU render エンジン (worker GPU world transport) の別バグ」と記録していたが、正確には
**transport の +24 上書きと dispatch-format 登録の順序不整合**であり、GPU compute 自体
(cl_mem 転送・kernel) は健全だった。SmartRenderGPU に到達する前に PF_GetPixelFormat で
弾かれていた。

## 2026-07-22 #339 classic session の audio sidecar 経路 (観察 → 実装 → A/B)

`--render-experimental-image-audio-sidecar` は session 不適格として one-shot に落ちていた
唯一の classic 系 shape だった。one-shot 撤廃 (W4) には session 側にこの経路が必要。

### 設計 (実装済み)
one-shot は `--render-image-audio` という専用 command word の下で sample 数・rate・path を
**3つの裸の argv slot** に置く。session の tail は他の optional trailer (`render:v1|` /
`spatial:v` / `v2|` mask / `session-layers:v2|`) と共有されているため裸の slot は取れない。
そこで 1 つの marked trailer `session-audio:v1|<samples>|<rate>|<path>` に載せ、他と同じ
peel チェーンで剥がす。path を最後に置いているのは、path 中の `|` が数値フィールドを
ずらさないようにするため。audio は両経路とも classic 限定 (broker が SmartFX で拒否) なので
smart session 側は peel しない。

### 観察 1: fixture が動かなかった (実装とは無関係)
`instruments/pf-visual-audio-probe` の 7 target は `.rc` を持たず PiPL が無い。#84 で
worker が PiPL から Effect entrypoint を解決するようになって以降、これらは
`plugin_kind:"unknown_no_effect_entrypoint"` / exit 12 で load 前に弾かれ、**dispatch 不能**に
なっていた (`analysis/PF_VISUAL_AUDIO_ADMISSION_RESULT_2026-07-15.json` の凍結値は #84 以前の
観測)。#339 の A/B に必要なので、この PR で PiPL `.rc` と
`tools/build-pf-visual-audio-probe.ps1` を追加した (子 CMakeLists は add_subdirectory 用で
project() を持たないため、configure は `instruments` root から始める)。
`instruments/` 配下には同様に `.rc` を持たない target が他に 13 ある (未調査)。

### 観察 2: session が audio gate を素通ししていた (修正済み)
session wrapper は `InteractiveGateFacts.audio_present` を `false` にハードコードしていた。
実測: audio を advertise しない `SDK_Invert_ProcAmp_OpenCL.aex` に sidecar を渡すと
one-shot は validation で拒否、**session は exit 0 で通った**。同様に
`InteractiveImageReportFacts.audio_input_sha256` も `None` 固定で、session の公開 report からは
audio_* 24 キーが丸ごと消えていた。trailer と digest を 1 つの `SessionAudioSource` に束ねて、
gate・launch argv・公開 report が audio の有無で食い違えないようにした。

### A/B (確定)
fixture: `pf_visual_audio_sidecar_probe.aex` (PiPL 追加後)。sidecar は probe が期待する
window (`start=4, duration=6, scale=44100`) にちょうど合う 10 sample。

| 経路 | exit | output_sha256 | audio_checkout_calls | audio_lifetimes_balanced |
|------|------|---------------|----------------------|--------------------------|
| session (default) | 0 | 56a20c36...05b0 | 1 | true |
| one-shot (WRAPPER 無効) | 0 | 56a20c36...05b0 | 1 | true |

- **output_sha256 一致 = byte 等価**。PNG 実体も一致 (`a3e57d4d...`)。
- 公開 report の**キー集合が完全一致**し、値の差は `output_png` (出力先パス) と
  `worker_diagnostics` (elapsed_ms / メモリ / stage_events: session は sequence setup/setdown を
  frame から hoist するため必然的に異なる) のみ。

### 回帰テスト (否定側で検証済み)
`tests/test_render_session_worker.py` に worker session を直接叩く behavioral test を追加。
probe は読み戻した sample が完全一致したときだけ `PF_Err_NONE` を返すので、
`render_error == 0` がそのまま audio 到達の assertion になる。

peel (`l2_cli_dispatch.cpp`) を stash して worker を再ビルドすると、この test は
`worker closed the response pipe early` で fail した (trailer が余分な positional として
launch を fail-close させる)。peel を戻すと pass。**peel が必要十分であることを確認**。
trailer 無しの negative test も併せて追加 (`audio_source_available is False`)。

### ローカルレビューで出た指摘と対応

- **audio + secondary layer が session だけ通るようになっていた** (最重要)。
  one-shot の `--render-image-audio` は `effective_argc == 16` 完全一致なので layer を
  表現できない。gate から `audio.is_none()` を外した結果、この shape は session なら通り
  one-shot では失敗する = A/B 逃げ道が壊れる状態になっていた。classic 側の gate に
  `audio.is_none() || (secondaries.is_empty() && timed_secondaries.is_empty())` を足して
  #339 以前の挙動に戻し、#341 で追跡する。
- **修正した 2 つの分岐に回帰テストが無かった**。`build_interactive_image_report` /
  `validate_interactive_worker_report` を直接叩く unit test は、どちらも**修正前から
  正しかった純関数**を検証しているだけで、ハードコードを戻しても緑のままだった。
  `render_session_wrapper.rs` に A/B 統合テストを 2 本追加し、それぞれ該当行を戻すと
  fail することを実測で確認した。
  - `image_audio_sidecar_matches_the_one_shot_transport`: `audio_input_sha256` を
    `None` に戻すと key 集合比較で fail。
  - `an_unadvertised_plugin_with_a_sidecar_is_refused_on_both_routes`: `audio_present` を
    `false` に戻すと「session が受理した」で fail。fixture は `pf_sampling_probe`
    (audio を advertise せず、audio suite に一切触らない)。
    `pf_visual_audio_unadvertised_probe` は**使えない**: 意図的に unadvertised checkout を
    試みるので worker 側が先に `status: render_failed` を立て、gate に届く前に
    session が close で拒否してしまう。
- `tools/build-pf-visual-audio-probe.ps1` が共有の `target/instruments-build` を
  multi-config generator で configure しており、CI が同じディレクトリを Ninja で
  configure する (`.github/workflows/ae-sdk-tests.yml`) のと衝突していた。専用の
  `target/pf-visual-audio-probe-build` に変更し、テスト側は両レイアウトを探索する。

### ローカルレビュー 2 周目の指摘と対応

1 周目の対応を検証した上で、以下が新規に出た。

- `Cleanup` guard を fallible な `write_all` の**後**に構築していた。`open` が成功した
  時点でファイルは存在するので、write 失敗で partial file が leak する窓が残っていた。
  `render_session.rs` の layer sidecar が同じ規則をコメントで明示している
  ("track it for cleanup BEFORE the fallible write")。open 直後に構築するよう修正。
- gate ヘッダのコメント「audio はもう render を除外しない」が、classic 側の
  layer 除外を足した時点で**偽になっていた**。訂正。
- `SessionOpenRequest::audio_trailer` の doc「最末尾、後ろには何も無い」も偽
  (auxiliary option ペアはこの後ろに積まれる)。訂正。
- probe パス解決が `-Configuration Debug` のレイアウトを見ておらず、ビルド成功後も
  テストが黙って skip する形だった。両 configuration を探索するよう修正。
  併せてコメントの誤り (「Ninja レイアウトは CI が作る」) を訂正: CI の
  `instruments` configure は AE_SDK_ROOT 無しで走り、probe は
  `if(DEFINED ENV{AE_SDK_ROOT})` の内側なので **CI はこの probe を作らない**。
- Rust の文字列内行継続 (`\` + 改行) が編集の過程で潰れ、パスに空白が混入して
  A/B が黙って skip していた。`--nocapture` で skip 行を確認して発覚。
  `Path::join` に組み替えて修正 (「テストが緑」だけでは走った証明にならない実例)。

### 未解決 (この PR の範囲外)
- `tests/test_ae_reference_capture_automation.py::test_reference_capture_fails_closed_without_loaded_module_identity`
  が clean main でも fail する。#175 に追加観察をコメント済み。

## 2026-07-22 追記 (issue #365 / W4): one-shot 削除により A/B 手順 E は実施不能

**事実**: #365 で one-shot argv render 経路 (`--render-image*` /
`--render-audio` / `--smart-image*`) と escape hatch
`AEXCOMPAT_DISABLE_RENDER_SESSION_WRAPPER` を broker・worker 双方から削除した。

このため本ノートの以下の項目は**もう実行できない**。将来のセッションが未消化
タスクとして拾わないよう明記する。

- 「E. 実 render A/B: `--render-experimental-smart-32-gpu-policy` を session /
  one-shot (DISABLE env) で回して byte + report 等価」(残タスクとして記録されて
  いたもの) — 比較対象の one-shot が存在しない。
- 上記 302 行目の「A/B は同コマンドを DISABLE env の on/off で切替」も同様。
- 326 行目の one-shot `dispatch_secure_gpu_image` も削除済み
  (session 側 `dispatch_secure_gpu_image_session` は現存し、CPU backend 拒否と
  session identity 不一致拒否の 2 ガードは同一)。

**観察 (このノートの結論のうち今も有効なもの)**: 36-45 行目の「one-shot GPU も
policy 必須」「成功 path の report 等価は成立、乖離は GPU 失敗時のみ (one-shot は
CPU 再試行、session は fail-closed)」という調査結論は、#365 の判断根拠として使った。
session の fail-closed 側を canonical として残し、CPU 再試行の記録
(`gpu_attempt` / `gpu_fallback_used`) を生む経路ごと削除している。

**残る検証手段**: GPU render の実機確認は session 単独で行う
(`--render-experimental-smart-32-gpu-policy` は既定で session を通る)。等価性の
基準は「one-shot と一致すること」ではなく、レンダー結果そのものの性質
(#361 が A/B を session 直接検証に置き換えたときと同じ方針)。

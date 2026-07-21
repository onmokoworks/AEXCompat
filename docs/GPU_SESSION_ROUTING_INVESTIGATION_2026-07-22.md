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

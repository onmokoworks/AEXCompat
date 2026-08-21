# Build Requirements

このリポジトリの各コンポーネントをビルド・検証するために必要なものをまとめる。
「何を動かしたいか」によって必要なものが段階的に増える構成になっているため、
コンポーネント別に記載する。English summary at the end.

## 全体像

| コンポーネント | 生成物 | 必要なもの |
|---|---|---|
| Apple Silicon correctness CLI (`guest/`) | arm64 worker / local ad-hoc DMG | Apple Silicon Mac + Rust/Cargo |
| Rust broker / harness (`broker/`) | `aexcompat-harness.exe` (GUI) ほか | Rust + MSVC Build Tools + Windows SDK |
| Python テスト (`tests/`) | - | uv (`pyproject.toml` + `uv.lock`、一部は下記 SDK / VS も) |
| C++ worker (`minihost/`) | `aex_l2_worker.exe` / `aex_render_worker.exe` ほか | CMake + MSVC (After Effects SDK 不要) |
| probe AEX (`instruments/`) | `pf_*_probe.aex` ほか | CMake + Visual Studio + After Effects SDK |
| SDK sample fixture (v143 固定分: Grabba / Supervisor 等) | `Grabba.aex` ほか | v143 toolset + MSBuild + After Effects SDK (Supervisor は VS 2022 Build Tools 既定パス固定) |
| AE oracle 取得 (`tools/*.jsx`) | 参照画像 / trace | After Effects 25.2 実機 |

## Apple Silicon Mac単体の通常経路

Windows x64 AEXをarm64 Unicorn correctness backendでbuild・署名・package・実行・診断する
通常経路に必要なのは、Apple Silicon MacとRust/Cargoだけである。Windows実機、VM、Wine、
Windowsでbuildしたworker、Rosetta、Visual Studio、After Effects SDK、Developer ID証明書、
notarization credentialは必要ない。

```sh
tools/prepare-local-macos-aex-carriers.sh /tmp/aexcompat-local.dmg
```

このコマンドはarm64 Release build、ad-hoc Hardened Runtime署名、arm64-only DMG作成、
read-only mount、manifest/hash/signature/architecture/launch検証を順番に行う。手元のAEXとPNGを
使って、mountしたDMG内workerによるrenderとstructured diagnosticも検証できる。

```sh
AEXCOMPAT_SMOKE_AEX=/absolute/path/to/effect.aex \
AEXCOMPAT_SMOKE_INPUT_PNG=/absolute/path/to/input.png \
  tools/prepare-local-macos-aex-carriers.sh /tmp/aexcompat-local-smoke.dmg
```

smoke入力はpackageへ複製されない。どちらか一方だけの指定、render/cleanup失敗、不正JSON、
unsupported suite evidence、欠落/不正PNGはfail-closedで非0終了する。
`AEXCOMPAT_INCLUDE_NATIVE_CARRIER=1`はtrusted-only x86_64/Rosetta比較経路の明示opt-inであり、
通常経路では設定しない。Developer ID/notarizationは第三者配布向けの任意tierである。

## Windowsコンポーネントの共通要件

- Windows 10 / 11 x64
- Git

## Windows guest workspace tests

The Windows guest workspace tests exercise the Unicorn correctness backend and
require LLVM's `libclang.dll` because `unicorn-engine-sys` invokes bindgen at
build time. Install LLVM x64 and set `LIBCLANG_PATH` to the directory containing
both `libclang.dll` and `clang.exe`:

```powershell
$env:LIBCLANG_PATH = 'C:\Program Files\LLVM\bin'
cargo test --manifest-path guest\Cargo.toml --workspace --locked
```

CI does not hardcode that path. It takes the first directory holding both
`clang.exe` and `libclang.dll`, records the version it loaded, and fails before
Cargo when no candidate qualifies (#1457), so a runner-image update cannot
silently change what bindgen links against. LLVM/Clang 20.1.0 is verified on a
maintainer workstation; the 22.1.1 recorded here before 2026-08-20 was the
retired self-hosted runner's. The test count is intentionally not pinned because
it grows with compatibility work.
The macOS `native-carrier` path remains a separate Apple Silicon/Rosetta build
and is not compiled by this Windows gate.

## Rust broker / harness

- Rust toolchain (rustup / cargo)。workspace は edition 2024 (Rust 1.85 以降)。
- 標準の `x86_64-pc-windows-msvc` target ではリンクに MSVC linker と
  Windows SDK が必要。Visual Studio (または Build Tools) の
  「C++ によるデスクトップ開発」workload を入れておく。

```powershell
cd broker
cargo build -p aexcompat-harness --release
cargo test --workspace
```

When a broker API changes, check the AviUtl2 multifilter bridge and its examples
as well as the broker workspace:

```powershell
cargo check --manifest-path bridges\aviutl2-multifilter\Cargo.toml --all-targets --locked
```

After Effects SDK は不要。GUI の起動だけならこれで足りる
(ただし AEX の inspect / render には後述の worker ビルドが必要)。

## Python テスト

- uv。Python 本体は `.python-version` (3.12) に従い uv が管理版を解決する。
- `uv sync --locked` で `pyproject.toml` + `uv.lock` から `.venv` を構築する
  (pytest / Pillow / jsonschema / OpenEXR)。Pillow と jsonschema はテストが
  collection 時に import するため必須。

```powershell
uv run python -m pytest -q
```

期待結果は「ソースのみ検証」と「SDK 込み検証」で異なるため、入口で分けて
考える。

- **ソースのみ検証 (SDK なし)**: clean clone + `uv sync --locked` だけで
  実行した場合。SDK ヘッダの ABI を検証するテストは `AFTER_EFFECTS_SDK_ROOT`
  が未設定 (または無効) なら `pytest.skip` で明示的に skip される (一部は
  Visual Studio C++ tools 不在時も skip)。一方、probe / fixture を
  `tools/build-*.ps1` 経由で実際にビルドするテスト群は SDK の有無を事前
  チェックせず、SDK 解決の throw で fail する。**SDK なしでは fail が残るのが
  現状の想定結果**。fail / skip の件数はテストの増減で変わるため固定値は
  記載しない。判定基準は件数ではなく「fail がすべて
  `Set AFTER_EFFECTS_SDK_ROOT to a valid After Effects SDK root` に起因する
  ビルド系である」こと。それ以外の fail は regression を疑う。
- **SDK 込み検証**: `AFTER_EFFECTS_SDK_ROOT` と Visual Studio を揃えた構成。
  0 failed が期待結果 (「検証済み構成」の節を参照)。
- 上記と別軸で、ローカル生成物を要するテストは 2 つの manifest に分かれており、
  いずれも既定で skip される。
  - `tests/built_artifact_tests.txt` (`--run-built-artifact-tests`): この
    checkout からビルドした worker / probe / 生成入力 fixture を実行し、期待値を
    実行時に自己計算するテスト。フレッシュビルドで成立するため CI でも実行される
    (「CI (GitHub Actions)」の節を参照)。ローカルでは minihost の Ninja ビルド、
    worker の複製配置、probe ビルド、入力 fixture 生成 (workflow
    `windows-clean-clone.yml` のSDK利用時stepと同じ手順) の後にフラグを付けて実行する。
    加えて `abi_layout_probe` が要る (issue #981)。`tools/build-*.ps1` は
    `target\pf-*-probe-build` を configure するので、この probe は別に
    ビルドする:
    ```powershell
    cmake -S instruments -B target\instruments-build -G Ninja -DCMAKE_BUILD_TYPE=Release
    cmake --build target\instruments-build --target abi_layout_probe
    ```
    これを飛ばすと `test_abi_layout_observation_matches_probe.py` が
    `abi_layout_probe is not built` で skip され、`analysis/` の ABI 観測を
    実 SDK に繋ぎ止めている唯一の照合が走らないまま 0 failed になる。
    **`tools\refresh-aex-abi-layout-evidence.ps1` で代用しないこと**: あれは
    probe をビルドしたうえで観測 JSON を**上書き**するので、直後にこの
    テストを回しても「今書いた文書」と「それを書いた probe」を比べるだけに
    なる。観測を更新する意図があるときだけ使う。
  - `tests/local_artifact_tests.txt` (`--run-local-artifact-tests`): 記録済み
    evidence (sha256 / receipt) をローカル現物と照合する machine-bound テスト。
    evidence を採取したビルド状態でのみ成立するため CI 対象外。対象のビルド /
    gate スクリプトを走らせた後にフラグを付けて実行する。

## C++ worker (minihost)

harness からの AEX inspect / render は `target\minihost-build\` 直下の worker
実行ファイルを参照する。clone 直後に inspect / render まで進むには、harness の
ビルドに加えて minihost を single-config generator (Ninja) でビルドする必要がある。
After Effects SDK は不要 (include は Windows SDK と C++ 標準ライブラリのみ)。

```powershell
# Visual Studio developer 環境 (vcvars64) で。Ninja / CMake は VS 付属のもので可。
cmake -S minihost -B target\minihost-build -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build target\minihost-build
```

生成物の確認:

```powershell
Get-ChildItem target\minihost-build\aex_*.exe
# aex_l2_worker.exe / aex_render_worker.exe / aex_smart_worker.exe ほか
```

### ヘッダ依存追跡の検証 (issue #657)

ビルド後にこれを実行する:

```powershell
pwsh -File tools\verify-minihost-build-deps.ps1
```

Ninja + MSVC では、ヘッダ依存は cl の `/showIncludes` 出力を configure 時に
記録した `msvc_deps_prefix` と突き合わせて復元している。このプレフィックスは
**ローカライズされる**ため、configure 時と build 時で cl の言語 (あるいは
コンソールのコードページ) が食い違うと一行も一致せず、ninja はオブジェクトごとに
**ヘッダ依存を 0 件**として記録する。以後そのビルドディレクトリはヘッダを
書き換えても `no work to do` と答え続け、古いヘッダでコンパイルされた
オブジェクトを抱えたまま「最新」に見える。

実際にこれが起き、全 AEX が GLOBAL_SETUP で access violation を起こす worker が
できて AviUtl2 のフィルタ登録が 0 件になった (#651)。`minihost/CMakeLists.txt` は
configure と build の双方に `VSLANG` を固定してこの食い違いを防ぐが、
**それ以前に作られたビルドディレクトリは壊れたまま**なので、上の検証で落ちたら
ディレクトリごと削除して configure し直すこと。増分ビルドは復旧しない。

broker / harness と gate スクリプト (`tools/refresh-sdk-grabba-evidence.ps1` 等)
はこのパス直下の exe を前提にしているため、multi-config generator
(Visual Studio) で `Release\` 配下に出すと参照されない点に注意。

### worker admission と実行時provenance

harness の image dispatch は、dispatch 時点で`target\minihost-build\`の
ローカルビルドworkerをハッシュしてadmissionし、実行されるcopyがそのバイトと
一致することを確認する。凍結trust定数は撤去済みなので
(`docs/EVIDENCE_POLICY_2026-07-18.md` section 3)、第三者環境でもworkerを
ビルドすれば動き、定数の再生成や手動のhash合わせは不要である。

L2 / render / smart / SmartFX render requestを含むplug-in実行経路は、選択時の
identityをlaunch拒否条件にしない。実際に読み込んだplug-in bytesとmoduleを
毎回recordし、差分は再discovery・再検証・evidence不成立として扱う。workerが
未ビルド・空・読めない場合は、必要な実行ファイルが無いため起動前に失敗する。

## After Effects SDK

- Adobe の developer サイトから取得した After Effects SDK をローカルに展開し、
  環境変数 `AFTER_EFFECTS_SDK_ROOT` に root を設定する。
- root は **直下に `Examples\` を含むディレクトリ**を指すこと。配布 zip は
  `<展開先>\AfterEffectsSDK\Examples\...` のように一段入れ子になっている場合が
  あるので注意。
- 世代は **AE 25.2 SDK** を推奨 (下記の検証済み構成で使用したもの)。
- `analysis/` の receipt は、検証時に使われた SDK ヘッダのファイル identity
  (例: `AE_GeneralPlug.h` = 212,088 bytes / SHA-256 `632a648d...`) を記録して
  いる。これは「現在の検証済み SDK ファイル identity」であり、hash 単体で SDK
  世代を断定する識別子ではない。内容の異なるヘッダを持つ SDK を指すと
  provenance 照合系のテストが hash 不一致で fail する。
- AE 実機側の証跡が `25.2x131` であること
  (`docs/AE_REFERENCE_TRACE_2026-07-13.md`) は上記 SDK ファイル identity とは
  別の事実として扱う。
- SDK はリポジトリへ複製しない (`CLAUDE.md` の安全ルール)。

## CMake と Visual Studio

- `instruments/` のビルドスクリプト (`tools/build-*.ps1`) は、`-Generator`
  未指定時に `tools/resolve-cmake-generator.ps1` が vswhere で検出した VS の
  major version から generator を導出する (17 → `Visual Studio 17 2022`、
  18 → `Visual Studio 18 2026`。未知の major は明示 fail)。従来のような
  VS 2026 固定の既定値ではないため、VS 2022 のみの環境でもそのまま動く。
  `-Generator` 引数で明示上書きもできる
  (`tools/refresh-runtime-evidence.ps1` は実際に VS 2022 generator を渡している)。
- CMake の最低要件は各 `CMakeLists.txt` の `cmake_minimum_required` で 3.20。
  ただし `Visual Studio 18 2026` generator を使う場合は、その generator を
  認識するより新しい CMake が必要 (bundled 4.3.1 で検証。3.24 は
  `Visual Studio 18 2026` を解決できない)。CMake は Visual Studio bundled の
  もので足りる。
- CMake は `-CMake` 未指定時に `tools/resolve-build-cmake.ps1` が自動発見する
  (指定 generator を認識する cmake.exe を、vswhere で見つかる VS bundled →
  既知パス → PATH の順で探索)。以前は `$CMake` の既定値を VS 2026 Community の
  bundled パスに固定したスクリプトが混在していたが、現在は CMake を使う
  `tools/build-*.ps1` すべてが resolver 経由に統一されている。

generator / cmake を明示したい場合の実行例:

```powershell
# 通常は引数なしで、インストール済み VS から generator / CMake が解決される
powershell -File tools\build-pf-composite-rect-probe.ps1

# 明示上書きする場合
powershell -File tools\build-pf-adv-time-probe.ps1 -Generator "Visual Studio 17 2022" `
  -CMake "C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe"
```

- 例外として `tools/build-pf-fill-premultiply-probe.ps1` は CMake を使わず、
  vcvars64 環境で cl / rc / link を直接呼ぶ。`-Generator` / `-CMake` 引数は
  存在せず、`-VisualStudio` (既定は
  `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools`) で
  `VC\Auxiliary\Build\vcvars64.bat` を持つ VS instance の root を指定する。
  既定パスに VS 2022 Build Tools が無い環境では `-VisualStudio` の明示指定が
  必須 (VS 2026 等でも vcvars64.bat があれば可)。
- SDK sample fixture のうち `.vcxproj` を `PlatformToolset=v143` 固定でビルド
  するもの (`tools/build-sdk-grabba.ps1` / `tools/build-sdk-supervisor.ps1` の
  MSBuild build と、`tools/sdk-fixtures/*-v143.props` を使う fixture 群) は
  **v143 toolset (MSVC 14.3x/14.4x)** と MSBuild が別途必要。
- `build-sdk-grabba.ps1` は `tools/resolve-msvc-tools.ps1` で v143 toolset と
  MSBuild を持つ VS instance を自動発見する (vswhere と既知パスを探索。
  `-VisualStudioRoot` で明示指定も可)。VS 2022 のほか、v143 コンポーネントを
  追加した instance であれば条件を満たせる (VS 2022 Community 14.44 での
  build 成功を 2026-07-18 に確認)。
- 一方 `build-sdk-supervisor.ps1` は現時点では vcvars64.bat と MSBuild を
  `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools` の固定
  パスで検証・使用し、上書き引数を持たない。この fixture を build するには
  **VS 2022 Build Tools (v143 込み) をその既定の場所に導入**する必要がある
  (固定パス不在の環境で required input missing により fail することを
  2026-07-18 に確認)。
- 上記は v143 固定の fixture build に限った要件で、cl 直接呼び出しでビルド
  する fixture (`tools/build-sdk-invert-*.ps1`) は `-VisualStudio` 引数で
  VS instance を指定できる (既定はスクリプトごとに異なる)。
- `tools/refresh-sdk-grabba-evidence.ps1` は minihost を Ninja generator で
  ビルドする。Ninja は vcvars 経由で Visual Studio 付属のものが使われる。

## 任意 (機能別)

- **GPU render / device world 検証 (runtime)**: 対応 GPU と runtime
  (CUDA `nvcuda.dll` / OpenCL / DirectX 12)。結果は driver-build 固有。
- **GPU fixture の build (`tools/build-sdk-invert-*.ps1`)**: runtime とは別に、
  ビルド時の依存が必要。いずれも引数で場所を上書きできるが、既定パスは
  検証環境の固有値なので第三者環境ではまず一致しない。
  - 共通: **Boost の preprocessor ヘッダ** (`-BoostInclude`。既定は
    MotionBuilder 2024 OpenReality SDK の include パス) と、SDK の
    `GPUUtils` スクリプトを動かす **Python**。
  - `build-sdk-invert-directx.ps1`: **DXC** (`-Dxc`。未指定なら
    Windows Kits 10 配下から自動発見) と **OpenCL ヘッダ / OpenCL.lib**
    (`-OpenClSdk`。既定は CUDA v13.2 パス)。vcvars64 は未指定なら
    VS 既知パスから最新を自動発見。
  - `build-sdk-invert-cuda.ps1`: **CUDA toolkit (nvcc)** (`-CudaRoot`。既定は
    CUDA v13.2)。`-VisualStudio` の既定は VS 2022 Build Tools 固定パス。
  - `build-sdk-invert-opencl.ps1`: **OpenCL ヘッダ / OpenCL.lib**
    (`-OpenClSdk`。既定は CUDA v13.2 パス)。`-VisualStudio` の既定は
    VS 2026 Community 固定パス。
  - GPU デバイス自体はビルドには不要 (必要になるのは runtime 検証時)。
- **AE oracle**: After Effects 25.2 実機。`tools/*.jsx` を AE 内で実行して
  参照画像 / trace を取得する。通常の harness 実行には不要。

## CI (GitHub Actions)

`.github/workflows/windows-clean-clone.yml` の1本が push (main)、pull request、
scheduleでWindows runner上を走り、SDKを取得できるかで検証範囲を切り替える。

- 全runで`cargo build --workspace --locked`、Cargo workspace/bridgeのtest、
  minihost workerのNinja build、`uv run python -m pytest -q`を行う。
- fork PRなどSDK配布元へアクセスできない実行ではSDK依存stepをskipし、
  上記のsource-only範囲を検証する。
- private repositoryにおける同一repositoryのPR、main push、scheduleでは、
  非公開のR2バケット `aexcompat-ci` からhash-pinnedな
  `sdk/AfterEffectsSDK-ae25.2-win.zip` を
  取得する (#1445)。取得成功時だけprobe AEXとSDK fixtureを追加buildし、
  pytestへ`--run-sdk-tests`と`--run-built-artifact-tests`を追加する。
  SDK/成果物不足によるskipが残ればworkflowをfailさせ、silent successを防ぐ。
- 取得は `tools/fetch-r2-object.ps1` が行う。バケットは非公開のままで、
  read-onlyのR2 APIトークンで署名 (AWS SigV4) したGetObjectを投げる。
  資格情報はrepository secretsの `R2_SDK_ENDPOINT` /
  `R2_SDK_ACCESS_KEY_ID` / `R2_SDK_SECRET_ACCESS_KEY` から渡す。fork PRは
  secretsを受け取れないので、以前のprivate release時代と同じアクセス境界に
  なる。hashが `SDK_SHA256` と一致しない限り出力ファイルは作られない。
- SDK世代を更新するときは、新しいzipをバケットへ置いてからworkflow内の
  `SDK_OBJECT_KEY`、`SDK_ASSET`、`SDK_SHA256` を同時に更新する。書き込みは
  CIのread-onlyトークンではできないので、write権限のあるトークンを持った手元
  から行う (例: rcloneのR2 remoteで
  `rclone copyto <zip> r2:aexcompat-ci/sdk/<name>.zip`)。

local artifact テスト (`--run-local-artifact-tests`、machine-bound evidence
照合)、prebuilt テスト、AE 実機 oracle、GPU runtime 検証は CI の対象外で、
従来どおりローカル gate で実行する。SDK zip を置いたバケットは非公開で、資格情報を
持つ経路からしか読めない。SDK の公開再配布ではない (リポジトリへ SDK を複製しない
方針、およびバケットをpublic accessにしない方針は維持)。

Python は CI・ローカルとも `.python-version` (3.12) に従い uv が解決する
(OpenEXR の win_amd64 wheel が 3.14 に無く、ソースビルドで約 2.5 分かかる
ため 3.12 に留めている)。3.14 に wheel が出たら `.python-version` を上げて
`uv lock` し直せばよい。

## 最低対応版と検証済み構成

最低対応バージョンとして確認できているのは CMake の 3.20
(各 `CMakeLists.txt` の `cmake_minimum_required`) のみで、それも
`Visual Studio 18 2026` generator を使う場合はより新しい CMake が要る
(前節参照)。Rust / Python の最低バージョンは未確認。以下は実際に検証した
構成であり、これより古い版でも動く可能性はあるが未検証。

### 検証済み構成 (2026-07-18)

| 項目 | バージョン |
|---|---|
| OS | Windows 11 Pro |
| Rust | 1.93.1 |
| Python | 3.14.3 |
| Visual Studio | 2026 Community (generator / bundled CMake 4.3.1) + 2022 Community (v143 = MSVC 14.44) |
| After Effects SDK | ae25.2 (20.64bit) |

この構成で `python -m pytest -q` (SDK 環境変数設定済み) が **0 failed**、
`cargo test --workspace` が成功する。passed / skipped の件数はテストの増減で
変わるためここには固定値を記載しない (最新の内訳は手元の実行結果を見る)。

注: 上記は uv 移行 (2026-07-19、#78) 前に system Python 3.14.3 で検証した
記録。移行後は `.python-version` (3.12) の uv 管理 Python + `uv run` が
正準経路で、移行時に failure 集合が移行前後で一致することを確認済み
(0 failed の SDK 込み再検証はまだ)。

---

## English Summary

Per-component prerequisites on Windows x64:

- **Guest workspace tests**: Rust plus an x64 LLVM installation whose `bin`
  directory contains `libclang.dll` and `clang.exe`. Set `LIBCLANG_PATH` to
  that directory, then run
  `cargo test --manifest-path guest\Cargo.toml --workspace --locked`. LLVM
  20.1.0 is verified on a maintainer workstation; the 22.1.1 recorded here
  before 2026-08-20 was the retired self-hosted runner's. CI does not hardcode
  this directory: it takes the first one holding both `clang.exe` and
  `libclang.dll`, then records the version it loaded (#1457), so a runner-image
  update cannot silently change what bindgen links against. This Windows gate
  covers the Unicorn backend; the macOS-only native carrier is a separate check.
- **Rust broker / harness**: Rust toolchain plus MSVC Build Tools and the
  Windows SDK (the default `x86_64-pc-windows-msvc` target needs the MSVC
  linker). No After Effects SDK.
- **Python tests**: uv (`uv sync --locked` builds `.venv` from
  `pyproject.toml` + `uv.lock`: pytest, Pillow, jsonschema, OpenEXR; Pillow
  and jsonschema are imported at collection time). Expected
  results differ by entry point. Source-only verification (no SDK):
  SDK-header ABI tests explicitly skip, but the probe / fixture build tests
  that invoke `tools/build-*.ps1` do not pre-check and fail without the SDK.
  This document intentionally does not pin pass / fail / skip counts (they
  drift as tests are added); the acceptance criterion is that every failure
  is an SDK-absence build failure (`Set AFTER_EFFECTS_SDK_ROOT ...`) — any
  other failure suggests a regression. SDK-backed verification (SDK plus
  Visual Studio) expects 0 failed.
  Independently, tests needing local artifacts are split across two
  default-skip manifests: `tests/built_artifact_tests.txt`
  (`--run-built-artifact-tests`; runs artifacts built from this checkout
  with self-computed expectations, so CI runs it too) and
  `tests/local_artifact_tests.txt` (`--run-local-artifact-tests`;
  machine-bound evidence comparison, local-only).
- **C++ workers (minihost)**: build with the Ninja generator into
  `target\minihost-build\` so the harness and gate scripts find the four
  `aex_*_worker.exe` binaries directly under that directory. No SDK needed.
  The harness's image dispatch admits the locally built workers at dispatch
  time (frozen trust constants were retired; see
  `docs/EVIDENCE_POLICY_2026-07-18.md` section 3), so third-party builds work
  as soon as the workers exist. Plug-in identity is recorded from the bytes
  that actually load; a selection-time mismatch triggers rediscovery or
  evidence rejection rather than refusing launch.
- **After Effects SDK**: set `AFTER_EFFECTS_SDK_ROOT` to a directory that
  directly contains `Examples\`. The verified configuration uses the AE 25.2
  SDK generation. Provenance receipts record the verified SDK header file
  identity (for example `AE_GeneralPlug.h`, 212,088 bytes, SHA-256
  `632a648d...`); differing headers fail hash comparison. This file identity
  is separate from the AE runtime evidence being `25.2x131`.
- **CMake / Visual Studio**: when `-Generator` is omitted, probe build
  scripts derive the CMake generator from the installed Visual Studio via
  `tools/resolve-cmake-generator.ps1` (vswhere major version 17 →
  `Visual Studio 17 2022`, 18 → `Visual Studio 18 2026`; unknown majors fail
  explicitly), so machines with only VS 2022 work without overrides.
  `cmake_minimum_required` is 3.20, but the VS 2026 generator needs a newer
  CMake (bundled 4.3.1 verified). All CMake-based `tools/build-*.ps1`
  scripts auto-discover cmake.exe through `tools/resolve-build-cmake.ps1`
  (`-CMake` still overrides). One
  probe, `build-pf-fill-premultiply-probe.ps1`, does not use CMake at all:
  it drives cl / rc / link directly through vcvars64 and has no `-Generator`
  or `-CMake` parameter, only `-VisualStudio` (defaulting to the VS 2022
  Build Tools path), so machines without that default install must pass
  `-VisualStudio` explicitly. Only the
  v143-pinned SDK sample fixture builds (Grabba / Supervisor and the
  `*-v143.props` fixtures) additionally require the v143 toolset
  (MSVC 14.3x/14.4x) and MSBuild. `build-sdk-grabba.ps1` discovers a
  qualifying VS instance via `tools/resolve-msvc-tools.ps1` (overridable
  with `-VisualStudioRoot`), while `build-sdk-supervisor.ps1` currently
  validates hard-coded vcvars64 / MSBuild paths under
  `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools` with no
  override parameter, so that fixture needs VS 2022 Build Tools with v143 at
  that default location.
- **CI**: one conditional Windows workflow, `windows-clean-clone.yml`, runs
  for main pushes, pull requests, and schedules. Fork PRs, which receive no
  repository secrets and therefore cannot reach the SDK, still build/test the
  Cargo workspaces and bridges, build the minihost workers, and run
  source-only pytest. Same-repository runs while the repository is private
  receive the secrets and additionally fetch the
  hash-pinned AE 25.2 SDK from the private R2 bucket via
  `tools/fetch-r2-object.ps1`, build the probe AEX and SDK fixtures, and add
  `--run-sdk-tests` and `--run-built-artifact-tests` to pytest; missing-SDK or
  missing-artifact skips then fail the workflow. Local-artifact (machine-bound
  evidence), prebuilt, AE oracle, and GPU gates stay local-only.
- **Optional**: a matching GPU runtime for GPU render checks, and After
  Effects 25.2 itself for oracle capture only. Building the GPU SDK fixtures
  (`tools/build-sdk-invert-*.ps1`) additionally needs build-time inputs
  independent of any GPU device: Boost preprocessor headers and Python for
  all three, plus DXC and OpenCL headers / `OpenCL.lib` for the DirectX
  variant, the CUDA toolkit (nvcc) for the CUDA variant, and an OpenCL SDK
  for the OpenCL variant. All locations are overridable via script
  parameters; the defaults point at machine-specific install paths that a
  clean environment will not have.

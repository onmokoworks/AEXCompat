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

When a broker API changes, check both AviUtl2 bridge crates and their examples
as well as the broker workspace:

```powershell
cargo check --manifest-path bridges\aviutl2\Cargo.toml --all-targets --locked
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
    `ae-sdk-tests.yml` の該当 step と同じ手順) の後にフラグを付けて実行する。
    加えて `abi_layout_probe` が要る (issue #981):
    `tools/build-*.ps1` は `target\pf-*-probe-build` を configure するので
    `target\instruments-build` は作られない。
    ```powershell
    cmake -S instruments -B target\instruments-build -G Ninja -DCMAKE_BUILD_TYPE=Release
    cmake --build target\instruments-build --target abi_layout_probe
    ```
    これを飛ばすと `test_abi_layout_observation_matches_probe.py` が
    `abi_layout_probe is not built` で skip され、`analysis/` の ABI 観測を
    実 SDK に繋ぎ止めている唯一の照合が走らないまま 0 failed になる。
  - `tests/local_artifact_tests.txt` (`--run-local-artifact-tests`): 記録済み
    evidence (sha256 / receipt) をローカル現物と照合する machine-bound テスト。
    evidence を採取したビルド状態でのみ成立するため CI 対象外。対象のビルド /
    gate スクリプトを走らせた後にフラグを付けて実行する。

## C++ worker (minihost)

harness からの AEX inspect / render は `target\minihost-build\` 直下の worker
実行ファイルを参照する。clone 直後に inspect / render まで進むには、harness の
ビルドに加えて minihost を single-config generator (Ninja) でビルドし、さらに
後述の **worker trust** を自分のビルドに合わせて更新する必要がある。
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

### worker trust (第三者環境での注意)

harness の secure dispatch 経路 (image dispatch) は、dispatch 時点で
`target\minihost-build\` のローカルビルド worker をハッシュして admission し、
実行される staged copy がそのバイトと一致することを保証する。凍結 trust
定数は撤去済みなので (`docs/EVIDENCE_POLICY_2026-07-18.md` section 3 の
amendment を参照)、第三者環境でも worker をビルドすればそのまま該当経路が
動く。定数の再生成や手動でのハッシュ合わせは不要になった。

- receipt 駆動の経路 (L2 / render / smart / SmartFX render request) は従来
  どおり approval receipt に記録された worker identity と照合し、不一致は
  fail-closed のまま。
- worker が未ビルド・空・読めない場合、image dispatch は起動前に失敗する。

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

CI は 2 本の workflow に分かれる。いずれも push (main) / pull request ごとに
windows runner で走る。

- `.github/workflows/windows-clean-clone.yml`: source-only 検証。SDK なしの
  clean clone 相当で `cargo check` / `cargo test` (hosted runner の restricted
  token では起動できない 2 テストを `--skip`) と `uv run python -m pytest -q`
  を実行する。
- `.github/workflows/ae-sdk-tests.yml`: SDK 込み検証 + built artifact 検証。
  - private release `ci-sdk-ae25.2` の asset
    `AfterEffectsSDK-ae25.2-win.zip` を `GITHUB_TOKEN` でダウンロード・展開し、
    `AFTER_EFFECTS_SDK_ROOT` を設定する (Gyroflow が CI で Adobe SDK zip を
    取得するのと同じ方式)。zip の SHA-256 は workflow に pin されており、
    不一致は fail-closed。SDK 世代を更新するときは新しい asset を release に
    上げ、workflow の `SDK_RELEASE_TAG` / `SDK_ASSET` / `SDK_SHA256` を
    合わせて更新する。
  - minihost workers を Ninja でビルドして `target\minihost-build` に置き、
    multi-config 時代の固定パス (`minihost-build-v18\[Release]` /
    `minihost-timed-layers\Release` / `minihost-build\Release`) へ複製する。
    probe .aex 群を `tools/build-*.ps1` でビルドし、probe 入力 fixture
    (37x23 raw RGBA) を決定論的に生成する。
  - `uv run python -m pytest -q -rs --run-sdk-tests --run-built-artifact-tests
    --validate-local-artifact-manifest` を実行する。SDK 依存テスト
    (`tests/sdk_required_tests.txt` と `AFTER_EFFECTS_SDK_ROOT` を inline skip
    で見るテスト) に加え、`tests/built_artifact_tests.txt` の built artifact
    テストが実行対象になる。実行後、pytest 出力に SDK 起因 skip
    (`set AFTER_EFFECTS_SDK_ROOT`) や成果物不在 skip (`is not built` /
    `are not present`) が残っていれば fail させ、skip されたまま green になる
    silent success を防ぐ。

local artifact テスト (`--run-local-artifact-tests`、machine-bound evidence
照合)、prebuilt テスト、AE 実機 oracle、GPU runtime 検証は CI の対象外で、
従来どおりローカル gate で実行する。SDK asset は private repo の collaborator 限定 asset であり、SDK の
公開再配布ではない (リポジトリへ SDK を複製しない方針は維持)。

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
  as soon as the workers exist. Receipt-driven routes still verify workers
  against the identity recorded in their approval receipts.
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
- **CI**: two Windows workflows run per push / pull request.
  `windows-clean-clone.yml` covers source-only verification (`cargo check`,
  `cargo test` minus two launch tests the hosted runner's restricted token
  cannot execute, and `uv run python -m pytest -q` without the SDK).
  `ae-sdk-tests.yml` covers SDK-backed and built-artifact verification: it
  downloads the hash-pinned SDK zip from the private release
  `ci-sdk-ae25.2` with `GITHUB_TOKEN`, sets `AFTER_EFFECTS_SDK_ROOT`,
  builds the minihost workers (Ninja) and the probe AEX set, mirrors the
  workers into the multi-config layout paths, generates the deterministic
  probe input fixture, runs
  `uv run python -m pytest -q -rs --run-sdk-tests --run-built-artifact-tests
  --validate-local-artifact-manifest`, and fails if any test was skipped
  for a missing SDK or missing built artifact. Local-artifact
  (machine-bound evidence), prebuilt, AE oracle, and GPU gates stay
  local-only.
- **Optional**: a matching GPU runtime for GPU render checks, and After
  Effects 25.2 itself for oracle capture only. Building the GPU SDK fixtures
  (`tools/build-sdk-invert-*.ps1`) additionally needs build-time inputs
  independent of any GPU device: Boost preprocessor headers and Python for
  all three, plus DXC and OpenCL headers / `OpenCL.lib` for the DirectX
  variant, the CUDA toolkit (nvcc) for the CUDA variant, and an OpenCL SDK
  for the OpenCL variant. All locations are overridable via script
  parameters; the defaults point at machine-specific install paths that a
  clean environment will not have.

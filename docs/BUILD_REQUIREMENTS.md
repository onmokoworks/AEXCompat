# Build Requirements

このリポジトリの各コンポーネントをビルド・検証するために必要なものをまとめる。
「何を動かしたいか」によって必要なものが段階的に増える構成になっているため、
コンポーネント別に記載する。English summary at the end.

## 全体像

| コンポーネント | 生成物 | 必要なもの |
|---|---|---|
| Rust broker / harness (`broker/`) | `aexcompat-harness.exe` (GUI) ほか | Rust + MSVC Build Tools + Windows SDK |
| Python テスト (`tests/`) | - | Python + `requirements-dev.txt` (一部は下記 SDK / VS も) |
| C++ worker (`minihost/`) | `aex_l1_worker.exe` / `aex_l2_worker.exe` ほか | CMake + MSVC (After Effects SDK 不要) |
| probe AEX (`instruments/`) | `pf_*_probe.aex` ほか | CMake + Visual Studio + After Effects SDK |
| SDK sample fixture (v143 固定分: Grabba / Supervisor 等) | `Grabba.aex` ほか | v143 toolset + MSBuild + After Effects SDK (Supervisor は VS 2022 Build Tools 既定パス固定) |
| AE oracle 取得 (`tools/*.jsx`) | 参照画像 / trace | After Effects 25.2 実機 |

## 共通要件

- Windows 10 / 11 x64
- Git

## Rust broker / harness

- Rust toolchain (rustup / cargo)。workspace は edition 2021。
- 標準の `x86_64-pc-windows-msvc` target ではリンクに MSVC linker と
  Windows SDK が必要。Visual Studio (または Build Tools) の
  「C++ によるデスクトップ開発」workload を入れておく。

```powershell
cd broker
cargo build -p aexcompat-harness --release
cargo test --workspace
```

After Effects SDK は不要。GUI の起動だけならこれで足りる
(ただし AEX の inspect / render には後述の worker ビルドが必要)。

## Python テスト

- Python 3.x (3.14 で検証)
- `python -m pip install -r requirements-dev.txt` (pytest / Pillow / jsonschema)。
  Pillow と jsonschema はテストが collection 時に import するため必須。

```powershell
python -m pytest -q
```

期待結果は「ソースのみ検証」と「SDK 込み検証」で異なるため、入口で分けて
考える。

- **ソースのみ検証 (SDK なし)**: clean clone + `requirements-dev.txt` だけで
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
- 上記と別軸で、ローカル生成物 (ビルド済み worker、machine-bound receipt 等)
  を要するテストは `tests/local_artifact_tests.txt` に列挙されており、常に
  既定で skip される。実行するには対象のビルド / gate スクリプトを走らせた後
  `--run-local-artifact-tests` を付ける。

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
# aex_l1_worker.exe / aex_l2_worker.exe / aex_render_worker.exe / aex_smart_worker.exe
```

broker / harness と gate スクリプト (`tools/refresh-sdk-grabba-evidence.ps1` 等)
はこのパス直下の exe を前提にしているため、multi-config generator
(Visual Studio) で `Release\` 配下に出すと参照されない点に注意。

### worker trust (第三者環境での注意)

harness の secure dispatch 経路は、worker 実行ファイルを **broker ソースに
埋め込まれた SHA-256 / size (trust tuple)** と照合してから起動する。worker の
バイナリはビルド環境 (toolchain のバージョン等) で変わるため、第三者環境で
ビルドした worker は記録済みの trust と一致せず、該当経路の inspect / render
は拒否される。ビルドしただけでは足りない点に注意。

- **L2 worker**: trust は `broker/crates/broker/src/generated_l2_worker_trust.rs`
  に生成される。`tools/refresh-sdk-grabba-evidence.ps1` が minihost の
  再ビルド → hash 計測 → この定数の再生成までを行う (After Effects SDK が
  必要。Grabba fixture の再ビルドと evidence 更新も同時に走る)。
- **render / smart worker**: trust は
  `broker/crates/broker/src/image_render.rs` 内の `RENDER_WORKER_TRUST` /
  `SMART_WORKER_TRUST` 定数。現時点で再生成スクリプトは無く、自分のビルドの
  SHA-256 / size に手動で合わせる必要がある。
- trust はコンパイル時に harness へ取り込まれるため、更新後は
  `cargo build -p aexcompat-harness --release` で harness を再ビルドする。

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

- `instruments/` のビルドスクリプト (`tools/build-*.ps1`) は既定 generator が
  `Visual Studio 18 2026`。VS 2026 は必須ではなく既定・検証済み構成であり、
  各スクリプトの `-Generator` 引数で `Visual Studio 17 2022` 等へ上書きできる
  (`tools/refresh-runtime-evidence.ps1` は実際に VS 2022 generator を渡している)。
- CMake の最低要件は各 `CMakeLists.txt` の `cmake_minimum_required` で 3.20。
  ただし `Visual Studio 18 2026` generator を使う場合は、その generator を
  認識するより新しい CMake が必要 (bundled 4.3.1 で検証。3.24 は
  `Visual Studio 18 2026` を解決できない)。CMake は Visual Studio bundled の
  もので足りる。
- CMake の見つけ方はスクリプトにより二通り混在している。
  `tools/resolve-build-cmake.ps1` で VS bundled CMake を自動発見するもの
  (`build-pf-composite-rect-probe.ps1` 等) と、`$CMake` の既定値を
  VS 2026 Community の bundled パスに固定したままのもの
  (`build-pf-adv-time-probe.ps1` / `build-pf-transform-affine-probe.ps1` 等)
  がある。後者は VS 2026 Community が無い環境ではそのままでは fail するため、
  `-CMake` で cmake.exe を明示指定する。

VS 2022 でビルドする場合の実行例:

```powershell
# resolver 対応スクリプト: generator の指定だけでよい
powershell -File tools\build-pf-composite-rect-probe.ps1 -Generator "Visual Studio 17 2022"

# 既定 CMake パス固定のスクリプト: cmake.exe も明示する
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

---

## English Summary

Per-component prerequisites on Windows x64:

- **Rust broker / harness**: Rust toolchain plus MSVC Build Tools and the
  Windows SDK (the default `x86_64-pc-windows-msvc` target needs the MSVC
  linker). No After Effects SDK.
- **Python tests**: Python 3.x plus `requirements-dev.txt` (pytest, Pillow,
  jsonschema; the latter two are imported at collection time). Expected
  results differ by entry point. Source-only verification (no SDK):
  SDK-header ABI tests explicitly skip, but the probe / fixture build tests
  that invoke `tools/build-*.ps1` do not pre-check and fail without the SDK.
  This document intentionally does not pin pass / fail / skip counts (they
  drift as tests are added); the acceptance criterion is that every failure
  is an SDK-absence build failure (`Set AFTER_EFFECTS_SDK_ROOT ...`) — any
  other failure suggests a regression. SDK-backed verification (SDK plus
  Visual Studio) expects 0 failed.
  Independently, tests listed in `tests/local_artifact_tests.txt`
  always skip by default (`--run-local-artifact-tests` to opt in).
- **C++ workers (minihost)**: build with the Ninja generator into
  `target\minihost-build\` so the harness and gate scripts find the four
  `aex_*_worker.exe` binaries directly under that directory. No SDK needed.
  Note the harness's secure dispatch verifies workers against SHA-256 / size
  trust tuples embedded in the broker sources, so third-party builds are
  rejected until the trust is refreshed: the L2 tuple is regenerated by
  `tools/refresh-sdk-grabba-evidence.ps1` into
  `broker/crates/broker/src/generated_l2_worker_trust.rs`, while
  `RENDER_WORKER_TRUST` / `SMART_WORKER_TRUST` in
  `broker/crates/broker/src/image_render.rs` currently have no regeneration
  script and must be updated manually; rebuild the harness afterwards.
- **After Effects SDK**: set `AFTER_EFFECTS_SDK_ROOT` to a directory that
  directly contains `Examples\`. The verified configuration uses the AE 25.2
  SDK generation. Provenance receipts record the verified SDK header file
  identity (for example `AE_GeneralPlug.h`, 212,088 bytes, SHA-256
  `632a648d...`); differing headers fail hash comparison. This file identity
  is separate from the AE runtime evidence being `25.2x131`.
- **CMake / Visual Studio**: probe build scripts default to the
  `Visual Studio 18 2026` generator, overridable per script with
  `-Generator` (VS 2026 is the verified default, not a hard requirement).
  `cmake_minimum_required` is 3.20, but the VS 2026 generator needs a newer
  CMake (bundled 4.3.1 verified). CMake lookup is mixed: some scripts
  auto-discover the VS-bundled CMake via `tools/resolve-build-cmake.ps1`,
  while others still default `$CMake` to the VS 2026 Community bundled path
  and need an explicit `-CMake` on machines without it (a VS 2022 example
  command line is included in the Japanese section and was verified). One
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
- **Optional**: a matching GPU runtime for GPU render checks, and After
  Effects 25.2 itself for oracle capture only. Building the GPU SDK fixtures
  (`tools/build-sdk-invert-*.ps1`) additionally needs build-time inputs
  independent of any GPU device: Boost preprocessor headers and Python for
  all three, plus DXC and OpenCL headers / `OpenCL.lib` for the DirectX
  variant, the CUDA toolkit (nvcc) for the CUDA variant, and an OpenCL SDK
  for the OpenCL variant. All locations are overridable via script
  parameters; the defaults point at machine-specific install paths that a
  clean environment will not have.

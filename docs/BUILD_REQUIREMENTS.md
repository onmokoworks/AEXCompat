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
| SDK sample fixture (v143 固定分: Grabba / Supervisor 等) | `Grabba.aex` ほか | v143 toolset + MSBuild + After Effects SDK |
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
- `python -m pip install -r requirements-dev.txt` (pytest)

```powershell
python -m pytest -q
```

- ローカル生成物 (ビルド済み worker、machine-bound receipt 等) を要するテストは
  `tests/local_artifact_tests.txt` に列挙されており、既定で skip される。
  実行するには対象のビルド / gate スクリプトを走らせた後
  `--run-local-artifact-tests` を付ける。
- SDK ヘッダを使うテストは `AFTER_EFFECTS_SDK_ROOT` が未設定 (または無効) の
  場合 `pytest.skip` で明示的に skip される。一部は Visual Studio C++ tools が
  見つからない場合も skip する。つまり clean clone での `python -m pytest -q`
  はこれらが skip されて通る。下記の SDK と Visual Studio が必要になるのは、
  probe / fixture を実際にコンパイル・実行する場合のみ。

## C++ worker (minihost)

harness からの AEX inspect / render は `target\minihost-build\` 直下の worker
実行ファイルを参照する。clone 直後に inspect / render まで進むには、harness の
ビルドに加えて minihost を single-config generator (Ninja) でビルドしておく。
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
- SDK sample fixture のうち `.vcxproj` を `PlatformToolset=v143` 固定でビルド
  するもの (`tools/build-sdk-grabba.ps1` / `tools/build-sdk-supervisor.ps1` の
  MSBuild build と、`tools/sdk-fixtures/*-v143.props` を使う fixture 群) は
  **v143 toolset (MSVC 14.3x/14.4x)** と MSBuild を持つ Visual Studio instance
  が別途必要。VS 2026 の既定 toolset (14.5x) では代用できないので、VS 2022 を
  併設するか VS 2026 に v143 コンポーネントを追加する。これは v143 固定の
  fixture build に限った要件で、cl 直接呼び出しでビルドする fixture
  (`tools/build-sdk-invert-*.ps1` 等) には適用されない。
- `tools/refresh-sdk-grabba-evidence.ps1` は minihost を Ninja generator で
  ビルドする。Ninja は vcvars 経由で Visual Studio 付属のものが使われる。

## 任意 (機能別)

- **GPU render / device world 検証**: 対応 GPU と runtime
  (CUDA `nvcuda.dll` / OpenCL / DirectX 12)。結果は driver-build 固有。
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

この構成で `python -m pytest -q` (SDK 環境変数設定済み) が
970 passed / 96 skipped / 0 failed、`cargo test --workspace` が成功する。

---

## English Summary

Per-component prerequisites on Windows x64:

- **Rust broker / harness**: Rust toolchain plus MSVC Build Tools and the
  Windows SDK (the default `x86_64-pc-windows-msvc` target needs the MSVC
  linker). No After Effects SDK.
- **Python tests**: Python 3.x plus `requirements-dev.txt`. Tests that need
  locally built workers or machine-bound receipts are listed in
  `tests/local_artifact_tests.txt` and skip by default
  (`--run-local-artifact-tests` to opt in). SDK-header tests explicitly skip
  when `AFTER_EFFECTS_SDK_ROOT` is unset (some also skip without VS C++
  tools), so a clean-clone `python -m pytest -q` passes with skips; the SDK
  and Visual Studio are needed only to actually compile probes / fixtures.
- **C++ workers (minihost)**: build with the Ninja generator into
  `target\minihost-build\` so the harness and gate scripts find the four
  `aex_*_worker.exe` binaries directly under that directory. No SDK needed.
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
  CMake (bundled 4.3.1 verified). Only the v143-pinned SDK sample fixture
  builds (Grabba / Supervisor and the `*-v143.props` fixtures) additionally
  require the v143 toolset (MSVC 14.3x/14.4x) and MSBuild, e.g. a
  side-by-side VS 2022.
- **Optional**: a matching GPU runtime for GPU render checks, and After
  Effects 25.2 itself for oracle capture only.

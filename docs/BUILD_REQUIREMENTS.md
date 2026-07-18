# Build Requirements

このリポジトリの各コンポーネントをビルド・検証するために必要なものをまとめる。
「何を動かしたいか」によって必要なものが段階的に増える構成になっているため、
コンポーネント別に記載する。English summary at the end.

## 全体像

| コンポーネント | 生成物 | 必要なもの |
|---|---|---|
| Rust broker / harness (`broker/`) | `aexcompat-harness.exe` (GUI) ほか | Rust toolchain のみ |
| Python テスト (`tests/`) | - | Python + `requirements-dev.txt` (一部は下記 SDK / VS も) |
| C++ worker (`minihost/`) | `aex_l1_worker.exe` / `aex_l2_worker.exe` ほか | CMake + MSVC |
| probe AEX (`instruments/`) | `pf_*_probe.aex` ほか | CMake + Visual Studio 2026 + After Effects SDK |
| SDK sample fixture (Grabba ほか) | `Grabba.aex` ほか | v143 toolset + MSBuild + After Effects SDK |
| AE oracle 取得 (`tools/*.jsx`) | 参照画像 / trace | After Effects 25.2 実機 |

## 共通要件

- Windows 10 / 11 x64
- Git

## Rust broker / harness

- Rust toolchain (rustup / cargo)。workspace は edition 2021。

```powershell
cd broker
cargo build -p aexcompat-harness --release
cargo test --workspace
```

SDK も Visual Studio も不要。GUI の起動だけならこれで足りる
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
- probe / fixture をコンパイルするテスト群は skip されず、下記の
  After Effects SDK と Visual Studio が無い環境では fail する。

## After Effects SDK

- Adobe の developer サイトから取得した After Effects SDK をローカルに展開し、
  環境変数 `AFTER_EFFECTS_SDK_ROOT` に root を設定する。
- root は **直下に `Examples\` を含むディレクトリ**を指すこと。配布 zip は
  `<展開先>\AfterEffectsSDK\Examples\...` のように一段入れ子になっている場合が
  あるので注意。
- 世代は **AE 25.2 SDK** を推奨。`analysis/` の receipt に記録された SDK ヘッダの
  SHA-256 (例: `AE_GeneralPlug.h` = `632a648d...`、212,088 bytes) は 25.2 世代の
  ものなので、別世代 (May2023 / 25.6 等) を指すと provenance 照合系のテストが
  hash 不一致で fail する。
- SDK はリポジトリへ複製しない (`CLAUDE.md` の安全ルール)。

## CMake と Visual Studio

- `instruments/` と `minihost/` の CMake configure は、ビルドスクリプトの既定
  generator が `Visual Studio 18 2026` のため **Visual Studio 2026** が必要。
  別バージョンを使う場合は各スクリプトの `-Generator` 引数で上書きする。
- CMake は Visual Studio bundled のもので足りる。スタンドアロン CMake を使う
  場合は該当 generator を認識するバージョンであること (4.x で検証。3.24 は
  `Visual Studio 18 2026` を解決できない)。
- SDK sample fixture (`tools/build-sdk-grabba.ps1` 等) は `.vcxproj` を
  `PlatformToolset=v143` でビルドするため、**v143 toolset (MSVC 14.3x/14.4x)**
  と MSBuild を持つ Visual Studio instance が別途必要。VS 2026 の既定 toolset
  (14.5x) では代用できないので、VS 2022 を併設するか VS 2026 に v143
  コンポーネントを追加する。
- `tools/refresh-sdk-grabba-evidence.ps1` は minihost を Ninja generator で
  ビルドする。Ninja は vcvars 経由で Visual Studio 付属のものが使われる。

## 任意 (機能別)

- **GPU render / device world 検証**: 対応 GPU と runtime
  (CUDA `nvcuda.dll` / OpenCL / DirectX 12)。結果は driver-build 固有。
- **AE oracle**: After Effects 25.2 実機。`tools/*.jsx` を AE 内で実行して
  参照画像 / trace を取得する。通常の harness 実行には不要。

## 検証済み構成 (2026-07-18)

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

- **Rust broker / harness**: Rust toolchain only (`cargo build -p aexcompat-harness`).
- **Python tests**: Python 3.x plus `requirements-dev.txt`. Tests that need
  locally built workers or machine-bound receipts are listed in
  `tests/local_artifact_tests.txt` and skip by default
  (`--run-local-artifact-tests` to opt in). Probe-compiling tests additionally
  need the SDK and Visual Studio below.
- **After Effects SDK**: set `AFTER_EFFECTS_SDK_ROOT` to a directory that
  directly contains `Examples\`. Use the AE 25.2 SDK generation; provenance
  tests pin header hashes recorded from 25.2.
- **CMake / Visual Studio**: probe and worker builds default to the
  `Visual Studio 18 2026` generator (override per script with `-Generator`).
  The bundled VS CMake is sufficient; a standalone CMake must be new enough to
  know that generator. SDK sample fixtures (Grabba) additionally require the
  v143 toolset (MSVC 14.3x/14.4x) and MSBuild, e.g. a side-by-side VS 2022.
- **Optional**: a matching GPU runtime for GPU render checks, and After
  Effects 25.2 itself for oracle capture only.

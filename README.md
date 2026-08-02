# AEXCompat

**After Effects Effect AEXを、AEの外で読み込み・描画・デバッグする互換ホスト。**<br>
**A compatibility host for loading, rendering, and debugging After Effects Effect AEX plug-ins outside AE.**

[日本語](#日本語) | [English](#english) | [AEX移植解析ガイド](docs/aex-porting-dossier.md) | [互換性ステータス](docs/COMPATIBILITY_STATUS_2026-07-16.md) | [プロジェクト方針](docs/PROJECT_DIRECTION.md)

> [!WARNING]
> **Experimental / pre-alpha:** 未知のAEXはネイティブコードまたはguest codeとして実行されます。隔離機構はありますが、完全なsecurity sandboxではありません。Apple Siliconのx86_64 native carrierはtrusted plug-insにのみ使用してください。

## 日本語

> [!IMPORTANT]
> 日本語をこのプロジェクトの正本とします。英語版と解釈が異なる場合は、日本語版を優先します。

### 概要

AEXCompatは、Adobe After EffectsのEffect AEXをAfter Effects本体の外で実行するためのクリーンルーム互換ホストです。画像や音声をAEXへ入力し、結果の表示・保存・AE実機との比較・selector/Suite診断を行えます。

> **Public contribution safety:** proprietary AEX、Adobe SDK、DLL、dump、
> private asset/corpus、秘密情報、個人パスをIssueやPRへ投稿しないでください。
> 公開準備の境界と未解決のlicense判断は
> [docs/PUBLIC_RELEASE_AUDIT.md](docs/PUBLIC_RELEASE_AUDIT.md) を参照してください。

目標は特定のfixture専用エミュレーターではなく、一般のEffect AEXを実用的かつ忠実に動かすことです。After Effects全体、AEP編集環境、AEGP host全体の再実装は現在の主目的ではありません。

### スクリーン上の基本フロー

1. AEXを選択します。
2. `PF_PARAMS_SETUP`が自動実行され、左側の**Effect Controls**へパラメーターが表示されます。
3. 入力画像を選択し、必要なパラメーターを変更します。
4. **Quick render**または**Render and save PNG**を実行します。
5. FHD viewerで入力とAEX出力を比較します。

パラメーター取得は隔離workerで非同期実行されるため、手動のInspect操作は不要です。

### 主な機能

- 登録済み・未登録AEXの読み込みとSHA-256による実行直前の同一性確認
- 通常・delay-load PE importから到達する隣接DLLの再帰的な自動検出とhash固定
- AE風の常設Effect Controlsと型付きパラメーター編集
- PNG、JPEG、BMP、TIFF、WebPの画像入力とPNG出力
- Classic RenderおよびSmartFX
- ARGB8、ARGB16、ARGB32F
- 複数Layer入力、時間、FPS、downsample、pixel aspect、sequence state
- mono float32 audio入力とvisual audio sidecar
- 8/16/32 bpcの6-case互換matrix
- After Effects参照画像とのpixel比較
- custom UI、PF Suite、selector lifecycleの診断probe
- restricted worker、timeout、Job Object、ACL、sealed load tree、pixel guard
- crash、hang、selector error、host validation errorの分類表示

### 対応環境

| 項目 | 現在の対象 |
|---|---|
| Windows x64 | primary。Rust broker / harness + C++ / MSVC x64 worker |
| macOS Apple Silicon | experimental。arm64 Unicorn backend、任意でRosetta x86_64 native carrier |
| Windows ARM64 | 未対応 |
| SDK | Adobe After Effects SDK 2025を基準に検証 |
| UI / broker | Rust 2024 edition |
| native worker | C++ / MSVC x64 |
| 入力画像 | PNG、JPEG、BMP、TIFF、WebP |
| 出力画像 | PNG |

Apple Siliconではarm64 Unicorn workerがWindows x64 AEXをguestとして実行します。`AEXCOMPAT_NATIVE_CARRIER=1`を設定すると、Rosettaが利用可能な環境ではx86_64 native carrierを試します。このcarrierは明示opt-inの高速経路であり、未信頼AEX向けsandboxではないためtrusted plug-insにのみ使用してください。After Effects本体は通常のharness実行には不要ですが、AE oracleの取得とpixel一致検証には必要です。

### クイックスタート

#### 必要なもの

- Windows 10または11 x64
- Rust toolchainとCargo
- Visual Studio（MSVC C++ toolchain。必要なedition・toolsetはコンポーネントごとに異なる）
- CMake
- ローカルのAfter Effects SDK（probeやSDK fixtureをbuildする場合。minihost workerのbuildには不要）

Apple SiliconではApple Silicon MacとRust/Cargoを用意し、`tools/build-macos-aex-carriers.sh`でcarrierをbuildします。任意のnative carrierにはRosettaとx86_64 worker buildも必要です。

コンポーネント別の詳細な要件（SDKの世代、Visual Studioのバージョンとtoolset、CMakeの条件など）は[Build Requirements](docs/BUILD_REQUIREMENTS.md)を参照してください。

SDKを使うテスト・ビルドの前に、SDKルートをユーザー環境変数へ設定し、PowerShellを開き直してください。

```powershell
[Environment]::SetEnvironmentVariable('AFTER_EFFECTS_SDK_ROOT', 'C:\path\to\AfterEffectsSDK', 'User')
```

#### UIを起動

```powershell
git clone https://github.com/onmokoworks/AEXCompat.git
cd AEXCompat\broker
cargo run -p aexcompat-harness
```

#### release build

```powershell
cd broker
cargo build -p aexcompat-harness --release
.\target\release\aexcompat-harness.exe
```

release生成物はGit管理対象に含まれません。

### テスト

```powershell
# Python test dependency (uv が pyproject.toml + uv.lock から .venv を構築する)
uv sync --locked

# Rust broker / harness / isolation tests
cargo test --manifest-path broker\Cargo.toml --workspace

# Contract, ABI, oracle, and source-level regression tests
uv run python -m pytest -q
```

`pytest` がPythonテストの正規ランナーです。`unittest discover` ではbare function形式のテストを収集できないため、完全な検証には使用しません。一部のnative fixture、GPU、After Effects oracleテストには、ローカルSDK、対応GPU runtime、またはAE本体が必要です。ビルド生成物やローカル承認receiptを必要とするテストは、それらを生成する明示的なgateまたはbuild手順と組み合わせて実行します。

ローカル成果物を要するテストは2つに分かれます。この checkout からビルドした worker / probe を自己計算の期待値で検証するテストは `--run-built-artifact-tests` (CIでも実行)、記録済み evidence をローカル現物と照合する machine-bound テストは `--run-local-artifact-tests` (ローカル専用) を付けて実行します。通常のclean cloneではどちらも理由付きでskipします。

SDKや実AEXのないclean cloneでも、標準の`pytest`は依存テストを理由付きでskipし、0 failで完了します。SDK、checkoutからbuildしたartifact、machine-bound evidenceを使う検証は、それぞれ`--run-sdk-tests`、`--run-built-artifact-tests`、`--run-local-artifact-tests`で明示的にopt-inします。必要条件とCI matrixは[Build Requirements](docs/BUILD_REQUIREMENTS.md)を参照してください。

### アーキテクチャ

```text
Desktop Harness / CLI
        |
        v
Rust Broker
request validation / identity / sealed transport
        |
        v
Restricted Worker Process
        |
        v
Clean-room C++ Effect Host
selectors / PF worlds / parameters / suites
        |
        v
Selected AEX -> image / audio / diagnostic report
```

| ディレクトリ | 役割 |
|---|---|
| `broker/` | Rust broker、隔離起動、デスクトップharness |
| `minihost/` | AE Effect ABIとPF Suiteを提供するC++ worker |
| `instruments/` | SDK ABI、Suite、selectorを測定するprobe AEX |
| `tests/` | 契約、境界、回帰、fail-closedテスト |
| `analysis/` | AE oracle、runtime結果、互換性の証拠 |
| `docs/` | 設計、方針、互換表、安全境界 |

### 現在の制限

- 任意のAEXが動作する段階にはまだ到達していません。
- 未対応Suiteやeffect固有のhost前提により、loadまたはrenderが失敗する場合があります。
- 「workerが完走した」「画像が生成された」「AEとpixel一致した」は別の到達段階です。
- custom UI、GPU backend、深度、複数入力、sequence semanticsはAEXごとに対応状況が異なります。
- AEGPはEffectデバッグに必要な補助経路のみを優先しています。
- 隔離は偶発的なcrashや多くの不正動作の影響を減らしますが、信頼できないバイナリの安全を保証しません。

詳細は[互換性ステータス](docs/COMPATIBILITY_STATUS_2026-07-16.md)と[Windows Native Hardening Plan](docs/WINDOWS_NATIVE_HARDENING_PLAN_2026-07-16.md)を参照してください。

### 開発方針

互換機能は次の順序で追加します。

1. 実AEX、SDK sample、または自己作成probeで差を再現する。
2. fixture固有分岐ではなく、一般化した最小host機能を実装する。
3. focused testと境界検証を追加する。
4. 可能ならAfter Effects実機oracleとpixel出力を比較する。
5. 実装済み範囲と未検証範囲を同じcommitへ記録する。

詳細な目標、達成事項、評価基準、roadmapは[プロジェクト方針と現在地](docs/PROJECT_DIRECTION.md)に記載しています。

### Design Principles

- 実測可能な互換性を、fixture固有分岐より優先する。
- 入力、identity、resource use、出力をboundedかつ検証可能にする。
- crash containmentとsecurity sandboxを混同しない。
- SDK、oracle、private asset依存を通常のclean-clone検証から分離する。
- 実装済み範囲と未検証範囲を同時に記録する。

脆弱性の報告は[SECURITY.md](SECURITY.md)、開発・投稿ルールは[CONTRIBUTING.md](CONTRIBUTING.md)を参照してください。proprietary AEX、Adobe SDK、DLL、dump、private asset、秘密情報、個人パスをIssueやPRへ投稿しないでください。

### 参考プロジェクト

READMEの構成とAE SDK上の用語は、次の公開プロジェクトを参考にしています。

- [ISF4AE](https://github.com/baku89/ISF4AE): 対応環境、機能、制限、build手順を明確に分けたAE Effect plug-in
- [After Effects PopcornFX Plugin](https://github.com/PopcornFX/AfterEffectsPopcornFXPlugin): platform、setup、build、supportを入口で示すnative AE plug-in
- [GPU_Skeleton](https://github.com/timurco/GPU_Skeleton): GPU対応AE plug-in templateの機能・導入・参考資料構成
- [After Effects C++ Plugin SDK Guide](https://github.com/docsforadobe/after-effects-plugin-guide): Effect、AEGP、Suite、selectorの公開ドキュメント

### ライセンスと商標

`broker/`、`guest/`および一部Rust bridgeはCargo metadataでMITと宣言されています。一方、repository全体、`minihost/`、Python、文書、schema、instrumentには包括的なlicense grantがまだありません。第三者依存やAdobe SDK-backed componentを含む正確な公開条件は[Public Release Audit](docs/PUBLIC_RELEASE_AUDIT.md)を参照してください。Adobe SDKは再配布せず、各自の正規なローカルSDKを利用してください。

Adobe、After Effects、および関連する製品名は各権利者の商標です。AEXCompatはAdobeによる公式プロジェクトではありません。

---

## English

> [!NOTE]
> The Japanese section is authoritative. This English section is a collaboration-oriented translation.

### Overview

AEXCompat is a clean-room compatibility host for running Adobe After Effects Effect AEX plug-ins outside After Effects. It sends image or audio data to an AEX and supports output preview, file export, comparison with AE, and selector/suite diagnostics.

> **Public contribution safety:** do not post proprietary AEX plug-ins, Adobe
> SDK content, DLLs, dumps, private assets/corpora, secrets, or personal paths
> in issues or pull requests. See
> [the public release audit](docs/PUBLIC_RELEASE_AUDIT.md) for the publication
> boundary and unresolved licensing decisions.

The goal is practical, faithful compatibility with general Effect AEX plug-ins, not an emulator for one fixture. Recreating the complete After Effects application, AEP editor, or full AEGP host is not the current focus.

This project is experimental and pre-alpha. The optional Apple Silicon x86_64 native carrier is not a sandbox for untrusted code and must only be used with trusted plug-ins.

### Basic Workflow

1. Select an AEX.
2. A background worker runs `PF_PARAMS_SETUP` and fills the left-side **Effect Controls** automatically.
3. Select an input image and edit parameters.
4. Run **Quick render** or **Render and save PNG**.
5. Compare input and AEX output in the FHD viewer.

### Features

- Registered and previously unknown AEX execution with pre-launch SHA-256 revalidation
- Recursive discovery and hash pinning of adjacent DLLs referenced by normal and delay-load PE imports
- Persistent AE-inspired Effect Controls with typed parameter editing
- PNG, JPEG, BMP, TIFF, and WebP input; PNG output
- Classic Render and SmartFX
- ARGB8, ARGB16, and ARGB32F
- Multiple Layer inputs, timing/FPS, downsampling, pixel aspect, and sequence state
- Mono float32 audio and visual-audio sidecars
- Six-case 8/16/32 bpc compatibility matrix
- Pixel comparison against After Effects reference output
- Custom UI, PF Suite, and selector-lifecycle probes
- Restricted workers, timeouts, Job Objects, ACLs, sealed load trees, and pixel guards
- Separate reporting for crashes, hangs, selector errors, and host validation errors

### Supported Environment

| Component | Current target |
|---|---|
| Windows x64 | Primary: Rust broker / harness and C++ / MSVC x64 worker |
| macOS Apple Silicon | Experimental: arm64 Unicorn backend, optionally Rosetta x86_64 native carrier |
| Windows ARM64 | Unsupported |
| SDK baseline | Adobe After Effects SDK 2025 |
| UI / broker | Rust 2024 edition |
| native worker | C++ / MSVC x64 |
| image input | PNG, JPEG, BMP, TIFF, WebP |
| image output | PNG |

On Apple Silicon, the arm64 Unicorn worker executes Windows x64 AEX code as a guest. Setting `AEXCOMPAT_NATIVE_CARRIER=1` tries the Rosetta x86_64 native carrier where available. This is an explicit opt-in acceleration path, not a sandbox for untrusted AEX, and must only be used with trusted plug-ins. After Effects itself is not required for ordinary harness runs, but it is required to capture AE oracles and establish pixel equivalence.

### Quick Start

Requirements: Windows x64, Rust/Cargo, Visual Studio with the MSVC C++ toolchain (the required edition and toolset vary by component), CMake, and a local After Effects SDK when building probes or SDK fixtures (not needed for minihost worker builds). See [Build Requirements](docs/BUILD_REQUIREMENTS.md) for per-component details, SDK generation, and toolset requirements.

Before SDK-backed tests or builds, set the SDK root as a user environment variable and reopen PowerShell:

```powershell
[Environment]::SetEnvironmentVariable('AFTER_EFFECTS_SDK_ROOT', 'C:\path\to\AfterEffectsSDK', 'User')
```

```powershell
git clone https://github.com/onmokoworks/AEXCompat.git
cd AEXCompat\broker
cargo run -p aexcompat-harness
```

Release build:

```powershell
cd broker
cargo build -p aexcompat-harness --release
.\target\release\aexcompat-harness.exe
```

### Tests

```powershell
uv sync --locked
cargo test --manifest-path broker\Cargo.toml --workspace
uv run python -m pytest -q
```

`pytest` is the canonical Python test runner. `unittest discover` does not collect the repository's bare-function tests and must not be used as the complete verification command. Some native-fixture, GPU, and AE-oracle tests require a local SDK, a matching GPU runtime, or After Effects. Tests that require generated binaries or local approval receipts must be paired with their explicit build or gate step.

Tests that need local artifacts are split in two: `--run-built-artifact-tests` runs tests that execute workers / probes built from this checkout against self-computed expectations (CI runs these too), while `--run-local-artifact-tests` runs machine-bound tests that authenticate recorded evidence against local files (local-only). A normal clean clone skips both with an explicit reason.

A clean clone without an SDK or real AEX completes the standard `pytest` run with zero failures; dependency-backed tests skip with explicit reasons. Opt in to SDK, checkout-built artifact, or machine-bound evidence tests with `--run-sdk-tests`, `--run-built-artifact-tests`, or `--run-local-artifact-tests`, respectively. See [Build Requirements](docs/BUILD_REQUIREMENTS.md) for prerequisites and the CI matrix.

### DirectX SDK fixture

The authenticated DirectX device-world fixture is rebuilt from the local After Effects SDK with:

```powershell
.\tools\build-sdk-invert-directx.ps1
```

The recorded device selection, shader build, worker identity, and render result are documented in [`analysis/SDK_DIRECTX_DEVICE_WORLD_RESULT_2026-07-16.json`](analysis/SDK_DIRECTX_DEVICE_WORLD_RESULT_2026-07-16.json). This fixture is a bounded DirectX compatibility check, not a claim that every GPU AEX or device produces Adobe-identical pixels.

### Architecture

```text
Desktop Harness / CLI
        -> Rust Broker
        -> Restricted Worker Process
        -> Clean-room C++ Effect Host
        -> Selected AEX
        -> Validated image / audio / diagnostics
```

### Current Limitations

- Universal compatibility with arbitrary AEX binaries is not yet claimed.
- Missing suites and effect-specific host assumptions may stop loading or rendering.
- “Worker completed,” “image produced,” and “pixel-equivalent to AE” are separate maturity levels.
- Custom UI, GPU backends, pixel depths, multiple inputs, and sequence semantics vary by plug-in.
- AEGP work is limited to helper routes useful for Effect debugging.
- Isolation reduces the impact of accidental crashes and many forms of misbehavior; it is not a complete security sandbox.

See [Compatibility Status](docs/COMPATIBILITY_STATUS_2026-07-16.md), [Project Direction](docs/PROJECT_DIRECTION.md), and the [Windows Native Hardening Plan](docs/WINDOWS_NATIVE_HARDENING_PLAN_2026-07-16.md).

For reproducing After Effects-dependent oracle captures on another Windows machine, see the [AE Oracle Cross-Machine Runbook](docs/AE_ORACLE_CROSS_MACHINE_RUNBOOK_2026-07-18.md). It defines the required AE build, AEX SHA-256, plug-in load gate, CDB/native tools, renderer metadata, retained artifacts, and return statuses.

### Development Method

Compatibility work follows a repeatable sequence: reproduce behavior with a real AEX, SDK sample, or self-authored probe; implement a minimal general host capability; add focused boundary tests; compare with an AE oracle where possible; and document both verified and unverified behavior in the same change.

See [SECURITY.md](SECURITY.md) for vulnerability reporting and [CONTRIBUTING.md](CONTRIBUTING.md) for development and submission rules. Do not post proprietary AEX plug-ins, Adobe SDK content, DLLs, dumps, private assets, secrets, or personal paths in issues or pull requests.

### References

- [ISF4AE](https://github.com/baku89/ISF4AE)
- [After Effects PopcornFX Plugin](https://github.com/PopcornFX/AfterEffectsPopcornFXPlugin)
- [GPU_Skeleton](https://github.com/timurco/GPU_Skeleton)
- [After Effects C++ Plugin SDK Guide](https://github.com/docsforadobe/after-effects-plugin-guide)

### License and Trademarks

`broker/`, `guest/`, and selected Rust bridges declare MIT in Cargo metadata. The repository as a whole, including `minihost/`, Python, documentation, schemas, and instruments, does not yet have a comprehensive license grant. See the [Public Release Audit](docs/PUBLIC_RELEASE_AUDIT.md) for the unresolved component and third-party licensing boundary. Adobe SDK files are not redistributed; contributors must use their own legitimate local SDK installation.

Adobe, After Effects, and related product names are trademarks of their respective owners. AEXCompat is not an official Adobe project.

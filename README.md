# AEXCompat

**After Effects Effect AEXを、AEの外で読み込み・描画・デバッグする互換ホスト。**<br>
**A compatibility host for loading, rendering, and debugging After Effects Effect AEX plug-ins outside AE.**

[日本語](#日本語) | [English](#english) | [互換性ステータス](docs/COMPATIBILITY_STATUS_2026-07-16.md) | [プロジェクト方針](docs/PROJECT_DIRECTION.md)

> [!WARNING]
> 開発中の実験的ソフトウェアです。未知のAEXはネイティブコードとして実行されます。隔離機構はありますが、完全なsecurity sandboxではありません。

## 日本語

> [!IMPORTANT]
> 日本語をこのプロジェクトの正本とします。英語版と解釈が異なる場合は、日本語版を優先します。

### 概要

AEXCompatは、Adobe After EffectsのEffect AEXをAfter Effects本体の外で実行するためのクリーンルーム互換ホストです。画像や音声をAEXへ入力し、結果の表示・保存・AE実機との比較・selector/Suite診断を行えます。

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
| OS | Windows x64 |
| SDK | Adobe After Effects SDK 2025を基準に検証 |
| UI / broker | Rust 2021 edition |
| native worker | C++ / MSVC x64 |
| 入力画像 | PNG、JPEG、BMP、TIFF、WebP |
| 出力画像 | PNG |

macOS、Apple Silicon、Windows ARM64は現時点で対象外です。After Effects本体は通常のharness実行には不要ですが、AE oracleの取得とpixel一致検証には必要です。

### クイックスタート

#### 必要なもの

- Windows 10または11 x64
- Rust toolchainとCargo
- Visual Studio（MSVC C++ toolchain。必要なedition・toolsetはコンポーネントごとに異なる）
- CMake
- ローカルのAfter Effects SDK（probeやSDK fixtureをbuildする場合。minihost workerのbuildには不要）

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
# Python test dependency
python -m pip install -r requirements-dev.txt

# Rust broker / harness / isolation tests
cargo test --manifest-path broker\Cargo.toml --workspace

# Contract, ABI, oracle, and source-level regression tests
python -m pytest -q
```

`pytest` がPythonテストの正規ランナーです。`unittest discover` ではbare function形式のテストを収集できないため、完全な検証には使用しません。一部のnative fixture、GPU、After Effects oracleテストには、ローカルSDK、対応GPU runtime、またはAE本体が必要です。ビルド生成物やローカル承認receiptを必要とするテストは、それらを生成する明示的なgateまたはbuild手順と組み合わせて実行します。

ローカル成果物を生成した開発環境で、それらを認証・実行するテストも含める場合は `python -m pytest -q --run-local-artifact-tests` を使用します。通常のclean cloneでは該当テストを理由付きでskipし、ソースだけで再現可能なテストをすべて実行します。

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

### 参考プロジェクト

READMEの構成とAE SDK上の用語は、次の公開プロジェクトを参考にしています。

- [ISF4AE](https://github.com/baku89/ISF4AE): 対応環境、機能、制限、build手順を明確に分けたAE Effect plug-in
- [After Effects PopcornFX Plugin](https://github.com/PopcornFX/AfterEffectsPopcornFXPlugin): platform、setup、build、supportを入口で示すnative AE plug-in
- [GPU_Skeleton](https://github.com/timurco/GPU_Skeleton): GPU対応AE plug-in templateの機能・導入・参考資料構成
- [After Effects C++ Plugin SDK Guide](https://github.com/docsforadobe/after-effects-plugin-guide): Effect、AEGP、Suite、selectorの公開ドキュメント

### ライセンスと商標

Rust workspaceはMIT Licenseとして設定されています。Adobe SDK由来ファイルはリポジトリへ複製せず、各自のローカルSDKを利用してください。

Adobe、After Effects、および関連する製品名は各権利者の商標です。AEXCompatはAdobeによる公式プロジェクトではありません。

---

## English

> [!NOTE]
> The Japanese section is authoritative. This English section is a collaboration-oriented translation.

### Overview

AEXCompat is a clean-room compatibility host for running Adobe After Effects Effect AEX plug-ins outside After Effects. It sends image or audio data to an AEX and supports output preview, file export, comparison with AE, and selector/suite diagnostics.

The goal is practical, faithful compatibility with general Effect AEX plug-ins, not an emulator for one fixture. Recreating the complete After Effects application, AEP editor, or full AEGP host is not the current focus.

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
| OS | Windows x64 |
| SDK baseline | Adobe After Effects SDK 2025 |
| UI / broker | Rust 2021 edition |
| native worker | C++ / MSVC x64 |
| image input | PNG, JPEG, BMP, TIFF, WebP |
| image output | PNG |

macOS, Apple Silicon, and Windows ARM64 are not currently supported. After Effects itself is not required for ordinary harness runs, but it is required to capture AE oracles and establish pixel equivalence.

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
python -m pip install -r requirements-dev.txt
cargo test --manifest-path broker\Cargo.toml --workspace
python -m pytest -q
```

`pytest` is the canonical Python test runner. `unittest discover` does not collect the repository's bare-function tests and must not be used as the complete verification command. Some native-fixture, GPU, and AE-oracle tests require a local SDK, a matching GPU runtime, or After Effects. Tests that require generated binaries or local approval receipts must be paired with their explicit build or gate step.

After generating the local artifacts, run `python -m pytest -q --run-local-artifact-tests` to include tests that authenticate or execute them. A normal clean clone skips those tests with an explicit reason and runs every source-reproducible test.

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

### References

- [ISF4AE](https://github.com/baku89/ISF4AE)
- [After Effects PopcornFX Plugin](https://github.com/PopcornFX/AfterEffectsPopcornFXPlugin)
- [GPU_Skeleton](https://github.com/timurco/GPU_Skeleton)
- [After Effects C++ Plugin SDK Guide](https://github.com/docsforadobe/after-effects-plugin-guide)

### License and Trademarks

The Rust workspace is configured under the MIT License. Adobe SDK files are not redistributed; contributors must use their own local SDK installation.

Adobe, After Effects, and related product names are trademarks of their respective owners. AEXCompat is not an official Adobe project.

# AEXCompat

**After Effects用の`.aex`プラグインをAfter Effects本体の外で読み込み・描画・デバッグする互換ホスト。**

[English](README.en.md) · [クイックスタート](#クイックスタート) · [実AEX検証](#apple-siliconでの実aex検証) · [ドキュメント](#ドキュメント) · [ライセンス](#ライセンス)

<p align="center">
  <img src="docs/images/aexcompat-gui.png" alt="AEXCompat GUI：解析ログ、Effect Controls、入力・出力ビューアー" width="1200">
</p>

WindowsとApple Siliconで共通化を進めているRust/egui GUIです。AEXの選択、Effect Controls、入力・出力比較、診断ログを1つの画面で扱います。

## AEXCompatとは

AEXCompatは、Windows x64の`.aex`プラグインへ画像や音声を入力し、結果の表示・保存・診断を行うクリーンルーム互換ホストです。

- WindowsではRust製desktop harnessとC++/MSVC workerを使用
- Apple Silicon Macではarm64 Unicorn workerがWindows x64 AEXをguest実行
- Effect Controls、Classic Render、SmartFX、ARGB8/16/32Fに対応
- selector、Suite、parameter、crash、hangをstructured diagnosticとして記録
- After Effects実機の参照画像とpixel比較可能

After Effects全体、AEP編集環境、AEGP host全体の再実装は目的としていません。

| Platform | 実行経路 |
|---|---|
| Windows x64 | desktop harness + native C++/MSVC worker |
| Apple Silicon | arm64 Unicorn workerによるWindows x64 guest実行 |

## Apple Siliconでの実AEX検証

以下はApple Siliconのarm64 Unicorn workerだけを対象にした実測です（2026-08-20）。Windows native workerの集計ではありません。

| 結果 | 本数 |
|---|---:|
| 画像を生成（`rendered`） | **27** |
| 未対応importで停止 | 4 |
| worker終了 | 8 |

> [!NOTE]
> これはlocal inventory 969本全体の成功率ではありません。`rendered`もAfter Effectsとのpixel一致や、全parameter・GPU経路の対応を意味しません。

<details>
<summary>検証範囲と再現用identity</summary>

static PE解析でこの実行経路へ投入できると確認した39本が対象です。残る930本はこのrunに含めておらず、failedまたはskippedには分類していません。集計は `27 rendered / 4 unsupported_import / 8 worker_exit` です。

- Source baseline: `b99dc6bad11d8dd38fd4ed54f6ba020c431dc614`
- Worker SHA-256: `6581854c2d8eaa015f7c79146b424e29818c91ed4cc1db478124fb75f4b1213d`
- Private report SHA-256: `53221497a11227f2f5df7db6afdc245ccba08a91e3f437620107d0bd2f68253d`

commercial/private AEX本体とcorpusはrepositoryへ同梱しません。

</details>

## クイックスタート

### Apple Silicon Mac

Rust/Cargoだけでarm64 workerのRelease build、ad-hoc署名、DMG作成、mount後の検証まで実行できます。Windows、Rosetta、Adobe証明書は不要です。

```sh
git clone https://github.com/onmokoworks/AEXCompat.git
cd AEXCompat
tools/prepare-local-macos-aex-carriers.sh /tmp/aexcompat-local.dmg
```

手元のAEXとPNGを実際にrenderする場合:

```sh
AEXCOMPAT_SMOKE_AEX=/absolute/path/to/effect.aex \
AEXCOMPAT_SMOKE_INPUT_PNG=/absolute/path/to/input.png \
  tools/prepare-local-macos-aex-carriers.sh /tmp/aexcompat-local-smoke.dmg
```

AEXと入力画像はDMGへ収録されません。通常経路はarm64 Unicornのみです。Rosetta native carrierはtrusted AEX向けの任意経路で、`AEXCOMPAT_INCLUDE_NATIVE_CARRIER=1`を指定した場合だけ追加されます。

### Windows

必要なもの:

- Windows 10/11 x64
- Rust/Cargo
- Visual StudioまたはBuild Tools（MSVC C++ toolchain + Windows SDK）
- CMake

GUIを起動:

```powershell
git clone https://github.com/onmokoworks/AEXCompat.git
cd AEXCompat\broker
cargo run -p aexcompat-harness
```

AEXのinspect/renderには`minihost/` workerのbuildも必要です。Adobe SDKはprobeやSDK fixtureのbuild時だけ必要です。正確なコマンドとtoolsetは[Build Requirements](docs/BUILD_REQUIREMENTS.md)を参照してください。

## 使い方

1. AEXを選択する
2. 自動実行された`PF_PARAMS_SETUP`からEffect Controlsを生成する
3. 入力画像とparameterを指定する
4. **Render current frame**または**Render and save PNG**を実行する
5. viewerで入力とAEX出力を比較する

対応入力はPNG、JPEG、BMP、TIFF、WebP、出力はPNGです。複数Layer、time/FPS、downsample、pixel aspect、sequence state、mono float32 audioも扱えます。

## アーキテクチャ

```text
Desktop Harness / CLI
        ↓
Rust Broker（入力検証・staging・provenance）
        ↓
Isolated Worker Process
        ↓
Clean-room Effect Host（selectors・PF worlds・Suites）
        ↓
AEX → image / audio / diagnostics
```

| ディレクトリ | 役割 |
|---|---|
| `broker/` | Rust broker、desktop harness、隔離起動 |
| `minihost/` | AE Effect ABIとPF Suiteを提供するC++ worker |
| `guest/` | Apple Silicon上でWindows x64 AEXを動かすguest worker |
| `instruments/` | SDK ABI、Suite、selectorを測定するprobe AEX |
| `tests/` | 契約、境界、回帰、fail-closedテスト |
| `analysis/` | AE oracleとruntime evidence |

## テスト

```powershell
uv sync --locked
cargo test --manifest-path broker\Cargo.toml --workspace
uv run python -m pytest -q
```

SDK、build artifact、machine-bound evidenceを使う検証は明示opt-inです。公開repositoryとforkのCIはsource-onlyで実行し、Adobe SDKは取得・再配布しません。全matrixと環境要件は[Build Requirements](docs/BUILD_REQUIREMENTS.md)にあります。

## 現在の制限

- 未対応Suiteやeffect固有のhost前提で停止する場合があります
- worker完走、画像生成、AEとのpixel一致はそれぞれ別の到達段階です
- custom UI、GPU backend、pixel depth、複数入力、sequence semanticsはAEXごとに異なります

## ドキュメント

| 内容 | 文書 |
|---|---|
| Build、SDK、CI要件 | [Build Requirements](docs/BUILD_REQUIREMENTS.md) |
| 互換性の現在地 | [Compatibility Status](docs/COMPATIBILITY_STATUS_2026-07-16.md) |
| 方向性とroadmap | [Project Direction](docs/PROJECT_DIRECTION.md) |
| AEX移植・解析 | [AEX Porting Dossier](docs/aex-porting-dossier.md) |
| Windows native hardening | [Windows Native Hardening Plan](docs/WINDOWS_NATIVE_HARDENING_PLAN_2026-07-16.md) |
| AE oracleの別machine取得 | [AE Oracle Cross-Machine Runbook](docs/AE_ORACLE_CROSS_MACHINE_RUNBOOK_2026-07-18.md) |
| 公開・第三者material境界 | [Public Release Audit](docs/PUBLIC_RELEASE_AUDIT.md) |
| 脆弱性報告 | [Security Policy](SECURITY.md) |
| 開発・投稿ルール | [Contributing](CONTRIBUTING.md) |

## Contributing

互換機能は、実際のAEX、SDKのサンプル、自作の検証用AEXで動作の違いを再現してから追加します。特定のAEXだけに通用する処理ではなく、一般化した最小限のホスト機能として実装し、対象を絞ったテストと境界値の検証を行います。可能であればAfter Effects実機の結果とも比較します。

非公開・市販のAEX、Adobe SDK、DLL、メモリダンプ、非公開の素材や検証データ、秘密情報、個人のファイルパスをIssueやPRへ投稿しないでください。

## ライセンス

AEXCompatの開発者が権利を持つソースコード、文書、仕様、テスト、検証用ツールは、下記の除外対象を除き[Mozilla Public License 2.0](LICENSE)で提供します。Adobe、After Effectsおよび関連製品名は各権利者の商標であり、本プロジェクトはAdobe公式ではありません。

デスクトップUIの「ライセンス」から、AEXCompat自身のライセンスと、Windows/macOS版UIに組み込まれるCargo依存関係のライセンス告知をオフラインで確認できます。配布物には [`LICENSE`](LICENSE)、[`THIRD_PARTY_LICENSES.txt`](THIRD_PARTY_LICENSES.txt)、[`THIRD_PARTY_LICENSES.html`](THIRD_PARTY_LICENSES.html) を同梱してください。依存関係を更新した場合は `python tools/generate-third-party-licenses.py --refresh` で告知を更新し、通常実行（`--refresh`なし）でlocked graphとの一致を検証します。

<details>
<summary>第三者の成果物とUnicorn Engineを含む実行ファイルについて</summary>

次のものにはAEXCompatのMPL-2.0ライセンスは適用されず、それぞれ元のライセンスや利用条件が引き続き適用されます。

- 外部ライブラリなど、第三者が権利を持つもの
- `imports/aviutlas-rust-contracts/`と、[来歴台帳](contracts/PROVENANCE.md)で外部から取り込んだものとされているファイル
- Adobe SDKと、それを使って生成した成果物
- 別のライセンスが明記されたテスト用ファイル
- 非公開または市販のAEXと、その検証データ

`guest/`のうちAEXCompatが独自に作成したソースコードはMPL-2.0です。一方、通常のguest実行ファイルにはGPLv2のUnicorn Engineが組み込まれます。この実行ファイルを配布する場合は、MPL 2.0第3.3節に従い、対象となるAEXCompatのソースコードをMPL-2.0とGPL-2.0の両方の条件で提供し、実行ファイルについてもGPLv2の配布条件を満たす必要があります。詳しくは[公開時の確認事項](docs/PUBLIC_RELEASE_AUDIT.md)を参照してください。

</details>

# AEXCompat プロジェクト方針と現在地 / Project Direction and Status

最終更新: 2026-07-17  
Last updated: 2026-07-17

> [!IMPORTANT]
> この文書では日本語を正本とします。英語は共同作業のための参考訳であり、解釈が異なる場合は日本語版を優先します。
>
> The Japanese text is authoritative. The English text is a convenience translation for collaboration; if the two differ, the Japanese text prevails.

## 1. 目的 / Mission

AEXCompat は、Adobe After Effects の Effect AEX を After Effects 本体の外で読み込み、画像・音声を入出力し、挙動を観察・比較・デバッグできるクリーンルーム互換ホストを作るプロジェクトです。最終目標は、特定の一つのエフェクトではなく、一般の Effect AEX に対する実用的かつ忠実な互換性です。

AEXCompat is a clean-room compatibility host for loading Adobe After Effects Effect AEX plug-ins outside After Effects, processing image and audio data, and observing, comparing, and debugging their behavior. The long-term goal is practical, faithful compatibility with general Effect AEX plug-ins rather than support for one specific effect.

これは After Effects の代替アプリケーション全体、AEP プロジェクト編集環境、または AEGP ホスト全体を直ちに再実装する計画ではありません。現在の主対象はエフェクト開発・検証に必要な Effect API、レンダー経路、パラメーター、関連 Suite です。

This is not an immediate attempt to recreate the complete After Effects application, its AEP project editor, or the entire AEGP host surface. The present focus is the Effect API, render paths, parameters, and related suites needed for effect development and verification.

## 2. 設計原則 / Design Principles

1. **観測可能な互換性を優先する。** SDK の型と契約を起点にし、After Effects 実機のオラクル、自己作成fixture、SDKサンプル、再現可能なプローブで挙動を確定します。未観測の内部実装は推測しません。
2. **「実装済み」と「AE同等」を区別する。** ローカルテスト通過、実AEX通過、AE実機とのピクセル一致は別の到達段階として記録します。
3. **未知のAEXを実行できる開発体験を作る。** 事前登録を必須にせず、選択したバイナリを都度ハッシュ化し、時間制限付きの隔離ワーカーで実行します。
4. **失敗をホストから隔離する。** AEXのクラッシュ、ハング、不正な出力、境界外アクセスをワーカープロセス内に閉じ込め、入力・出力・診断情報を検証します。
5. **互換性差を隠さない。** フォールバックやfixture固有処理を暗黙に成功扱いせず、経路、警告、未対応Suite、AEとの差をレポートします。
6. **証拠をコードと同じ成果物として残す。** プローブ、オラクル、JSON結果、focused test、互換表をリポジトリで追跡します。

1. **Prioritize observable compatibility.** Start from SDK types and contracts, then establish behavior with After Effects oracles, self-authored fixtures, SDK samples, and reproducible probes. Do not guess unobserved internals.
2. **Separate “implemented” from “equivalent to AE.”** Local test coverage, successful real-AEX execution, and pixel equivalence with AE are recorded as distinct maturity levels.
3. **Make unknown AEX execution useful for development.** Pre-registration is not required; the selected binary is hashed for each session and executed in a timeout-limited isolated worker.
4. **Contain failures outside the host UI.** Plug-in crashes, hangs, malformed output, and boundary violations remain inside worker processes, while transport and diagnostics are validated.
5. **Do not hide compatibility gaps.** Fallbacks and fixture-specific behavior must not silently count as success; reports expose execution routes, warnings, missing suites, and differences from AE.
6. **Treat evidence as a first-class artifact.** Probes, oracles, JSON results, focused tests, and compatibility tables are versioned with the implementation.

## 3. 現在のアーキテクチャ / Current Architecture

```text
Rust desktop harness / CLI
        |
        v
Rust broker: request validation, identity checks, sealed transport
        |
        v
Restricted worker process
        |
        v
Clean-room C++ Effect host: selectors, PF worlds, parameters, suites
        |
        v
Selected AEX -> validated image/audio/report output
```

- `broker/crates/harness`: AEX、入力画像、パラメーター、レンダー経路を選択するデスクトップUIとCLI。
- `broker/crates/broker`: 入力検証、AEX/workerの同一性確認、依存DLL manifest、隔離起動、出力検証。
- `minihost`: Effect Main を呼び出し、AE Effect API ABIとSuiteを提供するC++ホスト。
- `instruments`: SDK ABI、Suite、selector lifecycleを測定する自己作成AEXとネイティブプローブ。
- `tests` と `analysis`: 契約テスト、実行証拠、AEオラクル結果、既知の境界。

- `broker/crates/harness`: Desktop UI and CLI for selecting an AEX, input images, parameters, and render paths.
- `broker/crates/broker`: Input validation, AEX/worker identity verification, dependency manifests, isolated launch, and output validation.
- `minihost`: C++ host that invokes Effect Main and supplies the AE Effect API ABI and suites.
- `instruments`: Self-authored AEX and native probes for measuring SDK ABI, suites, and selector lifecycles.
- `tests` and `analysis`: Contract tests, runtime evidence, AE oracle results, and known boundaries.

## 4. 達成済み / What Has Been Achieved

### 4.1 実用ハーネス / Usable Harness

- 未登録AEXを選択し、隔離ワーカーでネイティブ実行できます。
- PNG、JPEG、BMP、TIFF、WebP等の入力画像を読み込み、AEXへ渡し、RGBA画像として保存・プレビューできます。
- Classic と SmartFX、8/16/32 bpc の互換マトリクスを個別ワーカーで実行できます。
- FHD基準の可変サイズビューアーで入力、AEX出力、左右比較を確認できます。
- パラメーター検査、型付き代入、複数Layer入力、時間・FPS、downsample、pixel aspect、音声sidecar、custom UIや診断プローブを扱えます。
- ワーカー成功後のホスト検証失敗と、ワーカー自体のクラッシュをUI上で区別します。

- An unregistered AEX can be selected and executed natively in an isolated worker.
- PNG, JPEG, BMP, TIFF, WebP, and similar inputs can be passed to an AEX, then saved and previewed as RGBA output.
- Classic and SmartFX compatibility matrices at 8/16/32 bpc run in separate workers.
- A resizable FHD-oriented viewer presents input, AEX output, or side-by-side comparison.
- The harness supports parameter inspection, typed assignments, multiple Layer inputs, timing/FPS, downsampling, pixel aspect, audio sidecars, custom UI, and diagnostic probes.
- The UI distinguishes host validation failures after worker success from actual worker crashes.

### 4.2 Effectホスト互換層 / Effect Host Compatibility Layer

- `GLOBAL_SETUP`、`PARAMS_SETUP`、`SEQUENCE_SETUP`、`FRAME_SETUP`、Classic `RENDER`、SmartFX、各setdownを含む主要lifecycleを実装しています。
- ARGB8、ARGB16、ARGB32F world、rowbytes、origin、extent、複数入力world、時間付きLayer checkoutを扱います。
- SDKで公開された多数のPF Suiteとlegacy callbackを、境界検証・所有権管理・focused test付きで実装しています。
- sequence data、dynamic flags、supervised parameter、custom UI event、audio checkout、GPU経路とCPU再試行などを段階的に実装しています。
- AEGPはEffectデバッグに必要な限定的な補助経路を実装していますが、プロジェクトの主軸ではありません。

- Major lifecycle stages are implemented, including `GLOBAL_SETUP`, `PARAMS_SETUP`, `SEQUENCE_SETUP`, `FRAME_SETUP`, Classic `RENDER`, SmartFX, and corresponding setdown selectors.
- The host supports ARGB8, ARGB16, and ARGB32F worlds, rowbytes, origins, extents, multiple input worlds, and timed Layer checkout.
- Many SDK-published PF suites and legacy callbacks are implemented with boundary checks, ownership tracking, and focused tests.
- Sequence data, dynamic flags, supervised parameters, custom UI events, audio checkout, GPU routes, and fresh-worker CPU retry are being implemented incrementally.
- Limited AEGP helper paths exist where useful for Effect debugging, but AEGP hosting is not the primary track.

### 4.3 隔離と検証 / Isolation and Validation

- AEX本体、隣接依存DLL、workerをSHA-256とmanifestで固定し、起動直前にも同一性を再検証します。
- broker所有のsealed load treeとtransportを使い、restricted token、ACL、job object、timeoutを適用します。
- pixel guard、サイズ・stride・形式、module audit、selector timeline、suite lease、exit classificationを検証します。
- 未知AEXの互換性メタデータ差は警告として保持しつつ、クラッシュやメモリ境界違反は成功扱いしません。

- The AEX binary, adjacent dependency DLLs, and workers are pinned by SHA-256 and manifests, with identity revalidated immediately before launch.
- Broker-owned sealed load trees and transport use restricted tokens, ACLs, job objects, and timeouts.
- Pixel guards, dimensions, stride, format, module audits, selector timelines, suite leases, and exit classifications are validated.
- Compatibility metadata differences from unknown AEX files are retained as warnings, while crashes and memory-boundary violations never count as success.

## 5. 現在の到達度の読み方 / How to Read Current Status

| 段階 / Level | 意味 / Meaning |
|---|---|
| Source-wired | SDK ABIのslotと型が実装されているが、実行確認は未完了。 / SDK ABI slots and types are wired, but runtime verification is incomplete. |
| Locally verified | focused testまたはnative self-testが通過。 / Focused tests or native self-tests pass. |
| Real AEX verified | 実際のAEXが隔離workerで対象経路を完走。 / A real AEX completes the route in an isolated worker. |
| AE-oracle verified | After Effects実機の観測結果と契約またはpixel出力を比較済み。 / Contracts or pixels have been compared with an After Effects oracle. |
| General compatibility | 複数の独立AEXで再現し、fixture固有分岐に依存しない。 / Reproduced across independent AEX files without fixture-specific branching. |

詳細なSuite単位の状態は [COMPATIBILITY_STATUS_2026-07-16.md](COMPATIBILITY_STATUS_2026-07-16.md) を参照してください。「完全移植」または「任意のAEXが動作する」とはまだ宣言しません。

See [COMPATIBILITY_STATUS_2026-07-16.md](COMPATIBILITY_STATUS_2026-07-16.md) for suite-level details. The project does not yet claim a complete reimplementation or universal compatibility with arbitrary AEX binaries.

## 6. 今後の実装方針 / Roadmap

### 優先度1: 一般Effect AEXの画像入出力 / Priority 1: General Effect AEX Image I/O

- 未知AEXで不足したSuite、callback、selector contractを診断から自動的に特定し、fixture固有処理ではなく一般ホスト機能として追加します。
- Classic、SmartFX、8/16/32 bpc、複数Layer、時間依存、sequence stateを独立AEX群で横断検証します。
- 出力の「処理完了」と「AE pixel一致」を別々に可視化します。

- Identify missing suites, callbacks, and selector contracts from unknown-AEX diagnostics, then add them as general host capabilities rather than fixture-specific behavior.
- Cross-check Classic, SmartFX, 8/16/32 bpc, multiple Layers, time dependence, and sequence state across independent AEX fixtures.
- Report “render completed” separately from “pixel-equivalent to AE.”

### 優先度2: AE実機オラクル / Priority 2: After Effects Oracles

- SDKサンプルと自己作成probeをAE 2025で実行し、selector順序、Suite意味論、pixel rounding、ownershipを記録します。
- AE出力とAEXCompat出力を同一入力・同一パラメーター・同一時刻で比較し、許容差を明示します。
- 推測実装はオラクルが得られ次第置換し、根拠JSONとテストを同じcommitに残します。

- Run SDK samples and self-authored probes in AE 2025 to record selector order, suite semantics, pixel rounding, and ownership behavior.
- Compare AE and AEXCompat using identical inputs, parameters, and times, with explicit tolerances.
- Replace inferred behavior when oracle evidence becomes available, committing evidence JSON and tests together.

### 優先度3: 開発者体験 / Priority 3: Developer Experience

- 通常操作を「AEX選択 → 画像選択 → パラメーター確認 → レンダー」の短い導線に保ちます。
- 高度なprobeや診断は折りたたみ、失敗理由をselector、Suite、worker、host validationの階層で表示します。
- 再現可能なrequest JSON、診断bundle、比較画像を一操作で保存できるようにします。

- Keep the primary flow short: select AEX, select image, inspect parameters, render.
- Keep advanced probes collapsed and classify failures by selector, suite, worker, and host validation.
- Make reproducible request JSON, diagnostic bundles, and comparison images exportable in one operation.

## 7. 安全性と信頼境界 / Safety and Trust Boundary

未知AEXのネイティブコードは実行されます。現在の隔離は、開発中のAEXによる偶発的クラッシュや多くの不正動作の影響を減らすための多層防御ですが、完全なセキュリティsandboxではありません。信頼できない第三者バイナリを安全に解析できるという保証はありません。詳細は [WINDOWS_NATIVE_HARDENING_PLAN_2026-07-16.md](WINDOWS_NATIVE_HARDENING_PLAN_2026-07-16.md) を参照してください。

Unknown AEX files execute native code. The current isolation is defense in depth intended to reduce the impact of accidental crashes and many forms of misbehavior during plug-in development; it is not a complete security sandbox. It does not guarantee safe analysis of untrusted third-party binaries. See [WINDOWS_NATIVE_HARDENING_PLAN_2026-07-16.md](WINDOWS_NATIVE_HARDENING_PLAN_2026-07-16.md).

## 8. 共同開発の進め方 / Collaboration Workflow

- 互換機能の追加は、再現fixtureまたはprobe、最小の一般実装、focused test、可能ならAEオラクルの順で進めます。
- 成功条件と未検証条件をPRまたはcommitに明記します。
- Adobe SDK由来コードを複製せず、利用条件に従ってローカルSDKの公開headerとサンプルを参照します。
- 互換性を広げる変更では、既存AEXの回帰と隔離境界を同時に確認します。

- Add compatibility features through a reproducing fixture or probe, a minimal general implementation, focused tests, and an AE oracle where possible.
- State both success criteria and unverified conditions in each PR or commit.
- Do not copy Adobe SDK source into this repository; consult local public headers and samples under their applicable terms.
- Changes that broaden compatibility must also check existing-AEX regressions and isolation boundaries.

## 9. 参考にした公開プロジェクト / Public References

- [After Effects C++ Plugin SDK Guide](https://github.com/docsforadobe/after-effects-plugin-guide): Effect、AEGP、Suite、selectorの公開用語と責務の整理に使用。
- [Adobe CEP Samples](https://github.com/Adobe-CEP/Samples): 対象host、前提条件、sampleごとの対応範囲を明示するREADME構成を参考にしたもの。AEXCompatはCEP実装ではありません。
- [OpenFX](https://github.com/AcademySoftwareFoundation/openfx): host、plug-in、support library、examples、documentation、security/release文書を分離する構成を参考にしたもの。AEXCompatはOFX hostではありません。
- [Natron](https://github.com/NatronGitHub/Natron) と [openfx-misc](https://github.com/NatronGitHub/openfx-misc): compositing hostとplug-in間の機能差を明記し、hostごとのcaveatを隠さない姿勢を参考にしたもの。

- [After Effects C++ Plugin SDK Guide](https://github.com/docsforadobe/after-effects-plugin-guide): Public terminology and responsibilities for Effects, AEGP, suites, and selectors.
- [Adobe CEP Samples](https://github.com/Adobe-CEP/Samples): README conventions that make target hosts, prerequisites, and per-sample scope explicit. AEXCompat is not a CEP implementation.
- [OpenFX](https://github.com/AcademySoftwareFoundation/openfx): Separation of host, plug-in, support library, examples, documentation, security, and release material. AEXCompat is not an OFX host.
- [Natron](https://github.com/NatronGitHub/Natron) and [openfx-misc](https://github.com/NatronGitHub/openfx-misc): Explicit host-specific caveats and transparent compatibility gaps between compositing hosts and plug-ins.

Adobe、After Effects、および関連する製品名は各権利者の商標です。AEXCompatはAdobeによる公式プロジェクトではありません。

Adobe, After Effects, and related product names are trademarks of their respective owners. AEXCompat is not an official Adobe project.

# macOS / Apple Silicon 移植境界

Issue #47 の調査結果。これは macOS 対応を完了したという記録ではなく、現行の
Windows x64 実装を壊さずに、どこまでを共通化し、どこからを macOS 専用実装に
分けるかを固定するための境界文書である。

## 現行の正本

- 対応対象は Windows x64。README の macOS / Apple Silicon / Windows ARM64 未対応という
  宣言を維持する。
- `broker` の workspace は Rust 共通コードを持つが、`windows-sys` の Job Object、
  restricted token、Win32 process creation、ACL、PE/module audit を実行境界に使う。
- `broker/crates/broker/src/lib.rs` は Windows 専用の launch/render 系を `cfg(windows)` で
  分離している。`harness` と一部の policy は非Windowsで静的な検証を持つが、これは
  macOSでAEXをロードできることを意味しない。
- `minihost` は CMake で Win32/MSVC と After Effects の Windows SDK/PE module を前提に
  する。単一の worker 実行ファイル `aex_worker.exe` (discovery/classic/smart を
  `--kind` で切替) を macOS arm64 へクロスビルドすることは、単なる
  Rust target追加では代替できない。
- 現行の AEX は Windows の PE DLL と After Effects Effect SDK ABI を前提にするため、
  macOSで同じバイナリをロードする設計は対象外。macOS版AE plugin形式とSDK ABIの
  実物確認なしに、Windows AEXの移植成功を表示してはならない。

## 移植境界

| 層 | 共通化するもの | Windows専用のまま残すもの | macOS側で新規に調査・実装するもの |
| --- | --- | --- | --- |
| Rust model | request、receipt、manifest、hash、bounded report、exit分類のschema | なし（path/identityの表現はOS別に抽象化） | `aarch64-apple-darwin` の依存・filesystem identity・hash実測 |
| broker launch | timeout、stdout/stderr上限、cleanup、fail-closed分類の抽象契約 | Job Object、restricted token、ACL、HANDLE_LIST、CreateProcessAsUserW | `posix_spawn`/`fork+exec`、process group、sandbox/entitlement方針、signal/timeout cleanup |
| artifact loading | canonical identity、size、SHA-256、sealed treeのmanifest | PE import、WinSxS、Authenticode/catalog、Win32 reparse | Mach-O bundle/framework/dylibの依存閉包、署名/notarization、symlink/path境界 |
| minihost | selector report、JSON schema、bounded ABI snapshot | MSVC、Win32、PE、After Effects Windows SDK | macOS AE SDKのentry ABIとloaderを確認してから別targetを設計 |
| UI / bridge | broker APIとの入力・出力契約 | AviUtl2とWindows host固有のDLL ABI | macOS hostは別consumerとして、AEXを直接再利用せず接続可能性を判定 |
| oracle | fixture manifest、identity binding、raw/export比較、exact/tolerance規則 | aerender/AfterFX.comのWindows運用 | macOS版AEのcapture手段、build/version/renderer identity、差分の比較可能性 |

「共通化するもの」はデータ契約と診断意味論に限定する。OSのprocess/security/load
機構を一つの実装へ押し込んで、Windowsのfail-closed境界を弱めない。

## 最初に到達するマイルストーン

### M0: cross-target の静的境界確認

- `cargo check -p aexcompat-broker --target aarch64-apple-darwin` を実行できる最小の
  依存・`cfg` 分割を作る。
- Windows専用の `image_render`、`secure_launch`、`render_session`、native workerを
  macOSで「使える」とするshimは作らない。
- schema/manifest/hash/diffの純粋RustテストだけをmacOS target候補にする。

### M1: macOSのcrash-containment probe

- AEXをまだロードせず、ダミーworkerで process group、timeout、stdout/stderr上限、
  child cleanup、exit分類を実証する。
- Job Object相当の実装を仮定せず、`launch_backend` のOS別実装として隔離する。
- sandboxは別のsecurity review対象とし、「別プロセス」をconfidentiality sandboxと
  表示しない。

### M2: macOS plugin形式の実物調査

- 対応するAfter Effects版、SDK、plugin bundle形式、entry ABI、依存dylib、署名要件を
  固定する。
- 実物identityが得られるまで、Windows `.aex` のコピーや拡張子変更をmacOS対応と
  扱わない。

### M3: 最小fixtureのinspect

- M2で確認したmacOS pluginを1つだけ対象に、load前のidentity/hash、selector lifecycle、
  failure classificationをcaptureする。
- render/pixel equivalence、SmartFX、AEGP、GPUはM3の成功から自動的に含めない。

## 子Issue候補

1. **macOS targetでschema/manifest/hash純粋層をcheckする**
   - Windows APIを参照しないcrate/moduleを明示し、target checkとJSON契約を固定する。
2. **macOS process containment probeを追加する**
   - dummy workerだけで timeout、group cleanup、bounded output、crash分類を検証する。
3. **macOS plugin bundle / Mach-O identity inventoryを作る**
   - PE/AEXの実装を流用せず、依存dylib、署名、canonical path、file identityを調べる。
4. **macOS版AE SDK entrypoint probeを設計する**
   - SDKと実機確認後に、Effect/AEGP/SmartFXのどこを最初に測るかを決める。
5. **macOS AE oracle captureの最小fixtureを作る**
   - M3のinspect成功後にのみ、入力・renderer・plugin identityを固定して比較する。

各子Issueは一つのOS境界と一つのfixtureに限定する。macOS対応Epicに戻す場合も、
Windows x64の既存テストを同じCI jobへ混ぜず、target/fixture別のgateとして追加する。

## Windows x64を守る検証方針

- 変更前後で `cargo test --manifest-path broker/Cargo.toml --workspace` と、既存の
  minihost Release build/self-testを維持する。
- `cfg(windows)` の変更は、Windowsのsecure launch、sealed load tree、module audit、
  cleanupを省略しないfocused testを持つ。
- macOS側は「buildできる」「dummy workerが隔離できる」「pluginをinspectできる」
  「AEとpixel一致する」を別状態で記録し、前段の成功を後段の成功へ丸めない。
- GitHub Actionsの課金・runner状態はコード結果とは分離する。macOS runnerが無い場合も、
  未検証範囲と再現手順をevidenceへ残す。

## 現時点の結論

最初に実装すべきなのは、macOSでWindows AEXを動かすことではなく、純粋なreport/manifest
層とdummy process containmentのOS境界を分離することである。macOS版AE plugin形式・
SDK ABI・署名/依存関係が実測できるまで、render、SmartFX、AEGP、GPUの対応可否は未確定
として扱う。

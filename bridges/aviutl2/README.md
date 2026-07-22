# aexcompat-aviutl2-bridge

AviUtl2 のフィルタ効果 (`.auf2`) から AEXCompat の常駐 `RenderSession` を駆動し、
After Effects の AEX プラグインを AviUtl2 上で動かすブリッジ (issue #269)。

AEX 本体は `RenderSession::open` が起動する worker サブプロセスで実行される。
プラグインが AviUtl2 に in-process ロードされても、crash containment
(プロセス分離 + Job Object + per-frame watchdog) は保たれる。

現状: 画像レンダリング (8bit RGBA、SmartFX 含む) + パラメーターマッピング
(float/checkbox/color/popup dropdown) を実装。env AEX を既定に、設定項目
"AEX" (File) で**実行中に別 AEX へ差し替え可能** (段階4)。差し替えた AEX は
パラメーター非公開で自前の既定値描画 (AviUtl2 config は静的)。
設計・経緯は [`docs/AVIUTL2_BRIDGE_2026-07-21.md`](../../docs/AVIUTL2_BRIDGE_2026-07-21.md)。

## 前提

- Windows x64、Rust/Cargo、MSVC C++ toolchain、CMake
- After Effects SDK (フィクスチャ AEX をビルドする場合のみ。`AFTER_EFFECTS_SDK_ROOT`)
- AviUtl2 (`C:\Program Files\AviUtl2`)。プラグイン/データは `C:\ProgramData\aviutl2\`

## 1. worker をビルドする

ブリッジは 2 種類の worker を使う: **render** (`aex_render_worker.exe`、フレーム
描画) と **L2** (`aex_l2_worker.exe`、パラメーター discovery = `inspect`)。
`RenderSession` / `inspect` は `<repository>/target/minihost-build/` **直下**
(`Release\` サブディレクトリではない) の exe をハッシュして admission する。

**4 種すべてを同一ソースから一括ビルドして直下に置くこと。** 片方だけ再ビルド
すると共有 object (`aex_worker_runtime_core`) が再コンパイルされ、他の worker が
ステール化して render が `worker_exited` で落ちる (2026-07-21 に踏んだ)。

canonical (Ninja、直下に出力):

```powershell
cmake -S minihost -B target\minihost-build -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build target\minihost-build
Get-ChildItem target\minihost-build\aex_*.exe
# aex_l1_worker / aex_l2_worker / aex_render_worker / aex_smart_worker
```

VS generator で出す場合は `Release\` に出るので直下へコピーする:

```powershell
$g = & .\tools\resolve-cmake-generator.ps1 ""
$cmake = & .\tools\resolve-build-cmake.ps1 "" $g
& $cmake -S minihost -B target\minihost-build -G $g -A x64
& $cmake --build target\minihost-build --config Release
Copy-Item target\minihost-build\Release\aex_*.exe target\minihost-build\
```

詳細は [`docs/BUILD_REQUIREMENTS.md`](../../docs/BUILD_REQUIREMENTS.md)。

## 2. ブリッジ (`.auf2`) をビルドする

```powershell
# release (実用。最適化 + tracing は INFO)
cargo build --release --manifest-path bridges\aviutl2\Cargo.toml
# debug (開発。tracing は DEBUG で verbose、低速・大サイズ)
cargo build --manifest-path bridges\aviutl2\Cargo.toml
```

cdylib (`aexcompat_aviutl2_bridge.dll`) を `.auf2` にリネームして AviUtl2 の
Plugin フォルダへ:

```powershell
Copy-Item bridges\aviutl2\target\release\aexcompat_aviutl2_bridge.dll `
          C:\ProgramData\aviutl2\Plugin\aexcompat.auf2
```

AviUtl2 起動中は `.auf2` がロックされ上書きできない。閉じてから配置する。

## 3. 対象 AEX と repository を設定する

固定 AEX の絶対パスと、ビルド済み worker のある repo root を環境変数で渡す
(User scope に設定し、AviUtl2 は新規プロセスから起動して継承させる):

```powershell
[Environment]::SetEnvironmentVariable('AEXCOMPAT_AVIUTL2_PLUGIN',     'C:\path\to\effect.aex', 'User')
[Environment]::SetEnvironmentVariable('AEXCOMPAT_AVIUTL2_REPOSITORY', 'C:\path\to\AEXCompat',   'User')
```

未設定/discovery 失敗時はパラメーターなしで launch 既定値レンダリングに縮退する
(`C:\ProgramData\aviutl2\Log` に warn)。

## 4. AviUtl2 で使う

1. AviUtl2 を (env 設定後に) 起動
2. レイヤーに図形/画像などメディアオブジェクトを置く
3. その直後を右クリック →「フィルタ効果を追加」→「AEXCompat (AEX bridge)」
4. AEX がパラメーターを持てば設定項目に出る (float→スライダー等)。値を変えると
   per-frame でセッションに反映される
5. 設定項目 "AEX" (File) で別の `.aex` を選ぶと、AviUtl2 を再起動せず実行中に
   その AEX へ切替わる。空に戻すと env AEX に戻る。差し替えた AEX は自前の既定値で
   描画され、パラメーターコントロールは公開されない (AviUtl2 の config は静的なため、
   コントロールはロード時の env AEX のものに固定される)

## テスト (AviUtl2 なし)

discovery + render 経路を standalone で再現する例 (broker を直に叩く):

```powershell
$env:AEXCOMPAT_AVIUTL2_PLUGIN     = 'C:\path\to\effect.aex'
$env:AEXCOMPAT_AVIUTL2_REPOSITORY = 'C:\path\to\AEXCompat'
cargo run --example repro_render --manifest-path bridges\aviutl2\Cargo.toml
```

discovered パラメーター一覧と、フレームごとの出力ピクセルを表示する。

## フィクスチャ AEX (テスト用)

`instruments/pf-*` の probe をビルドできる (要 AE SDK)。段階2 の検証に使ったもの:

- `pf_parameter_echo_probe` — float slider 1 つ。値を全ピクセルの赤に書く
  (`tools\build-pf-parameter-echo-probe.ps1`)
- `pf_sampling_probe` — パラメーターなし。列ごとにサンプル色 (段階1 の縦縞)

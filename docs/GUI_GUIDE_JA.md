# GUI 操作ガイド

[English translation](GUI_GUIDE.md) · [README](../README.md)

Windows x64 の AEX を読み込み、画像に適用して、パラメーターや出力を調べます。After Effects 本体や AEP 編集には対応しません。画像が出ても、After Effects とのピクセル一致を確認したことにはなりません。

このガイドは `olm/X01-platform-parity`（`13bda6be`）時点の内容です。英語版は、この日本語版を訳したものです。ボタン名は英語表示に合わせています。Windows の日本語表示では対応する項目を選んでください。

## 起動する

ソースから起動する場合は、AEXCompat のリポジトリ直下で実行します。ビルド環境の詳細は [Build Requirements](BUILD_REQUIREMENTS.md) を参照してください。

### Windows x64

Rust/Cargo、MSVC C++ ツールチェーン、Windows SDK、CMake を用意します。PowerShell で worker をビルドしてから GUI を起動します。

```powershell
pwsh -File tools/build-native.ps1
cargo run --manifest-path broker/Cargo.toml -p aexcompat-harness --jobs 1
```

Adobe SDK はSDK の probe やサンプルプラグインをビルドする場合に必要です。

### Apple Silicon Mac

Rust/Cargo、Xcode Command Line Tools、CMake を用意します。Unicorn のビルドには空白やコロンを含まないリポジトリパスを使ってください。一時ファイルの保存先にも空白を含めないでください。

```sh
mkdir -p "$HOME/.cache/aexcompat-build-tmp"
export TMPDIR="$HOME/.cache/aexcompat-build-tmp"
cargo build --release --manifest-path guest/Cargo.toml -p aex-guest-worker --jobs 1
export AEXCOMPAT_GUEST_WORKER="$PWD/guest/target/release/aex-guest-worker"
cargo run --manifest-path broker/Cargo.toml -p aexcompat-harness --jobs 1
```

既にビルド済みの arm64 worker があれば、ビルドを省き、`AEXCOMPAT_GUEST_WORKER` にその絶対パスを指定します。`CARGO_TARGET_DIR` を設定している場合も、実際の生成先に合わせてください。通常の Unicorn 実行に Windows や Rosetta は不要です。

## AEX と画像を選ぶ

画面は Windows と Mac で同じです。

1. Add AEX... で Windows x64 の `.aex` を選びます。解析が終わると Effect Controls に編集可能なパラメーターが表示されます。
2. Choose image... で入力画像を選びます。画像のドラッグ＆ドロップも使えます。
3. Render preview を押します。詳細パネルでは Render current frame も使えます。

入力は PNG、JPEG、BMP、TIFF、WebP に対応します。Mac では幅 3840、高さ 2160 ピクセル以下にしてください。

## Effect Controls を編集する

数値、スライダー、チェックボックス、選択肢、色、座標を変更します。レイヤー入力を持つエフェクトでは、その項目から別の画像を選びます。表示される項目と編集範囲は AEX によって変わります。

各項目の Reset で初期値に戻します。全パラメーターを戻すには、Effect Controls の見出しにある Reset を押してください。チェーンでは選択中の段に適用されます。

Auto Update をオンにすると、編集後に自動で再描画します。重いエフェクトではオフにして、編集をまとめてから描画ボタンを押してください。処理中は一部の操作が無効になります。無効な項目や未対応の項目は、診断欄の理由を確認してください。

## 入力と出力を見る

INPUT は入力、AEX OUTPUT は描画結果、COMPARE は両方を横に並べます。画像上でホイールを動かすと拡大・縮小し、ドラッグすると表示位置を動かせます。Fit で全体表示に戻します。倍率はウィンドウに収めた状態を基準にしています。

Mac の 16 bpc と 32 bpc float は、表示用の PNG と、値を保った raw または EXR を書き出します（「保存する」を参照）。

## 色深度を選ぶ

Analysis & Logs → Render Settings → Pixel depth で選びます。

| 色深度 | 用途・注意点 |
|---|---|
| 8 bpc / ARGB8 | PNG プレビューで画像を確認する |
| 16 bpc / ARGB16 | AE 内部の整数値を扱う。raw の白基準は 32768 |
| 32 bpc float / ARGB32f | 浮動小数点値を扱う。PNG 表示だけでは HDR 値や負値を保存できない |

AEX が対応する色深度を選んでください。高い色深度を選んでも、8-bit の入力画像に精度が追加されるわけではありません。数値の比較には対応する raw や EXR とメタデータを使います。

## エフェクトを連続して適用する

Windows と Mac でチェーンを使えます。Mac のチェーンは今のところ 8 bpc だけです。

1. 入力画像と最初の AEX を選びます。
2. Effect Controls → Effect chain の Add... で次の AEX を追加します。最大 8 段です。
3. 行を選んで、その段のパラメーターを編集します。同じ AEX を複数回使っても、設定は段ごとに保持されます。
4. Up / Down で順序を変え、チェックを外すとその段を飛ばします。Remove で選択中の段を削除します。
5. Render preview または PNG 保存を実行します。有効な段を上から順に適用します。

Windows のバックエンドは Auto または CPU を使います。有効な全段が選択した色深度に対応する必要があります。途中で失敗すると後続の段を止め、失敗した段を報告します。

チェーンは静止画用で、音声には対応しません。設定は現在の GUI セッション内で保持されます。Windows の debug request の保存・読込は選択中の段だけが対象で、チェーン全体は保存しません。詳細は [Windows GUI effect chains](WINDOWS_GUI_EFFECT_CHAIN.md) を参照してください。

## 保存する

### Windows

Save PNG... または詳細パネルの Render and save PNG... で保存先を指定します。チェーンの出力は次の組み合わせです。

| 色深度 | 保存される内容 |
|---|---|
| ARGB8 | PNG |
| ARGB16 | 16-bit PNG と `.rgba16le` |
| ARGB32f | 表示用の 8-bit PNG と `.rgba32f-le` |

raw サイドカーは行間の余白を含まない little-endian の RGBA です。寸法と形式はチェーンのレポートで確認します。16-bit PNG は画像形式の全範囲を使い、raw は AE の白基準 32768 を使います。float raw は有限の HDR 値と負値を保持します。PNG と raw を一緒に保管してください。既存ファイルは上書きしないため、新しい保存名を指定します。

### Mac

Windows と同じく Render preview（保存先は `target/harness-output`）、Save PNG...、Render and save PNG... で保存します。

| 色深度 | 保存される内容 |
|---|---|
| ARGB8 | PNG |
| ARGB16 | 表示用の PNG と、隣の `<名前>.<番号>.artifact/` 内の `output.bin` と `output.json` |
| ARGB32f | 表示用の PNG と、隣の `<名前>.<番号>.artifact/` 内の `output.exr` と `output.json` |

Mac の ARGB16 raw は little-endian の ARGB です。Windows チェーンの RGBA サイドカーと取り違えないでください。`output.json` に寸法、チャンネル順、色深度、ハッシュなどが記録されます。PNG と `.artifact/` を一緒に保管してください。

## 診断とレポートを読む

Analysis & Logs で、選択中の AEX と入力、描画設定、実行結果を確認します。互換性、GPU、性能などの診断操作は Windows で使えます。Mac では理由を表示して止まります。操作が無効なら、表示される未対応理由を確認してください。

不具合の報告にはOS、AEXCompat のコミット、AEX のバージョン、入力の寸法と色深度、パラメーター、実行した操作、エラー文を残します。レポート内の個人の絶対パスを取り除き、非公開 AEX、SDK、画像そのものは公開しないでください。

## よくあるエラー

| 症状 | 対処 |
|---|---|
| worker が見つからない | Windows は native worker をビルドする。Mac は `AEXCOMPAT_GUEST_WORKER` が存在する arm64 実行ファイルを指すか確認する |
| `Could not inspect AEX parameters` | Windows x64 の AEX か確認し、診断欄で不足 DLL や未対応 Suite を調べる。Windows では依存 DLL の配置・検索先も確認する |
| Mac で `Input exceeds the 4K UHD boundary` | 幅 3840、高さ 2160 以下の画像で試す |
| 描画ボタンが無効 | AEX の解析と画像の読込が完了しているか、別の処理中でないか確認する |
| Mac で `… is not available in the macOS x64 guest` | その機能は Windows でだけ使える。「残る Windows / Mac の差分」を参照する |
| Mac で `dependency DLLs are not loaded by the macOS x64 guest` | AEX の隣に置いた DLL を guest は読み込めない。その AEX は Windows で描画する |
| チェーンの途中で失敗 | 報告された段を確認し、単独でも再現するか調べる。全段の色深度対応を確認する |
| 保存先が既に存在する、書込に失敗する | 新しい名前と書込可能なフォルダーを使う。raw サイドカーの保存先も確認する |
| Windows で描画がタイムアウトする | 既定は 1 フレーム 30 秒。必要なら起動前に `$env:AEXCOMPAT_FRAME_DEADLINE_MS="120000"` を設定する。指定範囲は 1000〜600000 ms |
| 未対応 Suite、selector、出力サイズ・チェックサムのエラー | エラー文と再現条件を残して報告する。検証を無効にして出力を採用しない |

## 残る Windows / Mac の差分

X01b で Mac のチェーンと 3840×2160 の 4K UHD 入力・描画に対応しました。8/16/32 bpc の実 AEX テストを通しています。大きな複数レイヤーは既存のメモリー・ファイル数・時間制限に達する場合があります。[監査記録](../analysis/PLATFORM_PARITY_2026-10-26.md) も参照してください。

- 画面は同じで、実行方式は Windows が native worker、Mac が Unicorn の x64 guest です。
- 次の機能は Mac では理由を表示して止まります: GPU（OpenCL、CUDA、DirectX）、マスク・空間・描画の文脈、カスタム UI の操作、frame 0 / 30 fps 以外の時刻、依存 DLL、16/32 bpc のチェーン、音声、互換性・GPU・性能・AEGP の診断操作。
- Supervised なパラメーターの変更（USER_CHANGED_PARAM）は、Mac では同じ AEX を一度描画した後に使えます。
- Mac の入力は 3840×2160 以下です。
- 16-bit raw の順序は Windows チェーンが RGBA、Mac が ARGB。float の出力は Windows チェーンが PNG＋raw、Mac が PNG＋EXR＋JSON です。
- U03 の CLI は UI イベントと描画呼び出しを記録します。Mac の対話式カスタム UI や文字の描画は未実装です。Suite の全バージョン・全 slot、ACE、arbitrary stream の一致も未確認です。
- RVA・ポインターチェーン・画面全体の監視は guest 側にあります。native worker の対応機能、両側の ROI 比較、GUI の出力領域・geometry 操作は残っています。

この文書の手順はソースを確認して記述しました。Windows での実行確認や、両 OS の全機能一致を示すものではありません。

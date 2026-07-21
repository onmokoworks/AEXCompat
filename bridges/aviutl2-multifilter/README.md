# aexcompat-aviutl2-multifilter (issue #295)

AviUtl2 の generic プラグイン (`.aux2`)。フォルダ内の各 AEX を、それぞれ discovery した
パラメーターを**キーフレーム可能な固定 config 項目**として持つ**別フィルタ**として登録する。

## なぜ generic + libffi か

- AviUtl2 はキーフレームを登録済み config 項目 (Track 等) にしか効かせない。config セットは
  ロード時に凍結されるので、実行中に選んだ AEX のパラメーターを後からキーフレーム可能にはできない
  → 各 AEX を「ロード時に確定した固定スキーマの別フィルタ」にする。
- 単一 DLL で複数フィルタを登録するには generic 経路 (`register_filter_plugin!` は単一専用) を
  使う。generic の拡張子は **`.aux2`** (`.auf2` だと AviUtl2 が `GetFilterPluginTable` を探して失敗)。
- `func_proc_video` は per-filter context を受け取らず、`effect_id → フィルタ` の対応 API も無い。
  よってフィルタ毎に別の C 関数ポインタが要る。**libffi closure** で AEX 毎の C コールバックを
  実行時生成する (上限なしの実行時 N。コンパイル時スロットプールの上限を回避)。

## 段階 (increment)

1. **libffi 検証 (現状)**: config 項目なしの 2 フィルタ (Tint R / Tint G) を libffi closure で
   登録し、closure が `func_proc_video` として機能し captured state (channel) で正しく動くかを実機確認。
   crate ベースのスパイク (`bridges/aviutl2-multifilter-spike`, `.aux2`) で「複数登録 + 独立
   キーフレーム」は別途立証済み。
2. 各フィルタに discovery した Track/Checkbox/Select/Color を持たせ、キーフレーム値を読む。
   **AviUtl2 は func_proc_video 呼び出し直前に各テーブルの FILTER_ITEM 構造体の `value` を
   現在のキーフレーム値に更新する** (`FILTER_ITEM_TRACK.value` のコメント「フィルタ処理の
   呼び出し時に現在の値に更新されます」)。よって closure は userdata に自分の FILTER_ITEM
   ポインタ群を持ち、`(*track).value` を直接読むだけでよい。edit_section も effect_id も
   名前ベース lookup も不要。
3. 各フィルタ proc → その AEX の常駐 `RenderSession` に配線 (SmartFX 検出含む)。
4. フォルダ走査で実行時 N フィルタ登録。libffi closure・テーブル・item リストの生存期間管理。

## ビルド / 配置

```powershell
cargo build --release --manifest-path bridges\aviutl2-multifilter\Cargo.toml
Copy-Item bridges\aviutl2-multifilter\target\release\aexcompat_aviutl2_multifilter.dll `
          C:\ProgramData\aviutl2\Plugin\aexcompat_multifilter.aux2
```

`.aux2` で置くこと (`.auf2` は不可)。AviUtl2 起動中はロック。設計経緯は
`docs/AVIUTL2_BRIDGE_2026-07-21.md` の段階5。

## 設定 (issue #299)

対象 AEX フォルダ・worker repository・除外エフェクトを TOML で設定する。既定の設定パスは
Windows 標準の per-user 位置 **`%APPDATA%\aexcompat-multifilter\config.toml`**
(`AEXCOMPAT_MULTIFILTER_CONFIG` で明示パス上書き可)。

```toml
# 対象 AEX フォルダ (直下の *.aex を各々フィルタ登録)
dir = 'C:\Users\me\aex'
# 常駐 worker のある repo root (target/minihost-build/ を持つ)
repository = 'C:\path\to\AEXCompat'
# 除外するエフェクト (ファイル stem を大文字小文字無視でマッチ。.aex 付き/無し可)
ignore = ['pf_sampling_probe', 'broken-effect']
```

環境変数 `AEXCOMPAT_MULTIFILTER_DIR` / `AEXCOMPAT_MULTIFILTER_REPOSITORY` を設定すると
TOML の `dir` / `repository` を上書きする (env > TOML)。設定・env いずれも無ければ何も
登録しない。config はプラグインのロード時に一度だけ読むので、変更後は AviUtl2 を再起動する。

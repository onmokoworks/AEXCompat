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
# 対象 AEX フォルダ。dir(単一) と dirs(複数) は再帰スキャンされ、各 *.aex が
# 個別フィルタとして登録される。両方省略時は AE/MediaCore の既定を使う (後述)。
dir = 'C:\Users\me\aex'
dirs = ['C:\more\aex', 'D:\shared\aex']
# 常駐 worker のある repo root (target/minihost-build/ を持つ)
repository = 'C:\path\to\AEXCompat'
# 除外するエフェクト (ファイル stem を大文字小文字無視でマッチ。.aex 付き/無し可)
ignore = ['pf_sampling_probe', 'broken-effect']
# 依存 DLL の探索フォルダ (issue #304)。AEX 自身のフォルダは常に最優先で探索されるので
# ここには書かない。省略時は AE (最新版) の Support Files を使う。
dependency_dirs = ['C:\Program Files\Adobe\Adobe After Effects 2025\Support Files']
# 1つの AEX に封入する依存の上限 (省略時は無制限)。重いプラグイン (最大 227 DLL / 約 1GB) の
# discovery に時間をかけたくない場合だけ設定する。超えた AEX は discovery 失敗になる。
# dependency_module_limit = 64
# dependency_byte_limit = 268435456
```

環境変数 `AEXCOMPAT_MULTIFILTER_DIR` / `AEXCOMPAT_MULTIFILTER_REPOSITORY` /
`AEXCOMPAT_MULTIFILTER_DEPENDENCY_DIRS` (`;` 区切り) を設定すると TOML の
`dir`(+`dirs`) / `repository` / `dependency_dirs` を上書きする (env > TOML)。config は
プラグインのロード時に一度だけ読むので、変更後は AviUtl2 を再起動する。

### 依存 DLL の封入 (issue #304)

worker は AEX を隔離した sealed load tree からロードし、探索先は
「そのフォルダ + System32」しかない。AE のエフェクトの多くは `dvacore.dll` 等の Adobe
ランタイム DLL を import しており、隔離先に無いと `LoadLibraryExW` が失敗する
(worker exit 11)。そこで discovery / セッション開始時に **AEX の import を再帰的に辿って
依存クロージャを解決し、sealed tree に AEX と一緒に封入する**。

- 探索順は「AEX 自身のフォルダ → `dependency_dirs`」。Windows ローダーと同じく**最初に
  見つかったものが勝つ**。System32 にある名前は封入しない (worker のロードフラグが
  System32 を見るため)。
- 封入された DLL は AEX 本体とまったく同じ経路で認証される (sha256 + サイズ照合、reparse
  point 拒否、basename 衝突拒否)。探索フォルダを渡すことは worker の DLL 探索パスを
  広げることではない。
- クロージャの大きさに上限は設けない (`dependency_module_limit` / `dependency_byte_limit` で
  明示的に設定した場合を除く)。AE のエフェクトの一部は Adobe ランタイムをほぼ丸ごと
  引く (実測で最大 225 DLL / 約 1.0GB) が、封入こそがそれをロード可能にする唯一の手段なので、
  「大きいから」で弾かない。ただし sealed load tree は**コピー**なので、その分だけ discovery と
  セッション開始が遅くなる (実測: 220MB で 0.8s、1.0GB で十数秒)。依存の重い AEX を大量に
  抱えるフォルダを指定すると、バックグラウンド discovery は相応に長く走る。
- クロージャが解決できない AEX (上限超過、探索フォルダが不正、image が壊れている等) は
  discovery 失敗として扱う。依存無しで再試行しても同じロード失敗になるため。この negative も
  キャッシュされるが、**失敗時も「解決が辿ったファイル」を記録する** (ハッシュもコピーもしない
  survey を使う) ので、上限を超えていた依存 DLL が小さくなった・消えた・置き換わった場合は
  自動的に再 discovery される。上限自体を変えた場合はキャッシュ全体が無効化される。
- これで解けるのは L1 (LoadLibrary 失敗) だけで、Adobe ランタイムを引くエフェクトはさらに
  module audit (L2) と Adobe IPC 初期化 (L3) の壁がある。実測は
  `docs/AE_EFFECT_LOADING_INVESTIGATION_2026-07-22.md` を参照。

### 既定のスキャン対象 (issue #303)

`dir` / `dirs` / env いずれも無ければ、既定で **After Effects (最新版) の
`Support Files\Plug-ins` と Adobe の `Common\Plug-ins\<ver>\MediaCore`** を再帰スキャンする。
AE の全エフェクトをそのまま AviUtl2 のキーフレーム可能フィルタとして使える。effect でない
`.aex` (Format/codec 等) は discovery に失敗して自動的にスキップされる。

### discovery キャッシュ / バックグラウンド discovery

各 AEX の discovery (パラメーター取得) はロード時に worker を起動して行うため、AE の全
エフェクト (数百) を毎回起動時に discovery すると数分かかる。**起動をブロックしないため、
discovery はバックグラウンドスレッドで行う:**

- `RegisterPlugin` は**キャッシュ済み (discovery 成功済み) の効果を即座に登録して即リターン**
  する。起動はブロックされない。
- 未 discovery / 変更された AEX は**別スレッドで低並列に全部 discovery** し、結果を
  `%APPDATA%\aexcompat-multifilter\discovery-cache.json` に (mtime+len キーで) 書く。
- **新しく discovery された効果は次回の AviUtl2 起動時にキャッシュから登録されて出る。**
  初回 (や AEX 追加後) は該当フィルタがその起動では出ず、次の起動で出る。

つまり: 初回起動 → 即座に使える (バックグラウンドで数分かけて discovery) → 2 回目起動 → 全効果が
出て高速。AEX を差し替え・追加すると mtime/len 変化で再 discovery され、次回起動で反映される。

- キャッシュは worker exe / この DLL / **依存解決の入力** (探索フォルダの並び + 上限) の
  いずれかが変わると全体が無効化される。
- それとは別に、各エントリは**自分の依存解決の結果**を持っている: 探索した root の順序、
  封入した DLL の (パス, mtime, サイズ)、どの root も提供しなかった import 名 (Windows の
  API set は除く)。起動時にこれを stat で照合し、次のいずれかなら**その AEX だけ**再 discovery
  する。
  - 封入した DLL が書き換わった / 消えた (AE のアップデートが `dvacore.dll` を書き換えた等)
  - より優先度の高い root に同名ファイルが現れ、解決先が変わる
  - 見つからなかった import が置かれた (= 以前失敗した AEX が今なら動く)
- discovery は結果を全てキャッシュする (effect でない `.aex` = Format/codec 等の negative も)。
  低並列なので負荷下の偽タイムアウトは起きにくいが、稀に一時的失敗で effect が誤って除外・
  キャッシュされることがある。その場合は該当 AEX を touch するか
  **`discovery-cache.json` を削除**すれば再 discovery される。

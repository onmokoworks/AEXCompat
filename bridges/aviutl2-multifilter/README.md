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
```

環境変数 `AEXCOMPAT_MULTIFILTER_DIR` / `AEXCOMPAT_MULTIFILTER_REPOSITORY` を設定すると
TOML の `dir`(+`dirs`) / `repository` を上書きする (env > TOML)。config はプラグインの
ロード時に一度だけ読むので、変更後は AviUtl2 を再起動する。

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

- discovery は結果を全てキャッシュする (effect でない `.aex` = Format/codec 等の negative も)。
  低並列なので負荷下の偽タイムアウトは起きにくいが、稀に一時的失敗で effect が誤って除外・
  キャッシュされることがある。その場合は該当 AEX を touch するか
  **`discovery-cache.json` を削除**すれば再 discovery される。

> **警告**: この復旧操作 (touch / キャッシュ削除) をすると、対象フィルタは**次の 1 起動だけ
> 未登録**になる。未登録フィルタを使った保存済みプロジェクトをその起動で開くと、AviUtl2 が
> 該当オブジェクトを破棄し、保存すると失われる (下記)。**そのフィルタを使ったプロジェクトを
> 開かない状態で**行い、再 discovery が終わった次の起動まで待つこと。

#### 未登録フィルタとプロジェクトのデータ消失 (issue #307)

**フィルタが登録されていない起動で、そのフィルタを使った保存済み `.aup2` を開くと、AviUtl2 は
「一部のオブジェクトの読み込みに失敗しました」を出して該当オブジェクトをタイムラインから削除し、
そのまま保存するとファイルから完全に消える** (実測。未知セクションの保存・再出力は行われない)。

このため「一時的にフィルタが登録されない」状態は極力作らない設計にしてある。現状カバー
できているのは以下:

- ホスト (worker / この DLL) の更新 → 登録は維持したまま裏で再検証 (後述)
- 再検証中の一時的な discovery 失敗 (偽タイムアウト、stat 失敗) → 既存の成功結果を維持し降格しない
- スキャン対象フォルダが一時的に読めなかった / 一部しか列挙できなかった / 既定フォルダ
  (AE・MediaCore) の最新版が一時的に見つからなかった場合 → そのフォルダのキャッシュエントリを
  消さない (「スキャンされなかった」を「無くなった」と解釈しない)
- **スキャン対象フォルダを変えた起動** (`AEXCOMPAT_MULTIFILTER_DIR` を一時的に別フォルダへ
  向ける、`dir`/`dirs` を狭める、設定違いの AviUtl2 を併用する) → キャッシュファイルは設定を
  跨いで 1 本なので、今回スキャンしたフォルダ配下のエントリしか削除対象にしない
- discovery 中に AEX が差し替わった場合 → 登録は維持しつつ stale 印を付け、次回必ず再 discovery
  する (古い `sha`/`params` のまま固定されない)
- キャッシュの一部エントリが壊れた場合 → そのエントリだけ捨て、残りは保持する (ファイル全体を
  捨てて全フィルタを失わない)。ただしパラメーターの型 (broker の `InteractiveParameter`) が
  変わった場合はパラメーターを持つ全エントリが同時に読めなくなるので、この分離では防げない。
  そちらは型を固定するテストでビルド時に落とす

**まだカバーできていないケース**:

- AEX ファイル自体が差し替わったとき (AE 側のアップデート等) は `(mtime,len)` 変化で未登録に
  なり、その 1 起動は上記の消失が起きうる (issue #309)。
- 上のカバー範囲のうちスキャン失敗系が守るのは「**キャッシュエントリを消さない**」= 次回以降の
  起動であって、**その起動でフィルタが登録されること自体ではない**。スキャン対象フォルダが
  丸ごと見えなかった起動 (AE 更新中で既定フォルダが解決できない、junction 先のドライブ未接続、
  `AEXCOMPAT_MULTIFILTER_DIR` を一時的に別フォルダへ向けた等) では、そのフォルダのフィルタは
  その起動では出ない。登録は「今回のスキャン結果」だけで駆動されているため (issue #321)。

#### ホスト (worker / この DLL) を更新したとき

discovery の結果は AEX のバイト列だけでなく、**それを生成した互換ホスト**にも依存する
(AEX をロードして `EffectMain` を回す L2 worker と、sealed load tree のステージングを行う
この DLL 内の broker)。よってキャッシュの各エントリは自分を生成したホストの fingerprint
(worker exe とこの DLL の mtime+len) を持ち、ホストが更新されると**バックグラウンドで
再検証**される。ホスト側の対応が進めば (issue #304)、以前ロードできなかったエフェクトが
次回起動から出るようになる。

**再検証中も既存のキャッシュ内容でフィルタは登録され続ける** (issue #307)。ホスト更新後の
1 起動だけキャッシュを捨てて登録ゼロにする実装だと、AEX フィルタを使った保存済み
プロジェクトを開いたときに **AviUtl2 が該当オブジェクトを丸ごと破棄し、そのまま保存すると
不可逆に失われる**ため。fingerprint はファイル単位ではなくエントリ単位に持たせてあり、
再検証が途中で中断されても未検証のエントリは次回起動で再びキューに載る。

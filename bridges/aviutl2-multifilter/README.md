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

### config item名の衝突

AEXのパラメーター名は表示用ラベルであり、一意とは限らない。AviUtl2の保存キーが同名を
区別できないため、同じフィルタ内で重複するラベルには元のAEX slotを付ける
(`Intensity [slot 12]` のような形式)。空のラベルも `Parameter <slot> [slot <slot>]` に
正規化する。重複しない既存ラベルはそのまま維持し、描画時の値読み出しは表示名ではなく
従来どおりAEX slotで行う。これにより同名パラメーターのsave/loadが別slotへ混ざらない。

## ビルド / 配置

```powershell
cargo build --release --manifest-path bridges\aviutl2-multifilter\Cargo.toml
Copy-Item bridges\aviutl2-multifilter\target\release\aexcompat_aviutl2_multifilter.dll `
          C:\ProgramData\aviutl2\Plugin\aexcompat_multifilter.aux2
```

`.aux2` で置くこと (`.auf2` は不可)。AviUtl2 起動中はロック。設計経緯は
`docs/AVIUTL2_BRIDGE_2026-07-21.md` の段階5。

### worker の置き場所 (issue #650)

AEX は別プロセスの worker (`aex_l2_worker.exe` 等) で実行するので、DLL だけでは
動かない。worker の探索順は:

1. 環境変数 `AEXCOMPAT_MULTIFILTER_REPOSITORY`
2. `config.toml` の `repository`
3. **プラグインの隣**: DLL と同じフォルダ、次に `<DLLのフォルダ>\aexcompat`。
   それぞれ `target\minihost-build\aex_l2_worker.exe` がある場合にのみ採用

1 と 2 は開発者が自前ビルドの worker を使うための明示指定で、そこに worker がある
限り優先される。**worker が無い場合は 3 に降格する** (指した先が消えていても
プラグインが動き続けるように)。3 にも worker が無ければ 1・2 をそのまま採用する
(これから建てる場所を指しているとみなす)。

つまり明示指定していても、そこに worker が無くプラグインの隣にあれば、隣の worker が
使われる。意図しない worker で観測しないよう、開発中は指定先を建ててから使うこと。
ルートが切り替わった起動では worker fingerprint が変わるため、登録は維持したまま
全エントリがバックグラウンド再 discovery に回る (issue #307 の再検証機構)。

> worker freshness (#613) は #729 で「拒否」から「記録される警告」に降格された。
> ソースを含まない配布形態でも discovery / render は通り、worker と DLL を同じ
> コミットのクリーンなツリーからビルドしていれば build provenance (#649) の
> revision 一致で警告なしに認識される。それ以外の組み合わせは動作しつつ
> `worker_freshness_warning` が診断に記録される。

プラグインと worker を一緒に置くだけでユーザーのチェックアウトに
実行時依存しなくなる:

```powershell
$plugin = 'C:\ProgramData\aviutl2\Plugin'
Copy-Item bridges\aviutl2-multifilter\target\release\aexcompat_aviutl2_multifilter.dll `
          "$plugin\aexcompat_multifilter.aux2"
New-Item -ItemType Directory -Force "$plugin\aexcompat\target\minihost-build" | Out-Null
Copy-Item target\minihost-build\aex_*_worker.exe "$plugin\aexcompat\target\minihost-build\"
```

### ログ (issue #655)

フィルタが1件も出ないとき、原因は AviUtl2 の**ログ**に出る。この DLL は
`InitializeLogger` で受け取ったハンドルへ `[AEXCompat] ` 始まりの行を書く。

通常の起動で出る行 (下の warn で早期 return した場合は途中までになる):

- `worker root: <path> (<経路>)` — 採用した worker root と、それが config.toml /
  環境変数 / プラグインの隣のどれで決まったか。スキャン対象フォルダの実パスも出す
- 初回起動 (キャッシュファイルが無い) のみ:
  `no discovery cache yet: discovering N plug-in(s) before registration` —
  同期 discovery の開始 (issue #838、後述)。完了時に
  `first-launch discovery: N effect(s), R rejected`
- `registered N of M known plug-in(s); K queued for discovery` — 登録件数
- `discovering K plug-in(s) in the background` — バックグラウンド discovery の開始
- 完了時に `background discovery: N effect(s), R rejected`

以下は warn で出る。どれも「フィルタが空」という同じ症状になる別々の原因で、
ログが無かった頃は外から区別が付かなかった:

| 状況 | 出る内容 |
| --- | --- |
| worker がどこにも無い | 探索する実パス (`target\minihost-build\aex_l2_worker.exe`) と設定先 |
| 指定した root に worker が無い | root を出したうえで「そこには worker が無い」と明示 |
| config.toml が読めない / パースできない | パスとエラー。設定が全部無視されることを明示 |
| .aex が1件も見つからない | 実際に見たフォルダのパス。読めなかったフォルダがあればそれも |
| 全部 `ignore` に一致した | 件数と、`ignore` が原因であること |
| M件知っているのに0件登録・0件キュー | worker が全滅している可能性 (issue #651 の形) |
| discovery が全件 reject | 同上 |
| キャッシュを書けなかった | 再起動しても結果が残らないこと |

「0件登録・全件キュー」は (キャッシュファイルはあるが中身が読めない起動などで)
正常に起こり得る状態なので warn ではなく info にし、「次回起動で出る」と明示する。
ここを warn にすると本物の異常と区別が付かなくなる。

`InitializeLogger` が `RegisterPlugin` より後に呼ばれても行は落ちない (ハンドルを
受け取るまでバッファに溜め、受け取った時点で順序を保ったまま吐く)。

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

### 設定ダイアログ (issue #855)

TOML を手で書かなくても、AviUtl2 の設定メニュー内「AEXCompat multi-filter」から
同じ項目を編集できる。保存先は上記の config.toml と同一 (`AEXCOMPAT_MULTIFILTER_CONFIG`
も同様に効く) で、手書きした既存ファイルのコメント・未知キーは保存後も維持される
(`toml_edit` によるマージ。ただしダイアログが書き換えるキー自体に付けたコメントは
値と一緒に消える。単一フォルダ指定の `dir` は保存時に `dirs` へ畳まれる)。
既存ファイルが TOML として解析できない場合は `config.toml.bak` に退避してから書き直す。

`ignore` はチェックボックス一覧で選ぶ (issue #858)。一覧は「現在の設定フォルダの
スキャン結果 (ignore 済み含む) ∪ discovery キャッシュの既知エントリ」を stem で
大小無視の重複排除・ソートしたもので、チェック = 無視。一覧に無い名前 (まだ
スキャンされていない等) は下の手動欄に 1 行 1 つで書け、既存 `ignore` のうち
一覧に一致しないエントリはそこに事前充填される (黙って消えることはない)。

ダイアログが編集するのはファイルであってロード済みの状態ではない: config はロード時に
一度だけ読まれ、フィルタの config セットも AviUtl2 がロード時に凍結するため、**保存した
変更は次回の AviUtl2 起動から反映される**。また env 上書き (上記) はファイルより常に
優先されるので、該当の環境変数が設定されている間はダイアログにその旨の注意が出る。

### フィルタ効果メニューのカテゴリ (issue #871)

登録される各フィルタは、AEX 自身の PiPL `catg` プロパティ (AE 本体が
エフェクトメニューの分類に使う文字列。Adobe 純正の ZString 形式
`$$$/…/Category/Simulation=Simulation` は `=` 以降の表示名に展開) を
discovery 時にバイト列から読み取り、メニューカテゴリ
**`AEXCompat\{カテゴリ}`** (取れない場合は `AEXCompat`) を初期値として登録する
(`FILTER_PLUGIN_TABLE.label`)。これは**初期値**で、AviUtl2 は各エフェクトの
カテゴリ (ラベル)・表示順・表示/非表示を初回登録時に `aviutl2.ini` の
`[Effect.*]` に永続化し、以後はそちらが勝つ。既に登録済みのエフェクトの
カテゴリや並び順を変えたい場合は、AviUtl2 の「設定」→
「オブジェクト追加メニューの設定」で編集する (ラベルは自由入力で `\` 区切りの
階層も可)。

カテゴリは discovery キャッシュに追加フィールドとして記録されるため、この
機能より前に discovery されたエフェクトはホスト更新に伴うバックグラウンド
再検証で埋まる (それまでは `AEXCompat` 直下)。ただし aviutl2.ini に既に
ラベルが永続化されているエフェクトには、上記のとおり初期値は効かない。

### 依存 DLL の封入 (issue #304)

worker は AEX を隔離した sealed load tree からロードし、探索先は
「そのフォルダ + System32」しかない。AE のエフェクトの多くは `dvacore.dll` 等の Adobe
ランタイム DLL を import しており、隔離先に無いと `LoadLibraryExW` が失敗する
(worker exit 11)。そこで discovery / セッション開始時に **AEX の import を再帰的に辿って
依存クロージャを解決し、sealed tree に AEX と一緒に封入する**。

- 探索順は「AEX 自身のフォルダ → `dependency_dirs`」。Windows ローダーと同じく**最初に
  見つかったものが勝つ**。探索フォルダが提供する名前は、System32 に同名があっても封入する
  (`dvacore.dll` のような Adobe 同梱ランタイムを取りこぼさないため)。封入されないのは次の 3 つ:
  - どの探索フォルダも提供しない名前 (worker のロードフラグが System32 を見る)
  - API set (`api-ms-*` / `ext-ms-*`)。ローダーが schema から解決するので、探索フォルダに
    コピーがあっても読まれない
  - DLL 名として成立しない import 名 (区切り文字・ドライブレター・予約デバイス名・非 ASCII)。
    件数だけが記録される
- **封入 = そのコピーがロードされる、ではない**。ローダーは「既にプロセスに載っている
  モジュール」と KnownDLLs を先に解決するので、worker 自身が既に読んでいる CRT
  (`msvcp140.dll` / `vcruntime140.dll`) や `kernel32.dll` は、探索フォルダのコピーを封入しても
  そちらが使われることはない。sealed tree は「必要なものを含む集合」であって「実際にロード
  される集合」ではない。
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
  自動的に再 discovery される。上限自体を変えた場合も (エントリの「どのホストで作られたか」に
  含まれるため) 各エントリが再 discovery の対象になる — 登録は維持されたまま (issue #307)。
- これで解けるのは L1 (LoadLibrary 失敗) だけで、Adobe ランタイムを引くエフェクトはさらに
  module audit (L2) と Adobe IPC 初期化 (L3) の壁がある。実測は
  `docs/AE_EFFECT_LOADING_INVESTIGATION_2026-07-22.md` を参照。

### 既定のスキャン対象 (issue #303)

`dir` / `dirs` / env いずれも無ければ、既定で **After Effects (最新版) の
`Support Files\Plug-ins` と Adobe の `Common\Plug-ins\<ver>\MediaCore`** を再帰スキャンする。
AE の全エフェクトをそのまま AviUtl2 のキーフレーム可能フィルタとして使える。effect でない
`.aex` (Format/codec 等) は discovery に失敗して自動的にスキップされる。

### discovery キャッシュ / 初回同期 discovery / バックグラウンド discovery

各 AEX の discovery (パラメーター取得) はロード時に worker を起動して行う。2 回目以降の
起動をブロックしないため、キャッシュとバックグラウンドスレッドを使う:

- **初回起動 (キャッシュファイルが無い) だけは同期 discovery** (issue #838): 何も登録せず
  次回を待つ代わりに、`RegisterPlugin` がその場で全 AEX を discovery してから登録する。
  in-place ロード (issue #751) 後の AE 全エフェクト (224 本) で実測 40 秒前後。この間
  AviUtl2 はロード画面のまま止まって見える。discovery には deadline が無いので、inspect が
  返らない AEX がいると起動が終わらないことがあるが、discovery 開始前にキャッシュファイルを
  作成 (スタンプ) してあるため、**強制終了しても次の起動からは従来のバックグラウンドフローに
  落ちて二度とブロックしない** (原因の AEX は `ignore` で外せる)。
- 2 回目以降の `RegisterPlugin` は**キャッシュ済み (discovery 成功済み) の効果を即座に登録して
  即リターン**する。起動はブロックされない。
- 未 discovery / 変更された AEX は**別スレッドで低並列に全部 discovery** し、結果を
  `%APPDATA%\aexcompat-multifilter\discovery-cache.json` に (mtime+len キーで) 書く。
  **新しく discovery された効果は次回の AviUtl2 起動時にキャッシュから登録されて出る。**

つまり: 初回起動 → 数十秒待つと全効果が出る → 2 回目以降 → 即座に出て高速。AEX を
差し替え・追加すると mtime/len 変化で再 discovery され、次回起動で反映される (このとき
起動はブロックされない)。キャッシュファイルがあるのに中身が読めない起動 (スキーマ更新等) も
ブロックせず、従来どおり「その起動は 0 件登録 → バックグラウンド → 次回反映」になる。

- **依存解決の入力** (探索フォルダの並び + 上限) は worker exe / この DLL と並んで
  エントリの「どのホストで作られたか」に含まれる。変われば各エントリはバックグラウンドで
  再 discovery される (登録は維持されたまま。issue #307)。
- それとは別に、各エントリは**自分の依存解決の結果**を持っている: 探索した root の順序、
  封入した DLL の (パス, mtime, サイズ)、どの root も提供しなかった import 名 (Windows の
  API set は除く)。起動時にこれを stat で照合し、次のいずれかなら**その AEX だけ**再 discovery
  の対象になる (これも登録は維持される)。
  - 封入した DLL が書き換わった / 消えた (AE のアップデートが `dvacore.dll` を書き換えた等)
  - より優先度の高い root に同名ファイルが現れ、解決先が変わる
  - 見つからなかった import が置かれた (= 以前失敗した AEX が今なら動く)
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

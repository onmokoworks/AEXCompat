# selector families (issue #362) — 35 件の共通原因診断と対応 (2026-07-25)

対象: discovery sweep で `selector_error` / `exit_20` になっていた 35 件の実 AEX
(OCIO 系 3、レガシー古典 28、サードパーティ 4)。手法は VR ファミリと同じく、
代表件の staging + `--l2-params-only` inspect + cdb で失敗セレクタと error 値を
確定し、ファミリ横断の共通原因を最小修正で潰した。

## 結果サマリ

- **one-shot inspect で 28/35、sealed narrow sweep (--cluster) で 26/35 が
  `loaded` に到達** (開始時は 35 件全て `selector_error` / `exit_20`)。
- sweep での差分 2 件は host 契約ではなく seal/cluster ポリシー起因
  (AddGrain: リソース dir 非 seal、Scribble: BIB.dll 非 seal、B.Carve は
  Cineware クラッシュの巻き添え)。Lumetri は逆に staging 起因で
  one-shot のみ落ち、sweep では通る。
- 残り 7 件は全て「worker が意図的に fail-closed している領域」に依存:
  非公開/文書化されていない host 機能。個別の診断は末尾。

## 修正した共通原因 (発見順)

### 1. $$$ string table の value 規則が実機より厳しかった (11 件)

`aex_string_table_impl.cpp` は value を「非空 ASCII 印刷可能」に限定しており、
以下の全てを含むテーブル全体を `invalid` に落としていた (fail-closed の誤適用):

- 空 value: `$$$/AE/Effect/Name/OCIOColorSpaceTransform/ColorSpace/Options=` ほか
  (Colorama / Particle_Playground / Scribble / Stabilizer / Three_Way 等)
- 改行入り value: Lumetri `InvalidLUT/Message=Unable to load \n'@0'`、ApplyColorLUT
- UTF-8 value: OCIO `License/AgreementName=カスタム`

いずれも Adobe 純正プラグインが実機で普通に動く以上、実 host は value を
NUL までの任意バイト列として受理している。value 検証を撤廃し、構造チェック
(prefix / `/LStr/` / 数字 id / key path の ASCII) だけを残した。

### 2. 複数 LStr グループの id 衝突 (16 件)

1 イメージに複数の文字列グループが同居する: エフェクト本体の about+params
グループ、match-name/category グループ、共有ライブラリ (CAMLIGHT/SOUP)、
そして同一バイナリ内の兄弟エフェクト (`$$$/AE/Levels` と `$$$/AE/Levels2`)。
フラットな id→value マップでは id 0/1 が衝突して invalid になっていた。

runtime lookup は生の整数 id しか渡さない (引数に scope 情報は無いことを
cdb で実測: a0 はプラグイン自身の global オブジェクト)。正しいグループは
「id 0 が `<Name>, v%` の about-version パターンを持つグループ」で一意に
決まることを全対象で検証した (Card Dance は id=1 に match-name 表の
"Simulation" ではなく Res/4147 表の "Rows & Columns" を期待する)。
混在イメージの match-name/category/error 系 path エントリ (非 LStr) は
registration 層の文字列であり、runtime lookup からは落とす。

### 3. U.dll のプロセスワイド allocator が未初期化 (Curves ほか PIN 系)

Curves は GLOBAL_SETUP で `U_AllocateHandleClear` → 内部 allocator グローバル
(Up_TopLevelWindowP0+0x108) が NULL のまま使われ `A_Err_ALLOC`。
実 AE はプロセス開始時に `U_Birth` を呼んでこの領域を初期化するが、
closure 内のどの DLL も `U_Birth` を import していない (= host の責務)。
worker が U.dll 入り closure の初回ロード時に一度だけ `U_Birth` を
SEH ガード付きで呼ぶ `initialize_legacy_support_libraries()` を追加した
(one-shot / discovery inspect / cluster swap の 3 経路)。

### 4. AEGP Compute Cache Suite v1 が無かった (5 件)

Auto Color / Auto Contrast / Auto Levels / Levels / Shadow-Highlight は
GLOBAL_SETUP で "AEGP Compute Cache" v1 を acquire し、無いと
"Not able to acquire AEFX Suite." で落ちる。SDK (AE_ComputeCacheSuite.h)
通りの 6 関数 suite を `worker_compute_cache_suite.cpp` に実装
(class 登録簿 + (class, GUID key) キャッシュ + receipt 台帳。
cluster reset で purge)。`--self-test-compute-cache` も追加。

### 5. PF Color Settings Suite の v6 と plugin_id (OCIO 3 件 + Levels2)

OCIO 系は v6 (= struct Suite5、14 関数) を要求するが、worker は v7
(= struct Suite6、20 関数) のみ提供していた。v6 は v7 テーブルの先頭
14 関数の prefix なので同一テーブルを v6 としても登録。
さらに OCIO は `IsOCIOColorManagementUsed` 等に **plugin_id=0** を
ハードコードで渡す (cdb で実測) のに対し、worker は id==1 のみ受理
していた。OCIO クエリ族は read-only の host 状態取得なので id 0 も受理。

### 6. AEGP Utility Suite の version 穴 (2 件)

Liquify は v3 (= Suite1、9 スロット)、Cryptomatte は v11 (= Suite5、
31 スロット) を要求。worker は v7/v13 のみ。各 layout どおりの
テーブル (RegisterWithAEGP/GetMainHWND を正しい slot に配置) を追加。

### 7. utils+0xC8 の `app` コールバックが null (Drop_Shadow / Reshape_New)

PIN 系古典は GLOBAL_SETUP で `utils->app(effect_ref, selector, arg)` を呼ぶ
(Drop_Shadow は selector 3 にエフェクト所有のコールバックテーブルを渡す)。
headless worker では受理して 0 を返すだけの `host_app_callback` を
offset 200 に配線 (AbiHooks を 31→32 エントリに拡張)。

### 8. lookup が不在 id に null を返す (Three_Way / Caustics)

Three_Way_Color_Corrector は存在しない id 610 の結果を無条件に
strlen する。実 host は文字列リソースの LoadString 意味論
(不在 → 空文字列) のはずで、valid テーブルでは不在 id に空文字列を
返すようにした (invalid/NoEntries テーブルは従来どおり null で
fail-closed)。

### 9. BIB suite が BIB.dll 未リンクの closure で提供できない (Scribble)

`provide_bib_suite` は `GetModuleHandleW(L"BIB.dll")` 前提で、BIB.dll を
静的にも delay でもリンクしない closure (Scribble) では suite を
提供できなかった。実 AE の PICA basic は host 常設機能なので、
**admitted プラグインの自ディレクトリ + 固定名 "BIB.dll"** に限定した
1 回のオンデマンドロードを追加 (LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR |
SYSTEM32。任意パス・PATH・CWD は絡まない)。契約テスト
`test_bib_suite_contract.py` を新しい bounded 形状に更新。

### 10. staging 起因 (worker 修正なし)

- AddGrain は `<plugin dir>/Film Stocks/` の .grain を PARAMS_SETUP で
  再帰列挙し、無いと C++ 例外 → A_Err_GENERIC。実ディレクトリでは
  存在するため、sweep が in-place で動く限り発生しない (staging 側の話)。
- Lumetri の delay-load 失敗 (0xC06D007E) は `tbbmalloc.dll` の
  staging 漏れだった (closure 解決が import 表に出ない明示ロードを
  拾えない)。実 sweep の密封が完全なら発生しない。

## 残り 7 件 (fail-closed 維持領域への依存 / seal・cluster ポリシー)

| プラグイン | one-shot inspect | narrow sweep (--cluster) | 原因 (実測) |
|---|---|---|---|
| Lumetri | **sweep では loaded (130 params)** | loaded | one-shot staging で見えた 0xC06D007E は `tbbmalloc.dll` 等の delay-load closure 不足。sweep の完全な密封 (242 モジュール) では通る。"PF File Registration Suite" v1 の acquire 失敗は **非致命的** (警告のみ) で、原因ではなかった |
| Stabilizer | PARAMS_SETUP 13 | exit_20 (closure に unresolved 70) | "PF AE Private Effect Suite" v5 要求 (private suite、SDK 未収録)。sealed 環境ではクラッシュに転じる |
| 3D Camera Tracker | PARAMS_SETUP 13 | selector_error | "PF AE Private Effect Suite" v3 要求 (private) |
| Cineware_AE_Effect | GLOBAL_SETUP 13 | cluster_session_invalidated | "SP Suites Suite" v2 要求 (SPC meta-suite)。one-shot では error 13 を返して終わるが、cluster セッション内では worker ごと落ちる (teardown クラッシュ、要 follow-up) |
| Particle_Playground | GLOBAL_SETUP 11 | selector_error | BIB suite 取得後、host 非呼出しのまま内部で失敗 (BIB resolver 経由の proc 不足が疑われるが未確定) |
| ProfileToProfile | GLOBAL_SETUP 14 | selector_error | 同上 (error 値のみ異なる) |
| ApplyColorLUT | PARAMS_SETUP で C++ 例外 | selector_error | LUT host 連携用のグローバルコールバックが未登録 (登録経路となる host 機能が未提供) |

## seal / cluster ポリシー起因 (host 契約の修正対象外、sweep でのみ顕在化)

one-shot inspect では通るが sealed sweep では落ちる 3 件:

- **AddGrain**: `<plugin dir>/Film Stocks/` の .grain を PARAMS_SETUP で再帰列挙
  するが、seal は DLL クロージャのみでリソースディレクトリを含まない →
  例外 → A_Err_GENERIC。元の conformance matrix の AddGrain 失敗はこれ
  (host 契約バグではない)。
- **Scribble**: BIB.dll を import しない closure のため sealed root に
  BIB.dll が無く、今回の bounded オンデマンドロードも届かない → gs=13。
  PICA を host 常設とみなす seal 側のポリシー (BIB.dll を常に seal する等)
  が必要。
- **B.Carve**: Cineware と同じ cluster に入り、Cineware の worker 落ち
  (cluster_session_invalidated) の巻き添え。単独では pass する。

private suite 群 (Stabilizer / 3D Camera Tracker / Cineware) と
Particle_Playground / ProfileToProfile / ApplyColorLUT は「成功を捏造する
stub」を提供しない限り通らない。fail-closed 緩和はしない方針のため、
private suite の ABI を実測で特定する follow-up が必要 (trampoline probe
で呼出しパターンを採取するところから)。

## 検証

- minihost Release build: OK (C4819 は既存の encoding 警告のみ)
- selftest: 5 exe 全 pass + `--self-test-compute-cache` / `suite-entry-utility13`
  / `pf-color-suite` / `pf-color-param-suite` / `aegp-installed-effect-catalog` 全 pass
- VR ファミリ 12/12: 変わらず全 pass (回帰なし)
- pytest: 1620 passed / 1 件は環境 blocker (実 AE GUI が起動中で
  reference capture を拒否) / bootstrap component test は offset 200 追加に
  合わせて更新済み
- broker cargo test --workspace: OK
- narrow sweep (35 件、--cluster、12 clusters、105s): **26 loaded** /
  7 cluster_inspect_selector_error / 1 cluster_session_invalidated / 1 exit_20
- 契約テスト: `test_aex_selector_families_contract.py` 新規 (8 件)、
  `test_bib_suite_contract.py` 更新 (bounded load 形状)、
  `test_worker_effect_bootstrap_component.py` 更新 (32 エントリ)、
  aex_string_table native selftest を freeform value / primary group /
  ambiguity fail-closed に拡張

# Issue #84 作業ノート: PiPL entrypoint discovery + AEX 一覧

時系列で追記する。観察(事実)と仮説(推論)を分ける。結論が覆っても古い項目は消さず訂正を追記。

## 背景 / 経緯 (2026-07-21)

- #84 (owner onmokoworks 起票) = 「PiPL Kind / CodeWin64X86 に基づく Effect entrypoint
  discovery」。scope に「複数 PiPL / 複数 Effect を曖昧化せず identity 付きで列挙・選択する」
  が含まれ、ユーザー要望の「AEX を一覧で見れるようにしたい」はこの範囲内。
- 実装は owner の PR #87 に存在したが **CLOSED (未マージ)**。base が非 main ブランチ
  `codex/issue4-one-command-runner` にスタックされ CONFLICTING、Codex がレビュー使用制限
  (2026-07-20) に到達。実装ブランチ `origin/codex/issue84-pipl-entrypoint` は残存。
- **main には #84 成果は未反映** (観察: `l2_main.cpp:1654` で `GetProcAddress(module,"EffectMain")`
  固定探索 + `EntryPointFunc` の有無から AEGP を推定する旧ロジックのまま)。
- naari3 セッションが引き継ぎ claim をポスト済み
  (https://github.com/onmokoworks/AEXCompat/issues/84#issuecomment-5033329495)。

## 方針決定 (ユーザー合意済み)

- 機械的 cherry-pick は**不成立**と実測で確認。理由: ブランチ側 `l2_main.cpp` は 25,210 行、
  現 main は 2,302 行で大規模 TU 分割済み。`wmain` もリファクタされ、trial cherry-pick では
  旧 `wmain` 全体 (約1340行) が丸ごと衝突として吐かれた。
- 採用: **ハイブリッド流用**。owner の audit 済み自己完結パーサー本体 (約240行) は verbatim 流用、
  統合点 (dispatch の呼び出し側) だけ現構造へ手作業で再配線。一覧用の静的パーサーは Python 側に新規追加。
- 出力: JSON + 人間可読テーブル。入力: ディレクトリ再帰 + 単体パス両対応。
- worktree: `C:/Users/naari/src/github.com/onmokoworks/AEXCompat-issue84`
  branch `issue84-pipl-entrypoint-discovery` (origin/main f3e70c9 起点)。

## 確定した Windows PiPL バイナリ形式 (事実)

SDK_Backwards.aex を経験的にダンプし、`AE_General.r` テンプレートと owner C++ の両方で裏取り。

```
ヘッダ 10 byte: [0..3] LE u32 version(=1)  [4..5]=0  [6..7] LE u16 count  [8..9]=0
各プロパティ:
  [0..3]   vendor  "MIB8"  (= '8BIM' を LE 格納 → ASCII 逆順)
  [4..7]   key     4byte   (OSType を LE 格納 → ASCII 逆順。例 'kind'→"dnik")
  [8..11]  propID  LE u32  (=0)
  [12..15] length  LE u32
  [16..]   data[length]  (次プロパティは 4-align 済み境界から。padding は length に含む場合と
                          別 padding の場合があるが offset は (offset-10) が 4 の倍数を保つ)
```

主なキー (canonical OSType / 逆順ディスク表記):
- kind (dnik): 'eFKT'(TKFe)=AEEffect, 'AEgx'(xgEA)=AEGP, '8BFM'=Filter 他
- CodeWin64X86 (8664→"4668"): entrypoint export 名 (cstring)
- CodeWin32X86 (wx86→"68xw"): 32bit entrypoint (cstring)
- name (eman): 表示名 (pstring)
- catg (gtac): カテゴリ (pstring)
- eMNA (ANMe): Match Name (pstring)
- eVER (REVe): AE_Effect_Version (packed u32)
- eSVR (RVSe): spec version (u16 major, u16 minor)
- ePVR (RVPe): PiPL version (u16 major, u16 minor)
- eGLO/eGL2: global out flags / out flags 2 (u32)
- eINF: info flags (u16)
- eURL: support URL (pstring)

eVER packed u32 decode (Paramarama 1081345 = 2.1 で検証済み):
- vers = (v >> 19) & 0x1FF, subvers = (v >> 15) & 0xF, bugvers = (v >> 11) & 0xF,
  stage = (v >> 9) & 0x3 (0=develop/1=alpha/2=beta/3=release), build = v & 0x1FF

SDK_Backwards.aex 実測値: name="SDK_Backwards", catg="Sample Plug-ins",
match="ADBE SDK_Backwards", CodeWin64X86="EffectMain", kind=eFKT(Effect)。

## 実装計画

- [ ] Phase B (Python 静的一覧, ユーザー主目的): `tools/aex_pipl_identity.py` (load せず実 PiPL
      バイトを bounded/fail-closed でパース、identity 抽出) + `tools/aex_list.py` (一覧 CLI, JSON+表)。
      既存 `aex_static_probe.py` のスキーマ/テストは壊さず PE ヘルパのみ再利用。
- [ ] Phase A (C++ worker discovery, #84 核): owner の parser 群を現 `l2_main.cpp` へ流用、
      dispatch 統合点 (`l2_main.cpp:1654`, `main.cpp:118-121`, `worker_aegp_init_report.cpp:133`)
      を `discover_pipl_entrypoint` へ再配線、export 名 AEGP 推定を廃止、fail-closed 分類、
      `--self-test-pipl-entrypoint` 復活。
- [ ] Phase C: self-authored fixture (小文字名/任意有効名/複数PiPL/kind-code不一致)、
      pytest、docs、実 5-AEX corpus 再実行 (worker ビルド要、AE 排他資源に注意)。

## Phase A 進捗ログ (2026-07-21)

- Phase B commit 済み (377f25f): `aex_pipl_identity.py` / `aex_list.py` /
  `test_aex_pipl_identity.py` (14 テスト green)。実 SDK ColorGrid/SDK_Backwards/Grabba で検証。
- Phase A: owner の audit 済みパーサー群 (`PiplPluginKind`〜`verify_pipl_entrypoint_parser`)
  を `l2_main.cpp` の `aexcompat::l2_detail` namespace (EffectEntry/AegpEntry typedef 直後) へ
  verbatim 流用。関数名衝突なしを grep 確認済み。必要 include (windows/array/cstring/limits/
  vector/string/algorithm/cstdint) 全て既存。
- dispatch 再配線: `l2_main.cpp` の worker_main_impl 内 `GetProcAddress(module,"EffectMain")`
  固定探索 + `EntryPointFunc` 有無からの AEGP 推定を `discover_pipl_entrypoint(module)` へ置換。
  Effect 以外は fail-closed (aegp_candidate / invalid_pipl / unknown_no_effect_entrypoint) で 12。
- self-test: `--self-test-pipl-entrypoint` を worker_main_impl 冒頭 (bootstrap 後、
  dispatch_worker_selftests 前) に owner 流の早期チェックとして追加 (同 TU なのでヘッダ不要)。
- **判断 (逸脱ではなく scope 限定)**: owner の #84 diff は main.cpp / worker_aegp_init_report.cpp
  を触っていない (diff stat は l2_main.cpp 中心) ため、当方も触らない。main.cpp:118-123 の
  EffectMain/EntryPointFunc 併記は L1 identify の既存挙動 (selector dispatch なし) で #84 の
  「export 名 AEGP 推定廃止」対象外。g_aegp_init_mode(1590) は明示 AEGP テストモードで据え置き。
  Rowbyte 系 (EntryPointFunc 名の Effect) の誤分類は effect dispatch path の PiPL 化で解消される。
- ビルド: `cl` 未ロードのため VS generator (VS 18 2026 / MSVC 19.51) で configure、
  canonical `target\minihost-build` と衝突させないよう `target\minihost-build-issue84` に
  aex_l2_worker を Release ビルド中。検証後に必要なら Ninja で canonical パスにも生成する。

## Phase A テスト移植と逸脱 (2026-07-21)

- `test_minihost_l2_source.py`: owner のソースマーカー検査メソッドを verbatim 追加。
  挿入コードが全マーカーを満たし、`reinterpret_cast<EffectEntry>(GetProcAddress(module,
  "EntryPointFunc"))` が不在であることを確認済み。single method + full source-text test pass。
- `test_pipl_entrypoint_discovery.py`: **逸脱** — owner 版は `assert worker.exists()` で
  未ビルド時にハード失敗するが、これは clean checkout での `uv run python -m pytest -q`
  (CLAUDE.md canonical verification) を壊す。machine-portable 方針に合わせ、worker 未ビルド時は
  `pytest.skip` に変更し worker ごとに parametrize。ビルドがあれば l2/render/smart の
  `--self-test-pipl-entrypoint` を実走検証、なければ skip。
- self-test 実走確認: `aex_l2_worker.exe --self-test-pipl-entrypoint` →
  `{"pipl_entrypoint":"passed",...}` exit 0 (VS 18 2026 / MSVC 19.51 ビルド)。
- **ビルドの落とし穴**: canonical Ninja ビルドで PATH に Strawberry Perl の
  `c++.exe` (MinGW g++) があると cmake -G Ninja がそれを CXX に誤選択する。worker は MSVC 前提
  なので `-DCMAKE_CXX_COMPILER=cl` を明示し、ninja は `-DCMAKE_MAKE_PROGRAM` で指定、
  vcvars64 を取り込んで cl を有効化する。canonical パス `target\minihost-build` に flat 配置で
  全 worker を生成 (discovery テストの探索先)。

## End-to-end 検証 (broker inspect, 2026-07-21)

broker harness (`aexcompat-harness`, release ビルド) の `--inspect-experimental` を
worktree の built worker (`target/minihost-build`) 経由で実行:
- ColorGrid.aex (Effect): PiPL 経由で EffectMain を発見し PARAMS_SETUP 実行、
  パラメータ ("Color Grid" arbitrary_data) 取得成功。→ 受け入れ基準3 (SDK Effect 経路維持)。
- Grabba.aex (Kind=AEGP): worker が `exit_code:12` / `plugin_kind:"aegp_candidate"` で
  fail-closed、selector stage 実行ゼロ。→ 受け入れ基準4 (AEGP を Effect selector に渡さない)。

注意: フィクスチャは main repo 側 `target/sdk-fixtures/` にあり worktree には無いので絶対パス指定。
harness の repository ルートは exe の 4 親 = worktree なので built worker を正しく解決する。

## クロージャ状況

- 実装 (Phase A/B) + 検証可能な受け入れ基準 (3/4) 完了。docs/PIPL_ENTRYPOINT_DISCOVERY_2026-07-21.md。
- **残る外部ゲート**: (a) OLM/Rowbyte corpus と 5-AEX matrix 再実行 (基準1/2/6) は実 corpus が
  checkout に無く不可、(b) マージは Codex レビュー (2026-07-20 制限到達) + owner レビュー必須。
- self-authored の loadable AEX fixture (小文字/任意名/複数PiPL/kind不一致) はパースロジックを
  C++ 合成 self-test + Python 14 テストで担保済み。実 .aex 化は SDK ビルドを要し未実施。

## ローカルエージェントレビュー ラウンド1 と対応 (2026-07-21)

CLAUDE.md の [[review-order-local-then-codex]] に従い、Codex より先にローカルエージェントレビューを
実施 (順序を誤って先に @codex を投げてしまったのを是正)。指摘と対応:

- **[Medium] #2 invalid_pipl の下流未接続**: 新設した `plugin_kind:invalid_pipl` を broker/gate が
  認識せず握り潰し。原因は **owner の下流配線移植漏れ** (l2_main.cpp だけ移植し、image_render.rs +1・
  run-conformance-bundle.py・schema・test の invalid_pipl 追加を落としていた)。→ 全て移植:
  `image_render.rs:414` の match、`run-conformance-bundle.py:527/591` の集合、
  `conformance-report.schema.json` の enum、`test_run_conformance_bundle.py` の parametrize。
- **[Medium] #1 Python パーサーが worker より緩い**: `_cstring` がシンボル検証をせず、不正シンボル
  (ハイフン等) の Effect を dispatchable と誤表示。→ `_valid_export_symbol` を worker と同ロジックで
  追加し、`entrypoint_win64_valid` / `dispatchable_effect` を導入。不正シンボルは effect に分類しない。
- **[Low] #4 分類セマンティクス乖離**: kind/code 重複・kind 欠落・64個超を worker は Invalid にするが
  Python は黙認/切り詰め。→ `_classify` を worker 忠実化 (kind_count!=1 や eFKT-without-valid-code は
  invalid_pipl)、64個超は analyze で invalid_pipl。宣言文も「worker が受理するものを正確に」→
  「fail-closed 判定の静的近似」に honest 化。
- **[Medium/要確認] #3 実 AEX load 経路未検証**: レビュアーは broker inspect 実行を知らなかった。
  実際は ColorGrid(Effect 到達)/Grabba(AEGP fail-closed) を `--inspect-experimental` で実証済み。
  非標準 real Effect (PiPL 非搭載等) の fail-closed は corpus-gated として doc に明記済み。コード変更不要。
- 検証: identity unit 17 passed、conformance 109 passed、broker cargo check clean、実フィクスチャ分類維持。

## 制約 / 注意

- 最終マージは CLAUDE.md 上 Codex レビューループ + owner レビューが前提。Codex は 2026-07-20 に
  制限到達のため、このセッションでは実装完了・PR オープン・テスト green まで到達し、マージは
  Codex 復帰後になる可能性がある。
- source-text テストは `tests/source_owners.py` 経由。l2_main.cpp から別 TU へ実装を移す場合は
  `WORKER_RUNTIME_OWNERS` 等に追記する (assert マーカーを緩めない)。
```

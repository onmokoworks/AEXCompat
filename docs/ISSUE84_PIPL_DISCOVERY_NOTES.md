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

## 制約 / 注意

- 最終マージは CLAUDE.md 上 Codex レビューループ + owner レビューが前提。Codex は 2026-07-20 に
  制限到達のため、このセッションでは実装完了・PR オープン・テスト green まで到達し、マージは
  Codex 復帰後になる可能性がある。
- source-text テストは `tests/source_owners.py` 経由。l2_main.cpp から別 TU へ実装を移す場合は
  `WORKER_RUNTIME_OWNERS` 等に追記する (assert マーカーを緩めない)。
```

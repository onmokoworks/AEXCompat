# Test suite inventory - 2026-08-04

CI 高速化 (#664, PR #669/#673/#676) の続きとして、suite 全体 (409 ファイル /
2366 テスト、当時) を棚卸しした記録。速度だけでなく「何を守っているか」「過剰
ではないか」を判定した。観察 (事実) と仮説 (推測) を分けて記す。

## 方法

- CI run の `--durations=25` と進捗時系列 (run 30905687284, 30907523486)
- ファイル別テスト数の分布 (`pytest --collect-only`)
- クラスタ別のコード読解と、tools/ 参照の全数 grep (歴史文書 3 点を除外)

## カテゴリ別の観察

| カテゴリ | 規模 | 判定 |
|---|---|---|
| no-load 文書パイプライン (tools/aex_*, aepx_* とその 1:1 テスト) | 155 ファイル / 358 テスト | 大半が歴史的遺物の孤島 → #680 で退役 (本コミット) |
| source-text (grep) 系 | 221/409 ファイルに散在、純 grep ~500 テスト | 実行コストは合計 3 秒未満。否定 assert (「l2_main にもう無い」) は代替手段が無く正当。肯定 marker grep は EVIDENCE_POLICY §5.3 の既知負債 |
| GPU/probe contract 系 | 5 ファイル / 235 テスト | 159 テスト中 108 が同一制約のクロス積 parametrize。producer (Rust serde) と schema が未接続という穴あり → #683 |
| conformance bundle | 112 テスト | 67 テストが subprocess 実起動。~23 件は関数直呼びに置換可能と思われる → #682 派生で検討 |
| public_export | 48 テスト | E2E 2 本が CI で計 ~3.3 分。入力は合成 2 コミットで、遅さは git spawn ~100 回/export の固定費 → #682 |
| codex-review-loop monitor | 87 テスト | 内容は実事故由来で本物だが、テストごとに bash 1 プロセス起動 (~11 秒) → #684 |
| Frida 観測系 (known_function_observation ほか) | 64 テスト | 一度「更新停止」と判定しかけたが訂正: issue #34 の RE/観測ツールで、CI から呼ばれず手動専用なのが設計どおり。観測は evidence に昇格しない境界も構造化済み。現状維持 |
| canonical worker self-test | 2 テスト + session fixture | minihost 全 123 TU のビルドが CI pytest の床 (~225 秒)。suite catalog の本番配線が検証対象なので薄いターゲットへの切り出しは検証価値を失う。ae-sdk-tests 側の二重ビルドは #681 |
| ネイティブ実行系・ライブ検証 (render_session, aegp, trace, pipl, macOS sweep 等) | 残り ~1000+ | 健全。特に test_trace_contracts / test_aex_pipl_identity / test_conformance_bundle_schema / test_windows_clean_clone_workflow は費用対効果が高い |

## no-load 島の退役 (#680、本コミット)

観察:

- tools/ の aex_*/aepx_* 81 本 + labctl のうち、テストと歴史文書
  (analysis/AEX_COMPAT_LAB_PLAN_2026-06-05.md, docs/PROJECT_DESIGN_2026-07-03.md,
  docs/IMPLEMENTATION_ROADMAP_2026-07-06.md) 以外に消費者を持つのは 8 本のみ
  (全数 grep で確認)
- 出自の設計書は repo 自身が superseded/historical と明記。
  EVIDENCE_POLICY_2026-07-18 に言及ゼロ。成果物 (*.local.json) は gitignore 下で
  commit 実績ゼロ
- テスト側は 46/48 ファイルが同一の path-confinement/create-new 検証の同型
  コピー、`load_tool()` ヘルパが 69 ファイルに逐語コピー。`target/` 直下に
  テスト残骸 JSON が 1,000 個超蓄積

残した現役 (それぞれ現行の参照アンカーあり):

- aex_pipl_identity, aex_list, aex_static_probe,
  aex_missing_suite_diagnostic_gate — PiPL entrypoint discovery
  (#84/#16, docs/ISSUE84_PIPL_DISCOVERY_NOTES.md,
  docs/PIPL_ENTRYPOINT_DISCOVERY_2026-07-21.md)
- aex_plugindata_probe — PluginDataEntryFunction 登録の live probe (#326/#380)
- aex_sweep_checkpoint — installed AEX sweep (#478/#486)
- aex_descriptor_manifest_promotion — contracts/aex の schema と
  docs/DESCRIPTOR_MANIFEST_PROMOTION_2026-07-13.md
- aex_parameter_value_gate — analysis/SCATTERMAP_AE_PARAM_BOUNDS_RESULT_2026-07-13.md

削除に伴う参照更新: tools/public_export.py の SHA allowlist から削除 2 ファイル分
のエントリを除去。tests/fixtures/aepx は消費者が test_aepx_edge_fixtures.py のみ
だったため併せて削除。削除テストは 4 つの依存マニフェスト
(local_artifact/sdk_required/prebuilt_required/built_artifact) のいずれにも
載っていないことを確認済み。

効果: 2366 → 2056 テスト、tools/tests 合計 ~38k LOC の保守負債解消。
CI wall-clock への効果はほぼ無い (削除分は直列合計 ~27 秒で並列に吸収されていた)。

## 残作業 (issue 化済み)

- #681 ae-sdk-tests.yml の source-reproducible テスト丸ごと再実行と minihost 二重ビルド
- #682 public_export E2E の統合 + tools 側 `cat-file --batch` 化
- #683 GPU contract のクロス積圧縮 + producer↔schema 接続
- #684 codex monitor のバッチ実行化

## 追記 (2026-08-04, #691)

上記「source-text (grep) 系」の判定を owner 判断で前倒しし、純 grep テスト
578 個を全廃した (suite 1966 → 1388)。本文の「否定 assert は正当」という
評価は owner 判断で覆された (訂正として残す)。削除には「secure launch に
fallback が無いこと」「session が唯一の image transport であること」等の
negative guard も含まれる。例外として残したのは test_probe_pipl_contract
(.rc↔.cpp の意味的整合) と test_windows_clean_clone_workflow (CI workflow
契約) の 2 ファイル。凍結 evidence とソースを突き合わせる混在テスト
(~50 call site) は evidence 側の検証なので残置し、source_owners.py も
それらのために残る。

## 追記 (2026-08-08, #944: レビュー運用の前提変更)

owner 指示でレビュー運用を変更した: レビューは PR を開く前のローカル
エージェントレビューループで完結させ、PR に bot レビュー (`@codex review`) は
投げない。merge のゲートは CI green と owner レビューのみ。これに伴い

- `.claude/skills/codex-review-loop/` (SKILL.md + bash 3 本)
- `tests/test_codex_review_loop_monitor.py` (上表の 87 テスト)
- `windows-clean-clone.yml` の `dev-infra` job と `-m "not dev_infra"` フィルタ
- `pytest.ini` の `dev_infra` marker

を削除した。#684 (2026-08-04 完了、dev-infra job への分離) が作った job も
テスト本体ごと消えたことになる。上表の「codex-review-loop monitor」行の判定
(「内容は実事故由来で本物」) はテストの中身についての評価としては訂正しないが、
そのテストが守っていた運用自体が無くなったため、行ごと退役として扱う。

## 仮説 (未検証)

- CI pytest の下限は canonical worker ビルド (~225 秒) で決まっており、#681-684
  を全部やっても pytest ステップは ~4 分前後で頭打ちの可能性が高い。それ以下を
  狙うなら fixture ビルドの高速化 (キャッシュは #651/#657 の stale worker 問題と
  衝突するため非推奨) か、テストの層の再設計になる

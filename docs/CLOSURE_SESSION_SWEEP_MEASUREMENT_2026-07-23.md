# クラスタセッション sweep 計測 (issue #405、2026-07-23)

設計: `docs/CLOSURE_SESSION_PROTOCOL_2026-07-23.md`。
計測条件: AE 2025 `Support Files\Plug-ins` 再帰 353 件、
`bridges/aviutl2-multifilter/examples/discover_sweep.rs`、
依存探索は `Support Files`、seal あり。

**注意**: before は外部の重いワークロードと並走した時間帯の計測で、
one-shot 側がやや過大に出ている可能性がある。after (cluster) は別時間帯の
再測。行単位の比較は構造的差分 (staging/ロード回数) として読むこと。

## 全体

| 実行 | 総時間 |
|---|---|
| before (one-shot、初回) | 1,352,318 ms |
| after 初回 (cluster、全クラスタ fallback) | 1,363,373 ms |
| **after 修正後 (cluster)** | **509,326 ms** (before 比 -62%、2.66x) |

初回 after は全 17 クラスタが `worker_exited` (request 0 未完了) で
フォールバックし高速化ゼロだった。原因は 2 件 (どちらも実環境でのみ
顕在化、fixture/スモークでは検出できなかった):

1. broker が渡す verbatim (`\\?\`) パスを worker の MSVC
   `std::filesystem::canonical` が拒否し manifest ロードが exit 3。
   → worker 側で `\\?\` / `\\?\UNC\` を剥がしてから canonical 照合。
2. discovery inspect 列 (one-shot 同等の SETDOWN 内包) と swap 列で
   GLOBAL_SETDOWN が二重発行され、実 AEX が非 0 を返し exit 25。
   → worker が setup/setdown の対状態を保持し、未対の setup がある
   場合だけ SETDOWN を発行。

修正後の最小再現 (ColorTexture.aex + Tint.aex、48 deps) は 1 セッションで
2 メンバー inspect 成功・fallback なし・audit clean。

## クラスタ単位 (修正後)

17 クラスタ・329 メンバーがクラスタセッション経路に乗った。代表:

| cluster | members | 初回 after (全 fallback) | 修正後 |
|---|---|---|---|
| 06421f2a (308 deps) | 18 | 367,891 ms | 24,669 ms (open 16,381) |
| ccae8d87 | 38 | 61,600 ms | 4,157 ms (open 1,201) |
| e3b0c442 (0 deps) | 128 | 63,327 ms | 12,715 ms (open 3,473) |
| 32d92620 | 54 | 29,210 ms | 2,739 ms (open 784) |

308 deps クラスタは open (staging + 308 DLL のロード + Adobe ランタイム
初期化) が 1 回に償却され、2 件目以降は swap のみ。

## bucket 変化 (before → 修正後 after、93/353 件)

全て説明可能で、成功への丸め込みはない:

- `exit_20 → cluster_inspect_selector_error` 29 件、`exit_12_* →
  cluster_inspect_entrypoint_unresolved` 31 件: 同一の失敗のセッション
  経路での再分類 (plugin-local エラーとしてセッション継続)。
- `module_audit_limit_exceeded → loaded` 27 件 + `→ cluster_inspect_*`
  3 件: 宣言集合による検証 (固定 128 上限の置換、セッション経路のみ)
  により大型 closure のプラグインが inspect に到達。**設計どおりの
  解錠** (#394 のセッション経路側)。
- `module_audit_failure → cluster_session_invalidated` 2 件、
  `crashed → cluster_session_invalidated` 1 件: セッションを殺した
  メンバー。fail-closed が発動し、当該メンバーは失敗記録・残メンバーは
  one-shot フォールバック (16 メンバーが `cluster_fallback` 診断つきで
  one-shot 処理)。

## 残課題

- 静かな条件での before/after 再測 (負荷並走のノイズ除去)。
- 3 件のセッション無効化メンバーの詳細 (one-shot でも module_audit
  失敗/クラッシュするプラグインであり、セッション固有の問題ではない
  見込みだが個別確認は未実施)。
- render クラスタセッション (open_cluster + swap_plugin) の実 AEX での
  検証は未実施 (bridge の render プール経由)。
- 生データ: `target/sweep-405/before.json` / `after.json` (初回) /
  `after2.json` (修正後)。いずれも gitignore 対象。

---

# 並列化 (#404) との組み合わせ計測 (2026-07-24、#419)

#404 の `--jobs` (既定 8 並列) が main に入った後の条件で、同じ 353 件を
連続実行して再計測した (merge 後の main に相当するビルド)。

| 条件 | 総時間 |
|---|---|
| 直列 one-shot (初回、負荷並走) | 1,352,318 ms |
| 直列 cluster (#405 のみ) | 509,326 ms |
| **--jobs 8 (並列のみ)** | **238,032 ms** |
| --cluster --jobs 8 (両方) | 350,079 ms |

所見:

- **8 並列下では jobs-only が最速**。cluster は直列では 2.66x 有効だが、
  並列と組み合わせると逆効力になりうる。
- 原因は構造的。クラスタ内は 1 worker で直列処理するため、128 / 54
  メンバーの大クラスタが 8 ワーカーに分散できず並列度を食い、末尾で
  他ワーカーが待つ tail effect も出る。308 deps のロード重複も、
  jobs-only 側は「重複するが並列に流せる」ため支配的でなくなる。
- cluster + jobs では 9 クラスタが fallback (セッション無効化 2 件)。
  負荷下のセッション安定性にも改善余地がある。
- 結果の一致度: jobs8 vs cluster+jobs8 の bucket 差 63 件中 59 件は
  構造的差分 (監査上限解錠 27 件 + セッション経路再分類 32 件)。
  成功への丸め込みはない。

示唆: cluster セッションが効くのは「closure ロードが支配的で並列度が
限られる」場面。大クラスタを分割して並列化と両立させる (cluster chunk
化) か、closure サイズに応じた適応的切替がないと、並列運用では
逆効果になり得る。対応方針は #419 で追う。

生データ: `target/sweep-405/jobs8.json` / `cluster_jobs8.json`
(gitignore 対象)。

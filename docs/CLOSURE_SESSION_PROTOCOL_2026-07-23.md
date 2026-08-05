# クラスタセッション プロトコル v2

status: issue #816 で in-place 専用へ更新。

## 目的

複数の AEX を一つの resident worker で discovery / render し、認証済み集合内の
plugin index だけをセッション中に切り替える。プラグインと依存 DLL は実パスから
ロードし、コピー、sealed load tree、closure manifest は作らない。

## 起動時の信頼境界

broker は起動前に次を確定する。

- 各プラグインの絶対パス、SHA-256、サイズ
- 1～16 個の絶対 dependency search directory
- plugin 数と module 観測上限
- render の場合は plugin ごとの optional swap payload

worker はロード前に対象プラグインを再ハッシュする。依存探索は
`SetDefaultDllDirectories` と broker が検証した `AddDllDirectory` の組み合わせに
限定し、PATH / CWD を探索根として使わない。ロード後の module audit は provenance
として記録し、staging の宣言集合による enforcement は行わない。

## cluster-manifest-v2

broker は `target/image-transport` に bounded JSON を作成し、
`--cluster-manifest-v2 <absolute-path>` で worker に渡す。

```json
{
  "schema": "cluster-manifest-v2",
  "plugins": [
    {"path": "C:\\Effects\\A.aex", "sha256": "<64hex>", "size": 1234}
  ],
  "search_dirs": ["C:\\Effects", "C:\\Adobe\\Support Files"],
  "module_bound": 400
}
```

manifest は 4 MiB 以下、plugin は 1～256 件、search directory は 1～16 件とする。
plugin path と search directory は absolute / non-empty でなければならない。
basename の case-insensitive collision、重複 path、invalid hash、zero size は拒否する。
render の positional plugin は `plugins[0]` の path と hash に一致しなければならない。

## セッションメッセージ

- `swap_plugin`: 認証済み plugin index を選択する。path や hash は運ばない。
- `inspect_plugin`: discovery 対象の認証済み plugin index を選択する。
- `render_frame`: 現在選択中の plugin で既存 render-session protocol を実行する。
- `finish`: 最終 report と観測済み module audit を返して終了する。

index 範囲外、応答 index 不一致、generation 不一致、worker death、deadline 超過は
構造化エラーとして fail closed にする。plugin 固有の setup / inspect 失敗は、その
plugin の失敗として記録し、セッション全体の transport failure と混同しない。

## 廃止した経路

issue #816 で以下を削除した。

- `cluster-manifest-v1`
- `SealedLoadTree` と sealed resource staging
- `AEXCOMPAT_MULTIFILTER_STAGED_DISCOVERY`
- `AEXCOMPAT_MULTIFILTER_STAGED_RENDER`
- sealed discovery/render session open request

旧環境変数を設定しても経路は変わらない。failure survey の dependency closure 解析は
診断情報の生成だけに残り、worker launch の入力にはならない。

## 検証

受入条件は、Rust broker の Windows target build、v2 manifest の source-contract test、
focused pytest、native worker の Release build/self-test で確認する。GitHub Actions が
billing 制限で動かない期間はローカル結果を正本とする。

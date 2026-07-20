# AEXCompat 作業規則

このファイルは、AEXCompat の作業を開始するすべてのエージェントが最初に読む運用規則である。目的は、互換性作業を一件ずつ確実に完了し、worktree・Issue・PR の分散を増やさないこと。

## 0. 作業開始時の必須確認

1. この `AGENTS.md` を読む。
2. `git status --short --branch` と `git worktree list` を確認する。
3. 主worktree (`codex/harness-live-preview-ui`) の既存変更は、閲覧以外の操作をしない。revert、reset、checkout、clean、上書きを禁止する。
4. 設定済みのremote (通常は `origin`) から最新を fetch し、GitHub の Issue/PR の現在状態を確認する。remoteが無い、または名前が異なる環境では fetch は best-effort とし (`git remote` で実際の名前を確認する)、GitHub側の現在状態の確認を優先する。過去の会話や古いcommitを正本にしない。
5. 同時に複数のIssue、PR、worktree、エージェントを進めない。ユーザーが明示的に並列作業を依頼した場合だけ例外とする。

## 1. Issue-first / claim-first

1. `OPEN` かつ未解決のIssueを一覧化する。
2. 既存のclaimコメント、assignee、関連PR、stacked baseを確認する。既に他の作業がclaimしているIssueには重複着手しない。
3. 対象を一件だけ選び、コードを触る前にIssueへ実際の改行を含むclaimコメントを投稿する。コメントには次を明記する。
   - 対象Issue番号
   - 変更範囲（scope）
   - 完了条件（focused test / build / review / CI）
   - 依存Issue/PRと、依存が未完なら待つこと
4. claim対象を変更する必要が出たら、元Issueのclaimを放置せず、コメントで撤回・理由・次の対象を記録してから切り替える。
5. claimしていないIssueの実装、勝手なfollow-up PR、Issueを閉じる操作をしない。

## 2. 一件一worktree・一件一PR

1. 通常は現在のmain/作業branchで作業してよい。ただし無関係な未コミット変更がある、危険なrebase/buildを伴う、別作業との同時実行が必要、またはユーザーが指定した場合は、`AEXCompat-issue<N>-<slug>` worktreeと `codex/issue<N>-<slug>` branchへ隔離する。
2. 既存のユーザー変更をrevert、reset、checkout、clean、上書きしない。
3. 一つのIssueに対する変更、テスト、docsを一つのPRにまとめる。無関係なリファクタや別Issueの修正を混ぜない。
4. claimしたIssueを実装したPRは必ず `Closes #N` とする。`Refs #M` は依存Issue・関連Issueにだけ使い、claim対象の代用にしない。PR本文は `\n` という文字列を本文生成に渡さず、実改行で書く。
5. stacked PRは、依存PRがmergeされるまで新しいstackを増やさない。不要な古いworktreeを作らない。

## 3. 実装と検証の順序

1. Issueの受入条件を先にファイル・テスト・実行コマンドへ落とす。
2. 最小変更を実装する。
3. focused pytest / source-contract testを実行する。
4. 影響するnative workerのRelease buildとself-testを実行する。
5. 可能ならRust workspace test、Python全体、実AEX/SDK試験を実行する。実行できないものは理由と未検証範囲を記録する。
6. 文字コードを環境任せにしない。UTF-8ソースを読むPython検証は、Windowsでは `PYTHONUTF8=1` を明示して再現性を確認する。
7. 出力、manifest、JSONはschema・重複キー・path/identity・cleanupを確認する。失敗をsuccessへ丸めない。

## 4. Review / CI / merge gate

1. PRの全review threadを取得し、`isResolved` とoutdatedを確認する。未解決のP1/P2、owner指摘、Codex指摘が一つでもあればmergeしない。
2. latest headに対するCodex reviewとowner reviewを確認する。古いheadのcleanを新headのcleanとみなさない。
3. GitHub Actionsがgreenになるまでmergeしない。課金制限、usage limit、runner不調などの外部障害はコードの成功と混同せず、明示的にblockedとして報告する。
4. CI失敗を修正する場合は、まずログとannotationで根因を確認し、承認された小さな修正だけを行う。
5. merge後に次のIssueへ進む。merge前の別Issue着手は禁止。

## 5. 定期的な棚卸し

1. 次のタイミングで、read-onlyでopen Issue/PRを再取得する。
   - 作業セッション開始時
   - 次のIssueをclaimする直前
   - PRのmerge/close後
   - 同じ作業が3ターン以上続いたとき、または外部blockerが変化したとき
2. 棚卸しでは、`active claim`、`open PR`、`blocked`、`unclaimed` を分類する。既存claimや関連PRが見つかったIssueを新たに着手しない。
3. 棚卸し結果は現在の対象Issueに影響する差分だけ報告し、一覧を理由に作業範囲を広げない。

## 6. 進捗報告の形式

各ターンの報告は短く、次の順序にする。

1. 現在の対象Issue / worktree / branch
2. 今回行った変更または確認
3. 実行した検証と結果
4. 未解決のレビュー・CI・外部blocker
5. 次に行う一手（対象Issueを増やさない）

作業を止めるときは、変更なし・未commit・未push・claim状態を明記する。ユーザーが方針変更を示したら、実装を続けず、まずこの規則に従って作業台帳を整理する。

## 7. リポジトリ固有状態の扱い

- Issue番号、PR番号、branch名、blocked理由などの時限情報をこのファイルに固定しない。
- 現在の正本は、棚卸し時点のGitHub状態とユーザーが指定した対象である。
- 主worktreeの既存未コミット変更を保全する必要がある場合は、開始時のstatusで確認し、必要なら隔離worktreeを選ぶ。

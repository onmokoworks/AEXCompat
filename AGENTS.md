# AEXCompat 作業規則

このファイルは、AEXCompat の作業を開始するすべてのエージェントが最初に読む運用規則である。目的は、互換性作業を一件ずつ確実に完了し、worktree・Issue・PR の分散を増やさないこと。

## 0. 作業開始時の必須確認

1. この `AGENTS.md` を読む。実行時の安全規範は `CLAUDE.md` の
   Execution Floor and Safety Rules が正本 (単層の床 + record 方針、
   issue #678)。enforcement (receipt / 事前ハッシュ一致 / fail-closed
   module audit) を新設・拡張しない。
2. `git status --short --branch` と `git worktree list` を確認する。
3. 主worktree (`git worktree list` の先頭に出る、自分がこの作業のために作成したのではない既存worktree) の既存変更は、閲覧以外の操作をしない。特定のbranch名で固定せず、その時点の `git worktree list` で判定する。revert、reset、checkout、clean、上書きを禁止する。
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

1. 通常は現在のmain/作業branchで作業してよい。ただし無関係な未コミット変更がある、危険なrebase/buildを伴う、別作業との同時実行が必要、またはユーザーが指定した場合は、`AEXCompat-issue<N>-<slug>` worktreeと `issue<N>-<slug>` branchへ隔離する。
2. 既存のユーザー変更をrevert、reset、checkout、clean、上書きしない。
3. 一つのIssueに対する変更、テスト、docsを一つのPRにまとめる。無関係なリファクタや別Issueの修正を混ぜない。
4. claimしたIssueを実装したPRは必ず `Closes #N` とする。`Refs #M` は依存Issue・関連Issueにだけ使い、claim対象の代用にしない。PR本文は `\n` という文字列を本文生成に渡さず、実改行で書く。
5. stacked PRは、依存PRがmergeされるまで新しいstackを増やさない。不要な古いworktreeを作らない。

## 3. 実装と検証の順序

1. Issueの受入条件を先にファイル・テスト・実行コマンドへ落とす。
2. 最小変更を実装する。
3. focused pytest / behavioral self-test / compiled ABI assertionを実行する。製品ソースを文字列検索するだけの新規テストは追加しない。
4. 影響するnative workerのRelease buildとself-testを実行する。
5. 可能ならRust workspace test、Python全体、実AEX/SDK試験を実行する。実行できないものは理由と未検証範囲を記録する。
6. 文字コードを環境任せにしない。UTF-8ソースを読むPython検証は、Windowsでは `PYTHONUTF8=1` を明示して再現性を確認する。
7. 出力、manifest、JSONはschema・重複キー・path/identity・cleanupを確認する。失敗をsuccessへ丸めない。

## 4. Review / CI / merge gate

1. レビューはPRを開く前にローカルで完結させる。作業diffにローカルのエージェントレビューをかけ、指摘に対応し、修正後のdiffへ再度かける。指摘は妥当性を自分で判断し、盲従しない。**受理した未対応の指摘がゼロ**になるまでこのループを抜けず、PRはループを抜けてから開く。却下した指摘は理由とともにPR本文へ書く。ローカルループはPRに痕跡を残さないので、これが無いとownerがレビューの有無を検証できない。
2. **PR headに載る変更は、すべて第1項のループを通っていなければならない**。ループはPRを開いた時点で終わらない。merge時点で未レビューの変更がheadに残っていてはならず、CI失敗の修正もowner指摘への対応も追加実装も同じ扱いである。
   - **レビュー済みの境界を記録する**。ループを抜けたらレビュー済みの状態をcommitし、そのHEAD SHAをPR本文 (却下指摘と同じ場所) に記録する。抜けるたびに更新する。初回のループはPRより前に回るので、その分はPRを開くときに本文へ書く。文脈に置くだけではセッションを跨いだ時点で未レビュー分を計算できなくなり、ownerも境界をheadと突き合わせて検証できない。
   - **レビュー対象は未レビューの変更全体**とし、commit単位で見ない。`git diff <記録したSHA>` (working tree込み) を対象にする。後のcommitが前の修正を巻き戻す類の相互作用は、commitを個別に見ても現れない。
   - **自分がheadを動かす操作 (push、force-push) の前に**、未レビュー分を通す。
   - **base branchの取り込みは二段で扱う**。操作の前に既存の未レビュー分を通し、操作で新たに生じたconflict解決の結果を操作の直後にもう一度通す。解決結果は操作するまで存在せず、事前レビューでは捕まえられない。取り込みで継承したbase branch側の既merge済みの内容はレビュー対象ではなく、二度目のパスはconflict解決の結果だけを見る。パスを抜けたら取り込み後のHEAD SHA (mergeならmerge commit) を記録し直す。更新しないと継承分が以後のdiffに残り続ける。
   - **conflict解決の読み方**。取り込みはrebaseよりmergeを優先する。merge commitのcombined diff (`git show <merge commit>`) はどちらの親にも無い行を `++` で出すので、自分が書き足した解決を分離できる (`--stat` を付けると分離が効かず継承分まで並ぶ)。ただし片側を無修正で採用した解決 (`--theirs` 等) は `--cc` がhunkごと省略するため何も表示されない。捨てた側まで見るには `git show --remerge-diff <merge commit>` を併せて使う。rebaseを使った場合、`git diff <記録したSHA>` にbase branchから継承した既merge済みの内容が混ざって分離できないので、conflictしたpathを控えて個別に読む。
   - **head側が先に動いた場合** (GitHub UI上のsuggestion適用など) は、`git pull --ff-only origin <branch>` で手元のbranchへ取り込んでから、気づいた直後に通す。`git fetch` はremote-tracking refを更新するだけでworking treeに入らず、`git diff <記録したSHA>` に現れない。fast-forwardできずabortしたら手元に未pushのcommitがあるので、`git merge origin/<branch>` で取り込み、base branchの取り込みと同じ二段で扱う。abortに気づかずdiffを見ると、取り込めていない変更を「未レビュー分なし」と誤読する。通すまでmergeへ進まない。
3. PRにbotレビューを要求しない。`@codex review` コメント、レビューループskill、そしてbotのverdictをmerge条件にするmerge guardは廃止済みで復活させない。廃止したのはbot verdictのゲートであって、guardが併せ持っていたhead拘束 (第5項) とownerの再確認 (第4項) は規律として残る。PR上でbotの判定を待つゲートは無く、PR側のゲートはCIとownerレビューだけである (第2項のローカルループはPR側のゲートではないが、免除されるわけでもない)。
4. PRの全review threadを取得し、`isResolved` とoutdatedを確認する。未解決のowner指摘が一つでもあればCIがgreenでもmergeしない。解決とは「返信した上でthreadをresolveした」ことを指す。返信threadを持たないownerコメント (top-levelコメント、本文付きCOMMENTEDレビュー) は、新規のtop-levelコメントで明示的にackして解決する。**後続のpushも後続のCI greenもowner指摘を解決しない**。着手前から存在する未対応のownerコメントも同様にmergeをblockする。merge直前にownerの新しいコメントが無いか再確認する。
5. 第4項のownerゲートを満たした上で、GitHub Actionsがlatest headでgreenになるまでmergeしない。古いheadのgreenを新headのgreenとみなさない。mergeは `gh pr merge <PR> --merge --delete-branch --match-head-commit <greenになったSHA>` で行い、確認からmerge呼び出しの間に入ったpushでmergeが失敗するようにする。失敗したらheadが動いているので、第2項へ戻る。課金制限、usage limit、runner不調などの外部障害はコードの成功と混同せず、明示的にblockedとして報告する。
6. CI失敗を修正する場合は、まずログとannotationで根因を確認し、claimコメントで宣言したscopeに収まる小さな修正だけを行う。修正も第2項の対象で、pushする前にローカルループを通す。
7. 新規・更新テストが製品ソースを読み、特定の識別子・コメント・式のsubstringだけをassertしている場合は、behavioral evidenceとして受理しない。凍結evidence・schema・workflow自体のcontract検査を例外とする場合はPR本文で理由を明示する。self-testが固定のsuccess JSONを返し、外側がそれを照合するだけでは不十分で、具体的な出力・状態遷移・失敗条件または代表的mutationを検出できることを確認する。
8. merge後に次のIssueへ進む。merge前の別Issue着手は禁止。

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

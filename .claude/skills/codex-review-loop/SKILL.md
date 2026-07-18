---
name: codex-review-loop
description: >-
  GitHub PR に対する Codex (chatgpt-codex-connector) の自動レビューループ。
  "@codex review" をコメント → Monitor で応答を待つ → 指摘があれば対応 commit・
  push・返信して再トリガー、"Didn't find any major issues" が来たら終了。
  repo owner (onmokoworks/naari3) のコメントは Codex より最優先で処理し、
  owner の未解決レビューがある間は絶対に merge しない。
  ユーザーが「レビューループ」「codex review を回す」「PR に codex レビュー」
  などに言及したら invoke する。clean かつ owner 指摘なしならそのまま merge する。
---

# Codex Review Loop

PR を Codex にレビューさせ、指摘ゼロになるまで対応を繰り返すループ。

## 最優先ルール: repo owner のコメント

repo owner (`onmokoworks` および `naari3`) が PR に残したレビュー / コメントは、
**Codex の応答より常に優先して処理する**。owner は Codex が見落とす指摘を出す
(実例: PR #50 で Codex が clean を返した commit に対し owner が P1×3 を指摘した)。

- 監視 (手順 2) では Codex だけでなく **owner の inline review コメント・
  issue コメントも同時に拾う**。owner のコメントが来たら、Codex の状態に
  かかわらずそれを最優先で対処する。
- **owner の未解決レビュー / 未対応の指摘が1件でもある間は merge しない**
  (Codex が clean でも)。owner 指摘に対応・返信し、再レビューを促してから
  でなければ merge に進まない。
- merge の直前に必ず「owner が新しいレビュー / コメントを出していないか」を
  もう一度確認する (Codex clean と owner レビューが数十秒差で前後することが
  ある。時系列で owner の最新コメントが未対応でないか見る)。

## 前提

- 対象 repo で Codex の GitHub 連携が有効 (PR に "@codex review" コメントで発火)
- `gh` CLI が認証済み
- 対象 PR 番号が分かっている (`gh pr view --json number` などで確認)

## ループ手順

### 0. PR body の issue リンク確認

トリガー前に PR body を確認し、対応する issue がある場合は `Closes #N` が
含まれているかチェックする。無ければ `gh pr edit {PR} --body-file -` で追記する
(claim 済み issue を実装した PR は必ず Closes でリンクする。対応 issue が
無い場合は関連 issue への `Refs #N` に留め、存在しない issue への Closes は
書かない)。

### 1. トリガー

**PR を開いた直後はトリガー不要**。PR 作成 (および draft の ready 化) で最初の
レビューが自動で走るので、`@codex review` はコメントせず、PR の `created_at` を
`since` として手順 2 の監視に入る。

**追 commit を push した後の再レビューのみ**明示トリガーが必要:

```bash
gh api -X POST repos/{owner}/{repo}/issues/{PR}/comments -f body="@codex review" -q '.created_at'
```

出力の `created_at` を必ず控える (次の監視の `since` になる。これを polling 開始時刻に
すると、monitor 起動前に届いた応答を取り逃す)。

### 2. 応答待ち (Monitor)

Monitor ツールで以下を張る ({owner}/{repo}/{PR}/{since} を置換)。
1イベントで exit する作りなので、通知が来たら monitor は終了している。

```bash
since="{since}"
while true; do
  sleep 30
  # 最優先: repo owner の inline review コメント (Codex より先に判定する)
  owner=$(gh api "repos/{owner}/{repo}/pulls/{PR}/comments" --jq ".[] | select(.created_at > \"$since\") | select(.user.login == \"onmokoworks\" or .user.login == \"naari3\") | \"OWNER-FINDING id=\(.id) \(.path):\(.line // .original_line) \(.body | split(\"\n\")[0])\"" 2>/dev/null || true)
  owner_issue=$(gh api "repos/{owner}/{repo}/issues/{PR}/comments" --jq ".[] | select(.created_at > \"$since\") | select(.user.login == \"onmokoworks\" or .user.login == \"naari3\") | select(.body | test(\"@codex review\") | not) | \"OWNER-COMMENT: \(.body | split(\"\n\")[0])\"" 2>/dev/null || true)
  if [ -n "$owner" ] || [ -n "$owner_issue" ]; then
    [ -n "$owner" ] && echo "$owner"
    [ -n "$owner_issue" ] && echo "$owner_issue"
    break
  fi
  clean=$(gh api "repos/{owner}/{repo}/issues/{PR}/comments" --jq ".[] | select(.created_at > \"$since\") | select(.user.login | startswith(\"chatgpt-codex-connector\")) | select(.body | test(\"Didn.t find any major issues\")) | \"CLEAN: \(.body | split(\"\n\")[0])\"" 2>/dev/null || true)
  if [ -n "$clean" ]; then echo "$clean"; break; fi
  review=$(gh api "repos/{owner}/{repo}/pulls/{PR}/reviews" --jq ".[] | select(.submitted_at > \"$since\") | select(.user.login | startswith(\"chatgpt-codex-connector\")) | \"REVIEW \(.state) at \(.submitted_at)\"" 2>/dev/null || true)
  if [ -n "$review" ]; then
    echo "$review"
    gh api "repos/{owner}/{repo}/pulls/{PR}/comments" --jq ".[] | select(.created_at > \"$since\") | select(.user.login | startswith(\"chatgpt-codex-connector\")) | \"FINDING id=\(.id) \(.path):\(.line // .original_line) \(.body | split(\"\n\")[0])\"" 2>/dev/null || true
    break
  fi
done
```

`persistent: true` で張り、応答は通常1〜3分で来る。owner コメントは Codex 判定
より前に評価するので、両者が同時に来ても owner が優先される。

### 3. 分岐

- **OWNER-FINDING / OWNER-COMMENT (repo owner の指摘・コメント)**: 最優先。
  1. Codex の状態に関係なく、まず owner の指摘に対応する
  2. 各指摘の妥当性を判断し (owner 指摘も盲従はしないが、Codex より重い)、
     妥当なら対応・commit・push、inline コメントには返信
  3. owner に再確認を促す (`@codex review` の再トリガーとは別に、owner の
     指摘へ返信して対応済みを明示する)。手順 1 に戻る
  4. **owner 指摘が未解決の間は merge しない**
- **FINDING (Codex 指摘あり)**:
  1. 各指摘の妥当性を自分で判断する (盲従しない。妥当でなければ理由を付けて返信のみ)
  2. 妥当な指摘に対応し、commit・push
  3. 各 inline コメントに対応内容を返信:
     `gh api -X POST repos/{owner}/{repo}/pulls/{PR}/comments/{comment_id}/replies -f body="対応済み (<sha>)。<内容>"`
  4. 手順 1 に戻る (新しい `since` で再トリガー)
- **CLEAN (Codex 指摘なし)**: **merge の前に owner レビュー・コメントを必ず確認する**。
  owner の指摘は3面に出る (inline review コメント / review 本体 / top-level PR
  コメント)。GitHub では PR も issue なので、top-level コメントは
  `issues/{PR}/comments` に出る。3面すべてを見る:
  ```bash
  gh api repos/{owner}/{repo}/pulls/{PR}/comments  --jq '.[] | select(.user.login=="onmokoworks" or .user.login=="naari3") | "\(.created_at) inline \(.path):\(.line): \(.body | split("\n")[0])"'
  gh api repos/{owner}/{repo}/pulls/{PR}/reviews   --jq '.[] | select(.user.login=="onmokoworks" or .user.login=="naari3") | "\(.submitted_at) review \(.state): \(.body | split("\n")[0])"'
  gh api repos/{owner}/{repo}/issues/{PR}/comments --jq '.[] | select(.user.login=="onmokoworks" or .user.login=="naari3") | select(.body | test("@codex review") | not) | "\(.created_at) comment: \(.body | split("\n")[0])"'
  ```
  owner の未対応レビュー / コメントがあれば **merge せず** OWNER-FINDING の
  手順へ。無ければ最新 commit への clean であることを確認して merge:
  `gh pr merge {PR} --merge --delete-branch`
  merge 後は main を pull し、結果を報告する。merge が保護ルール等で失敗した
  場合は無理に押し通さず、状態を報告してユーザーに委ねる。

## Codex の応答パターン (誤判定防止)

- **actor login は API 面で表記が揺れる**。`issues/{PR}/comments` と
  `pulls/{PR}/reviews` の REST は `chatgpt-codex-connector[bot]` を返すが、
  `gh pr view --json author` など別経路は `[bot]` なしの
  `chatgpt-codex-connector` を返す。**必ず `startswith("chatgpt-codex-connector")`
  で prefix 一致させる** (完全一致だと経路差で1件も拾えずループが止まる)。
  この判定は `tests/test_codex_review_loop_monitor.py` の fixture テストで固定。
- 受理直後: trigger コメントに 👀 reaction → **単なる ack。シグナルとして扱わない**
- 指摘あり: review (state=COMMENTED) + inline コメント。P1/P2/P3 バッジ付き
- 指摘なし: issue コメントで "Didn't find any major issues ..." (文面の後半は変動する。
  reaction ではなくこのコメントだけが clean の合図)

## 注意

- clean は「最新 commit に対する応答」であることを確認してから merge する
  (clean コメントの "Reviewed commit" SHA が HEAD と一致するか見る)
- monitor を張る前に、応答が既に届いていないか一度手動で確認する (polling の隙間対策)
- 通知はバッチで届くことがある。CLEAN と stream-end が同時に来ても正常
- 監視が 1 時間 (timeout) を超えたら Codex 側の不調を疑い、PR の画面を直接確認する

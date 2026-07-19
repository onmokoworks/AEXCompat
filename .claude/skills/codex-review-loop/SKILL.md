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

**PR 作成直後も、追 commit の push 後も、毎回明示的に `@codex review` する**:

```bash
gh api -X POST repos/{owner}/{repo}/issues/{PR}/comments -f body="@codex review" -q '.created_at'
```

出力の `created_at` を必ず控える (次の監視の `since` になる。これを polling 開始時刻に
すると、monitor 起動前に届いた応答を取り逃す)。

PR 作成で auto first-review も走るが、**findings ゼロのときは非 mergeable な
👍 reaction しか出さない** (SHA 拘束が無く merge-guard が受理しない。実例 PR #44)。
明示トリガーは clean のとき `Didn't find any major issues` の **SHA 拘束 text
clean** を出すので、最初から明示トリガーしておけば reaction 待ちや再トリガーの
往復を避け、そのまま merge 可能な verdict に到達できる。

### 2. 応答待ち (Monitor)

監視ロジックは同梱スクリプト `codex-review-monitor.sh` に実装されている
(選択述語は `codex-review-lib.sh`、両方 `tests/test_codex_review_loop_monitor.py`
で検証)。inline bash を手で貼らず、これを Monitor ツールで `persistent: true`
で張る (SKILL_DIR は `.claude/skills/codex-review-loop`):

```bash
bash {SKILL_DIR}/codex-review-monitor.sh {owner} {repo} {PR} "{since}"
```

1イベントで exit する。emit される terminal イベントと意味:

- `OWNER-FINDING` / `OWNER-REVIEW` / `OWNER-COMMENT` — owner の指摘 (最優先)。
  owner review は **state ベース**で判定する (bodyless な `CHANGES_REQUESTED`
  も blocker)。純粋な `@codex review` トリガーのみのコメントは除外し、トリガー
  句を含む実フィードバックは拾う。
- `CLEAN: codex clean for head <sha>` — **SHA 拘束の text clean**
  (`Didn't find any major issues` の issue コメントが現在の head SHA を参照)
  で、それより新しい finding も owner 活動も無い。これは merge-guard が受理する
  = そのまま merge 可能な verdict。
- `CLEAN-REACTION: codex +1 on PR body ...` — **PR 本体への bot の 👍 (+1)
  リアクション**による clean。auto first-review で findings ゼロのとき、Codex は
  text コメントを出さず本体に +1 を付けるだけ (実例 PR #44)。これだけを拾って
  1h タイムアウトを防ぐ。ただし +1 は SHA を持たず head 拘束が committer.date
  頼み (cherry-pick/古い日時の push で偽装余地) なので **merge-guard は受理しない**。
  受け取ったら `@codex review` を再トリガーして SHA 拘束の text clean を得てから
  merge する。
- `FINDING ...` — Codex の inline 指摘 (エラー本文は除外)。
- `TIMEOUT` — 1時間到達。Codex 不調を疑い PR を直接確認。

**Codex の error/onboarding メッセージ ("Something went wrong" / "Unknown
error" / "To use Codex") と一時的な API 失敗は terminal ではない**。error は
transient で、Codex は内部リトライして数秒〜数分後に本物の verdict を出す
(実測: 18:59/19:08 に error、19:16 に clean)。monitor はこれらで停止せず、
本物の review を待ち続ける (停止すると verdict を取り逃す)。fail-closed は
merge 側 (`codex-merge-guard.sh` が head 拘束 clean なしに merge を拒否) が担う。

### 3. 分岐

- **OWNER-\* (repo owner の指摘・コメント・変更要求)**: 最優先。
  1. Codex の状態に関係なく、まず owner の指摘に対応する
  2. 各指摘の妥当性を判断し (owner 指摘も盲従はしないが、Codex より重い)、
     妥当なら対応・commit・push、inline コメントには返信
  3. owner の指摘へ返信して対応済みを明示し、手順 1 に戻る (再トリガー)。
     inline 指摘は返信後に GitHub review thread を resolve する。guard は
     GraphQL `reviewThreads.isResolved` を解決状態の正とする。top-level コメント・bodied
     review への対応完了は、本文に **`[ack]` マーカーを含む** top-level
     コメントで明示する (マーカーなしのステータス報告は解決として扱われない)
  4. **owner の要求が未解決の間は merge しない** (CHANGES_REQUESTED は特に)
- **TIMEOUT**: 1時間 verdict が来なかった。Codex 不調を疑い PR を直接確認し、
  必要なら `@codex review` を再トリガーして監視を張り直す。
- **CLEAN-REACTION (bot +1 のみ、text clean 無し)**: Codex は指摘ゼロだが SHA
  拘束 clean が無い。手順 1 で毎回明示トリガーしていれば通常は text clean が来る
  ので、これは fallback (auto-review の reaction だけ先着した等)。merge-guard は
  受理しないので、`@codex review` を再トリガーして SHA 拘束の text clean を得てから
  CLEAN 分岐へ進む。owner 指摘が別途あればそちらを先に処理する。
- **FINDING (Codex 指摘あり)**:
  1. 各指摘の妥当性を自分で判断する (盲従しない。妥当でなければ理由を付けて返信のみ)
  2. 妥当な指摘に対応し、commit・push
  3. 各 inline コメントに対応内容を返信:
     `gh api -X POST repos/{owner}/{repo}/pulls/{PR}/comments/{comment_id}/replies -f body="[ack] 対応済み (<sha>)。<内容>"`
  4. 手順 1 に戻る (新しい `since` で再トリガー)
- **CLEAN (Codex 指摘なし)**: **merge は `codex-merge-guard.sh` 経由でのみ行う**。
  これが (a) owner blocker の不在 (owner_review_gate の CHANGES_REQUESTED、
  および **未解決の owner フィードバック**の不在。解決は**明示シグナルのみ**:
  push しても inline コメントの `.commit_id` は旧 commit に残るだけなので
  「commit が進んだ」ことは対応済みを意味せず、後続の Codex clean も owner
  フィードバックを解決しない。inline は GraphQL の review thread 単位で
  `isResolved == false` を block する (REST の返信時刻から解決を推測しない)。
  bodied COMMENTED review と top-level コメントは返信スレッドを持たないため、
  「セッションのより新しい **`[ack]` マーカー付き** top-level コメント」で解決
  する (マーカーなしのステータス報告は ack にならない。ループ開始前から存在する
  未対応 owner コメントも fail-closed に block する)。いずれも
  作者自身の後続 approve でも clear される (per-reviewer)。dismiss は API 上で
  実際の dismissal 時刻を取得できないため、時刻ベース clearance には使わない。self-block
  回避は最小限: セッション自身の認証 login (`gh api user`) の **`[ack]` 付き** inline 返信と
  **`[ack]` マーカー付き top-level コメント (解決シグナルそのもの) だけ**を
  blocker から除外し、同 login でもマーカーなし top-level コメント・非返信
  inline・bodied review は本物のフィードバックとして拾う (owner 認証トークンで
  回している時に人間の「merge不可」コメントが落ちないように))、
  (b) 現在の
  head SHA に拘束された Codex text clean を fail-closed で再確認し (PR 本体 👍
  だけの reaction clean は受理しない)、(c) `gh pr merge --match-head-commit
  <head>` で atomic に merge する
  (確認後に head が進めば merge は失敗する):
  ```bash
  bash {SKILL_DIR}/codex-merge-guard.sh {owner} {repo} {PR}
  ```
  `REFUSE: ...` が出たら merge せず、示された未解決項目へ戻る。成功したら main を
  pull し、結果を報告する。手で `gh pr merge` を直接叩かない (head 拘束と
  owner/clean 再確認を飛ばすため)。

## Codex の応答パターン (誤判定防止)

- **actor login は API 面で2形式ある**。`issues/{PR}/comments` と
  `pulls/{PR}/reviews` の REST は `chatgpt-codex-connector[bot]` を返すが、
  `gh pr view --json author` など別経路は `[bot]` なしの
  `chatgpt-codex-connector` を返す。**両方を exact allowlist で受理する**
  (`== "chatgpt-codex-connector" or == "chatgpt-codex-connector[bot]"`)。
  `startswith` の prefix 一致は `chatgpt-codex-connector-fake` のような
  spoof login を拾うので使わない。この判定は
  `tests/test_codex_review_loop_monitor.py` の fixture テストで固定
  (両形式を受理し、prefix spoof を拒否する)。
- 受理直後: trigger コメント/PR 本体に 👀 (eyes) reaction → **レビュー中の ack。
  clean シグナルではない** (完了時に除去される)
- 指摘あり: review (state=COMMENTED) + inline コメント。P1/P2/P3 バッジ付き
- 指摘なし (2系統):
  - **再トリガー後**: issue コメントで "Didn't find any major issues ..."
    (文面の後半は変動。`Reviewed commit` の SHA で head 拘束)
  - **auto first-review (findings ゼロ)**: text コメントは出ず、**PR 本体に
    bot の 👍 (+1) リアクション**だけ (`issues/{PR}/reactions` に
    `content=="+1"`, `user.login` が Codex bot)。実例 PR #44。monitor はこれを
    committer.date で best-effort に head 拘束し `CLEAN-REACTION` advisory を
    出す (1h timeout 回避)。SHA を持たず偽装余地があるので merge-guard は受理
    せず、再トリガーで text clean を得てから merge する

## 注意

- merge に使う clean は「最新 commit に対する SHA 拘束の応答」であること。
  merge-guard は text clean の "Reviewed commit" SHA が HEAD と一致するかを
  自動判定する。reaction clean (👍) は merge に使わない (再トリガーで text clean 化)
- monitor を張る前に、応答が既に届いていないか一度手動で確認する (polling の隙間対策)
- 通知はバッチで届くことがある。CLEAN と stream-end が同時に来ても正常
- 監視が 1 時間 (timeout) を超えたら Codex 側の不調を疑い、PR の画面を直接確認する

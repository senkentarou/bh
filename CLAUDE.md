# bh

<!-- task-flow -->
## タスク運用

タスクの真実源は `.ideal/tasks/*.md`（YAML front matter 付きの Markdown。Ideal が読み書きする）。
着手 / 提出 / 完了は skill で行う: `/task-start <ノートのパス|id>` → `/task-submit` → `/task-done`。

- **ゲート**（`/task-submit` が commit の前に通す）: `cargo fmt --check` / `cargo clippy --all-targets` / `cargo test`
- 依頼にノートのパスや id があれば**そのノートで**始める（新規作成しない）。
- `status` は操作に追従して書く（着手→`doing`、PR 作成→`review`、マージ後→`done`）。
  手で書いてよい例外は差し戻し（`review`→`doing`）だけ。
- `## Verification` は `review` に上げる前に素の箇条書きで書き、人間への依頼文はそれをそのまま提示する。
- 切り出した子タスクは front matter に `lineage: <元 id>` を書く（親側に子の id は書かない）。
  **未完了の子があるタスクを `done` にしない。**
- worktree は `.claude/worktrees/wt<n>` のプールから取り、完了時は消さず detach して最新の main に更新して返す。
<!-- /task-flow -->

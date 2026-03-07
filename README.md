# bh - Bash History Search

スマートランキング付きのインタラクティブ bash history 検索 CLI。

## Install

```bash
cargo install --path .
```

## Usage

```bash
# インタラクティブ TUI モード
bh

# テーブル形式で標準出力（サマリ統計付き）
bh --table

# JSON 形式で標準出力（AI 分析用）
bh --json

# 上位 N 件に絞って出力
bh --json -n 50
```

## Features

### インタラクティブ検索（TUI モード）

`bh` を引数なしで実行するとインタラクティブモードが起動する。

- 部分一致検索。大文字小文字を区別しない
- ヒットした文字列を黄色でハイライト表示
- 各コマンドの出現回数を左側に表示
- Enter で選択したコマンドを stdout に出力

#### キーバインド

| キー | 動作 |
|---|---|
| 文字入力 | インクリメンタル検索 |
| `↑` / `Ctrl-p` | カーソル上移動 |
| `↓` / `Ctrl-n` | カーソル下移動 |
| `Enter` | 選択コマンドを stdout に出力して終了 |
| `Backspace` | 1 文字削除 |
| `Ctrl-u` | クエリ全消去 |
| `Esc` / `Ctrl-c` | キャンセル終了 |

### スマートランキング

スコア = `(recency × 0.6 + log(1 + frequency) × 0.4) × noise_penalty`

| 要素 | 重み | 説明 |
|---|---|---|
| Recency | 0.6 | ファイル内の出現位置。最後に使ったものほど高スコア |
| Frequency | 0.4 | 出現回数の対数スケール。極端な偏りを抑制 |
| Noise penalty | ×0.3 | 1 回しか使っていないコマンドに適用。打ち間違い・one-shot コマンドを降格 |

### ノイズ適応フィルタ

検索結果の件数に応じてフィルタ閾値を動的に変更する。

| 条件 | 挙動 |
|---|---|
| クエリなし | 2 回以上使ったコマンドのみ表示 |
| 検索結果 ≤ 20 件 | 全件表示（低頻度コマンドも含む） |
| 検索結果 > 20 件 | 上位 1/3 のスコアの 50% を閾値として低スコアをカット |

### 標準出力エクスポート

AI による分析や、alias / skill 化の判断材料として使う。

#### Table 形式 (`--table`)

```
Rank   Freq     Command
------------------------------------------------------------
1      86       claude
2      50       cargo tauri dev
3      45       bun dev
...
Total unique commands: 150
Total executions: 1200
Single-use commands: 80 (53%)

Top 20 base commands:
   120x  bun
    96x  claude
    56x  cargo
```

#### JSON 形式 (`--json`)

```json
[
  {
    "command": "claude",
    "frequency": 86,
    "recency_rank": 0,
    "score": 2.385
  }
]
```

## Tech Stack

- Rust (2024 edition)
- [ratatui](https://github.com/ratatui/ratatui) - TUI フレームワーク
- [crossterm](https://github.com/crossterm-rs/crossterm) - ターミナルバックエンド
- [clap](https://github.com/clap-rs/clap) - CLI パーサ

## Data Source

`~/.bash_history` を読み取る（read-only）。ファイルへの書き込みは一切行わない。

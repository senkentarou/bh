# bh — Bash History Search

Fast, interactive bash history search with fuzzy matching and smart ranking.
A lightweight Rust TUI alternative to `Ctrl-R`.

## Install

### Pre-built binary (recommended)

Download the latest binary from [GitHub Releases](../../releases/latest) and place it in your `PATH`:

```bash
# Apple Silicon Mac
curl -sL https://github.com/senkentarou/bh/releases/latest/download/bh-v0.1.0-aarch64-apple-darwin.tar.gz | tar xz
sudo mv bh-v0.1.0-aarch64-apple-darwin/bh /usr/local/bin/

# Bypass macOS Gatekeeper warning if needed
xattr -cr /usr/local/bin/bh
```

### Build from source

```bash
cargo install --path .
```

## Usage

```bash
# Interactive TUI (executes selected command directly when stdout is a TTY)
bh

# Table output with summary stats
bh --table

# JSON output (useful for AI analysis)
bh --json

# Limit to top N entries
bh --json -n 50

# Usage statistics
bh stats
bh stats --json
bh stats --reset
```

### Shell integration (Ctrl-R)

When stdout is piped, bh prints the selected command to stdout. Add this to your `.bashrc` to use it as `Ctrl-R`:

```bash
bh-search() {
  local cmd
  cmd=$(bh)
  if [ -n "$cmd" ]; then
    READLINE_LINE="$cmd"
    READLINE_POINT=${#cmd}
  fi
}
bind -x '"\C-r": bh-search'
```

### Direct execution

When stdout is a TTY (i.e. running `bh` directly in the terminal), the selected command is executed via `execvp` using `$SHELL` (defaults to `/bin/bash`).

## Features

### Fuzzy matching

fzf-style fuzzy matching — query characters are matched in order but don't need to be contiguous. Exact substring matches always rank above fuzzy results.

```
gtp  → git push     (g·t · p matched in order)
gpl  → git pull     (g · p·l matched in order)
```

Match quality scoring:

| Factor | Bonus | Description |
|---|---|---|
| Consecutive | +8 | Matched characters are adjacent |
| Word boundary | +5 | Match after ` ` `/` `-` `_` `.` |
| String start | +6 | Match at position 0 |
| Tightness | +4 | Shorter span between first and last match |
| Early position | +3 | Match near the start of the command |
| Exact substring | +100 | Full substring match |

### Smart ranking

Score = `(recency × 0.6 + log(1 + frequency) × 0.4) × noise_penalty`

| Factor | Weight | Description |
|---|---|---|
| Recency | 0.6 | Recently used commands rank higher |
| Frequency | 0.4 | Log-scaled occurrence count to prevent extreme skew |
| Noise penalty | ×0.3 | Applied to single-use commands to demote typos and one-offs |

### Adaptive noise filter

| Condition | Behavior |
|---|---|
| No query | Show only commands used 2+ times |
| ≤ 20 results | Show all (including low-frequency) |
| > 20 results | Cut entries below 50% of the top-third score |

### Usage tracking

Create `~/.config/bh/config.toml` to enable usage tracking:

```bash
mkdir -p ~/.config/bh
touch ~/.config/bh/config.toml
```

An empty file enables the feature. To explicitly disable:

```toml
[stats]
enabled = false
```

Enabling it adds a selection count on the right of each history entry, and stale
detection for `C-g`. Data is stored in `~/.bh/stats.json`.

### Stale cleanup (C-g)

A command is stale when its frequency is <= 2, it was first seen more than 14 days
ago, and it has not been selected in the last 30 days — unless one of these applies:

| Exclusion | Condition |
|---|---|
| New | First seen within 1 day |
| Hot | Selections in last 7 days >= 5 and >= 2x previous week, or history frequency in top 5% with delta >= 3 |
| Top | #1 most selected command in the last 30 days |

`C-g` prompts to bulk-delete all stale commands. Confirms with `y/N` before deleting.

### Keybindings

Press `C-?` / `C-/` to show the help overlay.

| Key | Action |
|---|---|
| `↑` `C-p` `C-k` | Move selection up |
| `↓` `C-n` `C-j` | Move selection down |
| `C-u` | Half page up |
| `C-d` | Half page down |
| `Enter` | Select command |
| `Tab` | Toggle multi-select |
| `Shift-Tab` | Deselect + move up |
| `C-g` | Bulk delete stale |
| `C-x` | Delete selected |
| `Esc` `C-c` `C-q` | Quit / clear selection |
| `←` `→` | Move cursor |
| `C-a` `C-e` | Cursor to start/end |
| `C-l` | Delete to end of line |
| `C-?` `C-/` | Help |

### Export

#### Table (`--table`)

```
Rank   Freq     Command
------------------------------------------------------------
1      86       claude
2      50       cargo tauri dev
3      45       bun dev

Total unique commands: 150
Total executions: 1200
Single-use commands: 80 (53%)

Top 20 base commands:
   120x  bun
    96x  claude
    56x  cargo
```

#### JSON (`--json`)

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

## Release

```bash
# 1. Update version in Cargo.toml
# 2. Commit the change
# 3. Run:
./release.sh
```

## Data source

Reads `~/.bash_history`. Write operations are `C-x` (delete selected) and `C-g` (bulk delete stale).

## Tech stack

- Rust (2024 edition)
- [crossterm](https://github.com/crossterm-rs/crossterm) — terminal control
- [clap](https://github.com/clap-rs/clap) — CLI argument parsing
- No async runtime, no TUI framework — raw escape sequences for minimal overhead

## License

MIT

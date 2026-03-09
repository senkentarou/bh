# bh — Bash History Search

Fast, interactive bash history search with frequency-weighted smart ranking.
A lightweight Rust TUI alternative to `Ctrl-R`.

## Install

### Pre-built binary (recommended)

Download the latest binary from [GitHub Releases](../../releases/latest) and place it in your `PATH`:

```bash
# Apple Silicon Mac
curl -sL https://github.com/senkentarou/bh/releases/latest/download/bh-v0.1.0-aarch64-apple-darwin.tar.gz | tar xz
sudo mv bh-v0.1.0-aarch64-apple-darwin/bh /usr/local/bin/

# macOS Gatekeeper の警告が出る場合
xattr -d com.apple.quarantine /usr/local/bin/bh
```

### Build from source

```bash
cargo install --path .
```

## Release

```bash
# 1. Update version in Cargo.toml
# 2. Commit the change
# 3. Run:
./release.sh
```

This tags the current commit with the version from `Cargo.toml` and pushes to GitHub, triggering the CI to build and publish binaries.

## Usage

```bash
# Interactive TUI mode
bh

# Table output with summary stats
bh --table

# JSON output (useful for AI analysis)
bh --json

# Limit to top N entries
bh --json -n 50
```

### Shell integration

Add to your `.bashrc` to bind `bh` to `Ctrl-R`:

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

## Features

### Interactive search (TUI)

Run `bh` with no arguments to launch the interactive mode.

- Incremental case-insensitive substring search
- Match highlighting
- Frequency count displayed per command
- `Enter` outputs the selected command to stdout

Press `C-?` / `C-/` to show the full keybinding list.

### Smart ranking

Score = `(recency × 0.6 + log(1 + frequency) × 0.4) × noise_penalty`

| Factor | Weight | Description |
|---|---|---|
| Recency | 0.6 | Position in history file — recently used commands rank higher |
| Frequency | 0.4 | Log-scaled occurrence count to prevent extreme skew |
| Noise penalty | ×0.3 | Applied to single-use commands to demote typos and one-offs |

### Adaptive noise filter

The filter threshold adjusts dynamically based on result count:

| Condition | Behavior |
|---|---|
| No query | Show only commands used 2+ times |
| ≤ 20 results | Show all (including low-frequency) |
| > 20 results | Cut entries below 50% of the top-third score |

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

## Data source

Reads `~/.bash_history` (read-only). The only write operation is `C-x` (delete entry), which removes the selected command from both memory and the history file.

## Tech stack

- Rust (2024 edition)
- [crossterm](https://github.com/crossterm-rs/crossterm) — terminal control
- [clap](https://github.com/clap-rs/clap) — CLI argument parsing
- No async runtime, no TUI framework — raw escape sequences for minimal overhead

## License

MIT

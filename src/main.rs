mod export;
mod history;
mod input;
mod labels;
mod stats;
mod stats_display;
mod tui;

use std::collections::HashMap;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "bh", about = "Interactive bash history search with smart ranking")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Output as JSON
    #[arg(long, conflicts_with = "table")]
    json: bool,

    /// Output as table with summary stats
    #[arg(long, conflicts_with = "json")]
    table: bool,

    /// Maximum number of entries (only with --json or --table)
    #[arg(short = 'n', long)]
    limit: Option<usize>,
}

#[derive(Subcommand)]
enum Command {
    /// Show usage statistics
    Stats {
        /// Reset stats by deleting ~/.bh/stats.json
        #[arg(long)]
        reset: bool,

        /// Output stats as JSON
        #[arg(long)]
        json: bool,
    },
}

fn stats_enabled() -> bool {
    std::env::var("BH_STATS")
        .map(|v| v == "1")
        .unwrap_or(false)
}

fn main() {
    let cli = Cli::parse();

    // Handle stats subcommand
    if let Some(Command::Stats { reset, json }) = cli.command {
        if reset {
            let path = stats::stats_path();
            if path.exists() {
                if let Err(e) = std::fs::remove_file(&path) {
                    eprintln!("Failed to delete {}: {e}", path.display());
                    std::process::exit(1);
                }
                eprintln!("Stats reset: deleted {}", path.display());
            } else {
                eprintln!("No stats file found at {}", path.display());
            }
            return;
        }

        let stats_data = stats::load_stats();
        if json {
            stats_display::print_stats_json(&stats_data);
        } else {
            stats_display::print_stats_text(&stats_data);
        }
        return;
    }

    let entries = history::load_history();

    if cli.json || cli.table {
        let entries = if let Some(n) = cli.limit {
            entries.into_iter().take(n).collect()
        } else {
            entries
        };
        if cli.json {
            export::export_json(&entries);
        } else {
            export::export_table(&entries);
        }
    } else if stats_enabled() {
        let mut stats_data = stats::load_stats();
        let commands: Vec<String> = entries.iter().map(|e| e.command.clone()).collect();
        stats::update_first_seen(&mut stats_data, &commands);
        stats::update_snapshot(&mut stats_data, &entries);
        stats::prune_old_data(&mut stats_data);
        let label_map = labels::compute_labels(&stats_data);

        match tui::run(entries, Some(&mut stats_data), &label_map) {
            Ok(Some(cmd)) => {
                stats::save_stats(&stats_data);
                println!("{cmd}");
            }
            Ok(None) => {
                stats::save_stats(&stats_data);
            }
            Err(e) => {
                stats::save_stats(&stats_data);
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
    } else {
        let empty_labels = HashMap::new();
        match tui::run(entries, None, &empty_labels) {
            Ok(Some(cmd)) => println!("{cmd}"),
            Ok(None) => {}
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
    }
}

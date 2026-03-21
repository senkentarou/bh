mod config;
mod export;
mod history;
mod input;
mod labels;
mod stats;
mod stats_display;
mod tui;

use std::collections::HashMap;
use std::os::unix::io::AsRawFd;

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

fn main() {
    let cli = Cli::parse();
    let config = config::load_config();
    let stats_enabled = config.as_ref().is_some_and(|c| c.stats.enabled);

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
    } else {
        let selected = if stats_enabled {
            let mut stats_data = stats::load_stats();
            let commands: Vec<String> = entries.iter().map(|e| e.command.clone()).collect();
            stats::update_first_seen(&mut stats_data, &commands);
            stats::update_snapshot(&mut stats_data, &entries);
            stats::prune_old_data(&mut stats_data);
            let label_map = labels::compute_labels(&stats_data);

            let result = tui::run(entries, Some(&mut stats_data), &label_map);
            stats::save_stats(&stats_data);
            result
        } else {
            let empty_labels = HashMap::new();
            tui::run(entries, None, &empty_labels)
        };

        match selected {
            Ok(Some(cmd)) => output_command(&cmd),
            Ok(None) => {}
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
    }
}

fn stdout_is_tty() -> bool {
    unsafe { libc::isatty(std::io::stdout().as_raw_fd()) != 0 }
}

fn output_command(cmd: &str) {
    if stdout_is_tty() {
        // Running directly — execute the selected command
        use std::ffi::CString;
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let c_shell = CString::new(shell.as_str()).expect("invalid SHELL");
        let c_flag = CString::new("-c").expect("CString");
        let c_cmd = CString::new(cmd).expect("invalid command");
        unsafe {
            libc::execvp(
                c_shell.as_ptr(),
                [c_shell.as_ptr(), c_flag.as_ptr(), c_cmd.as_ptr(), std::ptr::null()].as_ptr(),
            );
        }
        // execvp only returns on error
        eprintln!("Failed to execute: {cmd}");
        std::process::exit(1);
    } else {
        // Piped (e.g., Ctrl-R shell integration) — output to stdout
        println!("{cmd}");
    }
}

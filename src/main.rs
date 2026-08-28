mod config;
mod history;
mod input;
mod labels;
mod stats;
mod tui;

use std::collections::HashMap;
use std::os::unix::io::AsRawFd;

const USAGE: &str = "bh — Interactive bash history search with smart ranking

Usage: bh [OPTIONS]

Options:
  -v, --version  Print version
  -h, --help     Print help";

fn main() {
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-v" | "--version" => {
                println!("bh {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            _ => {
                eprintln!("bh: unrecognized argument '{arg}'\n\n{USAGE}");
                std::process::exit(2);
            }
        }
    }

    let config = config::load_config();
    let stats_enabled = config.as_ref().is_some_and(|c| c.stats.enabled);

    let entries = history::load_history();

    let selected = if stats_enabled {
        let mut stats_data = stats::load_stats();
        let commands: Vec<String> = entries.iter().map(|e| e.command.clone()).collect();
        stats::update_first_seen(&mut stats_data, &commands);
        stats::update_snapshot(&mut stats_data, &entries);
        stats::prune_old_data(&mut stats_data);
        let label_map = labels::compute_labels_with(&stats_data, &entries);
        let selection_counts = stats::total_selections(&stats_data.daily_selections);

        let result = tui::run(entries, Some(&mut stats_data), &label_map, &selection_counts);
        stats::save_stats(&stats_data);
        result
    } else {
        let empty_labels = HashMap::new();
        let empty_counts = HashMap::new();
        tui::run(entries, None, &empty_labels, &empty_counts)
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

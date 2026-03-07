mod export;
mod history;
mod input;
mod tui;

use clap::Parser;

#[derive(Parser)]
#[command(name = "bh", about = "Interactive bash history search with smart ranking")]
struct Cli {
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

fn main() {
    let cli = Cli::parse();

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
        match tui::run(entries) {
            Ok(Some(cmd)) => println!("{cmd}"),
            Ok(None) => {}
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
    }
}

use crate::history::HistoryEntry;

/// Export history as JSON to stdout for AI analysis.
pub fn export_json(entries: &[HistoryEntry]) {
    let output = serde_json::to_string_pretty(entries).expect("Failed to serialize");
    println!("{output}");
}

/// Export as human-readable table to stdout.
pub fn export_table(entries: &[HistoryEntry]) {
    println!("{:<6} {:<8} Command", "Rank", "Freq");
    println!("{}", "-".repeat(60));
    for (i, entry) in entries.iter().enumerate() {
        println!("{:<6} {:<8} {}", i + 1, entry.frequency, entry.command);
    }
    println!("\nTotal unique commands: {}", entries.len());

    // Summary stats
    let total_freq: usize = entries.iter().map(|e| e.frequency).sum();
    let single_use = entries.iter().filter(|e| e.frequency == 1).count();
    println!("Total executions: {total_freq}");
    println!(
        "Single-use commands: {single_use} ({:.0}%)",
        single_use as f64 / entries.len() as f64 * 100.0
    );

    // Top base commands (first word)
    let mut base_cmds: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for entry in entries {
        if let Some(base) = entry.command.split_whitespace().next() {
            *base_cmds.entry(base.to_string()).or_default() += entry.frequency;
        }
    }
    let mut base_sorted: Vec<_> = base_cmds.into_iter().collect();
    base_sorted.sort_by(|a, b| b.1.cmp(&a.1));

    println!("\nTop 20 base commands:");
    for (cmd, count) in base_sorted.iter().take(20) {
        println!("  {count:>6}x  {cmd}");
    }
}

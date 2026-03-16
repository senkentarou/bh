use crate::stats::{self, Stats};

pub fn print_stats_text(stats_data: &Stats) {
    let today = stats::today_str();
    let from_30d = stats::days_ago_from(&today, 30).unwrap_or_default();

    // Find earliest date in data
    let earliest = stats_data
        .daily_selections
        .keys()
        .chain(stats_data.first_seen.values())
        .min()
        .cloned()
        .unwrap_or_else(|| today.clone());

    let days_tracked = stats::days_since(&earliest).unwrap_or(0);

    println!("\u{1f4ca} bh stats (since {earliest}, {days_tracked} days)\n");

    // Top selected (last 30 days)
    let all_cmds = stats::all_selected_commands(&stats_data.daily_selections);
    let mut cmd_counts: Vec<(String, u32)> = all_cmds
        .iter()
        .map(|cmd| {
            let count = stats::selections_in_range(
                &stats_data.daily_selections,
                cmd,
                &from_30d,
                &today,
            );
            (cmd.clone(), count)
        })
        .filter(|(_, count)| *count > 0)
        .collect();
    cmd_counts.sort_by(|a, b| b.1.cmp(&a.1));

    // Compute labels for display
    let labels = crate::labels::compute_labels(stats_data);

    if !cmd_counts.is_empty() {
        println!("Top selected (last 30 days):");
        for (i, (cmd, count)) in cmd_counts.iter().take(5).enumerate() {
            let label = labels
                .get(cmd)
                .map(|l| format!("  {}", l.display()))
                .unwrap_or_default();
            println!("  {}. {:<30} \u{00d7}{}{}", i + 1, cmd, count, label);
        }
        println!();
    }

    // Trending (last 7 days vs prev 7 days)
    let from_7d = stats::days_ago_from(&today, 7).unwrap_or_default();
    let from_14d = stats::days_ago_from(&today, 14).unwrap_or_default();
    let prev_7d_end = stats::days_ago_from(&today, 8).unwrap_or_default();

    let mut trending: Vec<(String, u32, u32)> = Vec::new();
    for cmd in &all_cmds {
        let recent = stats::selections_in_range(
            &stats_data.daily_selections,
            cmd,
            &from_7d,
            &today,
        );
        let prev = stats::selections_in_range(
            &stats_data.daily_selections,
            cmd,
            &from_14d,
            &prev_7d_end,
        );
        if recent > prev && prev > 0 {
            trending.push((cmd.clone(), prev, recent));
        }
    }
    trending.sort_by(|a, b| {
        let a_pct = (a.2 as f64 - a.1 as f64) / a.1 as f64;
        let b_pct = (b.2 as f64 - b.1 as f64) / b.1 as f64;
        b_pct
            .partial_cmp(&a_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if !trending.is_empty() {
        println!("Trending (last 7 days vs prev 7 days):");
        for (cmd, prev, recent) in trending.iter().take(5) {
            let pct = (*recent as f64 - *prev as f64) / *prev as f64 * 100.0;
            let label = labels
                .get(cmd)
                .map(|l| format!("  {}", l.display()))
                .unwrap_or_default();
            println!(
                "  {:<30} \u{00d7}{} \u{2192} \u{00d7}{}  (+{:.0}%){}",
                cmd, prev, recent, pct, label
            );
        }
        println!();
    }

    // New commands (last 3 days)
    let new_cmds: Vec<&String> = stats_data
        .first_seen
        .iter()
        .filter(|(_, date)| stats::days_since(date).is_some_and(|d| d <= 3))
        .map(|(cmd, _)| cmd)
        .collect();

    if !new_cmds.is_empty() {
        println!("New commands (last 3 days):");
        for cmd in &new_cmds {
            println!("  {:<30} NEW", cmd);
        }
        println!();
    }

    // Totals
    let total_commands = stats_data.first_seen.len();
    let total_selections: u32 = stats_data
        .daily_selections
        .values()
        .flat_map(|day| day.values())
        .sum();
    println!(
        "Total: {} commands tracked, {} selections recorded",
        total_commands, total_selections
    );
    println!("Data: {}", stats::stats_path().display());
}

pub fn print_stats_json(stats_data: &Stats) {
    let today = stats::today_str();
    let from_30d = stats::days_ago_from(&today, 30).unwrap_or_default();

    let all_cmds = stats::all_selected_commands(&stats_data.daily_selections);
    let mut top_selected: Vec<serde_json::Value> = all_cmds
        .iter()
        .map(|cmd| {
            let count = stats::selections_in_range(
                &stats_data.daily_selections,
                cmd,
                &from_30d,
                &today,
            );
            serde_json::json!({ "command": cmd, "selections_30d": count })
        })
        .filter(|v| v["selections_30d"].as_u64().unwrap_or(0) > 0)
        .collect();
    top_selected.sort_by(|a, b| {
        b["selections_30d"]
            .as_u64()
            .cmp(&a["selections_30d"].as_u64())
    });

    let new_cmds: Vec<&String> = stats_data
        .first_seen
        .iter()
        .filter(|(_, date)| stats::days_since(date).is_some_and(|d| d <= 3))
        .map(|(cmd, _)| cmd)
        .collect();

    let total_commands = stats_data.first_seen.len();
    let total_selections: u32 = stats_data
        .daily_selections
        .values()
        .flat_map(|day| day.values())
        .sum();

    let output = serde_json::json!({
        "top_selected": top_selected,
        "new_commands": new_cmds,
        "total_commands": total_commands,
        "total_selections": total_selections,
        "data_path": stats::stats_path().display().to_string(),
    });

    println!(
        "{}",
        serde_json::to_string_pretty(&output).expect("Failed to serialize")
    );
}

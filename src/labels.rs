use std::collections::HashMap;

use crate::stats::{self, Stats};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Label {
    Stale,
    New,
    Hot { count: u32 },
    Top { count: u32 },
}

impl Label {
    pub fn display(&self) -> String {
        match self {
            Label::Stale => "💤".to_string(),
            Label::New => "✅".to_string(),
            Label::Hot { count } => format!("🔥{count}"),
            Label::Top { count } => format!("⭐{count}"),
        }
    }

    pub fn is_stale(&self) -> bool {
        matches!(self, Label::Stale)
    }
}

pub fn compute_labels(stats_data: &Stats) -> HashMap<String, Label> {
    compute_labels_with(stats_data, &[])
}

pub fn compute_labels_with(
    stats_data: &Stats,
    entries: &[crate::history::HistoryEntry],
) -> HashMap<String, Label> {
    let mut labels: HashMap<String, Label> = HashMap::new();
    let today = stats::today_str();

    // 0. STALE: frequency <= 2, first_seen > 14 days ago, not selected in last 30 days
    let from_30d = stats::days_ago_from(&today, 30).unwrap_or_default();
    for entry in entries {
        if entry.frequency > 2 {
            continue;
        }
        // Must have been seen > 14 days ago
        let first_seen_old = stats_data
            .first_seen
            .get(&entry.command)
            .and_then(|d| stats::days_since(d))
            .is_some_and(|days| days > 14);
        if !first_seen_old {
            continue;
        }
        // Must not have been selected in last 30 days
        let recent_selections = stats::selections_in_range(
            &stats_data.daily_selections,
            &entry.command,
            &from_30d,
            &today,
        );
        if recent_selections == 0 {
            labels.insert(entry.command.clone(), Label::Stale);
        }
    }

    // 1. TOP: #1 by selection count in last 30 days (overwrites Stale)
    let all_cmds = stats::all_selected_commands(&stats_data.daily_selections);
    let mut cmd_counts: Vec<(String, u32)> = all_cmds
        .iter()
        .map(|cmd| {
            let count =
                stats::selections_in_range(&stats_data.daily_selections, cmd, &from_30d, &today);
            (cmd.clone(), count)
        })
        .filter(|(_, count)| *count > 0)
        .collect();
    cmd_counts.sort_by(|a, b| b.1.cmp(&a.1));

    if let Some((cmd, count)) = cmd_counts.first() {
        labels.insert(cmd.clone(), Label::Top { count: *count });
    }

    // 2. NEW: first_seen within 1 day
    for (cmd, date) in &stats_data.first_seen {
        if let Some(days) = stats::days_since(date) {
            if days <= 1 {
                labels.insert(cmd.clone(), Label::New);
            }
        }
    }

    // 3. HOT (highest priority — overwrites NEW and TOP)
    let from_7d = stats::days_ago_from(&today, 7).unwrap_or_default();
    let from_14d = stats::days_ago_from(&today, 14).unwrap_or_default();
    let prev_7d_end = stats::days_ago_from(&today, 8).unwrap_or_default();

    // HOT condition 1: selection trend (>= 5 in recent 7d, >= 2x previous 7d)
    for cmd in &all_cmds {
        let recent =
            stats::selections_in_range(&stats_data.daily_selections, cmd, &from_7d, &today);
        let prev =
            stats::selections_in_range(&stats_data.daily_selections, cmd, &from_14d, &prev_7d_end);
        if recent >= 5 && recent >= prev * 2 {
            labels.insert(cmd.clone(), Label::Hot { count: recent });
        }
    }

    // HOT condition 2: bash_history frequency spike (top 5%, increase >= 3)
    if let (Some(curr), Some(prev)) =
        (&stats_data.history_snapshot, &stats_data.prev_snapshot)
    {
        let mut increases: Vec<(String, usize)> = Vec::new();
        for (cmd, &curr_freq) in &curr.frequencies {
            let prev_freq = prev.frequencies.get(cmd).copied().unwrap_or(0);
            if curr_freq > prev_freq {
                increases.push((cmd.clone(), curr_freq - prev_freq));
            }
        }

        if !increases.is_empty() {
            increases.sort_by(|a, b| b.1.cmp(&a.1));
            let top_idx = (increases.len() as f64 * 0.05).ceil() as usize;
            let threshold_increase = increases
                .get(top_idx.saturating_sub(1))
                .map(|(_, inc)| *inc)
                .unwrap_or(usize::MAX);

            for (cmd, inc) in &increases {
                if *inc >= 3 && *inc >= threshold_increase {
                    labels.insert(cmd.clone(), Label::Hot { count: *inc as u32 });
                }
            }
        }
    }

    labels
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::{Snapshot, Stats};

    fn make_stats() -> Stats {
        Stats::default()
    }

    #[test]
    fn test_empty_stats_no_labels() {
        let stats = make_stats();
        let labels = compute_labels(&stats);
        assert!(labels.is_empty());
    }

    #[test]
    fn test_new_label() {
        let mut stats = make_stats();
        let today = stats::today_str();
        stats.first_seen.insert("new_cmd".to_string(), today);

        let labels = compute_labels(&stats);
        assert_eq!(labels.get("new_cmd"), Some(&Label::New));
    }

    #[test]
    fn test_old_command_not_new() {
        let mut stats = make_stats();
        stats
            .first_seen
            .insert("old_cmd".to_string(), "2020-01-01".to_string());

        let labels = compute_labels(&stats);
        assert_ne!(labels.get("old_cmd"), Some(&Label::New));
    }

    #[test]
    fn test_top_label() {
        let mut stats = make_stats();
        // Use a date in prev 7 days so commands don't trigger HOT
        // (HOT requires recent > prev AND recent >= 2, but if all data is in prev period,
        //  recent=0 so HOT won't trigger)
        let prev_date = stats::days_ago_from(&stats::today_str(), 10).unwrap();

        let mut day = HashMap::new();
        day.insert("cmd1".to_string(), 10);
        day.insert("cmd2".to_string(), 8);
        day.insert("cmd3".to_string(), 6);
        day.insert("cmd4".to_string(), 4);
        day.insert("cmd5".to_string(), 2);
        day.insert("cmd6".to_string(), 1);
        stats.daily_selections.insert(prev_date, day);

        let labels = compute_labels(&stats);
        assert_eq!(labels.get("cmd1"), Some(&Label::Top { count: 10 }));
        assert_eq!(labels.get("cmd2"), None); // only #1 gets Top
        assert_eq!(labels.get("cmd4"), None);
    }

    #[test]
    fn test_hot_selection_trend() {
        let mut stats = make_stats();
        let today = stats::today_str();

        // Recent 7 days: 6 selections (>= 5, >= 2x prev)
        let mut recent_day = HashMap::new();
        recent_day.insert("trending_cmd".to_string(), 6);
        stats.daily_selections.insert(today.clone(), recent_day);

        // Previous 7 days: 2 selections
        let prev_date = stats::days_ago_from(&today, 10).unwrap();
        let mut prev_day = HashMap::new();
        prev_day.insert("trending_cmd".to_string(), 2);
        stats.daily_selections.insert(prev_date, prev_day);

        let labels = compute_labels(&stats);
        assert_eq!(labels.get("trending_cmd"), Some(&Label::Hot { count: 6 }));
    }

    #[test]
    fn test_hot_overrides_new() {
        let mut stats = make_stats();
        let today = stats::today_str();

        // Mark as new
        stats
            .first_seen
            .insert("hot_new_cmd".to_string(), today.clone());

        // Also make it hot via selection trend (>= 5, no prev = 2x satisfied)
        let mut recent_day = HashMap::new();
        recent_day.insert("hot_new_cmd".to_string(), 5);
        stats.daily_selections.insert(today.clone(), recent_day);

        let labels = compute_labels(&stats);
        // HOT should take priority over NEW
        assert_eq!(labels.get("hot_new_cmd"), Some(&Label::Hot { count: 5 }));
    }

    #[test]
    fn test_hot_history_spike() {
        let mut stats = make_stats();

        stats.prev_snapshot = Some(Snapshot {
            date: "2026-03-15".to_string(),
            frequencies: {
                let mut f = HashMap::new();
                f.insert("spiked_cmd".to_string(), 5);
                // Add many commands so top 5% threshold is meaningful
                for i in 0..100 {
                    f.insert(format!("other_cmd_{i}"), 10);
                }
                f
            },
        });

        stats.history_snapshot = Some(Snapshot {
            date: "2026-03-16".to_string(),
            frequencies: {
                let mut f = HashMap::new();
                f.insert("spiked_cmd".to_string(), 15); // +10 increase
                for i in 0..100 {
                    f.insert(format!("other_cmd_{i}"), 10); // no change
                }
                f
            },
        });

        let labels = compute_labels(&stats);
        assert!(matches!(labels.get("spiked_cmd"), Some(&Label::Hot { .. })));
    }

    use crate::history::HistoryEntry;

    fn make_entry(command: &str, frequency: usize) -> HistoryEntry {
        HistoryEntry {
            command: command.to_string(),
            frequency,
            recency_rank: 0,
            score: 0.0,
        }
    }

    #[test]
    fn test_stale_label() {
        let mut stats = make_stats();
        // first_seen 20 days ago
        let old_date = stats::days_ago_from(&stats::today_str(), 20).unwrap();
        stats.first_seen.insert("old_cmd".to_string(), old_date);

        let entries = vec![make_entry("old_cmd", 1)];
        let labels = compute_labels_with(&stats, &entries);
        assert_eq!(labels.get("old_cmd"), Some(&Label::Stale));
    }

    #[test]
    fn test_stale_not_if_recently_selected() {
        let mut stats = make_stats();
        let old_date = stats::days_ago_from(&stats::today_str(), 20).unwrap();
        stats.first_seen.insert("old_cmd".to_string(), old_date);

        // Selected today
        let today = stats::today_str();
        let mut day = HashMap::new();
        day.insert("old_cmd".to_string(), 1);
        stats.daily_selections.insert(today, day);

        let entries = vec![make_entry("old_cmd", 1)];
        let labels = compute_labels_with(&stats, &entries);
        assert_ne!(labels.get("old_cmd"), Some(&Label::Stale));
    }

    #[test]
    fn test_stale_not_if_high_frequency() {
        let mut stats = make_stats();
        let old_date = stats::days_ago_from(&stats::today_str(), 20).unwrap();
        stats.first_seen.insert("frequent_cmd".to_string(), old_date);

        let entries = vec![make_entry("frequent_cmd", 5)];
        let labels = compute_labels_with(&stats, &entries);
        assert_ne!(labels.get("frequent_cmd"), Some(&Label::Stale));
    }

    #[test]
    fn test_stale_not_if_new() {
        let mut stats = make_stats();
        let recent_date = stats::days_ago_from(&stats::today_str(), 3).unwrap();
        stats.first_seen.insert("recent_cmd".to_string(), recent_date);

        let entries = vec![make_entry("recent_cmd", 1)];
        let labels = compute_labels_with(&stats, &entries);
        assert_ne!(labels.get("recent_cmd"), Some(&Label::Stale));
    }

    #[test]
    fn test_hot_overrides_stale() {
        let mut stats = make_stats();
        let old_date = stats::days_ago_from(&stats::today_str(), 20).unwrap();
        stats.first_seen.insert("cmd".to_string(), old_date);

        // Make it HOT via selection trend
        let today = stats::today_str();
        let mut day = HashMap::new();
        day.insert("cmd".to_string(), 6);
        stats.daily_selections.insert(today, day);

        let entries = vec![make_entry("cmd", 1)];
        let labels = compute_labels_with(&stats, &entries);
        assert_eq!(labels.get("cmd"), Some(&Label::Hot { count: 6 }));
    }

    #[test]
    fn test_priority_hot_over_top() {
        let mut stats = make_stats();
        let today = stats::today_str();

        // Make cmd both TOP and HOT
        let mut recent_day = HashMap::new();
        recent_day.insert("popular_cmd".to_string(), 20);
        stats.daily_selections.insert(today.clone(), recent_day);

        // No previous selections = trend means HOT (20 >= 5, 20 >= 0*2)
        let labels = compute_labels(&stats);
        assert_eq!(labels.get("popular_cmd"), Some(&Label::Hot { count: 20 }));
    }
}

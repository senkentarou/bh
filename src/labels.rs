use std::collections::HashMap;

use crate::stats::{self, Stats};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Label {
    New,
    Hot,
    Top,
}

impl Label {
    pub fn display(&self) -> &'static str {
        match self {
            Label::New => "NEW",
            Label::Hot => "HOT",
            Label::Top => "★",
        }
    }
}

pub fn compute_labels(stats_data: &Stats) -> HashMap<String, Label> {
    let mut labels: HashMap<String, Label> = HashMap::new();
    let today = stats::today_str();

    // 1. TOP: top 5 by selection count in last 30 days
    let from_30d = stats::days_ago_from(&today, 30).unwrap_or_default();
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

    for (cmd, _) in cmd_counts.iter().take(5) {
        labels.insert(cmd.clone(), Label::Top);
    }

    // 2. NEW: first_seen within 3 days
    for (cmd, date) in &stats_data.first_seen {
        if let Some(days) = stats::days_since(date) {
            if days <= 3 {
                labels.insert(cmd.clone(), Label::New);
            }
        }
    }

    // 3. HOT (highest priority — overwrites NEW and TOP)
    let from_7d = stats::days_ago_from(&today, 7).unwrap_or_default();
    let from_14d = stats::days_ago_from(&today, 14).unwrap_or_default();
    // prev 7 days = [from_14d, from_7d)
    // To get the day before from_7d:
    let prev_7d_end = stats::days_ago_from(&today, 8).unwrap_or_default();

    // HOT condition 1: selection trend
    for cmd in &all_cmds {
        let recent =
            stats::selections_in_range(&stats_data.daily_selections, cmd, &from_7d, &today);
        let prev =
            stats::selections_in_range(&stats_data.daily_selections, cmd, &from_14d, &prev_7d_end);
        if recent >= 2 && recent > prev {
            labels.insert(cmd.clone(), Label::Hot);
        }
    }

    // HOT condition 2: bash_history frequency spike
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
            // Top 5% threshold
            let top_idx = (increases.len() as f64 * 0.05).ceil() as usize;
            let threshold_increase = increases
                .get(top_idx.saturating_sub(1))
                .map(|(_, inc)| *inc)
                .unwrap_or(usize::MAX);

            for (cmd, inc) in &increases {
                if *inc >= 3 && *inc >= threshold_increase {
                    labels.insert(cmd.clone(), Label::Hot);
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
        assert_eq!(labels.get("cmd1"), Some(&Label::Top));
        assert_eq!(labels.get("cmd5"), Some(&Label::Top));
        assert_eq!(labels.get("cmd6"), None);
    }

    #[test]
    fn test_hot_selection_trend() {
        let mut stats = make_stats();
        let today = stats::today_str();

        // Recent 7 days: 5 selections
        let mut recent_day = HashMap::new();
        recent_day.insert("trending_cmd".to_string(), 5);
        stats.daily_selections.insert(today.clone(), recent_day);

        // Previous 7 days: 1 selection
        let prev_date = stats::days_ago_from(&today, 10).unwrap();
        let mut prev_day = HashMap::new();
        prev_day.insert("trending_cmd".to_string(), 1);
        stats.daily_selections.insert(prev_date, prev_day);

        let labels = compute_labels(&stats);
        assert_eq!(labels.get("trending_cmd"), Some(&Label::Hot));
    }

    #[test]
    fn test_hot_overrides_new() {
        let mut stats = make_stats();
        let today = stats::today_str();

        // Mark as new
        stats
            .first_seen
            .insert("hot_new_cmd".to_string(), today.clone());

        // Also make it hot via selection trend
        let mut recent_day = HashMap::new();
        recent_day.insert("hot_new_cmd".to_string(), 5);
        stats.daily_selections.insert(today.clone(), recent_day);

        let labels = compute_labels(&stats);
        // HOT should take priority over NEW
        assert_eq!(labels.get("hot_new_cmd"), Some(&Label::Hot));
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
        assert_eq!(labels.get("spiked_cmd"), Some(&Label::Hot));
    }

    #[test]
    fn test_priority_hot_over_top() {
        let mut stats = make_stats();
        let today = stats::today_str();

        // Make cmd both TOP and HOT
        let mut recent_day = HashMap::new();
        recent_day.insert("popular_cmd".to_string(), 20);
        stats.daily_selections.insert(today.clone(), recent_day);

        // No previous selections = trend means HOT
        let labels = compute_labels(&stats);
        assert_eq!(labels.get("popular_cmd"), Some(&Label::Hot));
    }
}

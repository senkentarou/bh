use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::history::HistoryEntry;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub date: String,
    pub frequencies: HashMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stats {
    pub version: u32,
    pub first_seen: HashMap<String, String>,
    pub daily_selections: HashMap<String, HashMap<String, u32>>,
    pub history_snapshot: Option<Snapshot>,
    pub prev_snapshot: Option<Snapshot>,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            version: 1,
            first_seen: HashMap::new(),
            daily_selections: HashMap::new(),
            history_snapshot: None,
            prev_snapshot: None,
        }
    }
}

pub fn stats_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Could not determine home directory")
        .join(".bh")
}

pub fn stats_path() -> PathBuf {
    stats_dir().join("stats.json")
}

pub fn load_stats() -> Stats {
    let path = stats_path();
    match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => Stats::default(),
    }
}

pub fn save_stats(stats: &Stats) {
    let dir = stats_dir();
    if !dir.exists() {
        let _ = fs::create_dir_all(&dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(stats) {
        let _ = fs::write(stats_path(), json);
    }
}

pub fn record_selection(stats: &mut Stats, command: &str) {
    let today = today_str();
    let day_map = stats.daily_selections.entry(today).or_default();
    *day_map.entry(command.to_string()).or_default() += 1;
}

pub fn update_first_seen(stats: &mut Stats, commands: &[String]) {
    let today = today_str();
    // On first run (empty first_seen), treat all existing commands as old
    // so they don't all get labeled NEW.
    let is_first_run = stats.first_seen.is_empty();
    let date_for_existing = if is_first_run {
        days_ago_str(30)
    } else {
        today.clone()
    };
    for cmd in commands {
        stats
            .first_seen
            .entry(cmd.clone())
            .or_insert_with(|| date_for_existing.clone());
    }
}

pub fn update_snapshot(stats: &mut Stats, entries: &[HistoryEntry]) {
    let today = today_str();

    // Only update snapshot once per day
    if let Some(ref snap) = stats.history_snapshot {
        if snap.date == today {
            return;
        }
    }

    // Promote current snapshot to prev
    if stats.history_snapshot.is_some() {
        stats.prev_snapshot = stats.history_snapshot.take();
    }

    // Build new snapshot from history entries
    let mut frequencies = HashMap::new();
    for entry in entries {
        frequencies.insert(entry.command.clone(), entry.frequency);
    }

    stats.history_snapshot = Some(Snapshot {
        date: today,
        frequencies,
    });
}

pub fn prune_old_data(stats: &mut Stats) {
    let cutoff = days_ago_str(30);
    stats.daily_selections.retain(|date, _| date.as_str() >= cutoff.as_str());
}

pub fn today_str() -> String {
    date_str_from_epoch(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    )
}

pub fn days_since(date_str: &str) -> Option<i64> {
    let today = today_str();
    let today_days = date_to_days(&today)?;
    let target_days = date_to_days(date_str)?;
    Some(today_days - target_days)
}

fn days_ago_str(n: i64) -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let past = secs.saturating_sub((n * 86400) as u64);
    date_str_from_epoch(past)
}

fn date_str_from_epoch(epoch_secs: u64) -> String {
    // Convert epoch seconds to YYYY-MM-DD without chrono
    let days = (epoch_secs / 86400) as i64;
    let (y, m, d) = days_to_ymd(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn date_to_days(date_str: &str) -> Option<i64> {
    let parts: Vec<&str> = date_str.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let y: i64 = parts[0].parse().ok()?;
    let m: i64 = parts[1].parse().ok()?;
    let d: i64 = parts[2].parse().ok()?;
    Some(ymd_to_days(y, m, d))
}

// Civil date <-> day count algorithms (proleptic Gregorian)
fn ymd_to_days(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn days_to_ymd(days: i64) -> (i64, i64, i64) {
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Get the date string for N days ago from a reference date.
pub fn days_ago_from(reference: &str, n: i64) -> Option<String> {
    let ref_days = date_to_days(reference)?;
    let target_days = ref_days - n;
    let (y, m, d) = days_to_ymd(target_days);
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

/// Sum selections for a command within a date range [from_date, to_date] inclusive.
pub fn selections_in_range(
    daily: &HashMap<String, HashMap<String, u32>>,
    command: &str,
    from_date: &str,
    to_date: &str,
) -> u32 {
    let mut total = 0u32;
    for (date, cmds) in daily {
        if date.as_str() >= from_date && date.as_str() <= to_date {
            if let Some(&count) = cmds.get(command) {
                total += count;
            }
        }
    }
    total
}

/// Compute total selection counts per command across all dates.
pub fn total_selections(daily: &HashMap<String, HashMap<String, u32>>) -> HashMap<String, u32> {
    let mut totals: HashMap<String, u32> = HashMap::new();
    for cmds in daily.values() {
        for (cmd, &count) in cmds {
            *totals.entry(cmd.clone()).or_default() += count;
        }
    }
    totals
}

/// Get all unique commands that appear in daily_selections.
pub fn all_selected_commands(daily: &HashMap<String, HashMap<String, u32>>) -> Vec<String> {
    let mut cmds: std::collections::HashSet<String> = std::collections::HashSet::new();
    for day in daily.values() {
        for cmd in day.keys() {
            cmds.insert(cmd.clone());
        }
    }
    cmds.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_date_roundtrip() {
        // Known epoch: 2026-03-16 is approx 20529 days from epoch
        let s = date_str_from_epoch(0);
        assert_eq!(s, "1970-01-01");

        let s = date_str_from_epoch(86400);
        assert_eq!(s, "1970-01-02");
    }

    #[test]
    fn test_ymd_to_days_roundtrip() {
        for days in [0, 1000, 18000, 20000, 20529] {
            let (y, m, d) = days_to_ymd(days);
            let back = ymd_to_days(y, m, d);
            assert_eq!(days, back, "roundtrip failed for days={days} -> {y}-{m}-{d}");
        }
    }

    #[test]
    fn test_days_since() {
        let today = today_str();
        assert_eq!(days_since(&today), Some(0));
    }

    #[test]
    fn test_date_to_days() {
        assert_eq!(date_to_days("1970-01-01"), Some(0));
        assert_eq!(date_to_days("1970-01-02"), Some(1));
    }

    #[test]
    fn test_selections_in_range() {
        let mut daily: HashMap<String, HashMap<String, u32>> = HashMap::new();
        let mut day1 = HashMap::new();
        day1.insert("git push".to_string(), 3);
        daily.insert("2026-03-15".to_string(), day1);

        let mut day2 = HashMap::new();
        day2.insert("git push".to_string(), 2);
        daily.insert("2026-03-16".to_string(), day2);

        assert_eq!(
            selections_in_range(&daily, "git push", "2026-03-15", "2026-03-16"),
            5
        );
        assert_eq!(
            selections_in_range(&daily, "git push", "2026-03-16", "2026-03-16"),
            2
        );
        assert_eq!(
            selections_in_range(&daily, "git status", "2026-03-15", "2026-03-16"),
            0
        );
    }

    #[test]
    fn test_prune_old_data() {
        let mut stats = Stats::default();
        stats
            .daily_selections
            .insert("2020-01-01".to_string(), HashMap::new());
        stats
            .daily_selections
            .insert(today_str(), HashMap::new());

        prune_old_data(&mut stats);
        assert_eq!(stats.daily_selections.len(), 1);
        assert!(stats.daily_selections.contains_key(&today_str()));
    }

    #[test]
    fn test_record_selection() {
        let mut stats = Stats::default();
        record_selection(&mut stats, "git push");
        record_selection(&mut stats, "git push");
        record_selection(&mut stats, "ls");

        let today = today_str();
        let day = &stats.daily_selections[&today];
        assert_eq!(day["git push"], 2);
        assert_eq!(day["ls"], 1);
    }
}

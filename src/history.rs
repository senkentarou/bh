use std::collections::HashMap;
use std::fs;
use std::ops::Range;
use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct HistoryEntry {
    pub command: String,
    pub frequency: usize,
    pub recency_rank: usize,
    pub score: f64,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub entry: HistoryEntry,
    /// Byte range in the command where the query matched (on lowercased text)
    pub match_range: Option<Range<usize>>,
}

/// Parse ~/.bash_history and build ranked entries.
///
/// Scoring: `(recency × 0.6 + log(1 + frequency) × 0.4) × noise_penalty`
pub fn load_history() -> Vec<HistoryEntry> {
    let path = history_path();
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to read {}: {e}", path.display());
            return Vec::new();
        }
    };

    let mut freq: HashMap<String, usize> = HashMap::new();
    let mut last_seen: HashMap<String, usize> = HashMap::new();
    let mut total_lines = 0;

    for (i, line) in content.lines().enumerate() {
        total_lines = i + 1;
        let cmd = line.trim();
        if cmd.is_empty() || cmd.starts_with('#') {
            continue;
        }
        *freq.entry(cmd.to_string()).or_default() += 1;
        last_seen.insert(cmd.to_string(), i);
    }

    let max_line = total_lines.max(1) as f64;

    let mut entries: Vec<HistoryEntry> = freq
        .into_iter()
        .map(|(command, frequency)| {
            let line_idx = last_seen[&command];
            let recency = line_idx as f64 / max_line;
            let freq_factor = (frequency as f64).ln_1p();
            let noise_penalty = if frequency == 1 { 0.3 } else { 1.0 };
            let score = (recency * 0.6 + freq_factor * 0.4) * noise_penalty;

            HistoryEntry {
                command,
                frequency,
                recency_rank: 0,
                score,
            }
        })
        .collect();

    entries.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    for (i, entry) in entries.iter_mut().enumerate() {
        entry.recency_rank = i;
    }

    entries
}

/// Case-insensitive partial match with match range for highlighting.
pub fn search(entries: &[HistoryEntry], query: &str) -> Vec<SearchResult> {
    if query.is_empty() {
        return entries
            .iter()
            .map(|e| SearchResult {
                entry: e.clone(),
                match_range: None,
            })
            .collect();
    }

    let query_lower = query.to_lowercase();
    let mut results: Vec<SearchResult> = entries
        .iter()
        .filter_map(|e| {
            let cmd_lower = e.command.to_lowercase();
            let start = cmd_lower.find(&query_lower)?;
            Some(SearchResult {
                entry: e.clone(),
                match_range: Some(start..start + query_lower.len()),
            })
        })
        .collect();

    // Prefix matches first, then by score
    results.sort_by(|a, b| {
        let a_prefix = a.match_range.as_ref().is_some_and(|r| r.start == 0);
        let b_prefix = b.match_range.as_ref().is_some_and(|r| r.start == 0);
        b_prefix
            .cmp(&a_prefix)
            .then(b.entry.score.partial_cmp(&a.entry.score).unwrap_or(std::cmp::Ordering::Equal))
    });

    results
}

/// Adaptive filtering: few results → show all, many results → filter noise.
pub fn search_adaptive(entries: &[HistoryEntry], query: &str) -> Vec<SearchResult> {
    let results = search(entries, query);

    if query.is_empty() {
        return results
            .into_iter()
            .filter(|r| r.entry.frequency > 1)
            .collect();
    }

    if results.len() <= 20 {
        return results;
    }

    let threshold = results
        .get(results.len() / 3)
        .map(|r| r.entry.score * 0.5)
        .unwrap_or(0.0);
    results
        .into_iter()
        .filter(|r| r.entry.score >= threshold)
        .collect()
}

/// Remove all occurrences of `command` from ~/.bash_history and the in-memory list.
pub fn delete_command(entries: &mut Vec<HistoryEntry>, command: &str) {
    entries.retain(|e| e.command != command);

    let path = history_path();
    if let Ok(content) = fs::read_to_string(&path) {
        let filtered: Vec<&str> = content
            .lines()
            .filter(|line| line.trim() != command)
            .collect();
        let _ = fs::write(&path, filtered.join("\n") + "\n");
    }
}

fn history_path() -> PathBuf {
    dirs::home_dir()
        .expect("Could not determine home directory")
        .join(".bash_history")
}

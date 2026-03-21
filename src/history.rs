use std::collections::HashMap;
use std::fs;
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
    /// Byte indices of matched characters in the command (for highlight)
    pub match_positions: Vec<usize>,
    /// Fuzzy match quality score (higher = tighter match). 0 for no query.
    pub match_quality: f64,
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

// ── Fuzzy matching ────────────────────────────────────────────────────

/// Try substring match first (returns contiguous byte positions).
fn substring_match(cmd_lower: &str, query_lower: &str) -> Option<Vec<usize>> {
    let start = cmd_lower.find(query_lower)?;
    let positions: Vec<usize> = cmd_lower[start..]
        .char_indices()
        .take(query_lower.chars().count())
        .map(|(i, _)| start + i)
        .collect();
    Some(positions)
}

/// fzf-style fuzzy match: all query chars must appear in order.
/// Returns (matched byte positions, quality score).
fn fuzzy_match(cmd_lower: &str, query_lower: &str) -> Option<(Vec<usize>, f64)> {
    let query_chars: Vec<char> = query_lower.chars().collect();
    if query_chars.is_empty() {
        return Some((Vec::new(), 0.0));
    }

    // First pass: check if all query chars exist in order (greedy forward).
    // Then do a best-match pass to maximise quality.
    let cmd_chars: Vec<(usize, char)> = cmd_lower.char_indices().collect();

    // ── Best-match via recursive search with memoisation is expensive.
    // Use a simpler two-pass approach (forward + backward) like fzf v1.
    let positions = best_match(&cmd_chars, &query_chars)?;

    let quality = score_positions(&positions, cmd_lower);
    Some((positions, quality))
}

/// Forward-pass greedy match preferring word boundaries and consecutive runs.
fn best_match(cmd_chars: &[(usize, char)], query_chars: &[char]) -> Option<Vec<usize>> {
    let n = cmd_chars.len();
    let m = query_chars.len();

    // Forward pass: find the earliest positions where all query chars match.
    let mut forward = Vec::with_capacity(m);
    let mut ci = 0;
    for &qch in query_chars {
        loop {
            if ci >= n {
                return None;
            }
            if cmd_chars[ci].1 == qch {
                forward.push(ci);
                ci += 1;
                break;
            }
            ci += 1;
        }
    }

    // Backward pass: from the last matched position, greedily match backwards
    // to find the tightest cluster.
    let mut backward = vec![0usize; m];
    let mut ci = forward[m - 1]; // start from last forward match
    for qi in (0..m).rev() {
        // Search backwards from ci for query_chars[qi]
        loop {
            if cmd_chars[ci].1 == query_chars[qi] {
                backward[qi] = cmd_chars[ci].0; // byte index
                if ci > 0 && qi > 0 {
                    ci -= 1;
                }
                break;
            }
            if ci == 0 {
                break;
            }
            ci -= 1;
        }
    }

    Some(backward)
}

/// Score matched positions: consecutive chars, word boundaries, tightness.
fn score_positions(positions: &[usize], cmd: &str) -> f64 {
    if positions.is_empty() {
        return 0.0;
    }

    let cmd_bytes = cmd.as_bytes();
    let mut score: f64 = 0.0;

    for (i, &pos) in positions.iter().enumerate() {
        // Consecutive bonus: matched char immediately follows previous match
        if i > 0 {
            // Check if this position is the very next character after previous
            let prev_pos = positions[i - 1];
            let prev_char_len = cmd[prev_pos..].chars().next().map_or(1, |c| c.len_utf8());
            if pos == prev_pos + prev_char_len {
                score += 8.0; // consecutive bonus
            }
        }

        // Word boundary bonus: char at start or after separator
        if pos == 0 {
            score += 6.0; // start of string
        } else {
            let prev_byte = cmd_bytes[pos - 1];
            if matches!(prev_byte, b' ' | b'/' | b'-' | b'_' | b'.' | b'\\') {
                score += 5.0; // word boundary
            }
        }

        // Base score per matched char
        score += 1.0;
    }

    // Tightness bonus: shorter span between first and last match is better
    if positions.len() > 1 {
        let span = positions.last().unwrap() - positions.first().unwrap() + 1;
        let ideal = positions.len(); // minimum possible span
        let tightness = ideal as f64 / span as f64; // 1.0 = perfect
        score += tightness * 4.0;
    }

    // Early position bonus: matches near the start of command are better
    let first_pos = *positions.first().unwrap() as f64;
    score += 3.0 / (1.0 + first_pos * 0.1);

    score
}

/// Case-insensitive search with fuzzy matching and match positions for highlighting.
pub fn search(entries: &[HistoryEntry], query: &str) -> Vec<SearchResult> {
    if query.is_empty() {
        return entries
            .iter()
            .map(|e| SearchResult {
                entry: e.clone(),
                match_positions: Vec::new(),
                match_quality: 0.0,
            })
            .collect();
    }

    let query_lower = query.to_lowercase();

    let mut results: Vec<SearchResult> = entries
        .iter()
        .filter_map(|e| {
            let cmd_lower = e.command.to_lowercase();

            // Try exact substring first (higher quality)
            if let Some(positions) = substring_match(&cmd_lower, &query_lower) {
                let quality = score_positions(&positions, &cmd_lower) + 100.0; // substring bonus
                return Some(SearchResult {
                    entry: e.clone(),
                    match_positions: positions,
                    match_quality: quality,
                });
            }

            // Fall back to fuzzy match
            let (positions, quality) = fuzzy_match(&cmd_lower, &query_lower)?;
            Some(SearchResult {
                entry: e.clone(),
                match_positions: positions,
                match_quality: quality,
            })
        })
        .collect();

    // Sort: match quality first (exact > fuzzy > weak), then by entry score
    results.sort_by(|a, b| {
        b.match_quality
            .partial_cmp(&a.match_quality)
            .unwrap_or(std::cmp::Ordering::Equal)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(cmd: &str) -> HistoryEntry {
        HistoryEntry {
            command: cmd.to_string(),
            frequency: 5,
            recency_rank: 0,
            score: 1.0,
        }
    }

    fn search_cmds(commands: &[&str], query: &str) -> Vec<String> {
        let entries: Vec<HistoryEntry> = commands.iter().map(|c| make_entry(c)).collect();
        search(&entries, query)
            .into_iter()
            .map(|r| r.entry.command)
            .collect()
    }

    #[test]
    fn exact_substring_match() {
        let results = search_cmds(&["git push", "git pull", "echo hello"], "push");
        assert_eq!(results, vec!["git push"]);
    }

    #[test]
    fn fuzzy_gtp_matches_git_push() {
        let results = search_cmds(
            &["git push", "git pull", "echo hello", "ls -la"],
            "gtp",
        );
        assert!(results.contains(&"git push".to_string()), "gtp should match 'git push'");
    }

    #[test]
    fn fuzzy_gpl_matches_git_pull() {
        let results = search_cmds(
            &["git push", "git pull", "echo hello"],
            "gpl",
        );
        assert!(results.contains(&"git pull".to_string()), "gpl should match 'git pull'");
    }

    #[test]
    fn exact_match_ranks_above_fuzzy() {
        let entries = vec![
            make_entry("git pull"),      // fuzzy match for "gp"
            make_entry("gp"),            // exact match for "gp"
            make_entry("git push"),      // fuzzy match for "gp"
        ];
        let results = search(&entries, "gp");
        // Exact substring "gp" should rank first
        assert_eq!(results[0].entry.command, "gp");
    }

    #[test]
    fn fuzzy_prefers_tighter_match() {
        let entries = vec![
            make_entry("grep something_pattern"),  // g...p far apart
            make_entry("git push"),                 // g-i-t-_-p tight
        ];
        let results = search(&entries, "gp");
        // "git push" has g and p closer together with word boundary
        assert_eq!(results[0].entry.command, "git push");
    }

    #[test]
    fn empty_query_returns_all() {
        let results = search_cmds(&["git push", "echo hello"], "");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn no_match_returns_empty() {
        let results = search_cmds(&["git push", "echo hello"], "zzz");
        assert!(results.is_empty());
    }

    #[test]
    fn fuzzy_case_insensitive() {
        let results = search_cmds(&["Git Push"], "gtp");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn match_positions_are_correct_for_substring() {
        let entries = vec![make_entry("git push")];
        let results = search(&entries, "push");
        assert_eq!(results[0].match_positions, vec![4, 5, 6, 7]);
    }

    #[test]
    fn match_positions_are_correct_for_fuzzy() {
        let entries = vec![make_entry("git push")];
        let results = search(&entries, "gp");
        let positions = &results[0].match_positions;
        // g=0, p=4
        assert_eq!(positions.len(), 2);
        assert_eq!(positions[0], 0); // 'g'
        assert_eq!(positions[1], 4); // 'p'
    }
}

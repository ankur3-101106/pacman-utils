// ──────────────────────────────────────────────────────────────────────
// fuzzy.rs — Lightweight subsequence fuzzy matcher
//
// Replaces the gum/fzf filter from the bash version. Scores candidate
// items by greedy left-to-right matching with bonuses for consecutive
// matches, word boundaries, and start-of-string hits; penalizes gaps.
// ──────────────────────────────────────────────────────────────────────

/// Score `item` against `query` (both lowercased by the caller or here).
/// Returns None if not all query chars appear in order.
pub fn score(query: &str, item: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let q: Vec<char> = query.to_lowercase().chars().collect();
    let s: Vec<char> = item.to_lowercase().chars().collect();

    let mut total: i64 = 0;
    let mut search_from = 0usize;
    let mut last_hit: Option<usize> = None;

    for &qc in &q {
        // Find next occurrence of qc at/after search_from.
        let hit = (search_from..s.len()).find(|&i| s[i] == qc)?;
        let mut pts: i64 = 1;

        // Consecutive match bonus.
        if Some(hit) == last_hit.map(|h| h + 1) {
            pts += 4;
        }
        // Boundary bonus: after a separator or at string start.
        if hit == 0 {
            pts += 6;
        } else {
            let prev = s[hit - 1];
            if !prev.is_ascii_alphanumeric() {
                pts += 5;
            }
        }

        total += pts - (hit as i64 - last_hit.map_or(hit as i64, |h| h as i64)).min(3) / 2;
        last_hit = Some(hit);
        search_from = hit + 1;
    }

    // Prefer shorter items on ties.
    total -= (s.len() as i64) / 16;
    Some(total)
}

/// Indices into `items`, best match first.
pub fn filter_indices(items: &[String], query: &str) -> Vec<usize> {
    if query.is_empty() {
        return (0..items.len()).collect();
    }
    let mut scored: Vec<(i64, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| score(query, item).map(|s| (s, i)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, i)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<String> {
        vec!["linux".into(), "linux-headers".into(), "firefox".into(), "nodejs-lts-hydrogen".into()]
    }

    #[test]
    fn exact_prefix_ranks_first() {
        let list = items();
        let idx = filter_indices(&list, "linux");
        assert_eq!(list[idx[0]], "linux");
    }

    #[test]
    fn subsequence_matches() {
        assert!(score("ffx", "firefox").is_some());
        assert!(score("zzz", "firefox").is_none());
    }

    #[test]
    fn case_insensitive() {
        assert!(score("FFX", "firefox").is_some());
    }

    #[test]
    fn empty_query_matches_all() {
        assert_eq!(filter_indices(&items(), "").len(), 4);
    }
}

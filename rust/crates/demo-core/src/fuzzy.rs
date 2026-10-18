//! Fuzzy matching for the combobox and the command palette (protocol §7).
//!
//! Case-insensitive subsequence match. Score: +10 per matched char, +8 if it
//! starts a word, +6 if it directly follows the previous match, −1 per
//! skipped char before the first match. Among all ways to match, the best
//! score wins (leftmost on ties); ranking breaks score ties by input order.
//! The matched char positions are returned for highlighting.

/// A successful match: its score and the matched **char** indices (not byte
/// offsets) into the candidate, ascending.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub score: i32,
    pub indices: Vec<usize>,
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// A word starts at the beginning or after any non-alphanumeric char.
fn starts_word(chars: &[char], i: usize) -> bool {
    i == 0 || !chars[i - 1].is_alphanumeric()
}

/// Match `query` against `candidate`. `None` when the query is not a
/// subsequence. An empty (or all-space) query matches with score 0.
/// Whitespace in the query is ignored, so "ali ng" finds "Alice Nguyen".
pub fn fuzzy_match(query: &str, candidate: &str) -> Option<Match> {
    let pat: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).map(lower).collect();
    if pat.is_empty() {
        return Some(Match { score: 0, indices: vec![] });
    }
    let orig: Vec<char> = candidate.chars().collect();
    let low: Vec<char> = orig.iter().map(|&c| lower(c)).collect();
    let (n, m) = (pat.len(), low.len());
    if n > m {
        return None;
    }
    // best[i][j] = best (score, previous position) with pat[i] matched at j.
    let gain = |j: usize| 10 + if starts_word(&orig, j) { 8 } else { 0 };
    let mut best: Vec<Vec<Option<(i32, usize)>>> = vec![vec![None; m]; n];
    for j in 0..m {
        if low[j] == pat[0] {
            best[0][j] = Some((gain(j) - j as i32, usize::MAX));
        }
    }
    for i in 1..n {
        // running best of row i-1 over positions < j-1 (leftmost on ties)
        let mut far: Option<(i32, usize)> = None;
        for j in 1..m {
            if j >= 2 {
                if let Some((s, _)) = best[i - 1][j - 2] {
                    if far.is_none_or(|(fs, _)| s > fs) {
                        far = Some((s, j - 2));
                    }
                }
            }
            if low[j] != pat[i] {
                continue;
            }
            let adjacent = best[i - 1][j - 1].map(|(s, _)| (s + 6, j - 1));
            let pick = match (adjacent, far) {
                (Some(a), Some(f)) => Some(if f.0 > a.0 { f } else { a }),
                (a, f) => a.or(f),
            };
            best[i][j] = pick.map(|(s, k)| (s + gain(j), k));
        }
    }
    // best end position, leftmost on ties, then walk the back-pointers
    let mut end: Option<(i32, usize)> = None;
    for (j, cell) in best[n - 1].iter().enumerate() {
        if let Some((s, _)) = *cell {
            if end.is_none_or(|(es, _)| s > es) {
                end = Some((s, j));
            }
        }
    }
    let (score, mut j) = end?;
    let mut indices = vec![0; n];
    for i in (0..n).rev() {
        indices[i] = j;
        if i > 0 {
            j = best[i][j].map(|(_, k)| k)?;
        }
    }
    Some(Match { score, indices })
}

/// Rank candidates against a query: `(input index, match)` pairs, best
/// first, score ties in input order. An empty query returns every
/// candidate in its original order.
pub fn rank<'a, I>(query: &str, candidates: I) -> Vec<(usize, Match)>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut out: Vec<(usize, Match)> = candidates
        .into_iter()
        .enumerate()
        .filter_map(|(i, c)| fuzzy_match(query, c).map(|m| (i, m)))
        .collect();
    out.sort_by(|a, b| b.1.score.cmp(&a.1.score).then(a.0.cmp(&b.0)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoring_rules() {
        // "ali" at the start of "Alice": 3×10 + 8 (word start) + 2×6 (adjacent)
        let m = fuzzy_match("ali", "Alice Nguyen").unwrap();
        assert_eq!(m.indices, vec![0, 1, 2]);
        assert_eq!(m.score, 30 + 8 + 12);
        // "ng": skips 6 chars, then a word start and an adjacent char
        let m = fuzzy_match("ng", "Alice Nguyen").unwrap();
        assert_eq!(m.indices, vec![6, 7]);
        assert_eq!(m.score, 20 + 8 + 6 - 6);
        // word starts beat an earlier mid-word run
        let m = fuzzy_match("an", "Alice Nguyen alice").unwrap();
        assert_eq!(m.indices, vec![0, 6]);
        assert_eq!(m.score, 20 + 16);
    }

    #[test]
    fn case_whitespace_and_misses() {
        assert!(fuzzy_match("BOB", "bob okafor bob engineering").is_some());
        assert_eq!(fuzzy_match("bo ok", "Bob Okafor").unwrap().indices, vec![0, 1, 4, 5]);
        assert!(fuzzy_match("xyz", "Northwind").is_none());
        assert!(fuzzy_match("toolong", "short").is_none());
        let empty = fuzzy_match("  ", "anything").unwrap();
        assert_eq!((empty.score, empty.indices.len()), (0, 0));
    }

    #[test]
    fn unicode_positions_are_chars() {
        let m = fuzzy_match("ré", "Café résumé").unwrap();
        assert_eq!(m.indices, vec![5, 6]);
    }

    #[test]
    fn ranking() {
        let r = rank("bob", ["Alice", "Bob Smith", "Robert Bobson"]);
        assert_eq!(r.iter().map(|(i, _)| *i).collect::<Vec<_>>(), vec![1, 2]);
        // empty query: all, original order
        let r = rank("", ["c", "a", "b"]);
        assert_eq!(r.iter().map(|(i, _)| *i).collect::<Vec<_>>(), vec![0, 1, 2]);
        // equal scores keep input order
        let r = rank("a", ["xa", "ya"]);
        assert_eq!(r.iter().map(|(i, _)| *i).collect::<Vec<_>>(), vec![0, 1]);
    }
}

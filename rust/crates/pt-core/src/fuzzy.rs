//! Small fuzzy matcher for comboboxes and the command palette.
//!
//! Case-insensitive subsequence match. A contiguous substring scores best;
//! otherwise matched characters score more when they start a word or follow
//! the previous match. Returns the char indices that matched so UIs can
//! highlight them.

pub struct Match {
    pub score: i32,
    pub indices: Vec<usize>, // char indices into the candidate
}

fn is_boundary(prev: Option<char>, c: char) -> bool {
    match prev {
        None => true,
        Some(p) => {
            !p.is_alphanumeric() || (p.is_lowercase() && c.is_uppercase()) || (p.is_alphabetic() && c.is_numeric())
        }
    }
}

pub fn fuzzy_match(pattern: &str, candidate: &str) -> Option<Match> {
    let pat: Vec<char> = pattern
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if pat.is_empty() {
        return Some(Match { score: 0, indices: vec![] });
    }
    let orig: Vec<char> = candidate.chars().collect();
    let low: Vec<char> = orig.iter().map(|c| c.to_lowercase().next().unwrap_or(*c)).collect();

    // 1. Contiguous substring (ignoring pattern whitespace): best case.
    if pat.len() <= low.len() {
        let mut best: Option<(i32, usize)> = None;
        for start in 0..=low.len() - pat.len() {
            if low[start..start + pat.len()] == pat[..] {
                let bonus = if start == 0 {
                    40
                } else if is_boundary(Some(orig[start - 1]), orig[start]) {
                    25
                } else {
                    0
                };
                let score = 100 + pat.len() as i32 * 10 + bonus - start as i32;
                if best.is_none_or(|(s, _)| score > s) {
                    best = Some((score, start));
                }
            }
        }
        if let Some((score, start)) = best {
            return Some(Match {
                score,
                indices: (start..start + pat.len()).collect(),
            });
        }
    }

    // 2. Subsequence, preferring word starts: for each pattern char, take the
    //    next word-boundary occurrence if one exists before the next plain one
    //    would force a worse layout; simple greedy with a boundary look-ahead.
    let mut indices = Vec::with_capacity(pat.len());
    let mut pos = 0usize;
    for (pi, &pc) in pat.iter().enumerate() {
        let remaining = pat.len() - pi - 1;
        let mut chosen = None;
        // look for a boundary occurrence first (but leave room for the rest)
        for (i, &lc) in low.iter().enumerate().skip(pos) {
            if low.len() - i - 1 < remaining {
                break;
            }
            if lc == pc && is_boundary(if i == 0 { None } else { Some(orig[i - 1]) }, orig[i]) {
                chosen = Some(i);
                break;
            }
        }
        // a plain occurrence directly after the previous match beats a far boundary
        if let Some(&last) = indices.last() {
            let next: usize = last + 1;
            if next < low.len() && low[next] == pc {
                chosen = Some(next);
            }
        }
        if chosen.is_none() {
            chosen = (pos..low.len()).find(|&i| low[i] == pc);
        }
        let i = chosen?;
        indices.push(i);
        pos = i + 1;
    }
    let mut score = 0;
    for (k, &i) in indices.iter().enumerate() {
        score += 10;
        if is_boundary(if i == 0 { None } else { Some(orig[i - 1]) }, orig[i]) {
            score += 15;
        }
        if k > 0 && indices[k - 1] + 1 == i {
            score += 8;
        }
    }
    score -= indices[0] as i32;
    score -= (orig.len() as i32 - pat.len() as i32) / 4;
    Some(Match { score, indices })
}

/// Rank candidates; returns (index, match) pairs, best first. Ties keep input order.
pub fn rank<'a, I>(pattern: &str, candidates: I) -> Vec<(usize, Match)>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut out: Vec<(usize, Match)> = candidates
        .into_iter()
        .enumerate()
        .filter_map(|(i, c)| fuzzy_match(pattern, c).map(|m| (i, m)))
        .collect();
    out.sort_by(|a, b| b.1.score.cmp(&a.1.score).then(a.0.cmp(&b.0)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substring_beats_subsequence() {
        let a = fuzzy_match("ali", "Alice Nguyen").unwrap();
        let b = fuzzy_match("ali", "Alpha Lima India").unwrap();
        assert!(a.score > b.score);
        assert_eq!(a.indices, vec![0, 1, 2]);
    }

    #[test]
    fn word_starts() {
        let m = fuzzy_match("nwr", "Northwind Retail").unwrap();
        assert_eq!(m.indices[0], 0);
        assert_eq!(m.indices.len(), 3);
        let m = fuzzy_match("pr", "POS refresh").unwrap();
        assert_eq!(m.indices, vec![0, 4]);
        assert!(fuzzy_match("xyz", "Northwind").is_none());
        assert!(fuzzy_match("", "x").is_some());
    }

    #[test]
    fn ranking() {
        let r = rank("bob", ["Alice", "Bob Smith", "Robert Bobson"]);
        assert_eq!(r[0].0, 1);
        assert_eq!(r.len(), 2);
    }
}

package core

// Fuzzy matching for the combobox and the command palette (protocol §7):
// a case-insensitive subsequence match scored +10 per matched character,
// +8 when it starts a word, +6 when it directly follows the previous match,
// −1 per character skipped before the first match. Among all alignments
// the best-scoring one is chosen (a small dynamic programme), so "gl" on
// "Grace Liu" highlights G and L rather than the first g and l. Positions
// are rune indices into the candidate, for highlighting.

import (
	"sort"
	"unicode"
)

// Match is a successful match: its score and the matched rune positions.
type Match struct {
	Score     int
	Positions []int
}

func isWordChar(r rune) bool { return unicode.IsLetter(r) || unicode.IsDigit(r) }

// FuzzyMatch matches query against candidate; ok is false when query is not
// a subsequence. Whitespace in the query is ignored; an empty query matches
// with score 0 and no positions.
func FuzzyMatch(query, candidate string) (Match, bool) {
	var q []rune
	for _, r := range query {
		if !unicode.IsSpace(r) {
			q = append(q, unicode.ToLower(r))
		}
	}
	if len(q) == 0 {
		return Match{Positions: []int{}}, true
	}
	orig := []rune(candidate)
	n, m := len(orig), len(q)
	if m > n {
		return Match{}, false
	}
	low := make([]rune, n)
	start := make([]bool, n) // starts a word
	for i, r := range orig {
		low[i] = unicode.ToLower(r)
		start[i] = isWordChar(r) && (i == 0 || !isWordChar(orig[i-1]))
	}
	const none = -1 << 30
	// best[i][j]: best score with q[i] matched at j; from[i][j]: q[i-1]'s position.
	best := make([][]int, m)
	from := make([][]int, m)
	for i := range best {
		best[i] = make([]int, n)
		from[i] = make([]int, n)
		for j := range best[i] {
			best[i][j] = none
		}
	}
	for i := 0; i < m; i++ {
		// running best of row i-1 over positions < j-1 (the non-adjacent predecessors)
		runBest, runAt := none, -1
		for j := i; j < n; j++ {
			if i > 0 && j >= 2 && best[i-1][j-2] > runBest {
				runBest, runAt = best[i-1][j-2], j-2
			}
			if low[j] != q[i] {
				continue
			}
			gain := 10
			if start[j] {
				gain += 8
			}
			if i == 0 {
				best[i][j] = gain - j
				continue
			}
			// prefer the adjacent predecessor on ties (tighter highlight)
			if adj := best[i-1][j-1]; adj != none && adj+6 >= runBest {
				best[i][j], from[i][j] = adj+6+gain, j-1
			} else if runBest != none {
				best[i][j], from[i][j] = runBest+gain, runAt
			}
		}
	}
	end, score := -1, none
	for j := m - 1; j < n; j++ {
		if best[m-1][j] > score {
			score, end = best[m-1][j], j
		}
	}
	if end < 0 {
		return Match{}, false
	}
	pos := make([]int, m)
	for i, j := m-1, end; i >= 0; i-- {
		pos[i] = j
		j = from[i][j]
	}
	return Match{Score: score, Positions: pos}, true
}

// Ranked is one matching candidate: its index in the input and its match.
type Ranked struct {
	Index int
	Match Match
}

// FuzzyRank returns the candidates that match, best first; ties (and an
// empty query) keep the original order.
func FuzzyRank(query string, candidates []string) []Ranked {
	out := []Ranked{}
	for i, c := range candidates {
		if m, ok := FuzzyMatch(query, c); ok {
			out = append(out, Ranked{i, m})
		}
	}
	sort.SliceStable(out, func(a, b int) bool { return out[a].Match.Score > out[b].Match.Score })
	return out
}

// RankPeople fuzzy-matches People on "name username team". Positions index
// that search text; the display name is its prefix, so positions below
// len([]rune(p.Name)) fall in the name.
func RankPeople(query string) []Ranked {
	texts := make([]string, len(People))
	for i, p := range People {
		texts[i] = p.SearchText()
	}
	return FuzzyRank(query, texts)
}

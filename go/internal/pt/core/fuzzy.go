package core

// Small fuzzy matcher for comboboxes and the command palette (same
// algorithm as the Rust core). Case-insensitive subsequence match; a
// contiguous substring scores best; otherwise matched characters score more
// at word starts or right after the previous match. Indices are rune
// indices into the candidate, for highlighting.

import (
	"sort"
	"unicode"
)

type Match struct {
	Score   int
	Indices []int
}

func isAlnum(r rune) bool { return unicode.IsLetter(r) || unicode.IsDigit(r) }

func isBoundary(prev *rune, c rune) bool {
	if prev == nil {
		return true
	}
	p := *prev
	return !isAlnum(p) || (unicode.IsLower(p) && unicode.IsUpper(c)) || (unicode.IsLetter(p) && unicode.IsDigit(c))
}

func lowerRune(r rune) rune { return unicode.ToLower(r) }

// FuzzyMatch returns nil when pattern isn't a subsequence of candidate.
func FuzzyMatch(pattern, candidate string) *Match {
	var pat []rune
	for _, r := range pattern {
		if !unicode.IsSpace(r) {
			pat = append(pat, lowerRune(r))
		}
	}
	if len(pat) == 0 {
		return &Match{Score: 0, Indices: []int{}}
	}
	orig := []rune(candidate)
	low := make([]rune, len(orig))
	for i, r := range orig {
		low[i] = lowerRune(r)
	}
	prevOf := func(i int) *rune {
		if i == 0 {
			return nil
		}
		return &orig[i-1]
	}

	// 1. Contiguous substring.
	if len(pat) <= len(low) {
		bestScore, bestStart, found := 0, 0, false
		for start := 0; start <= len(low)-len(pat); start++ {
			eq := true
			for k := range pat {
				if low[start+k] != pat[k] {
					eq = false
					break
				}
			}
			if !eq {
				continue
			}
			bonus := 0
			if start == 0 {
				bonus = 40
			} else if isBoundary(prevOf(start), orig[start]) {
				bonus = 25
			}
			score := 100 + len(pat)*10 + bonus - start
			if !found || score > bestScore {
				bestScore, bestStart, found = score, start, true
			}
		}
		if found {
			idx := make([]int, len(pat))
			for k := range idx {
				idx[k] = bestStart + k
			}
			return &Match{Score: bestScore, Indices: idx}
		}
	}

	// 2. Subsequence, preferring word starts.
	indices := make([]int, 0, len(pat))
	pos := 0
	for pi, pc := range pat {
		remaining := len(pat) - pi - 1
		chosen := -1
		for i := pos; i < len(low); i++ {
			if len(low)-i-1 < remaining {
				break
			}
			if low[i] == pc && isBoundary(prevOf(i), orig[i]) {
				chosen = i
				break
			}
		}
		if len(indices) > 0 {
			next := indices[len(indices)-1] + 1
			if next < len(low) && low[next] == pc {
				chosen = next
			}
		}
		if chosen < 0 {
			for i := pos; i < len(low); i++ {
				if low[i] == pc {
					chosen = i
					break
				}
			}
		}
		if chosen < 0 {
			return nil
		}
		indices = append(indices, chosen)
		pos = chosen + 1
	}
	score := 0
	for k, i := range indices {
		score += 10
		if isBoundary(prevOf(i), orig[i]) {
			score += 15
		}
		if k > 0 && indices[k-1]+1 == i {
			score += 8
		}
	}
	score -= indices[0]
	score -= (len(orig) - len(pat)) / 4
	return &Match{Score: score, Indices: indices}
}

type Ranked struct {
	Index int
	Match Match
}

// Rank returns matching candidates, best first; ties keep input order.
func Rank(pattern string, candidates []string) []Ranked {
	var out []Ranked
	for i, c := range candidates {
		if m := FuzzyMatch(pattern, c); m != nil {
			out = append(out, Ranked{i, *m})
		}
	}
	sort.SliceStable(out, func(a, b int) bool { return out[a].Match.Score > out[b].Match.Score })
	return out
}

// Package textutil holds small text helpers shared by the parser, the index
// and the CLI: token estimation, folding for case/diacritic-insensitive
// matching, and edit distance for "did you mean" suggestions.
package textutil

import (
	"fmt"
	"strings"
	"unicode"
	"unicode/utf8"

	"golang.org/x/text/unicode/norm"
)

// Tokens estimates how many LLM tokens s costs. Real tokenizers average
// ~4 chars/token for English and ~3 for Turkish and code; 3.5 runes/token is a
// deliberately slightly pessimistic middle ground.
func Tokens(s string) int {
	n := utf8.RuneCountInString(s)
	if n == 0 {
		return 0
	}
	return (n*2 + 6) / 7
}

// FmtTokens renders a token count the way every pg output does: ~45t, ~2.1kt.
func FmtTokens(n int) string {
	if n < 1000 {
		return fmt.Sprintf("~%dt", n)
	}
	return fmt.Sprintf("~%.1fkt", float64(n)/1000)
}

// Fold lowercases s and strips diacritics so that "Çözüm", "cozum" and
// "ÇÖZÜM" compare equal. Turkish dotless ı is mapped to i as well.
func Fold(s string) string {
	var b strings.Builder
	b.Grow(len(s))
	for _, r := range norm.NFD.String(s) {
		if unicode.Is(unicode.Mn, r) {
			continue
		}
		switch r {
		case 'ı', 'I':
			r = 'i'
		default:
			r = unicode.ToLower(r)
		}
		b.WriteRune(r)
	}
	return b.String()
}

// Levenshtein returns the edit distance between a and b (rune based).
func Levenshtein(a, b string) int {
	ra, rb := []rune(a), []rune(b)
	if len(ra) == 0 {
		return len(rb)
	}
	if len(rb) == 0 {
		return len(ra)
	}
	prev := make([]int, len(rb)+1)
	cur := make([]int, len(rb)+1)
	for j := range prev {
		prev[j] = j
	}
	for i := 1; i <= len(ra); i++ {
		cur[0] = i
		for j := 1; j <= len(rb); j++ {
			cost := 1
			if ra[i-1] == rb[j-1] {
				cost = 0
			}
			cur[j] = min(prev[j]+1, cur[j-1]+1, prev[j-1]+cost)
		}
		prev, cur = cur, prev
	}
	return prev[len(rb)]
}

// Closest returns up to n candidates closest to q (folded comparison), best
// first. Candidates that contain q as a substring rank ahead of pure edit
// distance matches. Anything wildly different is dropped.
func Closest(q string, candidates []string, n int) []string {
	fq := Fold(q)
	type scored struct {
		s string
		d int
	}
	var out []scored
	seen := map[string]bool{}
	for _, c := range candidates {
		if seen[c] {
			continue
		}
		seen[c] = true
		fc := Fold(c)
		d := Levenshtein(fq, fc)
		if strings.Contains(fc, fq) || strings.Contains(fq, fc) {
			d = d / 4
		}
		limit := max(3, utf8.RuneCountInString(fq)/2)
		if d > limit {
			continue
		}
		out = append(out, scored{c, d})
	}
	// insertion sort: candidate lists are small
	for i := 1; i < len(out); i++ {
		for j := i; j > 0 && (out[j].d < out[j-1].d || (out[j].d == out[j-1].d && out[j].s < out[j-1].s)); j-- {
			out[j], out[j-1] = out[j-1], out[j]
		}
	}
	res := make([]string, 0, n)
	for i := 0; i < len(out) && i < n; i++ {
		res = append(res, out[i].s)
	}
	return res
}

// OneLine collapses whitespace runs (including newlines) into single spaces
// and trims the result to at most max runes, adding an ellipsis if cut.
func OneLine(s string, max int) string {
	s = strings.Join(strings.Fields(s), " ")
	if max > 0 && utf8.RuneCountInString(s) > max {
		r := []rune(s)
		s = strings.TrimSpace(string(r[:max-1])) + "..."
	}
	return s
}

// Package errsig turns raw error output into stable signatures. The same
// failure arrives with different paths, line numbers, ports, hashes and
// timestamps every time; normalising those away lets pg recognise it.
package errsig

import (
	"crypto/sha256"
	"encoding/hex"
	"regexp"
	"strings"
)

var (
	reANSI    = regexp.MustCompile(`\x1b\[[0-9;]*[A-Za-z]`)
	reUUID    = regexp.MustCompile(`(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b`)
	reTime    = regexp.MustCompile(`\b\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}(:\d{2}(\.\d+)?)?(Z|[+-]\d{2}:?\d{2})?\b|\b\d{2}:\d{2}:\d{2}(\.\d+)?\b|\b\d{4}/\d{2}/\d{2}\b|\b\d{4}-\d{2}-\d{2}\b`)
	reHex     = regexp.MustCompile(`(?i)\b0x[0-9a-f]+\b|\b[0-9a-f]*[a-f][0-9a-f]*[0-9][0-9a-f]*\b|\b[0-9a-f]*[0-9][0-9a-f]*[a-f][0-9a-f]*\b`)
	reWinPath = regexp.MustCompile(`(?i)\b[a-z]:[\\/](?:[^\s:"'<>|*?]+[\\/])*([^\s:"'<>|*?\\/]+)`)
	reNixPath = regexp.MustCompile(`(?:\.{1,2}/|/)?(?:[\w.@+-]+/)+([\w.@+-]+)`)
	reLoc     = regexp.MustCompile(`(\.[A-Za-z0-9]+)(?::\d+){1,2}\b|\(\d+,\s*\d+\)|\bline \d+(, column \d+)?`)
	reQuoted  = regexp.MustCompile("\"[^\"\n]{0,200}\"|'[^'\n]{1,200}'|`[^`\n]{1,200}`")
	reNum     = regexp.MustCompile(`\b\d+(\.\d+)?(ms|s|m|h|kb|mb|gb|b)?\b`)
	reSpace   = regexp.MustCompile(`\s+`)
)

// Normalize returns the signature of one error line: lower case, with
// paths reduced to file names and variable parts replaced by <loc>, <n>,
// <hex>, <uuid>, <ts> and <v>.
func Normalize(s string) string {
	s = reANSI.ReplaceAllString(s, "")
	s = reUUID.ReplaceAllString(s, "<uuid>")
	s = reTime.ReplaceAllString(s, "<ts>")
	s = reWinPath.ReplaceAllString(s, "$1")
	s = reNixPath.ReplaceAllStringFunc(s, func(p string) string {
		// keep things that only look like paths: "a/b" ratios, "and/or"
		if !strings.Contains(p, ".") && strings.Count(p, "/") < 2 {
			return p
		}
		return p[strings.LastIndex(p, "/")+1:]
	})
	s = reLoc.ReplaceAllString(s, "$1<loc>")
	s = reQuoted.ReplaceAllString(s, "<v>")
	s = reHex.ReplaceAllStringFunc(s, func(h string) string {
		// only long runs are hashes or addresses; "a1" stays
		if len(h) >= 7 || strings.HasPrefix(strings.ToLower(h), "0x") {
			return "<hex>"
		}
		return h
	})
	s = reNum.ReplaceAllString(s, "<n>")
	s = strings.ToLower(reSpace.ReplaceAllString(strings.TrimSpace(s), " "))
	if len(s) > 240 {
		s = s[:240]
	}
	return s
}

// Hash is the exact-match key of a normalised signature.
func Hash(norm string) string {
	h := sha256.Sum256([]byte(norm))
	return hex.EncodeToString(h[:8])
}

var reErrorish = regexp.MustCompile(`(?i)\b(error|errors|err|fail|failed|failure|panic|exception|fatal|cannot|can't|could not|unable|undefined|not found|no such|denied|refused|mismatch|invalid|timeout|timed out|unexpected|expected|missing|locked|violation|overflow|deadlock|segmentation)\b|\bE[A-Z]{3,}\b`)

var reNoise = regexp.MustCompile(`(?i)^(fail\s*$|fail\s+\S+\s+[\d.]+s$|ok\s|---\s*pass|=== run|exit status \d+$|\?\s|make: \*\*\*|error: process completed with exit code)`)

// Lines picks the lines of command output that look like errors, most
// specific first, with their normalised form. At most max lines.
func Lines(output string, max int) []Line {
	var out []Line
	seen := map[string]bool{}
	for _, raw := range strings.Split(strings.ReplaceAll(output, "\r\n", "\n"), "\n") {
		t := strings.TrimSpace(reANSI.ReplaceAllString(raw, ""))
		if len(t) < 6 || reNoise.MatchString(t) || !reErrorish.MatchString(t) {
			continue
		}
		n := Normalize(t)
		if seen[n] {
			continue
		}
		seen[n] = true
		out = append(out, Line{Raw: t, Norm: n})
		if len(out) >= max {
			break
		}
	}
	return out
}

// Line is one error line of output.
type Line struct {
	Raw  string
	Norm string
}

// Words returns the meaningful words of a normalised signature: no
// placeholders, no short or very common words.
func Words(norm string) []string {
	var out []string
	for _, w := range strings.FieldsFunc(norm, func(r rune) bool {
		return !(r >= 'a' && r <= 'z' || r >= '0' && r <= '9' || r == '_' || r > 127)
	}) {
		if len(w) < 3 || stop[w] {
			continue
		}
		out = append(out, w)
	}
	return out
}

var stop = map[string]bool{"the": true, "and": true, "for": true, "with": true, "was": true, "not": false,
	"loc": true, "hex": true, "uuid": true, "got": true, "want": true, "from": true, "this": true, "that": true, "are": true, "has": true}

// Overlap is the share of a's words found in b (0..1).
func Overlap(a, b []string) float64 {
	if len(a) == 0 {
		return 0
	}
	in := map[string]bool{}
	for _, w := range b {
		in[w] = true
	}
	n := 0
	for _, w := range a {
		if in[w] {
			n++
		}
	}
	return float64(n) / float64(len(a))
}

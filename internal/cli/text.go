package cli

import (
	"bytes"
	"crypto/rand"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"unicode/utf16"

	"github.com/Utku4836/Paragma/internal/textutil"
)

// Text helpers: input decoding, line surgery, front matter, names and ids.

// inputText returns the text a write command was given: the arguments
// joined by spaces, standard input when the only argument is "-", or the
// file named by --file. Files may be UTF-8 or UTF-16 (PowerShell's ">"
// writes UTF-16), with or without BOM. The result uses "\n" line ends and
// has no trailing newline.
func (a *app) inputText(args []string, required bool) (string, error) {
	var raw []byte
	switch {
	case a.textFile != "":
		b, err := os.ReadFile(a.textFile)
		if err != nil {
			return "", fail(ExitUsage, "--file: %v", err)
		}
		raw = b
	case len(args) == 1 && args[0] == "-":
		b, err := io.ReadAll(a.stdin)
		if err != nil {
			return "", fail(ExitUsage, "stdin: %v", err)
		}
		raw = b
	default:
		raw = []byte(strings.Join(args, " "))
	}
	s := decodeText(raw)
	s = strings.ReplaceAll(s, "\r\n", "\n")
	s = strings.TrimRight(s, "\n ")
	if required && strings.TrimSpace(s) == "" {
		return "", fail(ExitUsage, "no text given: pass it as arguments, '-' for stdin, or --file PATH")
	}
	return s, nil
}

func decodeText(b []byte) string {
	switch {
	case bytes.HasPrefix(b, []byte{0xEF, 0xBB, 0xBF}):
		return string(b[3:])
	case bytes.HasPrefix(b, []byte{0xFF, 0xFE}):
		return utf16String(b[2:], false)
	case bytes.HasPrefix(b, []byte{0xFE, 0xFF}):
		return utf16String(b[2:], true)
	}
	return string(b)
}

func utf16String(b []byte, bigEndian bool) string {
	u := make([]uint16, 0, len(b)/2)
	for i := 0; i+1 < len(b); i += 2 {
		if bigEndian {
			u = append(u, uint16(b[i])<<8|uint16(b[i+1]))
		} else {
			u = append(u, uint16(b[i+1])<<8|uint16(b[i]))
		}
	}
	return string(utf16.Decode(u))
}

func splitText(s string) []string {
	if s == "" {
		return nil
	}
	return strings.Split(s, "\n")
}

func splitList(vals []string) []string {
	var out []string
	for _, v := range vals {
		for _, p := range strings.Split(v, ",") {
			if p = strings.TrimSpace(p); p != "" {
				out = append(out, p)
			}
		}
	}
	return out
}

func isBlank(s string) bool { return strings.TrimSpace(s) == "" }

func isListLine(s string) bool {
	t := strings.TrimLeft(s, " \t")
	if len(t) >= 2 && strings.ContainsRune("-*+", rune(t[0])) && t[1] == ' ' {
		return true
	}
	i := 0
	for i < len(t) && t[i] >= '0' && t[i] <= '9' {
		i++
	}
	return i > 0 && i+1 < len(t) && (t[i] == '.' || t[i] == ')') && t[i+1] == ' '
}

func isQuoteLine(s string) bool { return strings.HasPrefix(strings.TrimLeft(s, " \t"), ">") }

func isHeadingLine(s string) bool {
	t := strings.TrimLeft(s, " ")
	return strings.HasPrefix(t, "#") && len(strings.TrimLeft(t, "#")) < len(t) && (len(strings.TrimLeft(t, "#")) == 0 || strings.HasPrefix(strings.TrimLeft(t, "#"), " "))
}

// sticky reports whether two adjacent lines belong to one Markdown block
// (consecutive list items, consecutive quote lines) and so need no blank
// line between them.
func sticky(prev, next string) bool {
	return (isListLine(prev) || strings.HasPrefix(prev, " ") || strings.HasPrefix(prev, "\t")) && isListLine(next) ||
		isQuoteLine(prev) && isQuoteLine(next) && !strings.HasPrefix(strings.TrimLeft(next, " >"), "[!")
}

// place inserts block after line `after` (1-based; 0 = top of file) and
// adds blank lines where Markdown needs them to keep blocks apart. It
// returns the new lines and the 1-based line where block starts.
func place(lines []string, after int, block []string) ([]string, int) {
	if len(block) == 0 {
		return lines, after + 1
	}
	var out []string
	out = append(out, lines[:after]...)
	if after > 0 && !isBlank(lines[after-1]) && !sticky(lines[after-1], block[0]) {
		out = append(out, "")
	}
	start := len(out) + 1
	out = append(out, block...)
	if after < len(lines) && !isBlank(lines[after]) && !sticky(block[len(block)-1], lines[after]) {
		out = append(out, "")
	}
	out = append(out, lines[after:]...)
	return out, start
}

// replaceRange replaces lines [from,to] (1-based, inclusive).
func replaceRange(lines []string, from, to int, block []string) []string {
	var out []string
	out = append(out, lines[:from-1]...)
	out = append(out, block...)
	out = append(out, lines[to:]...)
	return out
}

// lastLine is the last non-blank line in [from,to], or from-1 if all blank.
func lastLine(lines []string, from, to int) int {
	for i := min(to, len(lines)); i >= from; i-- {
		if !isBlank(lines[i-1]) {
			return i
		}
	}
	return from - 1
}

// setFront sets key: value in the note's front matter, adding a front
// matter block if there is none. An empty value removes the key.
func setFront(lines []string, key, value string) []string {
	end := -1
	if len(lines) > 0 && strings.TrimSpace(lines[0]) == "---" {
		for i := 1; i < len(lines); i++ {
			if t := strings.TrimSpace(lines[i]); t == "---" || t == "..." {
				end = i
				break
			}
		}
	}
	kv := key + ": " + value
	if end < 0 {
		if value == "" {
			return lines
		}
		return append([]string{"---", kv, "---"}, lines...)
	}
	for i := 1; i < end; i++ {
		if strings.HasPrefix(lines[i], key+":") {
			if value == "" {
				return append(lines[:i:i], lines[i+1:]...)
			}
			out := append([]string(nil), lines...)
			out[i] = kv
			return out
		}
	}
	if value == "" {
		return lines
	}
	out := append([]string(nil), lines[:end]...)
	out = append(out, kv)
	return append(out, lines[end:]...)
}

// frontValue reads a scalar from a note's front matter.
func frontValue(lines []string, key string) string {
	if len(lines) == 0 || strings.TrimSpace(lines[0]) != "---" {
		return ""
	}
	for _, l := range lines[1:] {
		if strings.TrimSpace(l) == "---" {
			break
		}
		if k, v, ok := strings.Cut(l, ":"); ok && strings.TrimSpace(k) == key {
			return strings.Trim(strings.TrimSpace(v), `"'`)
		}
	}
	return ""
}

// slug makes a file name from a title: folded, lowercase, dashes.
func slug(title string) string {
	f := textutil.Fold(title)
	var b strings.Builder
	dash := false
	for _, r := range f {
		if r >= 'a' && r <= 'z' || r >= '0' && r <= '9' {
			b.WriteRune(r)
			dash = false
		} else if !dash && b.Len() > 0 {
			b.WriteByte('-')
			dash = true
		}
	}
	s := strings.TrimRight(b.String(), "-")
	if len(s) > 60 {
		s = strings.TrimRight(s[:60], "-")
	}
	if s == "" {
		s = "note"
	}
	return s
}

// freePath returns rel, or rel with -2, -3... before .md if it exists.
func freePath(s *store, rel string) string {
	base := strings.TrimSuffix(rel, ".md")
	cand := rel
	for i := 2; ; i++ {
		if _, err := os.Stat(filepath.Join(s.v.Root, filepath.FromSlash(cand))); os.IsNotExist(err) {
			return cand
		}
		cand = fmt.Sprintf("%s-%d.md", base, i)
	}
}

const idChars = "0123456789abcdefghijklmnopqrstuvwxyz"

// newBlockID returns a short block id with the given prefix ("t" for tasks)
// that is not used anywhere in the space nor in taken.
func newBlockID(s *store, prefix string, taken map[string]bool) string {
	n := 3
	for tries := 0; ; tries++ {
		if tries > 20 {
			n++
			tries = 0
		}
		b := make([]byte, n)
		rand.Read(b)
		for i := range b {
			b[i] = idChars[int(b[i])%len(idChars)]
		}
		id := prefix + string(b)
		if !taken[id] && !s.db.BlockExists(id) {
			taken[id] = true
			return id
		}
	}
}

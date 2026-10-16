package cli

import (
	"bytes"
	"crypto/rand"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"time"
	"unicode/utf16"

	"github.com/Utku-Private/Paragma/internal/addr"
	"github.com/Utku-Private/Paragma/internal/index"
	"github.com/Utku-Private/Paragma/internal/md"
	"github.com/Utku-Private/Paragma/internal/textutil"
	"github.com/Utku-Private/Paragma/internal/vault"
)

// ---- text input ----

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

// ---- locking ----

// withLock runs fn while holding .paragma/lock, so concurrent agents never
// interleave read-modify-write cycles on the same vault.
func (a *app) withLock(s *store, fn func() error) error {
	p := filepath.Join(s.v.Root, "lock")
	deadline := time.Now().Add(5 * time.Second)
	for {
		f, err := os.OpenFile(p, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o644)
		if err == nil {
			fmt.Fprintf(f, "%d %s\n", os.Getpid(), time.Now().Format(time.RFC3339))
			f.Close()
			break
		}
		if st, serr := os.Stat(p); serr == nil && time.Since(st.ModTime()) > 30*time.Second {
			os.Remove(p) // left behind by a crashed process
			continue
		}
		if time.Now().After(deadline) {
			return fail(ExitConflict, "vault is locked by another pg process (%s); retry", filepath.ToSlash(p))
		}
		time.Sleep(20 * time.Millisecond)
	}
	defer os.Remove(p)
	return fn()
}

// ---- editing a note ----

// note is a file loaded for editing.
type note struct {
	s     *store
	rel   string // path relative to the vault root
	lines []string
	orig  []string
	crlf  bool
	isNew bool
}

func (n *note) addr() string { return strings.TrimSuffix(n.rel, ".md") }

// load reads a note for editing and enforces --if-hash.
func (a *app) load(s *store, f *index.File) (*note, error) {
	b, err := os.ReadFile(filepath.Join(s.v.Root, filepath.FromSlash(f.Path)))
	if err != nil {
		return nil, fail(ExitInternal, "read %s: %v", f.Path, err)
	}
	if a.ifHash != "" && !strings.HasPrefix(index.HashBytes(b), strings.TrimPrefix(a.ifHash, "hash:")) {
		return nil, fail(ExitConflict, "%s changed since you read it (hash is now %s); read it again", f.Addr(), index.HashBytes(b))
	}
	lines := md.SplitLines(b)
	return &note{s: s, rel: f.Path, lines: lines, orig: append([]string(nil), lines...), crlf: bytes.Contains(b, []byte("\r\n"))}, nil
}

// newNote starts a note that does not exist yet.
func newNote(s *store, rel string, lines []string) *note {
	return &note{s: s, rel: rel, lines: lines, isNew: true}
}

// save writes a changed note (or, with --dry-run, prints what would change)
// and re-indexes it.
func (a *app) save(n *note) error {
	if !n.isNew && equalLines(n.lines, n.orig) {
		return nil
	}
	if a.dryRun {
		a.printDiff(n)
		return nil
	}
	sep := "\n"
	if n.crlf {
		sep = "\r\n"
	}
	data := strings.Join(n.lines, sep) + sep
	p := filepath.Join(n.s.v.Root, filepath.FromSlash(n.rel))
	if n.isNew {
		if _, err := os.Stat(p); err == nil {
			return fail(ExitConflict, "%s already exists", n.addr())
		}
	}
	if err := vault.WriteFileAtomic(p, []byte(data)); err != nil {
		return fail(ExitInternal, "write %s: %v", n.rel, err)
	}
	return a.reindex(n.s, n.rel)
}

// reindex refreshes the index and resolver for files pg just changed.
func (a *app) reindex(s *store, rels ...string) error {
	if a.dryRun {
		return nil
	}
	if err := s.db.IndexFile(rels...); err != nil {
		return fail(ExitInternal, "index: %v", err)
	}
	res, err := addr.NewResolver(s.db)
	if err != nil {
		return fail(ExitInternal, "%v", err)
	}
	s.res = res
	return nil
}

func equalLines(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

// printDiff shows the changed region of a --dry-run edit.
func (a *app) printDiff(n *note) {
	o, w := n.orig, n.lines
	pre := 0
	for pre < len(o) && pre < len(w) && o[pre] == w[pre] {
		pre++
	}
	suf := 0
	for suf < len(o)-pre && suf < len(w)-pre && o[len(o)-1-suf] == w[len(w)-1-suf] {
		suf++
	}
	verb := "would change"
	if n.isNew {
		verb = "would create"
	}
	a.printf("%s %s at L%d\n", verb, n.addr(), pre+1)
	for _, l := range o[pre : len(o)-suf] {
		a.println("- " + l)
	}
	for _, l := range w[pre : len(w)-suf] {
		a.println("+ " + l)
	}
}

// ---- line surgery ----

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

// ---- front matter ----

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

// ---- names and ids ----

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

// freePath returns rel, or rel with -2, -3… before .md if it exists.
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
// that is not used anywhere in the vault nor in taken.
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

func now() time.Time { return time.Now() }
func today() string  { return now().Format("2006-01-02") }
func clock() string  { return now().Format("15:04") }
func stamp() string  { return now().Format("2006-01-02 15:04") }
func (a *app) ok(s string) {
	switch {
	case a.dryRun:
	case a.quiet:
		a.println(strings.Fields(s)[0]) // the address alone
	default:
		a.println("ok " + s)
	}
}

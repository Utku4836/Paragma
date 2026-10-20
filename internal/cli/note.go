package cli

import (
	"bytes"
	"fmt"
	"math/rand/v2"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/Utku4836/Paragma/internal/addr"
	"github.com/Utku4836/Paragma/internal/index"
	"github.com/Utku4836/Paragma/internal/md"
	"github.com/Utku4836/Paragma/internal/space"
)

// A note loaded for editing, the space lock, and writing changes back.

// note is a file loaded for editing.
type note struct {
	s     *store
	rel   string // path relative to the space root
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
	// the line numbers this command works with come from f: if another pg
	// process changed the note since, they point at the wrong lines
	if a.inLock && f.Hash != "" {
		if h := index.HashBytes(b); h != f.Hash && a.written[f.Path] != h {
			return nil, errStale
		}
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
	if err := space.WriteFileAtomic(p, []byte(data)); err != nil {
		return fail(ExitInternal, "write %s: %v", n.rel, err)
	}
	if a.written == nil {
		a.written = map[string]string{}
	}
	a.written[n.rel] = index.HashBytes([]byte(data))
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

// withLock runs fn while holding the space's .pg/lock, so concurrent agents
// never interleave read-modify-write cycles on the same space.
func (a *app) withLock(s *store, fn func() error) error {
	if a.heldLock == "" {
		p := s.v.Meta("lock")
		if err := acquireLock(p); err != nil {
			return err
		}
		defer os.Remove(p)
	}
	a.inLock = true
	defer func() { a.inLock = false }()
	return fn()
}

// acquireLock takes the space's lock file, waiting for other pg processes.
func acquireLock(p string) error {
	deadline := time.Now().Add(30 * time.Second)
	for {
		f, err := os.OpenFile(p, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o644)
		if err == nil {
			fmt.Fprintf(f, "%d %s\n", os.Getpid(), time.Now().Format(time.RFC3339))
			f.Close()
			return nil
		}
		if st, serr := os.Stat(p); serr == nil && time.Since(st.ModTime()) > 30*time.Second {
			os.Remove(p) // left behind by a crashed process
			continue
		}
		if time.Now().After(deadline) {
			return fail(ExitConflict, "space is locked by another pg process (%s); retry", filepath.ToSlash(p))
		}
		time.Sleep(time.Duration(10+rand.IntN(20)) * time.Millisecond)
	}
}

// fileLines reads a note from disk, split exactly like the parser splits it.
func fileLines(s *store, f *index.File) ([]string, error) {
	b, err := os.ReadFile(filepath.Join(s.v.Root, filepath.FromSlash(f.Path)))
	if err != nil {
		return nil, fail(ExitInternal, "read %s: %v", f.Path, err)
	}
	return md.SplitLines(b), nil
}

func sliceLines(lines []string, from, to int) []string {
	from = max(from, 1)
	to = min(to, len(lines))
	if from > to {
		return nil
	}
	return lines[from-1 : to]
}

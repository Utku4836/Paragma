// Package addr parses and resolves Paragma addresses:
//
//	note             whole file (path without .md, or any unique path suffix)
//	note#Heading     a heading and everything under it
//	note#Up/Down     nested heading path (any unique suffix of the path works)
//	note#Heading[2]  the 2nd heading with that path
//	note^blk         one block (paragraph, list item, task, callout)
//	^blk             a block anywhere in the space, if the id is unique
//	note:10-24       a line range (also :10 and :10-)
//	@ULID            a file by id (unique prefix of 6+ chars is enough)
package addr

import (
	"fmt"
	"strconv"
	"strings"

	"github.com/Utku-Private/Paragma/internal/index"
	"github.com/Utku-Private/Paragma/internal/space"
	"github.com/Utku-Private/Paragma/internal/textutil"
)

// Kind of target an address points at.
type Kind int

const (
	KindFile Kind = iota
	KindSection
	KindBlock
	KindLines
)

// Addr is a parsed, unresolved address.
type Addr struct {
	Raw     string
	File    string
	ULID    string
	Heading string
	Nth     int
	Block   string
	From    int
	To      int // 0 = to end of file
}

// Error carries an exit code and a message that tells the agent what to do.
type Error struct {
	Code int
	Msg  string
}

func (e *Error) Error() string { return e.Msg }

const (
	CodeUsage    = 2
	CodeNotFound = 3
)

func usage(f string, a ...any) error    { return &Error{CodeUsage, fmt.Sprintf(f, a...)} }
func notFound(f string, a ...any) error { return &Error{CodeNotFound, fmt.Sprintf(f, a...)} }

// Parse splits an address into its parts.
func Parse(s string) (Addr, error) {
	a := Addr{Raw: s}
	s = strings.TrimSpace(s)
	if strings.HasPrefix(s, "[[") && strings.HasSuffix(s, "]]") {
		s = strings.TrimSuffix(strings.TrimPrefix(s, "[["), "]]")
		s = strings.SplitN(s, "|", 2)[0]
	}
	if s == "" {
		return a, usage("empty address")
	}
	if strings.HasPrefix(s, "@") {
		a.ULID = strings.ToUpper(strings.TrimPrefix(s, "@"))
		if i := strings.IndexAny(a.ULID, "#^:"); i >= 0 {
			rest := s[1+i:]
			a.ULID = a.ULID[:i]
			return parseSub(a, rest)
		}
		return a, nil
	}
	i := strings.IndexAny(s, "#^")
	// a line range only counts as one when the suffix is numeric; Windows
	// drive letters and headings with colons stay intact
	if j := strings.LastIndex(s, ":"); j > 0 && (i < 0 || j < i) && isRange(s[j+1:]) {
		a.File = s[:j]
		return parseRange(a, s[j+1:])
	}
	if i < 0 {
		a.File = s
		return a, nil
	}
	a.File = s[:i]
	return parseSub(a, s[i:])
}

func parseSub(a Addr, rest string) (Addr, error) {
	switch {
	case strings.HasPrefix(rest, "^"):
		a.Block = strings.TrimPrefix(rest, "^")
		if a.Block == "" {
			return a, usage("empty block id in %q", a.Raw)
		}
	case strings.HasPrefix(rest, "#"):
		// everything after # is heading text, colons included ("Saat: 10:30")
		h := strings.TrimPrefix(rest, "#")
		if strings.HasSuffix(h, "]") {
			if k := strings.LastIndex(h, "["); k > 0 {
				if n, err := strconv.Atoi(h[k+1 : len(h)-1]); err == nil && n > 0 {
					a.Nth = n
					h = h[:k]
				}
			}
		}
		a.Heading = strings.Trim(strings.TrimSpace(h), "/")
		if a.Heading == "" {
			return a, usage("empty heading in %q", a.Raw)
		}
	case strings.HasPrefix(rest, ":"):
		return parseRange(a, rest[1:])
	}
	return a, nil
}

func isRange(s string) bool {
	if s == "" {
		return false
	}
	for _, r := range s {
		if (r < '0' || r > '9') && r != '-' {
			return false
		}
	}
	return s[0] != '-'
}

func parseRange(a Addr, s string) (Addr, error) {
	from, to, dash := strings.Cut(s, "-")
	f, err := strconv.Atoi(from)
	if err != nil || f < 1 {
		return a, usage("bad line range %q (use :10-24, :10 or :10-)", a.Raw)
	}
	a.From = f
	switch {
	case !dash:
		a.To = f
	case to == "":
		a.To = 0
	default:
		t, err := strconv.Atoi(to)
		if err != nil || t < f {
			return a, usage("bad line range %q (use :10-24, :10 or :10-)", a.Raw)
		}
		a.To = t
	}
	return a, nil
}

// Target is a resolved address.
type Target struct {
	Kind    Kind
	File    *index.File
	Section *index.Section // for KindSection
	Block   string         // for KindBlock
	From    int            // 1-based inclusive line range
	To      int
}

// Addr renders the canonical, copy-pasteable address of the target.
func (t *Target) Addr() string {
	base := t.File.Addr()
	switch t.Kind {
	case KindSection:
		if t.Section.Path == "" || t.Section.IsTitle {
			return base
		}
		return base + "#" + t.Section.Path
	case KindBlock:
		return base + "^" + t.Block
	case KindLines:
		if t.From == t.To {
			return fmt.Sprintf("%s:%d", base, t.From)
		}
		return fmt.Sprintf("%s:%d-%d", base, t.From, t.To)
	}
	return base
}

// Resolver resolves addresses against one index.
type Resolver struct {
	DB    *index.DB
	files []*index.File
	// Prefer is the current project: when a name or block id matches notes
	// in several projects, the one in this project wins.
	Prefer string
	keys   map[string][]*index.File   // every path suffix -> files ending in it
	secs   map[int64][]*index.Section // per-file section cache
}

func NewResolver(db *index.DB) (*Resolver, error) {
	fs, err := db.Files()
	if err != nil {
		return nil, err
	}
	return &Resolver{DB: db, files: fs, secs: map[int64][]*index.Section{}}, nil
}

// buildKeys indexes every path suffix; built on first use since commands
// like search never resolve a file name.
func (r *Resolver) buildKeys() {
	r.keys = make(map[string][]*index.File, len(r.files)*3)
	for _, f := range r.files {
		k := index.FileKey(f.Path)
		for {
			r.keys[k] = append(r.keys[k], f)
			i := strings.Index(k, "/")
			if i < 0 {
				break
			}
			k = k[i+1:]
		}
	}
}

// Sections returns a file's sections, cached for the resolver's lifetime.
func (r *Resolver) Sections(f *index.File) ([]*index.Section, error) {
	if s, ok := r.secs[f.ID]; ok {
		return s, nil
	}
	s, err := r.DB.Sections(f.ID)
	if err != nil {
		return nil, err
	}
	r.secs[f.ID] = s
	return s, nil
}

// Files returns all files known to the resolver.
func (r *Resolver) Files() []*index.File { return r.files }

// Resolve turns a parsed address into a concrete target.
func (r *Resolver) Resolve(a Addr) (*Target, error) {
	var f *index.File
	var err error
	switch {
	case a.ULID != "":
		f, err = r.byULID(a.ULID)
	case a.File == "" && a.Block != "":
		return r.globalBlock(a)
	default:
		f, err = r.ResolveFile(a.File)
	}
	if err != nil {
		return nil, err
	}
	lines := f.NLines
	t := &Target{Kind: KindFile, File: f, From: min(f.BodyLine, max(lines, 1)), To: lines}
	switch {
	case a.Heading != "":
		s, err := r.section(f, a)
		if err != nil {
			return nil, err
		}
		t.Kind, t.Section, t.From, t.To = KindSection, s, s.Line, s.End
	case a.Block != "":
		bs, err := r.DB.Blocks(a.Block, f.ID)
		if err != nil {
			return nil, err
		}
		if len(bs) == 0 {
			msg := fmt.Sprintf("not found: %s^%s", f.Addr(), a.Block)
			if c := textutil.Closest(a.Block, r.DB.BlockIDs(f.ID), 3); len(c) > 0 {
				msg += " — did you mean: " + prefixAll(f.Addr()+"^", c)
			}
			return nil, notFound("%s", msg)
		}
		t.Kind, t.Block, t.From, t.To = KindBlock, bs[0].ID, bs[0].Line, bs[0].End
	case a.From > 0:
		to := a.To
		if to == 0 || to > lines {
			to = lines
		}
		if a.From > lines {
			return nil, notFound("not found: %s has %d lines", f.Addr(), lines)
		}
		t.Kind, t.From, t.To = KindLines, a.From, to
	}
	return t, nil
}

func (r *Resolver) globalBlock(a Addr) (*Target, error) {
	bs, err := r.DB.Blocks(a.Block, 0)
	if err != nil {
		return nil, err
	}
	switch len(bs) {
	case 0:
		return nil, notFound("not found: ^%s (no block with that id; try pg search)", a.Block)
	case 1:
		f, err := r.DB.FileByID(bs[0].FileID)
		if err != nil {
			return nil, err
		}
		return &Target{Kind: KindBlock, File: f, Block: bs[0].ID, From: bs[0].Line, To: bs[0].End}, nil
	}
	if r.Prefer != "" {
		var mine []*Target
		for _, b := range bs {
			if f, err := r.DB.FileByID(b.FileID); err == nil && space.ProjectOf(f.Path) == r.Prefer {
				mine = append(mine, &Target{Kind: KindBlock, File: f, Block: b.ID, From: b.Line, To: b.End})
			}
		}
		if len(mine) == 1 {
			return mine[0], nil
		}
	}
	var c []string
	for _, b := range bs {
		if f, err := r.DB.FileByID(b.FileID); err == nil {
			c = append(c, f.Addr()+"^"+b.ID)
		}
	}
	return nil, notFound("ambiguous: ^%s is in %d files: %s", a.Block, len(bs), strings.Join(c, ", "))
}

func (r *Resolver) byULID(id string) (*index.File, error) {
	if len(id) < 6 {
		return nil, usage("@id needs at least 6 characters")
	}
	var match []*index.File
	for _, f := range r.files {
		if f.ULID != "" && strings.HasPrefix(strings.ToUpper(f.ULID), id) {
			match = append(match, f)
		}
	}
	switch len(match) {
	case 0:
		return nil, notFound("not found: @%s", id)
	case 1:
		return match[0], nil
	}
	return nil, notFound("ambiguous: @%s matches %d files; use more characters", id, len(match))
}

// ResolveFile finds a file by path, path suffix or basename (folded
// comparison). A unique match outside archive/ wins over archived copies.
func (r *Resolver) ResolveFile(name string) (*index.File, error) {
	key := index.FileKey(name)
	if key == "" {
		return nil, usage("empty file name")
	}
	if r.keys == nil {
		r.buildKeys()
	}
	var exact, suffix []*index.File
	for _, f := range r.keys[key] {
		if index.FileKey(f.Path) == key {
			exact = append(exact, f)
		} else {
			suffix = append(suffix, f)
		}
	}
	if len(exact) == 1 {
		return exact[0], nil
	}
	cands := append(exact, suffix...)
	if len(cands) == 0 && strings.Contains(key, "/") {
		// "project/name" for a note deeper in that project's folder
		// ("payments/refund-window" for payments/concepts/refund-window)
		proj, rest, _ := strings.Cut(key, "/")
		for _, f := range r.keys[rest] {
			if index.FileKey(space.ProjectOf(f.Path)) == proj {
				cands = append(cands, f)
			}
		}
	}
	if len(cands) == 0 && !strings.Contains(key, "/") {
		// names written without their date or number prefix:
		// "auth-refactor" for 2026-10-12-auth-refactor, "wal-mode" for 0007-wal-mode
		for _, f := range r.files {
			base := index.FileKey(f.Path)
			base = base[strings.LastIndex(base, "/")+1:]
			if i := strings.Index(base, "-"); i > 0 && isDigits(base[:i]) && strings.HasSuffix(base, "-"+key) {
				if p := base[:len(base)-len(key)-1]; isDatePrefix(p) || isDigits(p) {
					cands = append(cands, f)
				}
			}
		}
	}
	if len(cands) == 1 {
		return cands[0], nil
	}
	if len(cands) > 1 {
		var active, mine []*index.File
		for _, f := range cands {
			if !space.Archived(f.Path) {
				active = append(active, f)
				if r.Prefer != "" && space.ProjectOf(f.Path) == r.Prefer {
					mine = append(mine, f)
				}
			}
		}
		if len(active) == 1 {
			return active[0], nil
		}
		if len(mine) == 1 {
			return mine[0], nil
		}
		var names []string
		for _, f := range cands {
			names = append(names, f.Addr())
		}
		return nil, notFound("ambiguous: %q matches %s", name, strings.Join(names, ", "))
	}
	var all []string
	for _, f := range r.files {
		all = append(all, f.Addr())
		if i := strings.LastIndex(f.Addr(), "/"); i >= 0 {
			all = append(all, f.Addr()[i+1:])
		}
	}
	msg := fmt.Sprintf("not found: %s", name)
	if c := textutil.Closest(name, all, 3); len(c) > 0 {
		msg += " — did you mean: " + strings.Join(c, ", ")
	} else {
		msg += " (pg find lists files, pg search finds content)"
	}
	return nil, notFound("%s", msg)
}

func (r *Resolver) section(f *index.File, a Addr) (*index.Section, error) {
	secs, err := r.Sections(f)
	if err != nil {
		return nil, err
	}
	want := textutil.Fold(a.Heading)
	var exact, suffix []*index.Section
	for _, s := range secs {
		if s.Path == "" {
			continue
		}
		p := textutil.Fold(s.Path)
		if p == want {
			exact = append(exact, s)
		} else if strings.HasSuffix(p, "/"+want) {
			suffix = append(suffix, s)
		}
	}
	pick := exact
	if len(pick) == 0 {
		pick = suffix
	}
	if a.Nth > 0 {
		if a.Nth <= len(pick) {
			return pick[a.Nth-1], nil
		}
		return nil, notFound("not found: %s#%s[%d] (only %d match)", f.Addr(), a.Heading, a.Nth, len(pick))
	}
	switch {
	case len(pick) == 1:
		return pick[0], nil
	case len(pick) > 1 && len(exact) > 1:
		// same heading text repeated in one file: first wins, others need [n]
		return pick[0], nil
	case len(pick) > 1:
		var c []string
		for _, s := range pick {
			c = append(c, f.Addr()+"#"+s.Path)
		}
		return nil, notFound("ambiguous: #%s matches %s", a.Heading, strings.Join(c, ", "))
	}
	var paths []string
	for _, s := range secs {
		if s.Path != "" {
			paths = append(paths, s.Path)
			if i := strings.LastIndex(s.Path, "/"); i >= 0 {
				paths = append(paths, s.Path[i+1:])
			}
		}
	}
	msg := fmt.Sprintf("not found: %s#%s", f.Addr(), a.Heading)
	if c := textutil.Closest(a.Heading, paths, 3); len(c) > 0 {
		msg += " — did you mean: " + prefixAll(f.Addr()+"#", c)
	} else {
		msg += fmt.Sprintf(" (pg outline %s lists headings)", f.Addr())
	}
	return nil, notFound("%s", msg)
}

func prefixAll(p string, xs []string) string {
	out := make([]string, len(xs))
	for i, x := range xs {
		out[i] = p + x
	}
	return strings.Join(out, ", ")
}

func isDigits(s string) bool {
	if s == "" {
		return false
	}
	for _, r := range s {
		if r < '0' || r > '9' {
			return false
		}
	}
	return true
}

// isDatePrefix matches YYYY-MM-DD.
func isDatePrefix(s string) bool {
	return len(s) == 10 && isDigits(s[:4]) && s[4] == '-' && isDigits(s[5:7]) && s[7] == '-' && isDigits(s[8:])
}

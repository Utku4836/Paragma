package index

import (
	"database/sql"
	"fmt"
	"sort"
	"strings"
	"time"
	"unicode"
)

// File is one indexed Markdown file.
type File struct {
	ID       int64
	Path     string // relative to the vault root, slash separated, with .md
	ULID     string
	Type     string
	Layer    string
	Status   string
	Title    string
	Created  string
	Updated  string
	Verified string
	Hash     string
	Tags     string
	FMError  string
	Seen     int
	Tokens   int
	Size     int64
	NLines   int
	BodyLine int    // first line after the frontmatter
	Branch   string // front matter "branch" (sessions, branch notes)
	Mtime    int64
}

// Addr is the file's address: its path without the .md extension.
func (f *File) Addr() string { return strings.TrimSuffix(f.Path, ".md") }

// Date is the most meaningful date of the file: updated, verified, created or mtime.
func (f *File) Date() string {
	for _, s := range []string{f.Updated, f.Verified, f.Created} {
		if len(s) >= 10 {
			return s[:10]
		}
	}
	if f.Mtime > 0 {
		return time.Unix(0, f.Mtime).Format("2006-01-02")
	}
	return ""
}

// Inactive reports whether the file is superseded or archived.
func (f *File) Inactive() bool {
	return f.Status == "superseded" || f.Status == "archived" || strings.HasPrefix(f.Path, "archive/")
}

// Stale reports whether a semantic note has not been verified within days.
func (f *File) Stale(days int) bool {
	if f.Layer != "semantic" || f.Inactive() {
		return false
	}
	d := f.Verified
	if d == "" {
		d = f.Updated
	}
	if len(d) < 10 {
		return false
	}
	t, err := time.Parse("2006-01-02", d[:10])
	if err != nil {
		return false
	}
	return time.Since(t) > time.Duration(days)*24*time.Hour
}

const fileCols = `id, path, COALESCE(ulid,''), type, layer, status, title, created, updated, verified, hash, tags, fm_error, seen, tokens, size, lines, body_line, COALESCE(branch, ''), mtime`

func scanFile(sc interface{ Scan(...any) error }) (*File, error) {
	f := &File{}
	err := sc.Scan(&f.ID, &f.Path, &f.ULID, &f.Type, &f.Layer, &f.Status, &f.Title, &f.Created, &f.Updated, &f.Verified, &f.Hash, &f.Tags, &f.FMError, &f.Seen, &f.Tokens, &f.Size, &f.NLines, &f.BodyLine, &f.Branch, &f.Mtime)
	return f, err
}

// Files returns every indexed file, ordered by path. The list is cached
// until the next Sync that changes something.
func (d *DB) Files() ([]*File, error) {
	if d.files != nil {
		return d.files, nil
	}
	fs, err := d.loadFiles()
	if err != nil {
		return nil, err
	}
	d.files = fs
	d.byID = make(map[int64]*File, len(fs))
	for _, f := range fs {
		d.byID[f.ID] = f
	}
	return fs, nil
}

func (d *DB) fileMap() (map[int64]*File, error) {
	if _, err := d.Files(); err != nil {
		return nil, err
	}
	return d.byID, nil
}

func (d *DB) loadFiles() ([]*File, error) {
	rows, err := d.Query(`SELECT ` + fileCols + ` FROM files ORDER BY path`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []*File
	for rows.Next() {
		f, err := scanFile(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, f)
	}
	return out, rows.Err()
}

// FileByID returns one file.
func (d *DB) FileByID(id int64) (*File, error) {
	if d.byID != nil {
		if f, ok := d.byID[id]; ok {
			return f, nil
		}
	}
	return scanFile(d.QueryRow(`SELECT `+fileCols+` FROM files WHERE id=?`, id))
}

// Section is one heading section of a file.
type Section struct {
	ID        int64
	FileID    int64
	Ord       int
	Path      string
	Level     int
	Line      int
	BodyEnd   int
	End       int
	Tokens    int // heading + own body
	SubTokens int // whole subtree
	Kinds     string
	Tags      string
	IsTitle   bool // the note's single H1: addressing it means the whole note
}

const secCols = `id, file_id, ord, heading_path, level, line_start, body_end, line_end, tokens, sub_tokens, kinds, tags, is_title`

func scanSection(sc interface{ Scan(...any) error }) (*Section, error) {
	s := &Section{}
	err := sc.Scan(&s.ID, &s.FileID, &s.Ord, &s.Path, &s.Level, &s.Line, &s.BodyEnd, &s.End, &s.Tokens, &s.SubTokens, &s.Kinds, &s.Tags, &s.IsTitle)
	return s, err
}

// Sections returns the sections of a file in document order.
func (d *DB) Sections(fileID int64) ([]*Section, error) {
	rows, err := d.Query(`SELECT `+secCols+` FROM sections WHERE file_id=? ORDER BY ord`, fileID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []*Section
	for rows.Next() {
		s, err := scanSection(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, s)
	}
	return out, rows.Err()
}

// SectionByID returns one section.
func (d *DB) SectionByID(id int64) (*Section, error) {
	return scanSection(d.QueryRow(`SELECT `+secCols+` FROM sections WHERE id=?`, id))
}

// BlockRef locates a ^block.
type BlockRef struct {
	FileID int64
	ID     string
	Line   int
	End    int
}

// Blocks returns blocks with the given id, optionally limited to one file.
func (d *DB) Blocks(id string, fileID int64) ([]BlockRef, error) {
	q := `SELECT file_id, block_id, line, line_end FROM blocks WHERE block_id=? COLLATE NOCASE`
	args := []any{id}
	if fileID > 0 {
		q += ` AND file_id=?`
		args = append(args, fileID)
	}
	rows, err := d.Query(q, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []BlockRef
	for rows.Next() {
		var b BlockRef
		if err := rows.Scan(&b.FileID, &b.ID, &b.Line, &b.End); err != nil {
			return nil, err
		}
		out = append(out, b)
	}
	return out, rows.Err()
}

// BlockIDs returns all block ids of a file (for suggestions).
func (d *DB) BlockIDs(fileID int64) []string {
	rows, err := d.Query(`SELECT block_id FROM blocks WHERE file_id=?`, fileID)
	if err != nil {
		return nil
	}
	defer rows.Close()
	var out []string
	for rows.Next() {
		var s string
		rows.Scan(&s)
		out = append(out, s)
	}
	return out
}

// TaskCount summarises tasks.
type TaskCount struct {
	Total, Done, Open int
}

// Tasks returns the tasks of a file in line order.
func (d *DB) Tasks(fileID int64) ([]TaskRow, error) {
	rows, err := d.Query(`SELECT COALESCE(block_id,''), COALESCE(parent,''), line, line_end, state, text, priority, due FROM tasks WHERE file_id=? ORDER BY line`, fileID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []TaskRow
	for rows.Next() {
		var t TaskRow
		if err := rows.Scan(&t.ID, &t.Parent, &t.Line, &t.End, &t.State, &t.Text, &t.Priority, &t.Due); err != nil {
			return nil, err
		}
		out = append(out, t)
	}
	return out, rows.Err()
}

// TaskRow is one task line.
type TaskRow struct {
	ID, Parent    string
	Line, End     int
	State         string
	Text          string
	Priority, Due string
}

// Closed reports whether the task no longer needs work.
func (t TaskRow) Closed() bool { return t.State == "x" || t.State == "X" || t.State == "-" }

// CountTasks counts tasks whose line falls in [from,to].
func CountTasks(ts []TaskRow, from, to int) TaskCount {
	var c TaskCount
	for _, t := range ts {
		if t.Line < from || t.Line > to {
			continue
		}
		c.Total++
		if t.Closed() {
			c.Done++
		} else {
			c.Open++
		}
	}
	return c
}

// LinkRow is one outgoing link.
type LinkRow struct {
	SrcFile    int64
	SrcSection int64
	Line       int
	Target     string
	TargetFile string
	TargetSub  string
	Kind       string
	Context    string
}

func (d *DB) scanLinks(q string, args ...any) ([]LinkRow, error) {
	rows, err := d.Query(q, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []LinkRow
	for rows.Next() {
		var l LinkRow
		var sec sql.NullInt64
		if err := rows.Scan(&l.SrcFile, &sec, &l.Line, &l.Target, &l.TargetFile, &l.TargetSub, &l.Kind, &l.Context); err != nil {
			return nil, err
		}
		l.SrcSection = sec.Int64
		out = append(out, l)
	}
	return out, rows.Err()
}

const linkCols = `src_file, src_section, line, target, target_file, target_sub, kind, context`

// LinksFrom returns links whose source line is within [from,to] of a file.
func (d *DB) LinksFrom(fileID int64, from, to int) ([]LinkRow, error) {
	return d.scanLinks(`SELECT `+linkCols+` FROM links WHERE src_file=? AND line BETWEEN ? AND ? ORDER BY line`, fileID, from, to)
}

// LinksTo returns links whose folded target file key is one of keys.
func (d *DB) LinksTo(keys []string) ([]LinkRow, error) {
	if len(keys) == 0 {
		return nil, nil
	}
	ph := strings.TrimSuffix(strings.Repeat("?,", len(keys)), ",")
	args := make([]any, len(keys))
	for i, k := range keys {
		args[i] = k
	}
	return d.scanLinks(`SELECT `+linkCols+` FROM links WHERE target_file IN (`+ph+`) ORDER BY src_file, line`, args...)
}

// AllLinks returns every link (used for broken-link counts).
func (d *DB) AllLinks() ([]LinkRow, error) {
	return d.scanLinks(`SELECT ` + linkCols + ` FROM links`)
}

// FileTags returns the tags of every file keyed by file id.
func (d *DB) FileTags() (map[int64][]string, error) {
	rows, err := d.Query(`SELECT file_id, tag FROM tags`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := map[int64][]string{}
	for rows.Next() {
		var id int64
		var t string
		rows.Scan(&id, &t)
		out[id] = append(out[id], t)
	}
	return out, rows.Err()
}

// TaskCountsByFile returns open/total task counts per file.
func (d *DB) TaskCountsByFile() (map[int64]TaskCount, error) {
	rows, err := d.Query(`SELECT file_id, state, COUNT(*) FROM tasks GROUP BY file_id, state`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := map[int64]TaskCount{}
	for rows.Next() {
		var id int64
		var st string
		var n int
		rows.Scan(&id, &st, &n)
		c := out[id]
		c.Total += n
		if (TaskRow{State: st}).Closed() {
			c.Done += n
		} else {
			c.Open += n
		}
		out[id] = c
	}
	return out, rows.Err()
}

// Count runs a single-integer query.
func (d *DB) Count(q string, args ...any) int {
	var n int
	d.QueryRow(q, args...).Scan(&n)
	return n
}

// ---- search ----

// SearchOpts filters and pages a search.
type SearchOpts struct {
	Query    string
	Layers   []string
	Types    []string
	Statuses []string
	Tags     []string
	Kinds    []string
	All      bool // do not penalise superseded/archived
	Raw      bool // pass Query to FTS5 unchanged
	Max      int  // candidate cap
}

// Hit is one search result.
type Hit struct {
	File    *File
	SecID   int64
	Sec     *Section // set by Hydrate
	Score   float64
	Snippet string // set by Hydrate
	Via     string // fts | prefix | any | substr

	table, match string
}

func terms(q string) []string {
	f := strings.FieldsFunc(q, func(r rune) bool { return !unicode.IsLetter(r) && !unicode.IsDigit(r) })
	var out []string
	for _, t := range f {
		if t != "" {
			out = append(out, t)
		}
	}
	return out
}

func quote(s string) string { return `"` + strings.ReplaceAll(s, `"`, `""`) + `"` }

// Search runs a section-level search. It tries progressively looser queries
// (all terms exact, all terms as prefixes, any term) and falls back to
// trigram substring matching for code-like queries. Hits come back ranked
// but without section details or snippets; call Hydrate on the ones you show.
func (d *DB) Search(o SearchOpts) ([]Hit, error) {
	if o.Max <= 0 {
		o.Max = 200
	}
	byID, err := d.fileMap()
	if err != nil {
		return nil, err
	}

	filter, fargs := o.filterSQL()
	seen := map[int64]bool{}
	var hits []Hit
	run := func(table, match string, weight float64, via string) error {
		bm := "bm25(sections_fts, 4.0, 1.0, 2.0)"
		if table == "sections_tri" {
			bm = "bm25(sections_tri)"
		}
		q := `SELECT s.id, s.file_id, ` + bm + `
			FROM ` + table + ` JOIN sections s ON s.id = ` + table + `.rowid JOIN files f ON f.id = s.file_id
			WHERE ` + table + ` MATCH ? ` + filter + ` ORDER BY 3 LIMIT ?`
		args := append([]any{match}, fargs...)
		args = append(args, o.Max)
		rows, err := d.Query(q, args...)
		if err != nil {
			return fmt.Errorf("search: %w", err)
		}
		defer rows.Close()
		for rows.Next() {
			var sid, fid int64
			var rank float64
			if err := rows.Scan(&sid, &fid, &rank); err != nil {
				return err
			}
			if seen[sid] {
				continue
			}
			seen[sid] = true
			f := byID[fid]
			if f == nil {
				continue
			}
			hits = append(hits, Hit{File: f, SecID: sid, Score: -rank * weight * adjust(f, o.All), Via: via, table: table, match: match})
		}
		return rows.Err()
	}

	if o.Raw {
		if err := run("sections_fts", o.Query, 1, "fts"); err != nil {
			return nil, err
		}
	} else {
		ts := terms(o.Query)
		if len(ts) > 0 {
			exact := make([]string, len(ts))
			prefix := make([]string, len(ts))
			for i, t := range ts {
				exact[i] = quote(t)
				prefix[i] = quote(t) + "*"
			}
			if err := run("sections_fts", strings.Join(exact, " "), 1, "fts"); err != nil {
				return nil, err
			}
			if len(hits) < 10 {
				if err := run("sections_fts", strings.Join(prefix, " "), 0.7, "prefix"); err != nil {
					return nil, err
				}
			}
			if len(hits) == 0 && len(ts) > 1 {
				if err := run("sections_fts", strings.Join(prefix, " OR "), 0.4, "any"); err != nil {
					return nil, err
				}
			}
		}
		raw := strings.TrimSpace(o.Query)
		codeLike := strings.ContainsAny(raw, "_.:/()[]{}<>=-$'\"`\\") || hasInnerUpper(raw)
		if len([]rune(raw)) >= 3 && (len(hits) == 0 || codeLike) {
			if err := run("sections_tri", quote(raw), 0.5, "substr"); err != nil {
				return nil, err
			}
		}
	}

	sort.SliceStable(hits, func(i, j int) bool { return hits[i].Score > hits[j].Score })
	return hits, nil
}

// Hydrate loads the section and a one-line snippet for each hit. It is
// separate from Search because snippets are the expensive part and only the
// shown hits need them. Hits sharing a match expression are fetched in one
// query.
func (d *DB) Hydrate(hits []Hit) error {
	type key struct{ table, match string }
	groups := map[key][]int{}
	for i, h := range hits {
		k := key{h.table, h.match}
		groups[k] = append(groups[k], i)
	}
	for k, idx := range groups {
		col := 1
		if k.table == "sections_tri" {
			col = 0
		}
		ids := make([]any, 0, len(idx)+1)
		ids = append(ids, k.match)
		for _, i := range idx {
			ids = append(ids, hits[i].SecID)
		}
		ph := strings.TrimSuffix(strings.Repeat("?,", len(idx)), ",")
		q := fmt.Sprintf(`SELECT `+prefixCols("s.", secCols)+`, snippet(%s, %d, '', '', '…', 14)
			FROM %s JOIN sections s ON s.id = %s.rowid WHERE %s MATCH ? AND %s.rowid IN (%s)`,
			k.table, col, k.table, k.table, k.table, k.table, ph)
		rows, err := d.Query(q, ids...)
		if err != nil {
			return err
		}
		got := map[int64]*Section{}
		snips := map[int64]string{}
		for rows.Next() {
			s := &Section{}
			var snip string
			if err := rows.Scan(&s.ID, &s.FileID, &s.Ord, &s.Path, &s.Level, &s.Line, &s.BodyEnd, &s.End, &s.Tokens, &s.SubTokens, &s.Kinds, &s.Tags, &s.IsTitle, &snip); err != nil {
				rows.Close()
				return err
			}
			got[s.ID] = s
			snips[s.ID] = snip
		}
		rows.Close()
		for _, i := range idx {
			h := &hits[i]
			if s, ok := got[h.SecID]; ok {
				h.Sec, h.Snippet = s, snips[h.SecID]
				continue
			}
			s, err := d.SectionByID(h.SecID)
			if err != nil {
				return err
			}
			h.Sec = s
		}
	}
	return nil
}

func hasInnerUpper(s string) bool {
	rs := []rune(s)
	for i := 1; i < len(rs); i++ {
		if unicode.IsUpper(rs[i]) && unicode.IsLower(rs[i-1]) {
			return true
		}
	}
	return false
}

// adjust applies the ranking policy on top of bm25.
func adjust(f *File, all bool) float64 {
	m := 1.0
	if f.Inactive() && !all {
		m *= 0.15
	}
	if f.Layer == "episodic" {
		if f.Seen > 0 {
			m *= 1 + 0.1*float64(min(f.Seen, 5))
		}
		if d := f.Date(); d != "" {
			if t, err := time.Parse("2006-01-02", d); err == nil && time.Since(t) < 30*24*time.Hour {
				m *= 1.15
			}
		}
	}
	return m
}

func (o SearchOpts) filterSQL() (string, []any) {
	var b strings.Builder
	var args []any
	in := func(col string, vals []string) {
		if len(vals) == 0 {
			return
		}
		b.WriteString(" AND " + col + " IN (" + strings.TrimSuffix(strings.Repeat("?,", len(vals)), ",") + ")")
		for _, v := range vals {
			args = append(args, v)
		}
	}
	in("f.layer", o.Layers)
	in("f.type", o.Types)
	in("f.status", o.Statuses)
	if len(o.Tags) > 0 {
		b.WriteString(" AND f.id IN (SELECT file_id FROM tags WHERE tag IN (" + strings.TrimSuffix(strings.Repeat("?,", len(o.Tags)), ",") + "))")
		for _, t := range o.Tags {
			args = append(args, strings.ToLower(strings.TrimPrefix(t, "#")))
		}
	}
	for _, k := range o.Kinds {
		b.WriteString(" AND instr(' ' || s.kinds || ' ', ?) > 0")
		args = append(args, " "+strings.ToLower(k)+" ")
	}
	return b.String(), args
}

func prefixCols(p, cols string) string {
	parts := strings.Split(cols, ",")
	for i, c := range parts {
		parts[i] = p + strings.TrimSpace(c)
	}
	return strings.Join(parts, ", ")
}

// SigRow is one indexed error signature.
type SigRow struct {
	FileID int64
	Norm   string
	Hash   string
}

// ErrorSigs returns every error signature in the vault.
func (d *DB) ErrorSigs() ([]SigRow, error) {
	rows, err := d.Query(`SELECT file_id, sig_norm, sig_hash FROM error_sigs`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []SigRow
	for rows.Next() {
		var s SigRow
		if err := rows.Scan(&s.FileID, &s.Norm, &s.Hash); err != nil {
			return nil, err
		}
		out = append(out, s)
	}
	return out, rows.Err()
}

// CodeRefRow is one code reference in a note.
type CodeRefRow struct {
	FileID    int64
	SectionID int64
	Line      int
	Ref       string
}

// CodeRefs returns every code reference in the vault.
func (d *DB) CodeRefs() ([]CodeRefRow, error) {
	rows, err := d.Query(`SELECT file_id, COALESCE(section_id, 0), line, ref FROM code_refs`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []CodeRefRow
	for rows.Next() {
		var c CodeRefRow
		if err := rows.Scan(&c.FileID, &c.SectionID, &c.Line, &c.Ref); err != nil {
			return nil, err
		}
		out = append(out, c)
	}
	return out, rows.Err()
}

// BlockAt returns the id of the block holding a line of a file ("" if none).
func (d *DB) BlockAt(fileID int64, line int) string {
	var id string
	d.QueryRow(`SELECT block_id FROM blocks WHERE file_id=? AND line<=? AND line_end>=? ORDER BY line_end-line LIMIT 1`, fileID, line, line).Scan(&id)
	return id
}

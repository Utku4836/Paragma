package index

import (
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/Utku-Private/Paragma/internal/errsig"
	"github.com/Utku-Private/Paragma/internal/md"
	"github.com/Utku-Private/Paragma/internal/textutil"
	"github.com/Utku-Private/Paragma/internal/vault"
)

// SyncStats reports what an incremental sync did.
type SyncStats struct {
	Files   int
	Changed int
	Touched int // mtime changed but content identical
	Removed int
	Elapsed time.Duration
}

type diskFile struct {
	mtime int64
	size  int64
}

type dbFile struct {
	id    int64
	mtime int64
	size  int64
	hash  string
}

// Sync brings the index up to date with the Markdown files on disk. Unchanged
// files cost one stat each; no write transaction is opened when nothing
// changed.
func (d *DB) Sync() (SyncStats, error) {
	start := time.Now()
	var st SyncStats
	// list the disk and load the index at the same time
	diskc := make(chan map[string]diskFile, 1)
	go func() { diskc <- d.scan() }()
	d.files = nil
	files, err := d.Files()
	disk := <-diskc
	if err != nil {
		return st, err
	}
	st.Files = len(disk)

	known := make(map[string]dbFile, len(files))
	for _, f := range files {
		known[f.Path] = dbFile{f.ID, f.Mtime, f.Size, f.Hash}
	}

	var changed []string
	for rel, df := range disk {
		if k, ok := known[rel]; !ok || k.mtime != df.mtime || k.size != df.size {
			changed = append(changed, rel)
		}
	}
	var removed []string
	for rel := range known {
		if _, ok := disk[rel]; !ok {
			removed = append(removed, rel)
		}
	}
	if len(changed) == 0 && len(removed) == 0 {
		st.Elapsed = time.Since(start)
		return st, nil
	}

	tx, err := d.Begin()
	if err != nil {
		return st, err
	}
	defer tx.Rollback()
	w, err := newWriter(tx, d.V)
	if err != nil {
		return st, err
	}
	defer w.close()
	for _, rel := range removed {
		if err := w.deleteFile(rel); err != nil {
			return st, err
		}
		st.Removed++
	}
	for _, rel := range changed {
		src, err := os.ReadFile(filepath.Join(d.V.Root, filepath.FromSlash(rel)))
		if err != nil {
			continue // vanished between walk and read
		}
		h := hashBytes(src)
		df := disk[rel]
		if k, ok := known[rel]; ok && k.hash == h {
			if _, err := tx.Exec(`UPDATE files SET mtime=?, size=? WHERE id=?`, df.mtime, df.size, k.id); err != nil {
				return st, err
			}
			st.Touched++
			continue
		}
		if err := w.writeFile(rel, src, df.mtime, df.size, h); err != nil {
			return st, err
		}
		st.Changed++
	}
	if err := tx.Commit(); err != nil {
		return st, err
	}
	d.files = nil // reload on next use
	st.Elapsed = time.Since(start)
	return st, nil
}

func hashBytes(b []byte) string {
	s := sha256.Sum256(b)
	return hex.EncodeToString(s[:8])
}

type writer struct {
	tx   *sql.Tx
	v    *vault.Vault
	stmt map[string]*sql.Stmt
}

var stmts = map[string]string{
	"file":    `INSERT INTO files (path, ulid, type, layer, status, title, mtime, size, hash, created, updated, verified, seen, tags, tokens, lines, body_line, branch, fm_json, fm_error) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)`,
	"section": `INSERT INTO sections (file_id, ord, heading_path, level, line_start, body_end, line_end, tokens, sub_tokens, body, kinds, tags, is_title) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)`,
	"block":   `INSERT OR IGNORE INTO blocks (file_id, block_id, section_id, line, line_end) VALUES (?,?,?,?,?)`,
	"link":    `INSERT INTO links (src_file, src_section, line, target, target_file, target_sub, kind, context) VALUES (?,?,?,?,?,?,?,?)`,
	"task":    `INSERT INTO tasks (file_id, section_id, block_id, parent, line, line_end, state, text, priority, due, tags, done_at, blocked_by, claimed_by) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)`,
	"mark":    `INSERT INTO marks (file_id, section_id, block_id, line, kind, note) VALUES (?,?,?,?,?,?)`,
	"tag":     `INSERT INTO tags (file_id, tag) VALUES (?,?)`,
	"sig":     `INSERT INTO error_sigs (file_id, sig_norm, sig_hash) VALUES (?,?,?)`,
	"code":    `INSERT INTO code_refs (file_id, section_id, line, ref) VALUES (?,?,?,?)`,
}

func newWriter(tx *sql.Tx, v *vault.Vault) (*writer, error) {
	w := &writer{tx: tx, v: v, stmt: map[string]*sql.Stmt{}}
	for k, q := range stmts {
		s, err := tx.Prepare(q)
		if err != nil {
			w.close()
			return nil, err
		}
		w.stmt[k] = s
	}
	return w, nil
}

func (w *writer) close() {
	for _, s := range w.stmt {
		s.Close()
	}
}

func (w *writer) deleteFile(rel string) error {
	var id int64
	err := w.tx.QueryRow(`SELECT id FROM files WHERE path=?`, rel).Scan(&id)
	if err == sql.ErrNoRows {
		return nil
	}
	if err != nil {
		return err
	}
	for _, q := range []string{
		`DELETE FROM sections WHERE file_id=?`,
		`DELETE FROM blocks WHERE file_id=?`,
		`DELETE FROM links WHERE src_file=?`,
		`DELETE FROM tasks WHERE file_id=?`,
		`DELETE FROM marks WHERE file_id=?`,
		`DELETE FROM tags WHERE file_id=?`,
		`DELETE FROM error_sigs WHERE file_id=?`,
		`DELETE FROM code_refs WHERE file_id=?`,
		`DELETE FROM files WHERE id=?`,
	} {
		if _, err := w.tx.Exec(q, id); err != nil {
			return err
		}
	}
	return nil
}

func (w *writer) writeFile(rel string, src []byte, mtime, size int64, hash string) error {
	if err := w.deleteFile(rel); err != nil {
		return err
	}
	doc := md.Parse(src)

	typ := doc.FrontString("type")
	if typ == "" {
		typ = vault.InferType(rel)
	}
	layer := doc.FrontString("layer")
	if layer == "" {
		layer = vault.InferLayer(rel)
	}
	status := doc.FrontString("status")
	if strings.HasPrefix(rel, "archive/") && status == "" {
		status = "archived"
	}
	title := doc.FrontString("title")
	if title == "" {
		title = doc.Title
	}
	if title == "" {
		title = strings.TrimSuffix(rel[strings.LastIndex(rel, "/")+1:], ".md")
	}
	seen, _ := strconv.Atoi(doc.FrontString("seen"))

	tagSet := map[string]bool{}
	var tags []string
	for _, t := range append(doc.FrontList("tags"), doc.Tags...) {
		t = strings.ToLower(strings.TrimPrefix(t, "#"))
		if t != "" && !tagSet[t] {
			tagSet[t] = true
			tags = append(tags, t)
		}
	}
	body := strings.Join(doc.Lines[min(doc.BodyLine-1, len(doc.Lines)):], "\n")
	fmJSON := ""
	if doc.Front != nil {
		if b, err := json.Marshal(doc.Front); err == nil {
			fmJSON = string(b)
		}
	}
	fmErr := ""
	if doc.FrontErr != nil {
		fmErr = doc.FrontErr.Error()
	}

	res, err := w.stmt["file"].Exec(rel, nullStr(doc.FrontString("id")), typ, layer, status, title, mtime, size, hash,
		doc.FrontString("created"), doc.FrontString("updated"), doc.FrontString("verified"),
		seen, strings.Join(tags, " "), textutil.Tokens(body), len(doc.Lines), doc.BodyLine, doc.FrontString("branch"), fmJSON, fmErr)
	if err != nil {
		return err
	}
	fid, _ := res.LastInsertId()

	// keywords are search terms the writer added for later lookups
	// (synonyms, the other language, the words a future search will use);
	// they are searchable with the tags but are not tags
	fileTags := strings.TrimSpace(strings.Join(tags, " ") + " " + strings.Join(doc.FrontList("keywords"), " "))
	secIDs := make([]int64, len(doc.Sections))
	for i, s := range doc.Sections {
		bodyStart := s.Line
		if s.Level > 0 {
			bodyStart = s.Line + 1
		}
		sb := joinLines(doc.Lines, bodyStart, s.BodyEnd)
		own := joinLines(doc.Lines, s.Line, s.BodyEnd)
		sub := joinLines(doc.Lines, s.Line, s.End)
		stags := strings.TrimSpace(fileTags + " " + strings.Join(s.Tags, " "))
		r, err := w.stmt["section"].Exec(fid, i, s.Path, s.Level, s.Line, s.BodyEnd, s.End,
			textutil.Tokens(own), textutil.Tokens(sub), sb, strings.Join(s.Kinds, " "), stags, s.Title)
		if err != nil {
			return err
		}
		secIDs[i], _ = r.LastInsertId()
	}
	secID := func(i int) any {
		if i < 0 || i >= len(secIDs) {
			return nil
		}
		return secIDs[i]
	}
	for _, b := range doc.Blocks {
		if _, err := w.stmt["block"].Exec(fid, b.ID, secID(b.Section), b.Line, b.End); err != nil {
			return err
		}
	}
	self := strings.TrimSuffix(rel, ".md")
	for _, l := range doc.Links {
		tf, sub := SplitTarget(l.Target)
		if tf == "" {
			tf = textutil.Fold(self)
		}
		if _, err := w.stmt["link"].Exec(fid, secID(l.Section), l.Line, l.Target, tf, sub, l.Kind, textutil.OneLine(l.Context, 200)); err != nil {
			return err
		}
	}
	for _, t := range doc.Tasks {
		if _, err := w.stmt["task"].Exec(fid, secID(t.Section), nullStr(t.ID), nullStr(t.Parent), t.Line, t.End, string(t.State), t.Text,
			t.Priority, t.Due, strings.Join(t.Tags, " "), t.Done, t.Blocked, t.By); err != nil {
			return err
		}
	}
	for _, m := range doc.Marks {
		if _, err := w.stmt["mark"].Exec(fid, secID(m.Section), nullStr(m.BlockID), m.Line, m.Kind, m.Note); err != nil {
			return err
		}
	}
	for _, t := range tags {
		if _, err := w.stmt["tag"].Exec(fid, t); err != nil {
			return err
		}
	}
	for _, c := range doc.CodeRefs {
		if _, err := w.stmt["code"].Exec(fid, secID(c.Section), c.Line, c.Ref); err != nil {
			return err
		}
	}
	// error signatures: every raw `sig` (one or a list), normalised here so
	// hand-written notes match too, plus an explicit sig_norm if present
	sigSeen := map[string]bool{}
	for _, raw := range append(doc.FrontList("sig"), doc.FrontString("sig_norm")) {
		sn := errsig.Normalize(raw)
		if sn == "" || sigSeen[sn] {
			continue
		}
		sigSeen[sn] = true
		if _, err := w.stmt["sig"].Exec(fid, sn, errsig.Hash(sn)); err != nil {
			return err
		}
	}
	return nil
}

// SplitTarget splits a link target or address into a folded file key and the
// sub-address ("#Heading", "^block" or ""). The file key has no extension and
// uses forward slashes.
func SplitTarget(t string) (file, sub string) {
	i := strings.IndexAny(t, "#^")
	if i >= 0 {
		file, sub = t[:i], t[i:]
	} else {
		file = t
	}
	return FileKey(file), sub
}

// FileKey normalises a file reference for comparison.
func FileKey(s string) string {
	s = strings.TrimSpace(strings.ReplaceAll(s, "\\", "/"))
	s = strings.TrimPrefix(s, "./")
	s = strings.TrimPrefix(s, "/")
	s = strings.TrimPrefix(s, vault.DirName+"/")
	if strings.HasSuffix(strings.ToLower(s), ".md") {
		s = s[:len(s)-3]
	}
	return textutil.Fold(s)
}

func joinLines(lines []string, from, to int) string {
	if from < 1 {
		from = 1
	}
	if to > len(lines) {
		to = len(lines)
	}
	if from > to {
		return ""
	}
	return strings.Join(lines[from-1:to], "\n")
}

func nullStr(s string) any {
	if s == "" {
		return nil
	}
	return s
}

// scan lists every note under the vault root with its mtime and size.
// Directories are read concurrently: on Windows each directory listing has a
// high fixed latency (often from filesystem filter drivers), and reading them
// in parallel cut a 500-note scan from ~17ms to ~5ms.
func (d *DB) scan() map[string]diskFile {
	var (
		mu   sync.Mutex
		wg   sync.WaitGroup
		sem  = make(chan struct{}, 16)
		disk = map[string]diskFile{}
	)
	var walk func(dir, rel string)
	walk = func(dir, rel string) {
		defer wg.Done()
		sem <- struct{}{}
		entries, err := os.ReadDir(dir)
		<-sem
		if err != nil {
			return // unreadable directories are skipped, not fatal
		}
		local := map[string]diskFile{}
		for _, e := range entries {
			r := e.Name()
			if rel != "" {
				r = rel + "/" + r
			}
			if d.V.Skip(r, e.IsDir()) {
				continue
			}
			if e.IsDir() {
				wg.Add(1)
				go walk(filepath.Join(dir, e.Name()), r)
				continue
			}
			info, err := e.Info()
			if err != nil || !info.Mode().IsRegular() {
				continue
			}
			local[r] = diskFile{info.ModTime().UnixNano(), info.Size()}
		}
		mu.Lock()
		for k, v := range local {
			disk[k] = v
		}
		mu.Unlock()
	}
	wg.Add(1)
	walk(d.V.Root, "")
	wg.Wait()
	return disk
}

// IndexFile re-indexes one note right after pg wrote, moved or deleted it, so
// the next query sees the change without waiting for a full scan.
func (d *DB) IndexFile(rels ...string) error {
	tx, err := d.Begin()
	if err != nil {
		return err
	}
	defer tx.Rollback()
	w, err := newWriter(tx, d.V)
	if err != nil {
		return err
	}
	defer w.close()
	for _, rel := range rels {
		p := filepath.Join(d.V.Root, filepath.FromSlash(rel))
		info, err := os.Stat(p)
		if err != nil {
			if err := w.deleteFile(rel); err != nil {
				return err
			}
			continue
		}
		src, err := os.ReadFile(p)
		if err != nil {
			return err
		}
		if err := w.writeFile(rel, src, info.ModTime().UnixNano(), info.Size(), hashBytes(src)); err != nil {
			return err
		}
	}
	if err := tx.Commit(); err != nil {
		return err
	}
	d.files = nil
	return nil
}

// HashBytes is the content hash pg prints as hash:… and checks for --if-hash.
func HashBytes(b []byte) string { return hashBytes(b) }

// BlockExists reports whether a block id is used anywhere in the vault.
func (d *DB) BlockExists(id string) bool {
	var n int
	d.QueryRow(`SELECT COUNT(*) FROM blocks WHERE block_id = ? COLLATE NOCASE`, id).Scan(&n)
	return n > 0
}

package index

import (
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"testing"

	"github.com/Utku4836/Paragma/internal/space"
)

// newSpace makes a space with n notes, enough to use the bulk path.
func newSpace(t *testing.T, n int) *space.Space {
	t.Helper()
	v, err := space.Create(t.TempDir(), space.DefaultName)
	if err != nil {
		t.Fatal(err)
	}
	dir := filepath.Join(v.Root, "demo", "errors")
	os.MkdirAll(dir, 0o755)
	for i := range n {
		text := fmt.Sprintf("---\ntype: error\nsig: 'boom %d'\n---\n# Error %d\n\n## Fix\n\nRestart worker %d with retry. ^b%d\n\n- [ ] follow up %d ^t%d\n", i, i, i, i, i, i)
		if err := os.WriteFile(filepath.Join(dir, fmt.Sprintf("e%03d.md", i)), []byte(text), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	return v
}

// Several pg processes start at once on a space that has no index yet (the
// first commands after install, or agents running tools in parallel): each
// must succeed and the index must hold every note once.
func TestConcurrentFirstSync(t *testing.T) {
	v := newSpace(t, bulkMin+50)
	var wg sync.WaitGroup
	errs := make(chan error, 8)
	for i := range 8 {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			d, err := Open(v)
			if err != nil {
				errs <- err
				return
			}
			defer d.Close()
			if i == 3 {
				r, err := Reset(v) // pg reindex in the middle
				if err != nil {
					errs <- err
					return
				}
				r.Close()
			}
			if _, err := d.Sync(); err != nil {
				errs <- err
				return
			}
			if hs, err := d.Search(SearchOpts{Query: "restart worker"}); err != nil || len(hs) == 0 {
				errs <- fmt.Errorf("search after sync: %d hits, %v", len(hs), err)
			}
		}(i)
	}
	wg.Wait()
	close(errs)
	for err := range errs {
		t.Error(err)
	}
	d, err := Open(v)
	if err != nil {
		t.Fatal(err)
	}
	defer d.Close()
	if _, err := d.Sync(); err != nil {
		t.Fatal(err)
	}
	if n := d.Count(`SELECT COUNT(*) FROM files`); n != bulkMin+50 {
		t.Fatalf("%d files indexed, want %d", n, bulkMin+50)
	}
	if n := d.Count(`SELECT COUNT(*) FROM sections_fts WHERE sections_fts MATCH 'retry'`); n != bulkMin+50 {
		t.Fatalf("full-text index has %d sections with 'retry', want %d", n, bulkMin+50)
	}
}

// The bulk path (full-text tables rebuilt at the end) and the row-by-row
// path must index the same way, also for later edits and deletions.
func TestBulkSyncMatchesIncremental(t *testing.T) {
	v := newSpace(t, bulkMin+10)
	d, err := Open(v)
	if err != nil {
		t.Fatal(err)
	}
	defer d.Close()
	if _, err := d.Sync(); err != nil { // bulk
		t.Fatal(err)
	}
	dir := filepath.Join(v.Root, "demo", "errors")
	os.Remove(filepath.Join(dir, "e000.md"))
	os.WriteFile(filepath.Join(dir, "e001.md"), []byte("# Error 1\n\n## Fix\n\nPin the clock in tests.\n"), 0o644)
	st, err := d.Sync() // row by row
	if err != nil {
		t.Fatal(err)
	}
	if st.Changed != 1 || st.Removed != 1 {
		t.Fatalf("sync: %+v", st)
	}
	for q, want := range map[string]int{"restart": bulkMin + 8, "pin clock": 1, `"worker 0"`: 0} {
		hs, err := d.Search(SearchOpts{Query: q, Max: 1000})
		if err != nil {
			t.Fatal(err)
		}
		files := map[int64]bool{}
		for _, h := range hs {
			if h.Via == "fts" || h.Via == "prefix" {
				files[h.File.ID] = true
			}
		}
		if len(files) != want {
			t.Errorf("search %s: %d notes, want %d", q, len(files), want)
		}
	}
	if n := d.Count(`SELECT COUNT(*) FROM error_sigs`); n != bulkMin+8 {
		t.Errorf("%d error signatures, want %d", n, bulkMin+8)
	}
}

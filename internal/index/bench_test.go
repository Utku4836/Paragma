package index

import (
	"os"
	"testing"
	"time"

	"github.com/Utku-Private/Paragma/internal/space"
)

// Run against a generated space:
//
//	go run ./tools/gen -n 500 -out /tmp/bench
//	PARAGMA_BENCH_HOME=/tmp/bench go test ./internal/index -run Phases -v
func TestPhases(t *testing.T) {
	home := os.Getenv("PARAGMA_BENCH_HOME")
	if home == "" {
		t.Skip("PARAGMA_BENCH_HOME not set")
	}
	v, err := space.Open(home, space.DefaultName)
	if err != nil {
		t.Fatal(err)
	}
	root := v.Root
	db, err := Open(v)
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	db.Sync()
	for i := 0; i < 3; i++ {
		t0 := time.Now()
		st, err := db.Sync()
		if err != nil {
			t.Fatal(err)
		}
		t.Logf("sync %v (walk+compare, %d files)", time.Since(t0), st.Files)
		t1 := time.Now()
		hs, err := db.Search(SearchOpts{Query: "veritabanı kilit"})
		if err != nil {
			t.Fatal(err)
		}
		t.Logf("search %v (%d hits)", time.Since(t1), len(hs))
	}
	tw := time.Now()
	n := 0
	d, _ := os.ReadDir(root)
	_ = d
	t.Logf("readdir root %v %d", time.Since(tw), n)
}

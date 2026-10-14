package index

import (
	"os"
	"testing"
	"time"

	"github.com/Utku-Private/Paragma/internal/vault"
)

// Run against a generated vault:
//
//	go run ./tools/gen -n 500 -out /tmp/bench
//	PARAGMA_BENCH_VAULT=/tmp/bench/.paragma go test ./internal/index -run Phases -v
func TestPhases(t *testing.T) {
	root := os.Getenv("PARAGMA_BENCH_VAULT")
	if root == "" {
		t.Skip("PARAGMA_BENCH_VAULT not set")
	}
	v, err := vault.Open(root)
	if err != nil {
		t.Fatal(err)
	}
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

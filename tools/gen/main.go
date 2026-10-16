// Command gen writes a synthetic space for performance checks:
//
//	go run ./tools/gen -n 500 -out /tmp/bench
//
// It creates <out>/Spaces/main/bench/ (a Paragma home with one project) with
// notes spread over all layers, each with
// several headings, tasks, callouts and links to other notes.
package main

import (
	"flag"
	"fmt"
	"math/rand"
	"os"
	"path/filepath"
	"strings"
)

var words = strings.Fields(`veritabanı kilit bağlantı oturum token yenileme middleware handler
router migration şema indeks sorgu önbellek kuyruk işçi zamanlayıcı hata günlük dağıtım bölge
sertifika anahtar gizli yapılandırma ortam test entegrasyon birim performans bellek sızıntı
database lock connection session refresh cache queue worker scheduler error deploy region
certificate secret config environment integration unit performance memory leak timeout retry
backoff circuit breaker rate limit pagination cursor offset websocket stream grpc http json`)

func sentence(r *rand.Rand, n int) string {
	w := make([]string, n)
	for i := range w {
		w[i] = words[r.Intn(len(words))]
	}
	return strings.ToUpper(w[0][:1]) + strings.Join(w, " ")[1:] + "."
}

func main() {
	n := flag.Int("n", 500, "number of notes")
	out := flag.String("out", "bench", "output directory (a Paragma home)")
	flag.Parse()
	r := rand.New(rand.NewSource(42))
	root := filepath.Join(*out, "Spaces", "main", "bench")
	dirs := []struct{ dir, layer, typ string }{
		{"", "semantic", "note"},
		{"concepts", "semantic", "concept"},
		{"errors", "episodic", "error"},
		{"decisions", "episodic", "decision"},
		{"sessions", "working", "session"},
	}
	names := make([]string, *n)
	for i := range names {
		names[i] = fmt.Sprintf("note-%04d", i)
	}
	os.MkdirAll(root, 0o755)
	for i, name := range names {
		d := dirs[i%len(dirs)]
		var b strings.Builder
		fmt.Fprintf(&b, "---\nid: 01BENCH%019d\ntype: %s\nlayer: %s\ntitle: %s\ntags: [%s, %s]\nupdated: 2026-%02d-%02d\n---\n",
			i, d.typ, d.layer, sentence(r, 3), words[r.Intn(len(words))], words[r.Intn(len(words))], 1+r.Intn(9), 1+r.Intn(28))
		fmt.Fprintf(&b, "# %s\n\n%s\n\n", sentence(r, 4), sentence(r, 20))
		for h := 0; h < 3+r.Intn(5); h++ {
			fmt.Fprintf(&b, "## %s\n\n", sentence(r, 2))
			for p := 0; p < 1+r.Intn(3); p++ {
				fmt.Fprintf(&b, "%s %s See [[%s]].\n\n", sentence(r, 15), sentence(r, 12), names[r.Intn(len(names))])
			}
			if r.Intn(3) == 0 {
				fmt.Fprintf(&b, "> [!rule] %s ^r%d%d\n\n", sentence(r, 8), i, h)
			}
			if r.Intn(2) == 0 {
				for t := 0; t < 3; t++ {
					st := " x/!"[r.Intn(4)]
					fmt.Fprintf(&b, "- [%c] %s ^t%d%d%d\n", st, sentence(r, 5), i, h, t)
				}
				b.WriteString("\n")
			}
			if r.Intn(4) == 0 {
				fmt.Fprintf(&b, "### %s\n\n```go\nfunc x() { // [[not-a-link]]\n}\n```\n\n", sentence(r, 2))
			}
		}
		p := filepath.Join(root, filepath.FromSlash(d.dir), name+".md")
		os.MkdirAll(filepath.Dir(p), 0o755)
		if err := os.WriteFile(p, []byte(b.String()), 0o644); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
	}
	fmt.Printf("wrote %d notes to %s\n", *n, root)
}

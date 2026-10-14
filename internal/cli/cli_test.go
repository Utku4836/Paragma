package cli

import (
	"bytes"
	"encoding/json"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// copyVault copies testdata/sample/.paragma (without the index) to a temp dir.
func copyVault(t *testing.T) string {
	t.Helper()
	src := filepath.Join("..", "..", "testdata", "sample", ".paragma")
	dst := filepath.Join(t.TempDir(), ".paragma")
	err := filepath.WalkDir(src, func(p string, e fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		rel, _ := filepath.Rel(src, p)
		if strings.HasPrefix(e.Name(), "index.db") {
			return nil
		}
		if e.IsDir() {
			return os.MkdirAll(filepath.Join(dst, rel), 0o755)
		}
		b, err := os.ReadFile(p)
		if err != nil {
			return err
		}
		return os.WriteFile(filepath.Join(dst, rel), b, 0o644)
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Setenv("PARAGMA_HOME", filepath.Join(t.TempDir(), "nohome"))
	return dst
}

func run(t *testing.T, vault string, args ...string) (string, string, int) {
	t.Helper()
	var out, errb bytes.Buffer
	code := Execute(append([]string{"--vault", vault}, args...), &out, &errb, strings.NewReader(""))
	return out.String(), errb.String(), code
}

func TestCommands(t *testing.T) {
	v := copyVault(t)
	cases := []struct {
		args []string
		code int
		has  []string
		not  []string
	}{
		{[]string{"search", "database", "locked"}, 0, []string{"episodes/errors/sqlite-database-locked  [error resolved seen:3]", "#Belirti"}, nil},
		{[]string{"search", "cozum", "wal"}, 0, []string{"sqlite-database-locked#Çözüm"}, nil},
		{[]string{"search", "busy_timeout"}, 0, []string{"#Çözüm", "knowledge/rules#Veritabanı  [rule !rule]"}, nil},
		{[]string{"search", "journal", "DELETE"}, 0, []string{"0003-journal-mode"}, nil},
		{[]string{"search", "zzzqqq"}, 1, []string{"no results"}, nil},
		{[]string{"search", "wal", "--layer", "semantic"}, 0, []string{"knowledge/"}, []string{"episodes/"}},
		{[]string{"search", "wal", "--layer", "nope"}, 2, nil, nil},
		{[]string{"outline", "architecture"}, 0, []string{"### Katmanlar/Depolama", "[!rule]", "tasks 1/2"}, nil},
		{[]string{"read", "architecture#depolama"}, 0, []string{"knowledge/architecture#Katmanlar/Depolama  L20-27", "busy_timeout=5000"}, []string{"## Middleware"}},
		{[]string{"read", "auth-refactor^t002"}, 0, []string{"Refresh endpoint", "Testler ^t004"}, []string{"Prod migration"}},
		{[]string{"read", "^t005"}, 0, []string{"work/sessions/auth-refactor^t005", "Prod migration"}, nil},
		{[]string{"read", "stack:12-13"}, 0, []string{"knowledge/stack:12-13", "Go 1.24"}, nil},
		{[]string{"read", "@01JA3Z8K2MSTACK"}, 0, []string{"knowledge/stack  L9-", "## CI"}, []string{"verified:"}},
		{[]string{"read", "stack", "--meta"}, 0, []string{"verified: 2026-05-01"}, nil},
		{[]string{"read", "architecture#Middlewre"}, 3, nil, nil},
		{[]string{"read", "archtecture"}, 3, nil, nil},
		{[]string{"peek", "auth-refactor", "--lines", "2"}, 0, []string{"# Auth refactor", "more: pg read work/sessions/auth-refactor"}, nil},
		{[]string{"links", "auth-refactor"}, 0, []string{"episodes/errors/jwt-exp-timezone  [wiki]  L26,30", "knowledge/architecture#Middleware  [wiki]"}, nil},
		{[]string{"links", "0007-wal-modu"}, 0, []string{"0003-journal-mode  [supersedes]"}, nil},
		{[]string{"backlinks", "jwt-exp-timezone"}, 0, []string{"knowledge/rules#Güvenlik  [fixes]", "work/sessions/auth-refactor#Devir"}, nil},
		{[]string{"backlinks", "architecture#Middleware"}, 0, []string{"auth-refactor#Notlar"}, nil},
		{[]string{"backlinks", "architecture#Dağıtım"}, 1, nil, nil},
		{[]string{"find", "--type", "error"}, 0, []string{"jwt-exp-timezone", "sqlite-database-locked", "2 notes"}, nil},
		{[]string{"find", "--open-tasks"}, 0, []string{"auth-refactor", "tasks 2/7"}, nil},
		{[]string{"find", "--layer", "semantic", "--title", "rotasyon"}, 0, []string{"refresh-token-rotasyonu"}, nil},
		{[]string{"status"}, 0, []string{"files 12", "0 broken"}, nil},
		{[]string{"bogus"}, 2, nil, nil},
	}
	for _, c := range cases {
		out, errs, code := run(t, v, c.args...)
		name := strings.Join(c.args, " ")
		if code != c.code {
			t.Errorf("pg %s: exit %d want %d\nstdout:%s\nstderr:%s", name, code, c.code, out, errs)
			continue
		}
		for _, h := range c.has {
			if !strings.Contains(out, h) {
				t.Errorf("pg %s: missing %q in\n%s", name, h, out)
			}
		}
		for _, n := range c.not {
			if strings.Contains(out, n) {
				t.Errorf("pg %s: unexpected %q in\n%s", name, n, out)
			}
		}
	}
}

func TestSupersededSinks(t *testing.T) {
	v := copyVault(t)
	out, _, _ := run(t, v, "search", "journal", "mode", "--json")
	var res struct {
		Results []struct{ Addr string } `json:"results"`
	}
	if err := json.Unmarshal([]byte(out), &res); err != nil {
		t.Fatal(err, out)
	}
	if len(res.Results) < 2 || strings.Contains(res.Results[0].Addr, "0003") {
		t.Errorf("superseded decision should rank below active ones: %+v", res.Results)
	}
}

func TestIncrementalSync(t *testing.T) {
	v := copyVault(t)
	if _, _, code := run(t, v, "search", "kangal"); code != 1 {
		t.Fatal("unexpected hit before write")
	}
	p := filepath.Join(v, "knowledge", "dogs.md")
	os.WriteFile(p, []byte("# Dogs\n\n## Kangal\nBüyük çoban köpeği.\n"), 0o644)
	out, _, code := run(t, v, "search", "kangal")
	if code != 0 || !strings.Contains(out, "knowledge/dogs#Kangal") {
		t.Fatalf("new file not indexed: %d %s", code, out)
	}
	// edit with a different size so the stat check sees it
	os.WriteFile(p, []byte("# Dogs\n\n## Akbaş\nBeyaz çoban köpeği, kangal değil.\n"), 0o644)
	future := time.Now().Add(2 * time.Second)
	os.Chtimes(p, future, future)
	out, _, _ = run(t, v, "search", "akbas")
	if !strings.Contains(out, "knowledge/dogs#Akbaş") {
		t.Fatalf("edit not indexed: %s", out)
	}
	os.Remove(p)
	if _, _, code := run(t, v, "search", "akbas"); code != 1 {
		t.Fatal("deleted file still indexed")
	}
}

func TestReadLimitShowsOutline(t *testing.T) {
	v := copyVault(t)
	out, _, code := run(t, v, "read", "architecture", "--budget", "60")
	if code != 0 || !strings.Contains(out, "too large") || !strings.Contains(out, "knowledge/architecture#Middleware") {
		t.Fatalf("expected outline fallback:\n%s", out)
	}
	out, _, _ = run(t, v, "read", "architecture#Middleware", "--budget", "20")
	if !strings.Contains(out, "cut at L") || !strings.Contains(out, "next: pg read knowledge/architecture:") {
		t.Fatalf("expected cut with continuation:\n%s", out)
	}
	out, _, _ = run(t, v, "read", "architecture", "--budget", "60", "--full")
	if strings.Contains(out, "too large") {
		t.Fatal("--full must not fall back")
	}
}

func TestNoVault(t *testing.T) {
	t.Setenv("PARAGMA_HOME", filepath.Join(t.TempDir(), "nohome"))
	t.Setenv("PARAGMA_VAULT", "")
	wd, _ := os.Getwd()
	defer os.Chdir(wd)
	os.Chdir(t.TempDir())
	var out, errb bytes.Buffer
	if code := Execute([]string{"search", "x"}, &out, &errb, strings.NewReader("")); code != ExitNoVault {
		t.Fatalf("exit %d: %s", code, errb.String())
	}
}

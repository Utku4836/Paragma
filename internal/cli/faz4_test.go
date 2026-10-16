package cli

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/Utku-Private/Paragma/internal/textutil"
)

func TestInitStartsEmptyAndIgnored(t *testing.T) {
	t.Setenv("PARAGMA_HOME", filepath.Join(t.TempDir(), "home"))
	proj := t.TempDir()
	os.MkdirAll(filepath.Join(proj, ".git"), 0o755)
	os.WriteFile(filepath.Join(proj, ".gitignore"), []byte("bin/"), 0o644)
	var out, errb strings.Builder
	if code := Execute([]string{"init", proj}, &out, &errb, strings.NewReader("")); code != 0 {
		t.Fatalf("init: %d %s", code, errb.String())
	}
	if !strings.Contains(out.String(), "(0 notes)") || !strings.Contains(out.String(), "added .paragma/") {
		t.Fatalf("init output:\n%s", out.String())
	}
	gi, _ := os.ReadFile(filepath.Join(proj, ".gitignore"))
	if !strings.Contains(string(gi), "bin/\n") || !strings.Contains(string(gi), ".paragma/") {
		t.Fatalf(".gitignore:\n%s", gi)
	}
	// running again does not add it twice
	out.Reset()
	Execute([]string{"init", proj}, &out, &errb, strings.NewReader(""))
	gi, _ = os.ReadFile(filepath.Join(proj, ".gitignore"))
	if strings.Count(string(gi), ".paragma/") != 1 {
		t.Fatalf("duplicate entry:\n%s", gi)
	}
	// a directory that is not a git repository gets no .gitignore
	plain := t.TempDir()
	Execute([]string{"init", plain}, &out, &errb, strings.NewReader(""))
	if _, err := os.Stat(filepath.Join(plain, ".gitignore")); err == nil {
		t.Fatal(".gitignore created outside a git repository")
	}
}

func TestGlobalErrorFallback(t *testing.T) {
	v := copyVault(t)
	home := filepath.Join(t.TempDir(), "home")
	t.Setenv("PARAGMA_HOME", home)
	var out, errb strings.Builder
	if code := Execute([]string{"init", "--global"}, &out, &errb, strings.NewReader("")); code != 0 {
		t.Fatal(errb.String())
	}
	out2 := mustRun(t, v, "error", "record", "--global", "--sig", `exec: gcc: executable file not found in PATH`,
		"--cause", "cgo needs a C compiler", "--fix", "set CGO_ENABLED=0 or install mingw-w64")
	if !strings.Contains(out2, "ok ~episodes/errors/") {
		t.Fatalf("global record: %s", out2)
	}
	res, _, code := run(t, v, "run", "--", failing(`exec: gcc: executable file not found in PATH`, "2"))
	if code != 2 || !strings.Contains(res, "known error") || !strings.Contains(res, "~episodes/errors/") || !strings.Contains(res, "CGO_ENABLED=0") {
		t.Fatalf("global fallback:\n%s", res)
	}
}

func TestRelatedAndContext(t *testing.T) {
	v := copyVault(t)
	// architecture.md names internal/db (package db) and cmd/migrate
	out := mustRun(t, v, "related", "internal/db/open.go")
	if !strings.Contains(out, "knowledge/architecture#Katmanlar/Depolama") {
		t.Fatalf("related by directory:\n%s", out)
	}
	if !strings.Contains(out, "episodes/errors/sqlite-database-locked") {
		t.Fatalf("related by file name:\n%s", out)
	}
	if _, _, code := run(t, v, "related", "frontend/app.svelte"); code != ExitNoResults {
		t.Fatal("unrelated file matched")
	}
	out = mustRun(t, v, "context", "jwt timezone")
	for _, want := range []string{"episodes/errors/jwt-exp-timezone", "knowledge/rules^r003"} {
		if !strings.Contains(out, want) {
			t.Errorf("context lacks %s:\n%s", want, out)
		}
	}
	if n := textutil.Tokens(out); n > 700 {
		t.Errorf("context over budget: %d tokens", n)
	}
	small := mustRun(t, v, "context", "jwt timezone", "--budget", "120")
	if !strings.Contains(small, "more: ") {
		t.Errorf("small budget should list what did not fit:\n%s", small)
	}
}

func TestNeighborsOrphansBroken(t *testing.T) {
	v := copyVault(t)
	out := mustRun(t, v, "neighbors", "jwt-exp-timezone")
	for _, want := range []string{"← knowledge/rules", "fixes", "← work/sessions/auth-refactor"} {
		if !strings.Contains(out, want) {
			t.Errorf("neighbors lacks %q:\n%s", want, out)
		}
	}
	out = mustRun(t, v, "neighbors", "0007-wal-modu", "--depth", "2")
	if !strings.Contains(out, "→ episodes/decisions/0003-journal-mode") || !strings.Contains(out, "supersedes") {
		t.Fatalf("depth 2:\n%s", out)
	}
	if out := mustRun(t, v, "orphans"); !strings.Contains(out, "knowledge/stack") {
		t.Fatal(out)
	}
	os.WriteFile(filepath.Join(v, "knowledge", "x.md"), []byte("# X\n\nsee [[jwt-exp-timzone]]\n"), 0o644)
	if out := mustRun(t, v, "broken"); !strings.Contains(out, "[[jwt-exp-timzone]]  did you mean jwt-exp-timezone") {
		t.Fatal(out)
	}
}

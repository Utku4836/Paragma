package cli

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/Utku-Private/Paragma/internal/textutil"
)

func TestFirstUseAndWorkDirState(t *testing.T) {
	home := filepath.Join(t.TempDir(), "home")
	t.Setenv("PARAGMA_HOME", home)
	t.Setenv("PARAGMA_PROJECT", "")
	t.Setenv("PARAGMA_SPACE", "")
	repo := t.TempDir()
	os.MkdirAll(filepath.Join(repo, ".git"), 0o755)
	os.MkdirAll(filepath.Join(repo, "sub"), 0o755)
	t.Chdir(filepath.Join(repo, "sub"))
	var out, errb strings.Builder
	// nothing to set up: the default space appears on first use, empty
	if code := Execute([]string{"prime"}, &out, &errb, strings.NewReader("")); code != 0 || !strings.Contains(out.String(), "memory is empty") {
		t.Fatalf("prime on first use: %d %s%s", code, out.String(), errb.String())
	}
	if _, err := os.Stat(filepath.Join(home, "Spaces", "main")); err != nil {
		t.Fatal("main space not created")
	}
	// writes need a project
	out.Reset()
	errb.Reset()
	if code := Execute([]string{"log", "x"}, &out, &errb, strings.NewReader("")); code != ExitNotFound || !strings.Contains(errb.String(), "pg start") {
		t.Fatalf("log without a project: %d %s", code, errb.String())
	}
	Execute([]string{"start", "shop"}, &out, &errb, strings.NewReader(""))
	// the current project is remembered for the git work tree, not the subdirectory
	t.Chdir(repo)
	out.Reset()
	if code := Execute([]string{"log", "from the repo root"}, &out, &errb, strings.NewReader("")); code != 0 || !strings.Contains(out.String(), "ok shop/shop#Log") {
		t.Fatalf("current project at the repo root: %d %s %s", code, out.String(), errb.String())
	}
	// memory never lands in the repository
	if _, err := os.Stat(filepath.Join(repo, ".paragma")); err == nil {
		t.Fatal(".paragma created in the repo")
	}
}

func TestErrorFromOtherProject(t *testing.T) {
	v := copyVault(t)
	mustRun(t, v, "error", "record", "--sig", `exec: gcc: executable file not found in PATH`,
		"--cause", "cgo needs a C compiler", "--fix", "set CGO_ENABLED=0 or install mingw-w64")
	// working on another project: the fix is still found, marked with where it came from
	t.Setenv("PARAGMA_PROJECT", "auth-refactor")
	res, _, code := run(t, v, "run", "--", failing(`exec: gcc: executable file not found in PATH`, "2"))
	if code != 2 || !strings.Contains(res, "known error") || !strings.Contains(res, "(from project demo)") || !strings.Contains(res, "CGO_ENABLED=0") {
		t.Fatalf("cross-project match:\n%s", res)
	}
}

func TestRelatedAndContext(t *testing.T) {
	v := copyVault(t)
	// architecture.md names internal/db (package db) and cmd/migrate
	out := mustRun(t, v, "related", "internal/db/open.go")
	if !strings.Contains(out, "demo/architecture#Katmanlar/Depolama") {
		t.Fatalf("related by directory:\n%s", out)
	}
	if !strings.Contains(out, "demo/errors/sqlite-database-locked") {
		t.Fatalf("related by file name:\n%s", out)
	}
	if _, _, code := run(t, v, "related", "frontend/app.svelte"); code != ExitNoResults {
		t.Fatal("unrelated file matched")
	}
	out = mustRun(t, v, "context", "jwt timezone")
	for _, want := range []string{"demo/errors/jwt-exp-timezone", "demo/rules^r003", "all other project rules", "covered: "} {
		if !strings.Contains(out, want) {
			t.Errorf("context lacks %s:\n%s", want, out)
		}
	}
	if n := textutil.Tokens(out); n > 1600 {
		t.Errorf("context over budget: %d tokens", n)
	}
	small := mustRun(t, v, "context", "jwt timezone", "--budget", "120")
	if !strings.Contains(small, "over budget (pg read <addr>): ") {
		t.Errorf("small budget should list what did not fit:\n%s", small)
	}
}

func TestNeighborsOrphansBroken(t *testing.T) {
	v := copyVault(t)
	out := mustRun(t, v, "neighbors", "jwt-exp-timezone")
	for _, want := range []string{"<- demo/rules", "fixes", "<- auth-refactor/auth-refactor"} {
		if !strings.Contains(out, want) {
			t.Errorf("neighbors lacks %q:\n%s", want, out)
		}
	}
	out = mustRun(t, v, "neighbors", "0007-wal-modu", "--depth", "2")
	if !strings.Contains(out, "-> demo/decisions/0003-journal-mode") || !strings.Contains(out, "supersedes") {
		t.Fatalf("depth 2:\n%s", out)
	}
	if out := mustRun(t, v, "orphans"); !strings.Contains(out, "demo/stack") {
		t.Fatal(out)
	}
	os.WriteFile(filepath.Join(v, "Spaces", "main", "demo", "x.md"), []byte("# X\n\nsee [[jwt-exp-timzone]]\n"), 0o644)
	if out := mustRun(t, v, "broken"); !strings.Contains(out, "[[jwt-exp-timzone]]  did you mean jwt-exp-timezone") {
		t.Fatal(out)
	}
}

func TestResumeShowsCodeChanges(t *testing.T) {
	if _, err := exec.LookPath("git"); err != nil {
		t.Skip("git not installed")
	}
	v := copyVault(t)
	t.Setenv("PARAGMA_PROJECT", "")
	repo := t.TempDir()
	t.Chdir(repo)
	git := func(args ...string) {
		c := exec.Command("git", append([]string{"-c", "user.name=t", "-c", "user.email=t@example.com"}, args...)...)
		c.Dir = repo
		if out, err := c.CombinedOutput(); err != nil {
			t.Fatalf("git %v: %v %s", args, err, out)
		}
	}
	git("init", "-q")
	os.WriteFile(filepath.Join(repo, "a.go"), []byte("package a\n\nfunc Old() {}\n"), 0o644)
	git("add", "-A")
	git("commit", "-qm", "init")
	mustRun(t, v, "start", "feature", "--step", "x")
	os.MkdirAll(filepath.Join(repo, "pricing"), 0o755)
	os.WriteFile(filepath.Join(repo, "pricing", "discount.go"), []byte("package pricing\n\ntype Discount struct {\n\tp int\n}\n\nfunc Percent(p int) (Discount, error) { return Discount{p}, nil }\n"), 0o644)
	git("add", "-A")
	git("commit", "-qm", "step a")
	os.WriteFile(filepath.Join(repo, "a.go"), []byte("package a\n\nfunc Old() {}\n\nfunc New() int { return 1 }\n"), 0o644)
	out := mustRun(t, v, "resume", "feature")
	for _, want := range []string{"code changed since the project started (git):", "A pricing/discount.go: type Discount struct; func Percent(p int) (Discount, error)", "M a.go: func Old(); func New() int"} {
		if !strings.Contains(out, want) {
			t.Errorf("resume lacks %q:\n%s", want, out)
		}
	}
}

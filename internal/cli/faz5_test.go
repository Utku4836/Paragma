package cli

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestKeywordsAndMisses(t *testing.T) {
	v := copyVault(t)
	// the note says "database is locked"; the agent searches in Turkish
	out, _, code := run(t, v, "search", "kilidi", "çakışma")
	if code != ExitNoResults || !strings.Contains(out, "try: ") || !strings.Contains(out, "topics: ") {
		t.Fatalf("empty search should hint:\n%s", out)
	}
	mustRun(t, v, "keywords", "sqlite-database-locked", "kilidi çakışma, sqlite busy")
	if out := mustRun(t, v, "search", "kilidi", "çakışma"); !strings.Contains(out, "episodes/errors/sqlite-database-locked") {
		t.Fatalf("keyword not searchable:\n%s", out)
	}
	if s := readNote(t, v, "episodes/errors/sqlite-database-locked.md"); !strings.Contains(s, "keywords: [kilidi çakışma, sqlite busy]") {
		t.Fatalf("front matter:\n%s", s)
	}
	run(t, v, "search", "kubernetes", "ingress")
	run(t, v, "search", "Kubernetes", "Ingress")
	run(t, v, "error", "find", "panic: unknown frobnicator state")
	out = mustRun(t, v, "misses")
	if !strings.Contains(out, `2x  search  "kubernetes ingress"`) || !strings.Contains(out, "error find") {
		t.Fatalf("misses:\n%s", out)
	}
	if _, err := os.Stat(filepath.Join(v, "misses.log")); err != nil {
		t.Fatal(err)
	}
	// keywords given at record time
	mustRun(t, v, "error", "record", "--sig", "x509: certificate signed by unknown authority", "--cause", "corporate proxy", "--fix", "add the proxy CA", "--keywords", "sertifika hatası, TLS")
	if out := mustRun(t, v, "search", "sertifika"); !strings.Contains(out, "x509-certificate-signed-by-unknown-authority") {
		t.Fatalf("record --keywords:\n%s", out)
	}
}

func TestAutoPromote(t *testing.T) {
	v := copyVault(t)
	// sqlite-database-locked has seen: 3 already; jwt-exp-timezone has 1
	mustRun(t, v, "error", "seen", "jwt-exp-timezone")
	out := mustRun(t, v, "error", "seen", "jwt-exp-timezone")
	if !strings.Contains(out, "seen 3 times: promoted to rule knowledge/rules^r007") {
		t.Fatalf("auto promote:\n%s", out)
	}
	if s := readNote(t, v, "knowledge/rules.md"); !strings.Contains(s, "time.Now().UTC()") || !strings.Contains(s, "from [[jwt-exp-timezone#Çözüm]]") {
		t.Fatalf("rule:\n%s", s)
	}
	if s := readNote(t, v, "episodes/errors/jwt-exp-timezone.md"); !strings.Contains(s, "promoted:: [[rules^r007]]") {
		t.Fatalf("source link:\n%s", s)
	}
	// a further seen does not promote again
	if out := mustRun(t, v, "error", "seen", "jwt-exp-timezone"); strings.Contains(out, "promoted") {
		t.Fatalf("promoted twice:\n%s", out)
	}
}

func TestDupes(t *testing.T) {
	v := copyVault(t)
	os.WriteFile(filepath.Join(v, "episodes", "errors", "db-locked-again.md"), []byte("---\ntype: error\nlayer: episodic\nsig: \"database is locked (7)\"\n---\n# Database locked again\n\n## Belirti\nParalel testlerde database is locked hatası.\n\n## Çözüm\nWAL modu ve busy_timeout.\n"), 0o644)
	out := mustRun(t, v, "dupes", "--type", "error")
	if !strings.Contains(out, "episodes/errors/sqlite-database-locked ~ episodes/errors/db-locked-again") && !strings.Contains(out, "episodes/errors/db-locked-again ~ episodes/errors/sqlite-database-locked") {
		t.Fatalf("dupes:\n%s", out)
	}
	if !strings.Contains(out, "same error signature") {
		t.Fatalf("dupes reason:\n%s", out)
	}
}

func TestGC(t *testing.T) {
	v := copyVault(t)
	// close the sample session and pretend it ended long ago
	mustRun(t, v, "session", "close", "auth-refactor", "--summary", "paused")
	p := filepath.Join(v, "archive", "work", "sessions", "auth-refactor.md")
	b, err := os.ReadFile(p)
	if err != nil {
		t.Fatal(err)
	}
	s := string(b)
	i := strings.Index(s, "closed: ")
	s = s[:i] + "closed: 2025-01-10 10:00" + s[strings.Index(s[i:], "\n")+i:]
	os.WriteFile(p, []byte(s), 0o644)
	out := mustRun(t, v, "gc")
	if !strings.Contains(out, "archive/work/sessions/auth-refactor  ended 2025-01-10") || !strings.Contains(out, "--apply") {
		t.Fatalf("gc dry run:\n%s", out)
	}
	if _, err := os.Stat(p); err != nil {
		t.Fatal("dry run deleted the file")
	}
	mustRun(t, v, "gc", "--apply")
	if _, err := os.Stat(p); err == nil {
		t.Fatal("not deleted")
	}
	j := readNote(t, v, "episodes/journal/2025-01.md")
	if !strings.Contains(j, "## 2025-01-10") || !strings.Contains(j, `session "Auth refactor" (gc)`) {
		t.Fatalf("journal:\n%s", j)
	}
}

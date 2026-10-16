package cli

import (
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/Utku-Private/Paragma/internal/textutil"
)

// failing returns a shell one-liner that prints msg and exits with code.
func failing(msg, code string) string {
	if runtime.GOOS == "windows" {
		return "echo " + msg + " & exit " + code
	}
	return "echo '" + msg + "'; exit " + code
}

func TestErrorMemory(t *testing.T) {
	v := copyVault(t)
	out := mustRun(t, v, "error", "record", "--sig", "pool.go:88: redis: connection pool exhausted after 250ms (size 20)",
		"--cause", "pool too small for load tests", "--fix", "raise pool size to 50", "--files", "cache/redis.go", "--tags", "redis")
	if !strings.Contains(out, "ok demo/errors/redis-connection-pool-exhausted-after-250ms-size-20") {
		t.Fatalf("record/title: %s", out)
	}
	// the same error with other numbers and paths is an exact match
	out = mustRun(t, v, "error", "find", "C:/srv/app/pool.go:12: redis: connection pool exhausted after 900ms (size 64)")
	if !strings.Contains(out, "#Fix") || !strings.Contains(out, "exact") || !strings.Contains(out, "raise pool size to 50") {
		t.Fatalf("find: %s", out)
	}
	// recording it again changes nothing
	out = mustRun(t, v, "error", "record", "--sig", "redis: connection pool exhausted after 1s (size 8)", "--cause", "x", "--fix", "y")
	if !strings.Contains(out, "exists: demo/errors/redis-connection-pool") {
		t.Fatalf("duplicate record: %s", out)
	}
	if out := mustRun(t, v, "error", "seen", "redis-connection-pool-exhausted-after-250ms-size-20"); !strings.Contains(out, "seen:2") {
		t.Fatal(out)
	}
	// the sample vault's hand-written error note matches by its sig field
	if out := mustRun(t, v, "error", "find", "database is locked (42)"); !strings.Contains(out, "sqlite-database-locked") {
		t.Fatal(out)
	}
	if _, _, code := run(t, v, "error", "find", "segmentation fault in libfoo.so"); code != ExitNoResults {
		t.Fatal("unrelated error matched")
	}
	mustRun(t, v, "error", "reopen", "jwt-exp-timezone", "--note", "fails again in CI")
	if s := readNote(t, v, "demo/errors/jwt-exp-timezone.md"); !strings.Contains(s, "status: active") || !strings.Contains(s, "reopened") {
		t.Fatal(s)
	}
}

func TestRunShowsKnownFix(t *testing.T) {
	v := copyVault(t)
	mustRun(t, v, "error", "record", "--sig", "catalog checksum mismatch: got 3f2a9c1e0b7d want 4ff52f25fc9d",
		"--cause", "CRLF checkout", "--fix", "normalise line endings before hashing")
	out, _, code := run(t, v, "run", "--", failing("catalog checksum mismatch: got 0011aa22bb33 want 4ff52f25fc9d", "3"))
	if code != 3 {
		t.Fatalf("exit code not kept: %d\n%s", code, out)
	}
	if !strings.Contains(out, "--- paragma: known error (exact match") || !strings.Contains(out, "normalise line endings before hashing") {
		t.Fatalf("no known fix:\n%s", out)
	}
	// an unknown failure is saved for error record --from-last
	out, _, code = run(t, v, "run", "--", failing("fatal: widget frobnication failed at stage 2", "1"))
	if code != 1 || !strings.Contains(out, "not in memory") {
		t.Fatalf("unknown failure: %d\n%s", code, out)
	}
	var lr lastRun
	b, err := os.ReadFile(filepath.Join(v, "Spaces", "main", ".pg", "last-run.json"))
	if err != nil || json.Unmarshal(b, &lr) != nil || len(lr.Lines) == 0 {
		t.Fatalf("last-run.json: %v %s", err, b)
	}
	out = mustRun(t, v, "error", "record", "--from-last", "--cause", "stage 2 needs the cache", "--fix", "warm the cache first")
	if !strings.Contains(out, "ok demo/errors/fatal-widget-frobnication-failed-at-stage-2") {
		t.Fatal(out)
	}
	// success: output passes through, nothing added, exit 0
	ok := "echo all good"
	out, _, code = run(t, v, "run", "--", ok)
	if code != 0 || strings.Contains(out, "paragma") || !strings.Contains(out, "all good") {
		t.Fatalf("success run: %d\n%s", code, out)
	}
}

func TestRulesDecisionsPromote(t *testing.T) {
	v := copyVault(t)
	out := mustRun(t, v, "rule", "add", "Every handler validates input before calling a service", "--section", "Kod stili")
	if !strings.Contains(out, "ok demo/rules^r007") {
		t.Fatalf("next rule id: %s", out)
	}
	out = mustRun(t, v, "rule", "list", "--section", "Kod stili")
	if !strings.Contains(out, "demo/rules^r007  [Kod stili]  Every handler validates input") {
		t.Fatal(out)
	}
	out = mustRun(t, v, "promote", "jwt-exp-timezone#Çözüm", "--section", "Güvenlik", "--text", "Compare JWT times with time.Now().UTC()")
	if !strings.Contains(out, "ok demo/rules^r008") {
		t.Fatal(out)
	}
	if s := readNote(t, v, "demo/errors/jwt-exp-timezone.md"); !strings.Contains(s, "promoted:: [[demo/rules^r008]]") {
		t.Fatal(s)
	}
	if out := mustRun(t, v, "backlinks", "rules^r008"); !strings.Contains(out, "jwt-exp-timezone") {
		t.Fatal(out)
	}
	out = mustRun(t, v, "decision", "record", "Sessions in Postgres", "--context", "Redis is one more service", "--decision", "store sessions in Postgres", "--supersedes", "0007-wal-modu")
	if !strings.Contains(out, "ok demo/decisions/0008-sessions-in-postgres") {
		t.Fatal(out)
	}
	if s := readNote(t, v, "demo/decisions/0007-wal-modu.md"); !strings.Contains(s, "status: superseded") {
		t.Fatal(s)
	}
	mustRun(t, v, "journal", "released v0.3")
	mustRun(t, v, "verify", "stack")
	if s := readNote(t, v, "demo/stack.md"); strings.Contains(s, "verified: 2026-05-01") {
		t.Fatal("verify did not update the date")
	}
}

func TestAgentIntegration(t *testing.T) {
	v := copyVault(t)
	base := t.TempDir()
	t.Chdir(base)
	out := mustRun(t, v, "prime")
	if n := textutil.Tokens(out); n > 150 {
		t.Errorf("prime is %d tokens:\n%s", n, out)
	}
	if !strings.Contains(out, "projects: auth-refactor (1 notes, tasks 2/7), *demo") {
		t.Errorf("prime lacks the session:\n%s", out)
	}
	if n := textutil.Tokens(skillBody); n > 500 {
		t.Errorf("skill is %d tokens", n)
	}
	os.WriteFile(filepath.Join(base, "AGENTS.md"), []byte("# Team notes\n\nKeep PRs small.\n"), 0o644)
	if out := mustRun(t, v, "install", "agents-md"); !strings.Contains(out, "(written)") {
		t.Fatal(out)
	}
	if out := mustRun(t, v, "install", "agents-md"); !strings.Contains(out, "(up to date)") {
		t.Fatal(out)
	}
	agents, _ := os.ReadFile(filepath.Join(base, "AGENTS.md"))
	if !strings.HasPrefix(string(agents), "# Team notes") || strings.Count(string(agents), markStart) != 1 {
		t.Fatalf("AGENTS.md:\n%s", agents)
	}
	os.MkdirAll(filepath.Join(base, ".claude"), 0o755)
	os.WriteFile(filepath.Join(base, ".claude", "settings.json"), []byte(`{"model":"opus","hooks":{"Stop":[{"hooks":[{"type":"command","command":"say done"}]}]}}`), 0o644)
	mustRun(t, v, "install", "claude", "--hooks")
	mustRun(t, v, "install", "claude", "--hooks")
	sb, _ := os.ReadFile(filepath.Join(base, ".claude", "settings.json"))
	s := string(sb)
	if strings.Count(s, "pg hook session-start") != 1 || strings.Count(s, "pg hook pre-compact") != 1 || !strings.Contains(s, "say done") || !strings.Contains(s, `"model": "opus"`) {
		t.Fatalf("settings.json:\n%s", s)
	}
	if _, err := os.Stat(filepath.Join(base, ".claude", "skills", "paragma", "SKILL.md")); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PARAGMA_PROJECT", "auth-refactor")
	out, _, code := runIn(t, v, `{"hook_event_name":"PreCompact"}`, "hook", "pre-compact")
	if code != 0 || out != "" {
		t.Fatalf("pre-compact must be silent: %d %q", code, out)
	}
	if s := readNote(t, v, "auth-refactor/auth-refactor.md"); !strings.Contains(s, "Automatic checkpoint before context compaction") {
		t.Fatal(s)
	}
}

func TestDoctorGrepRecent(t *testing.T) {
	v := copyVault(t)
	if out := mustRun(t, v, "grep", "busy_timeout"); !strings.Contains(out, "demo/errors/sqlite-database-locked:") {
		t.Fatal(out)
	}
	if out := mustRun(t, v, "recent", "--limit", "3"); strings.Count(out, "\n") != 3 {
		t.Fatal(out)
	}
	os.WriteFile(filepath.Join(v, "Spaces", "main", "demo", "broken.md"), []byte("# Broken\n\nsee [[nowhere]]\n"), 0o644)
	out, _, code := run(t, v, "doctor")
	if code != ExitValidation || !strings.Contains(out, "broken link [[nowhere]]") || !strings.Contains(out, "demo/broken  no id") {
		t.Fatalf("doctor: %d\n%s", code, out)
	}
	// --fix adds the missing id; the broken link is still reported (exit 6)
	if out, _, code := run(t, v, "doctor", "--fix"); code != ExitValidation || !strings.Contains(out, "ok added ids") || strings.Contains(out, "no id") {
		t.Fatalf("doctor --fix: %d\n%s", code, out)
	}
	if s := readNote(t, v, "demo/broken.md"); !strings.HasPrefix(s, "---\nid: ") {
		t.Fatal(s)
	}
}

// A short, generic signature inside a longer unrelated error is not the
// same error (the false positive found against the benchmark memory).
func TestNoFalsePositiveOnGenericSig(t *testing.T) {
	v := copyVault(t)
	mustRun(t, v, "error", "record", "--sig", "context deadline exceeded", "--cause", "lint cache miss", "--fix", "key the cache on go.sum")
	out, _, _ := run(t, v, "run", "--", failing("context deadline exceeded while awaiting headers", "1"))
	if strings.Contains(out, "known error") {
		t.Fatalf("generic signature matched an unrelated error:\n%s", out)
	}
	out, _, _ = run(t, v, "run", "--", failing("golangci-lint: context deadline exceeded", "1"))
	if !strings.Contains(out, "known error") {
		t.Fatalf("the real recurrence was missed:\n%s", out)
	}
}

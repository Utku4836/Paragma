package cli

import (
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
)

// mustRun runs pg and fails the test on a non-zero exit.
func mustRun(t *testing.T, v string, args ...string) string {
	t.Helper()
	out, errs, code := run(t, v, args...)
	if code != 0 {
		t.Fatalf("pg %s: exit %d\n%s%s", strings.Join(args, " "), code, out, errs)
	}
	return out
}

func readNote(t *testing.T, v, rel string) string {
	t.Helper()
	b, err := os.ReadFile(filepath.Join(v, filepath.FromSlash(rel)))
	if err != nil {
		t.Fatal(err)
	}
	return string(b)
}

var reTaskID = regexp.MustCompile(`\^(t[0-9a-z]+)`)

func ids(out string) []string {
	var r []string
	for _, m := range reTaskID.FindAllStringSubmatch(out, -1) {
		r = append(r, m[1])
	}
	return r
}

func TestSessionFlow(t *testing.T) {
	v := copyVault(t)
	t.Chdir(filepath.Dir(v))
	out := mustRun(t, v, "session", "start", "Payment flow", "--goal", "Card payments via Stripe")
	if !strings.Contains(out, "ok work/sessions/") {
		t.Fatal(out)
	}
	sess := strings.TrimPrefix(strings.Fields(out)[1], "work/sessions/")
	plan := ids(mustRun(t, v, "session", "plan", "Webhook endpoint", "Signature check", "Tests"))
	if len(plan) != 3 {
		t.Fatalf("plan ids: %v", plan)
	}
	sub := ids(mustRun(t, v, "task", "add", "Retry logic", "--under", plan[0]))[0]

	// done refuses while a subtask is open, then the next task is printed
	if _, _, code := run(t, v, "task", "done", plan[0]); code != ExitValidation {
		t.Fatalf("done with open subtask: exit %d", code)
	}
	mustRun(t, v, "task", "start", plan[0])
	out = mustRun(t, v, "task", "done", sub)
	if !strings.Contains(out, "next: work/sessions/"+sess+"^"+plan[0]) {
		t.Fatalf("next after done should be the in-progress parent:\n%s", out)
	}
	mustRun(t, v, "task", "done", plan[0])
	mustRun(t, v, "task", "block", plan[1], "--by", "no test keys")
	mustRun(t, v, "session", "log", "- webhook needs the raw body")
	mustRun(t, v, "session", "checkpoint", "Webhook done. Next: tests.")

	out = mustRun(t, v, "session", "resume")
	for _, want := range []string{"goal: Card payments via Stripe", "progress: 2/4 done", "blocked: " + plan[1], "handoff: Checkpoint", "Next: tests.", "webhook needs the raw body", "next: pg task start " + plan[2]} {
		if !strings.Contains(out, want) {
			t.Errorf("resume lacks %q:\n%s", want, out)
		}
	}
	body := readNote(t, v, "work/sessions/"+sess+".md")
	for _, want := range []string{"- [x] Webhook endpoint ^" + plan[0], "    - [x] Retry logic ^" + sub, "@blocked(no test keys)", "## Notes\n\n- "} {
		if !strings.Contains(body, want) {
			t.Errorf("session note lacks %q:\n%s", want, body)
		}
	}

	// a new conversation finds it by name without the date prefix
	out = mustRun(t, v, "session", "status", "payment-flow")
	if !strings.Contains(out, "progress: 2/4") {
		t.Fatal(out)
	}
	out = mustRun(t, v, "session", "close", "--summary", "webhook shipped")
	if !strings.Contains(out, "ok archive/work/sessions/"+sess) || !strings.Contains(out, "episodes/journal/") {
		t.Fatal(out)
	}
	if j := mustRun(t, v, "search", "webhook", "shipped"); !strings.Contains(j, "episodes/journal") {
		t.Fatalf("journal entry not searchable:\n%s", j)
	}
}

func TestWriteCommands(t *testing.T) {
	v := copyVault(t)
	t.Chdir(filepath.Dir(v))
	mustRun(t, v, "append", "rules#Veritabanı", "> [!rule] Migrations run only via cmd/migrate. ^r007")
	if out := mustRun(t, v, "read", "rules^r007"); !strings.Contains(out, "cmd/migrate") {
		t.Fatal(out)
	}
	// write a heading body keeps the heading and the sub-headings
	mustRun(t, v, "write", "architecture#Katmanlar", "Two layers now.")
	arch := readNote(t, v, "knowledge/architecture.md")
	if !strings.Contains(arch, "## Katmanlar\n\nTwo layers now.\n\n### API") {
		t.Fatalf("write section:\n%s", arch)
	}
	// write a block keeps its id
	mustRun(t, v, "write", "auth-refactor^t004", "    - [ ] Tests incl. timezone")
	if s := readNote(t, v, "work/sessions/auth-refactor.md"); !strings.Contains(s, "    - [ ] Tests incl. timezone ^t004") {
		t.Fatalf("block id lost:\n%s", s)
	}
	// stale hash is refused
	if _, _, code := run(t, v, "append", "stack", "x", "--if-hash", "deadbeef"); code != ExitConflict {
		t.Fatalf("if-hash: exit %d", code)
	}
	// dash-led markdown text is text, not a flag
	mustRun(t, v, "append", "stack#CI", "- lint runs on every push")
	if s := readNote(t, v, "knowledge/stack.md"); !strings.Contains(s, "golangci-lint run`.\n\n- lint runs on every push") {
		t.Fatalf("dash text:\n%s", s)
	}
	// edit: ambiguous without --all
	if _, _, code := run(t, v, "edit", "architecture", "--find", "middleware", "--replace", "mw"); code != ExitValidation {
		t.Fatalf("ambiguous edit: exit %d", code)
	}
	mustRun(t, v, "edit", "stack", "--find", "Go 1.24", "--replace", "Go 1.25")
	// stdin text
	_, _, code := runIn(t, v, "line one\nline two\n", "insert", "rules^r002", "--after", "-")
	if code != 0 || !strings.Contains(readNote(t, v, "knowledge/rules.md"), "^r002\n\nline one\nline two") {
		t.Fatalf("insert from stdin:\n%s", readNote(t, v, "knowledge/rules.md"))
	}
	// UTF-16 file input (PowerShell >)
	f := filepath.Join(t.TempDir(), "u16.txt")
	os.WriteFile(f, []byte{0xFF, 0xFE, 'Ç', 0, 'o', 0, 'k', 0}, 0o644)
	mustRun(t, v, "append", "stack#Frontend", "--file", f)
	if !strings.Contains(readNote(t, v, "knowledge/stack.md"), "Çok") {
		t.Fatal("utf-16 --file not decoded")
	}
	// new + section add + link
	out := mustRun(t, v, "new", "decision", "Use Redis streams")
	if !strings.Contains(out, "ok episodes/decisions/0008-use-redis-streams") {
		t.Fatalf("decision numbering: %s", out)
	}
	mustRun(t, v, "section", "add", "0008-use-redis-streams", "Risks", "Queue may fill.")
	mustRun(t, v, "link", "use-redis-streams", "sqlite-database-locked", "--kind", "relates")
	if out := mustRun(t, v, "backlinks", "sqlite-database-locked"); !strings.Contains(out, "[relates]") {
		t.Fatal(out)
	}
}

func runIn(t *testing.T, vault, stdin string, args ...string) (string, string, int) {
	t.Helper()
	var out, errb strings.Builder
	code := Execute(append([]string{"--vault", vault}, args...), &out, &errb, strings.NewReader(stdin))
	return out.String(), errb.String(), code
}

func TestMoveRewritesLinks(t *testing.T) {
	v := copyVault(t)
	out := mustRun(t, v, "mv", "jwt-exp-timezone", "jwt-utc")
	if !strings.Contains(out, "links updated in 2 notes") {
		t.Fatal(out)
	}
	if s := readNote(t, v, "knowledge/rules.md"); !strings.Contains(s, "fixes:: [[jwt-utc]]") {
		t.Fatalf("rules not rewritten:\n%s", s)
	}
	if out := mustRun(t, v, "status"); !strings.Contains(out, "0 broken") {
		t.Fatalf("broken links after mv:\n%s", out)
	}
	mustRun(t, v, "archive", "jwt-utc")
	if out := mustRun(t, v, "read", "jwt-utc"); !strings.Contains(out, "archive/episodes/errors/jwt-utc") {
		t.Fatal(out)
	}
	if _, _, code := run(t, v, "rm", "old-deploy"); code != ExitUsage {
		t.Fatal("rm without --hard must refuse")
	}
	mustRun(t, v, "rm", "old-deploy", "--hard")
}

func TestBranchMergeAndMarks(t *testing.T) {
	v := copyVault(t)
	mustRun(t, v, "branch", "new", "queue", "--goal", "Move webhooks to a queue")
	mustRun(t, v, "session", "start", "Queue spike")
	tid := ids(mustRun(t, v, "session", "plan", "Consumer"))[0]
	mustRun(t, v, "append", "queue#Decisions", "- Use Redis Streams")
	out := mustRun(t, v, "branch", "merge", "queue", "--into", "main")
	for _, want := range []string{"1 tasks -> work/branches/main", "decision episodes/decisions/0008-queue", "notes archived"} {
		if !strings.Contains(out, want) {
			t.Errorf("merge lacks %q: %s", want, out)
		}
	}
	if out := mustRun(t, v, "read", tid); !strings.Contains(out, "work/branches/main^"+tid) {
		t.Fatalf("task not moved to main: %s", out)
	}

	mustRun(t, v, "mark", "architecture#Dağıtım", "--as", "stale", "--note", "moved to fly")
	mustRun(t, v, "mark", "rules^r004", "--as", "pin")
	out = mustRun(t, v, "marks", "--as", "stale,pin")
	if !strings.Contains(out, "[stale]  (moved to fly)  ## Dağıtım") || !strings.Contains(out, "knowledge/rules^r004  [pin]") {
		t.Fatal(out)
	}
	// the heading keeps its address with the mark on it
	if out := mustRun(t, v, "read", "architecture#Dağıtım"); !strings.Contains(out, "Fly.io") {
		t.Fatal(out)
	}
	mustRun(t, v, "unmark", "architecture#Dağıtım")
	if _, _, code := run(t, v, "marks", "--as", "stale"); code != ExitNoResults {
		t.Fatal("unmark left the mark")
	}
	mustRun(t, v, "callout", "stack:12", "--kind", "warning")
	if !strings.Contains(readNote(t, v, "knowledge/stack.md"), "> [!warning] Go 1.24") {
		t.Fatal(readNote(t, v, "knowledge/stack.md"))
	}
}

func TestTaskNextAndClaim(t *testing.T) {
	v := copyVault(t)
	mustRun(t, v, "session", "switch", "auth-refactor")
	out := mustRun(t, v, "task", "next")
	// t002 is in progress, its open child t004 is what to do now
	if !strings.Contains(out, "^t004") {
		t.Fatalf("task next: %s", out)
	}
	mustRun(t, v, "task", "claim", "t006")
	if !strings.Contains(readNote(t, v, "work/sessions/auth-refactor.md"), "@by(auth-refactor)") {
		t.Fatal("claim not written")
	}
	mustRun(t, v, "task", "move", "t006", "work/inbox")
	if s := readNote(t, v, "work/inbox.md"); !strings.Contains(s, "Eski cookie desteğini kaldır ^t006") {
		t.Fatalf("move: %s", s)
	}
	if out := mustRun(t, v, "task", "list", "--state", "question"); !strings.Contains(out, "^t007  [?]") {
		t.Fatal(out)
	}
}

func TestDryRunWritesNothing(t *testing.T) {
	v := copyVault(t)
	before := readNote(t, v, "knowledge/stack.md")
	out := mustRun(t, v, "append", "stack", "new line", "--dry-run")
	if !strings.Contains(out, "would change knowledge/stack") || !strings.Contains(out, "+ new line") || strings.Contains(out, "ok ") {
		t.Fatal(out)
	}
	if readNote(t, v, "knowledge/stack.md") != before {
		t.Fatal("dry-run wrote the file")
	}
}

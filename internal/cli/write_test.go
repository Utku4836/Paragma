package cli

import (
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"sync"
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
	b, err := os.ReadFile(filepath.Join(v, "Spaces", "main", filepath.FromSlash(rel)))
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

func TestProjectFlow(t *testing.T) {
	v := copyVault(t)
	t.Setenv("PARAGMA_PROJECT", "")
	out := mustRun(t, v, "start", "Payment flow", "--goal", "Card payments via Stripe",
		"--step", "Webhook endpoint", "--step", "Signature check", "--step", "Tests", "--decision", "Discount before VAT (user)")
	if !strings.Contains(out, "ok payment-flow/payment-flow  (current project here)") {
		t.Fatal(out)
	}
	plan := ids(out)
	if len(plan) != 3 {
		t.Fatalf("plan ids: %v\n%s", plan, out)
	}
	// starting it again continues it, changing nothing when nothing is given
	if out := mustRun(t, v, "start", "payment-flow"); !strings.Contains(out, "payment-flow continued") || !strings.Contains(out, "tasks 0/3") {
		t.Fatalf("second start:\n%s", out)
	}
	sub := ids(mustRun(t, v, "task", "add", "Retry logic", "--under", plan[0]))[0]
	mustRun(t, v, "task", "note", sub, "use exponential backoff")

	// done refuses while a subtask is open; done prints the next task in full
	if _, _, code := run(t, v, "done", plan[0]); code != ExitValidation {
		t.Fatalf("done with open subtask: exit %d", code)
	}
	mustRun(t, v, "task", "start", plan[0])
	mustRun(t, v, "done", sub)
	out = mustRun(t, v, "done", plan[0])
	if !strings.Contains(out, "next: payment-flow/payment-flow^"+plan[1]+" [ ] Signature check") {
		t.Fatalf("next after done:\n%s", out)
	}
	mustRun(t, v, "task", "block", plan[1], "--by", "no test keys")
	mustRun(t, v, "log", "- webhook needs the raw body")
	mustRun(t, v, "decide", "Retries stop after 5 attempts")
	mustRun(t, v, "checkpoint", "Webhook done. Next: tests.")
	mustRun(t, v, "new", "note", "Webhook findings", "--dir", "webhook")

	out = mustRun(t, v, "resume")
	for _, want := range []string{"project: payment-flow", "goal: Card payments via Stripe", "tasks 2/4",
		"Discount before VAT (user)", "Retries stop after 5 attempts", "^" + plan[1] + " [!] Signature check",
		"handoff: Checkpoint", "Next: tests.", "webhook needs the raw body", "webhook/webhook-findings", "next: ^" + plan[2]} {
		if !strings.Contains(out, want) {
			t.Errorf("resume lacks %q:\n%s", want, out)
		}
	}
	body := readNote(t, v, "payment-flow/payment-flow.md")
	for _, want := range []string{"type: project", "- [x] Webhook endpoint ^" + plan[0], "    - [x] Retry logic ^" + sub, "@blocked(no test keys)", "## Log\n\n- ", "## Decisions\n- "} {
		if !strings.Contains(body, want) {
			t.Errorf("project note lacks %q:\n%s", want, body)
		}
	}

	// another project, then back
	out = mustRun(t, v, "resume", "auth-refactor")
	if !strings.Contains(out, "project: auth-refactor") {
		t.Fatal(out)
	}
	if out := mustRun(t, v, "log", "back on auth"); !strings.Contains(out, "auth-refactor/auth-refactor#") {
		t.Fatalf("log should go to the current project: %s", out)
	}
	mustRun(t, v, "resume", "payment-flow")
	out = mustRun(t, v, "project", "list")
	if !strings.Contains(out, "* payment-flow") || !strings.Contains(out, "auth-refactor") {
		t.Fatal(out)
	}
	out = mustRun(t, v, "project", "close", "--summary", "webhook shipped")
	if !strings.Contains(out, "[closed]  2 decision records") || !strings.Contains(out, "payment-flow/journal/") {
		t.Fatal(out)
	}
	if j := mustRun(t, v, "search", "webhook", "shipped"); !strings.Contains(j, "payment-flow/journal") {
		t.Fatalf("journal entry not searchable:\n%s", j)
	}
	if d := mustRun(t, v, "find", "--type", "decision", "--title", "discount"); !strings.Contains(d, "payment-flow/decisions/0001-discount-before-vat") {
		t.Fatalf("decision record from the project: %s", d)
	}
}

// pg start on a project that exists continues it and adds what it brings;
// a name that swallowed the goal (a shell split the quotes) is refused.
func TestStartContinues(t *testing.T) {
	v := copyVault(t)
	t.Setenv("PARAGMA_PROJECT", "")
	first := ids(mustRun(t, v, "start", "billing", "--goal", "Invoices", "--step", "Model"))
	out := mustRun(t, v, "start", "Billing", "--goal", "Invoices and credit notes", "--step", "PDF export", "--decision", "Amounts in kurus (user)")
	for _, want := range []string{"ok billing continued", "2 added", "project: billing", "Amounts in kurus (user)", "PDF export", "^" + first[0]} {
		if !strings.Contains(out, want) {
			t.Errorf("continued start lacks %q:\n%s", want, out)
		}
	}
	body := readNote(t, v, "billing/billing.md")
	for _, want := range []string{"- [ ] Model ^" + first[0], "- [ ] PDF export ^", "Amounts in kurus (user)", "goal now: Invoices and credit notes", "## Goal\nInvoices\n"} {
		if !strings.Contains(body, want) {
			t.Errorf("billing.md lacks %q:\n%s", want, body)
		}
	}
	// by title: the sample project "Auth refactor"
	if out := mustRun(t, v, "start", "Auth refactor"); !strings.Contains(out, "auth-refactor continued") {
		t.Fatalf("start by title:\n%s", out)
	}
	if _, errs, code := run(t, v, "start", "billing", "Add", "refunds", "with", "a", "30", "day", "window"); code != ExitUsage || !strings.Contains(errs, "--goal") {
		t.Fatalf("long name: %d %s", code, errs)
	}
	// task ids as shells and habits write them
	mustRun(t, v, "resume", "billing")
	plan := ids(mustRun(t, v, "plan", "a", "b", "c"))
	for i, id := range []string{"%" + plan[0] + "%", "#" + plan[1], `"` + plan[2] + `"`} {
		if out := mustRun(t, v, "done", id); !strings.Contains(out, "^"+plan[i]+" [x]") {
			t.Errorf("done %s:\n%s", id, out)
		}
	}
}

func TestSpaces(t *testing.T) {
	v := copyVault(t)
	out := mustRun(t, v, "space", "list")
	if !strings.Contains(out, "* main  2 projects: auth-refactor, demo") {
		t.Fatal(out)
	}
	mustRun(t, v, "space", "new", "work")
	if _, _, code := run(t, v, "space", "new", "work"); code != ExitConflict {
		t.Fatal("space new twice")
	}
	mustRun(t, v, "space", "use", "work")
	// this directory now uses the empty space: nothing from main is visible
	if _, _, code := run(t, v, "search", "database"); code != ExitNoResults {
		t.Fatal("search crossed spaces")
	}
	if out := mustRun(t, v, "--space", "main", "search", "database"); !strings.Contains(out, "demo/errors/sqlite-database-locked") {
		t.Fatal(out)
	}
	if _, errs, code := run(t, v, "--space", "nope", "status"); code != ExitNoSpace || !strings.Contains(errs, "pg space new nope") {
		t.Fatalf("unknown space: %d %s", code, errs)
	}
}
func TestWriteCommands(t *testing.T) {
	v := copyVault(t)
	t.Chdir(v)
	mustRun(t, v, "append", "rules#Veritabanı", "> [!rule] Migrations run only via cmd/migrate. ^r007")
	if out := mustRun(t, v, "read", "rules^r007"); !strings.Contains(out, "cmd/migrate") {
		t.Fatal(out)
	}
	// write a heading body keeps the heading and the sub-headings
	mustRun(t, v, "write", "architecture#Katmanlar", "Two layers now.")
	arch := readNote(t, v, "demo/architecture.md")
	if !strings.Contains(arch, "## Katmanlar\n\nTwo layers now.\n\n### API") {
		t.Fatalf("write section:\n%s", arch)
	}
	// write a block keeps its id
	mustRun(t, v, "write", "auth-refactor^t004", "    - [ ] Tests incl. timezone")
	if s := readNote(t, v, "auth-refactor/auth-refactor.md"); !strings.Contains(s, "    - [ ] Tests incl. timezone ^t004") {
		t.Fatalf("block id lost:\n%s", s)
	}
	// stale hash is refused
	if _, _, code := run(t, v, "append", "stack", "x", "--if-hash", "deadbeef"); code != ExitConflict {
		t.Fatalf("if-hash: exit %d", code)
	}
	// dash-led markdown text is text, not a flag
	mustRun(t, v, "append", "stack#CI", "- lint runs on every push")
	if s := readNote(t, v, "demo/stack.md"); !strings.Contains(s, "golangci-lint run`.\n\n- lint runs on every push") {
		t.Fatalf("dash text:\n%s", s)
	}
	// edit: ambiguous without --all
	if _, _, code := run(t, v, "edit", "architecture", "--find", "middleware", "--replace", "mw"); code != ExitValidation {
		t.Fatalf("ambiguous edit: exit %d", code)
	}
	mustRun(t, v, "edit", "stack", "--find", "Go 1.24", "--replace", "Go 1.25")
	// stdin text
	_, _, code := runIn(t, v, "line one\nline two\n", "insert", "rules^r002", "--after", "-")
	if code != 0 || !strings.Contains(readNote(t, v, "demo/rules.md"), "^r002\n\nline one\nline two") {
		t.Fatalf("insert from stdin:\n%s", readNote(t, v, "demo/rules.md"))
	}
	// UTF-16 file input (PowerShell >)
	f := filepath.Join(t.TempDir(), "u16.txt")
	os.WriteFile(f, []byte{0xFF, 0xFE, 'Ç', 0, 'o', 0, 'k', 0}, 0o644)
	mustRun(t, v, "append", "stack#Frontend", "--file", f)
	if !strings.Contains(readNote(t, v, "demo/stack.md"), "Çok") {
		t.Fatal("utf-16 --file not decoded")
	}
	// new + section add + link
	out := mustRun(t, v, "new", "decision", "Use Redis streams")
	if !strings.Contains(out, "ok demo/decisions/0008-use-redis-streams") {
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
	code := Execute(append([]string{"--home", vault}, args...), &out, &errb, strings.NewReader(stdin))
	return out.String(), errb.String(), code
}

func TestMoveRewritesLinks(t *testing.T) {
	v := copyVault(t)
	out := mustRun(t, v, "mv", "jwt-exp-timezone", "jwt-utc")
	if !strings.Contains(out, "links updated in 2 notes") {
		t.Fatal(out)
	}
	if s := readNote(t, v, "demo/rules.md"); !strings.Contains(s, "fixes:: [[jwt-utc]]") {
		t.Fatalf("rules not rewritten:\n%s", s)
	}
	if out := mustRun(t, v, "status"); !strings.Contains(out, "0 broken") {
		t.Fatalf("broken links after mv:\n%s", out)
	}
	mustRun(t, v, "archive", "jwt-utc")
	if out := mustRun(t, v, "read", "jwt-utc"); !strings.Contains(out, "demo/archive/errors/jwt-utc") {
		t.Fatal(out)
	}
	if _, _, code := run(t, v, "rm", "old-deploy"); code != ExitUsage {
		t.Fatal("rm without --hard must refuse")
	}
	mustRun(t, v, "rm", "old-deploy", "--hard")
}

func TestMarks(t *testing.T) {
	v := copyVault(t)
	var out string
	mustRun(t, v, "mark", "architecture#Dağıtım", "--as", "stale", "--note", "moved to fly")
	mustRun(t, v, "mark", "rules^r004", "--as", "pin")
	out = mustRun(t, v, "marks", "--as", "stale,pin")
	if !strings.Contains(out, "[stale]  (moved to fly)  ## Dağıtım") || !strings.Contains(out, "demo/rules^r004  [pin]") {
		t.Fatal(out)
	}
	// the heading keeps its address with the mark on it
	if out := mustRun(t, v, "read", "architecture#Dağıtım"); !strings.Contains(out, "Fly.io") {
		t.Fatal(out)
	}
	mustRun(t, v, "mark", "architecture#Dağıtım", "--remove")
	if _, _, code := run(t, v, "marks", "--as", "stale"); code != ExitNoResults {
		t.Fatal("--remove left the mark")
	}
	mustRun(t, v, "mark", "stack:12", "--callout", "warning")
	mustRun(t, v, "mark", "stack:12", "--highlight", "1.24")
	if s := readNote(t, v, "demo/stack.md"); !strings.Contains(s, "> [!warning] Go ==1.24==") {
		t.Fatal(s)
	}
	if _, errs, code := run(t, v, "mark", "stack:12", "--as", "pin", "--remove"); code != ExitUsage || !strings.Contains(errs, "exactly one") {
		t.Fatalf("two actions at once: %d %s", code, errs)
	}
}

func TestTaskNextAndClaim(t *testing.T) {
	v := copyVault(t)
	t.Setenv("PARAGMA_PROJECT", "auth-refactor")
	out := mustRun(t, v, "task", "next")
	// t002 is in progress, its open child t004 is what to do now
	if !strings.Contains(out, "^t004") {
		t.Fatalf("task next: %s", out)
	}
	t.Setenv("PARAGMA_AGENT", "agent-a")
	mustRun(t, v, "task", "claim", "t006")
	if !strings.Contains(readNote(t, v, "auth-refactor/auth-refactor.md"), "@by(agent-a)") {
		t.Fatal("claim not written")
	}
	mustRun(t, v, "task", "move", "t006", "demo/inbox")
	if s := readNote(t, v, "demo/inbox.md"); !strings.Contains(s, "Eski cookie desteğini kaldır ^t006") {
		t.Fatalf("move: %s", s)
	}
	if out := mustRun(t, v, "task", "list", "--state", "question"); !strings.Contains(out, "^t007  [?]") {
		t.Fatal(out)
	}
}

func TestDryRunWritesNothing(t *testing.T) {
	v := copyVault(t)
	before := readNote(t, v, "demo/stack.md")
	out := mustRun(t, v, "append", "stack", "new line", "--dry-run")
	if !strings.Contains(out, "would change demo/stack") || !strings.Contains(out, "+ new line") || strings.Contains(out, "ok ") {
		t.Fatal(out)
	}
	if readNote(t, v, "demo/stack.md") != before {
		t.Fatal("dry-run wrote the file")
	}
}

// One pg mv for notes, folders and projects: rename in place, move into a
// folder, move out of one, rename a project; links keep finding their notes.
func TestMoveFoldersAndProjects(t *testing.T) {
	v := copyVault(t)
	t.Setenv("PARAGMA_PROJECT", "")
	mustRun(t, v, "start", "billing", "--step", "Invoices")
	mustRun(t, v, "new", "note", "Raw body", "--dir", "webhook")
	mustRun(t, v, "new", "note", "Index", "--dir", "a")
	mustRun(t, v, "append", "billing/a/index", "see [[billing/webhook/raw-body]] and [[raw-body#Raw body]]")

	// a folder of the current project, renamed in place (and by its other name)
	if out := mustRun(t, v, "rename", "webhook", "hooks"); !strings.Contains(out, "ok billing/hooks (1 notes)") {
		t.Fatalf("rename folder:\n%s", out)
	}
	idx := readNote(t, v, "billing/a/index.md")
	if !strings.Contains(idx, "[[raw-body]]") || strings.Contains(idx, "billing/webhook/") || !strings.Contains(idx, "[[raw-body#Raw body]]") {
		t.Fatalf("links after folder rename:\n%s", idx)
	}
	// out of a folder, into the project's root; then into a new folder by a path
	mustRun(t, v, "mv", "billing/hooks/raw-body", "billing/")
	readNote(t, v, "billing/raw-body.md")
	mustRun(t, v, "mv", "billing/raw-body", "notes/raw-body")
	readNote(t, v, "billing/notes/raw-body.md")
	// a folder into another folder
	if out := mustRun(t, v, "mv", "billing/a", "billing/notes/"); !strings.Contains(out, "billing/notes/a") {
		t.Fatalf("folder into folder:\n%s", out)
	}
	readNote(t, v, "billing/notes/a/index.md")
	// the project itself
	if out := mustRun(t, v, "mv", "billing", "invoicing"); !strings.Contains(out, "ok invoicing") {
		t.Fatalf("rename project:\n%s", out)
	}
	body := readNote(t, v, "invoicing/invoicing.md")
	if !strings.Contains(body, "title: Invoicing") || !strings.Contains(body, "# Invoicing") || !strings.Contains(body, "Invoices ^") {
		t.Fatalf("project note after rename:\n%s", body)
	}
	if _, err := os.Stat(filepath.Join(v, "Spaces", "main", "billing")); err == nil {
		t.Fatal("old project folder left behind")
	}
	if out := mustRun(t, v, "log", "after the rename"); !strings.Contains(out, "invoicing/invoicing#") {
		t.Fatalf("the renamed project is not current:\n%s", out)
	}
	if out := mustRun(t, v, "read", "invoicing/notes/a/index"); !strings.Contains(out, "[[raw-body]]") {
		t.Fatalf("index after all moves:\n%s", out)
	}
	if _, _, code := run(t, v, "mv", "invoicing", "auth-refactor"); code != ExitConflict {
		t.Fatalf("rename onto an existing project: exit %d", code)
	}
}

// pg start with the name of a note inside a project continues that project.
func TestStartFindsNote(t *testing.T) {
	v := copyVault(t)
	t.Setenv("PARAGMA_PROJECT", "")
	mustRun(t, v, "start", "billing")
	mustRun(t, v, "new", "note", "Webhook findings", "--dir", "webhook")
	mustRun(t, v, "resume", "auth-refactor")
	out := mustRun(t, v, "start", "Webhook findings", "--decision", "Verify signatures first")
	if !strings.Contains(out, "billing continued: a note of that name is in this project (pg read billing/webhook/webhook-findings)") || !strings.Contains(out, "Verify signatures first") {
		t.Fatalf("start by note name:\n%s", out)
	}
	if _, err := os.Stat(filepath.Join(v, "Spaces", "main", "webhook-findings")); err == nil {
		t.Fatal("a second project was made")
	}
}

// Writes to one note at the same time (agents run tools in parallel): each
// lands, none is lost, and a task marked done is the task that was meant.
func TestParallelWrites(t *testing.T) {
	v := copyVault(t)
	t.Setenv("PARAGMA_PROJECT", "")
	plan := ids(mustRun(t, v, "start", "busy", "--step", "one", "--step", "two", "--step", "three"))
	t.Setenv("PARAGMA_PROJECT", "busy")
	var wg sync.WaitGroup
	var mu sync.Mutex
	var failed []string
	call := func(args ...string) {
		defer wg.Done()
		if out, errs, code := run(t, v, args...); code != 0 {
			mu.Lock()
			failed = append(failed, fmt.Sprintf("%v: %d %s %s", args, code, out, errs))
			mu.Unlock()
		}
	}
	for i := range 8 {
		wg.Add(1)
		go call("log", fmt.Sprintf("parallel line %d", i))
	}
	wg.Add(4)
	go call("plan", "four")
	go call("decide", "parallel decision")
	go call("done", plan[0])
	go call("done", plan[2])
	wg.Wait()
	for _, f := range failed {
		t.Error(f)
	}
	body := readNote(t, v, "busy/busy.md")
	for i := range 8 {
		if !strings.Contains(body, fmt.Sprintf("parallel line %d", i)) {
			t.Errorf("log line %d lost", i)
		}
	}
	for _, want := range []string{"- [x] one ^" + plan[0], "- [ ] two ^" + plan[1], "- [x] three ^" + plan[2], "- [ ] four ^", "parallel decision"} {
		if !strings.Contains(body, want) {
			t.Errorf("busy.md lacks %q:\n%s", want, body)
		}
	}
}

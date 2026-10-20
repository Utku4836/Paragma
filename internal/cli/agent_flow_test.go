package cli

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/Utku4836/Paragma/internal/textutil"
)

func TestSkills(t *testing.T) {
	v := copyVault(t)
	out := mustRun(t, v, "skill")
	for _, want := range []string{"pg finish", "pg skill coding", "pg skill projects", "pg skill knowledge"} {
		if !strings.Contains(out, want) {
			t.Errorf("pg skill lacks %q:\n%s", want, out)
		}
	}
	out = mustRun(t, v, "skill", "coding")
	if strings.HasPrefix(out, "---") || !strings.Contains(out, "# ") || strings.Contains(out, "# Reference") {
		t.Fatalf("pg skill coding:\n%s", out)
	}
	if out := mustRun(t, v, "skill", "paragma-coding", "--ref"); !strings.Contains(out, "## pg run") || !strings.Contains(out, "## pg related") {
		t.Fatalf("--ref lacks the commands:\n%s", out)
	}
	if _, _, code := run(t, v, "skill", "cooking"); code != ExitUsage {
		t.Fatalf("unknown skill: exit %d", code)
	}
	for _, name := range []string{"coding", "projects", "knowledge"} {
		dir := filepath.Join(v, "skills", "paragma-"+name)
		b, err := os.ReadFile(filepath.Join(dir, "SKILL.md"))
		if err != nil || !strings.HasPrefix(string(b), "---\nname: paragma-"+name+"\n") {
			t.Errorf("%s/SKILL.md: %v %.60s", name, err, b)
		}
		if b, err := os.ReadFile(filepath.Join(dir, "REFERENCE.md")); err != nil || !strings.Contains(string(b), "usage: pg ") {
			t.Errorf("%s/REFERENCE.md: %v", name, err)
		}
		if n := textutil.Tokens(mustSkill(name)); n > 700 {
			t.Errorf("skill %s is %d tokens", name, n)
		}
	}
}

func TestBriefAndFinish(t *testing.T) {
	v := copyVault(t)
	t.Setenv("PARAGMA_PROJECT", "")
	// continuing with a new goal: the brief looks for that goal
	out := mustRun(t, v, "start", "auth-refactor", "--goal", "fix the jwt expiry timezone")
	for _, want := range []string{"## memory about this task", "jwt-exp-timezone", "pg finish"} {
		if !strings.Contains(out, want) {
			t.Errorf("brief lacks %q:\n%s", want, out)
		}
	}
	// a finding logged while closing a step, then the handoff
	if out := mustRun(t, v, "done", "t004", "--log", "tests pin the clock with clock.Fixed"); !strings.Contains(out, "^t004 [x]") {
		t.Fatalf("done --log:\n%s", out)
	}
	out = mustRun(t, v, "finish", "Refresh tests green; next: prod migration", "--done", "t002")
	for _, want := range []string{"^t005", "#Handoff", "pg resume auth-refactor"} {
		if !strings.Contains(out, want) {
			t.Errorf("finish lacks %q:\n%s", want, out)
		}
	}
	body := readNote(t, v, "auth-refactor/auth-refactor.md")
	for _, want := range []string{"Refresh tests green; next: prod migration", "tests pin the clock with clock.Fixed", "- [x] Refresh endpoint"} {
		if !strings.Contains(body, want) {
			t.Errorf("project note lacks %q:\n%s", want, body)
		}
	}
}

func TestRunHistory(t *testing.T) {
	v := copyVault(t)
	fail := failing("--- FAIL: TestRefund (0.01s)", "1")
	if out, _, code := run(t, v, "run", "--", fail); code != 1 || !strings.Contains(out, "TestRefund") {
		t.Fatalf("first failure: %d\n%s", code, out)
	}
	if out, _, _ := run(t, v, "run", "--", fail); !strings.Contains(out, "failing in the last run too") {
		t.Fatalf("second failure:\n%s", out)
	}
	if _, err := os.Stat(filepath.Join(v, "Spaces", "main", ".pg", "test-runs.json")); err != nil {
		t.Fatal(err)
	}
}

func TestStartRepeatsAndSimilarNames(t *testing.T) {
	v := copyVault(t)
	t.Setenv("PARAGMA_PROJECT", "")
	mustRun(t, v, "start", "cart-discounts", "--goal", "Discounts", "--step", "Percent and Fixed", "--decision", "At most 50%")
	// the same call again adds nothing
	if out := mustRun(t, v, "start", "cart-discounts", "--step", "Percent and Fixed", "--decision", "At most 50%"); strings.Contains(out, "added") {
		t.Fatalf("repeat added again:\n%s", out)
	}
	// a shorter name of the same work continues it
	out := mustRun(t, v, "start", "discount", "--step", "Cart field")
	if !strings.Contains(out, `cart-discounts continued: its name matches "discount"`) || !strings.Contains(out, "1 added") {
		t.Fatalf("similar name:\n%s", out)
	}
	body := readNote(t, v, "cart-discounts/cart-discounts.md")
	if strings.Count(body, "Percent and Fixed") != 1 || strings.Count(body, "At most 50%") != 1 || !strings.Contains(body, "Cart field") {
		t.Fatalf("cart-discounts.md:\n%s", body)
	}
	// the work directory's name means this directory's project
	wd, _ := os.Getwd()
	if out := mustRun(t, v, "start", filepath.Base(wd), "--step", "README"); !strings.Contains(out, "cart-discounts continued") {
		t.Fatalf("work directory name:\n%s", out)
	}
	// new work in a directory that was on another project says so
	if out := mustRun(t, v, "start", "billing", "--goal", "Invoices"); !strings.Contains(out, "this directory was on project cart-discounts") {
		t.Fatalf("new project:\n%s", out)
	}
}

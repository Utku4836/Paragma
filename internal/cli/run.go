package cli

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os/exec"
	"runtime"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/Utku4836/Paragma/internal/errsig"
	"github.com/Utku4836/Paragma/internal/index"
	"github.com/Utku4836/Paragma/internal/md"
	"github.com/Utku4836/Paragma/internal/space"
	"github.com/Utku4836/Paragma/internal/textutil"
	"github.com/spf13/cobra"
)

// pg run: run a command and, when it fails, print the known fix.

// tailBuffer keeps the last max bytes written to it.
type tailBuffer struct {
	mu  sync.Mutex
	buf []byte
	max int
}

func (t *tailBuffer) Write(p []byte) (int, error) {
	t.mu.Lock()
	defer t.mu.Unlock()
	t.buf = append(t.buf, p...)
	if len(t.buf) > t.max {
		t.buf = t.buf[len(t.buf)-t.max:]
	}
	return len(p), nil
}

func (t *tailBuffer) String() string {
	t.mu.Lock()
	defer t.mu.Unlock()
	return string(t.buf)
}

func (a *app) runCmd() *cobra.Command {
	var full, affected bool
	c := &cobra.Command{
		Use:   "run [--full] [--affected] -- <command> [args...]",
		Short: "Run tests or a build: short output, the known fix on failure, new vs old failures",
		Long: `Run a command and keep its exit code. Long output is shortened: on
success its start and end, on failure every error line with context and the
end (--full prints everything as it comes). On failure pg prints the
recorded fix when memory knows the error, and which failing tests are new
since the last run of the same command, were failing before, or are known
to be flaky (they failed and passed on the same code). When a failure is
fixed, pg lists the files changed since, ready for pg error record
--from-last. --affected runs a Go test command only for the packages with
changed files (./... becomes ./payments ./cart).
A single quoted argument is run through the shell (cmd /C on Windows,
sh -c elsewhere), so pipes work there.`,
		Example: `pg run -- go test ./...
pg run --affected -- go test ./...
pg run -- npm test
pg run --full -- "make build 2>&1"`,
		Args: cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			a.stdout.Flush()
			if affected {
				args = a.affectedArgs(args)
			}
			var c *exec.Cmd
			if len(args) == 1 && strings.ContainsAny(args[0], " \t|&<>") {
				if runtime.GOOS == "windows" {
					c = exec.Command("cmd", "/C", args[0])
				} else {
					c = exec.Command("sh", "-c", args[0])
				}
			} else {
				c = exec.Command(args[0], args[1:]...)
			}
			out := &tailBuffer{max: 4 << 20}
			c.Stdin = a.stdin
			if full {
				c.Stdout = io.MultiWriter(a.rawOut, out)
				c.Stderr = io.MultiWriter(a.rawErr, out)
			} else {
				c.Stdout, c.Stderr = out, out
			}
			err := c.Run()
			code := 0
			if err != nil {
				var ee *exec.ExitError
				if errors.As(err, &ee) {
					code = ee.ExitCode()
				} else {
					return fail(ExitUsage, "cannot run %s: %v", args[0], err)
				}
			}
			output := out.String()
			if !full {
				text, cut := summarise(output, code != 0)
				if text != "" {
					a.println(text)
				}
				if cut {
					a.printf("(output shortened; all of it: pg run --full -- %s)\n", strings.Join(args, " "))
				}
			}
			command := strings.Join(args, " ")
			a.recordRun(command, code, output)
			if code == 0 {
				a.afterSuccess()
				return nil
			}
			a.afterFailure(command, code, output)
			return &Error{Code: code}
		},
	}
	c.Flags().BoolVar(&full, "full", false, "print the whole output as it comes")
	c.Flags().BoolVar(&affected, "affected", false, "Go: test only the packages with changed files")
	return c
}

// recordRun keeps the run in the test history and says what it means
// next to earlier runs: new, old and flaky failures, or what fixed one.
func (a *app) recordRun(command string, code int, output string) {
	s, err := a.main()
	if err != nil || a.dryRun {
		return
	}
	head, files := a.worktreeState()
	run := testRun{Time: time.Now().Format(time.RFC3339), Exit: code, Failing: failingTests(output), Head: head, Files: files}
	h := loadHistory(s)
	prev := h.last(command)
	if code != 0 && len(run.Failing) > 0 {
		var parts []string
		for _, t := range run.Failing[:min(len(run.Failing), 12)] {
			switch {
			case slices.Contains(h.Flaky[command], t):
				parts = append(parts, t+" (known flaky: it failed and passed on the same code)")
			case prev != nil && slices.Contains(prev.Failing, t):
				parts = append(parts, t+" (failing in the last run too)")
			case prev != nil:
				parts = append(parts, t+" (new since the last run)")
			default:
				parts = append(parts, t)
			}
		}
		a.println("--- paragma: failing: " + strings.Join(parts, "; "))
	}
	if code == 0 && prev != nil && prev.Exit != 0 {
		if changed := changedBetween(prev.Files, files); len(changed) > 0 {
			a.printf("--- paragma: fixed since the failure at %s; changed since then: %s\n", clockOf(prev.Time), strings.Join(changed[:min(len(changed), 8)], ", "))
			if lr, err := loadLastRun(s); err == nil && lr.Cmd == command {
				lr.Fixed = changed
				if b, err := json.MarshalIndent(lr, "", " "); err == nil {
					space.WriteFileAtomic(lastRunPath(s), b)
				}
				a.println(`if the cause was not obvious: pg error record --from-last --cause "..." --fix "..." (the files are filled in)`)
			}
		}
	}
	h.add(command, run)
	h.save(s)
}

// afterFailure saves the failed run and prints the known fix, if any. It
// never fails the command for reasons of its own: a broken index or a
// timeout mean "print nothing".
func (a *app) afterFailure(command string, code int, output string) {
	s, err := a.main()
	if err != nil {
		return
	}
	lines := errsig.Lines(output, 30)
	var raw []string
	for _, l := range lines {
		raw = append(raw, l.Raw)
	}
	out := output
	if len(out) > 8000 {
		out = out[len(out)-8000:]
	}
	if b, err := json.MarshalIndent(lastRun{Cmd: command, Exit: code, Time: stamp(), Lines: raw, Output: out}, "", " "); err == nil {
		space.WriteFileAtomic(lastRunPath(s), b)
	}

	// the whole space is searched; the current project's records come first
	done := make(chan []errMatch, 1)
	go func() { done <- a.matchErrors(s, output, false) }()
	var ms []errMatch
	select {
	case ms = <-done:
	case <-time.After(2 * time.Second):
	}
	if len(ms) > 0 {
		m := ms[0]
		// the whole fix and its cause: a cut fix sends the agent to read the note
		addr, fix := a.fixText(s, m.f, 2000)
		from := ""
		if p := space.ProjectOf(m.f.Path); s.project != "" && p != s.project {
			from = " (from project " + p + ")"
		}
		a.printf("--- paragma: known error (%s match, seen %dx) %s%s\n", m.level, maxInt(m.f.Seen, 1), addr, from)
		if cause := a.sectionBody(s, m.f, "cause"); len(cause) > 0 {
			a.println("Cause: " + textutil.OneLine(strings.Join(cause, " "), 600))
		}
		if fix != "" {
			a.println("Fix: " + fix)
		}
		if files := a.sectionBody(s, m.f, "files"); len(files) > 0 {
			var fs []string
			for _, l := range files {
				fs = append(fs, strings.TrimSpace(strings.TrimPrefix(strings.TrimSpace(l), "-")))
			}
			a.println("Files: " + strings.Join(fs, ", "))
		}
		if len(ms) > 1 {
			var more []string
			for _, x := range ms[1:min(len(ms), 3)] {
				more = append(more, x.f.Addr())
			}
			a.println("also: " + strings.Join(more, ", "))
		}
		a.printf("if it works: pg error seen %s\n", m.f.Addr())
		a.printFileRules(s, raw)
		return
	}
	if s.v.Cfg.Hints && len(lines) > 0 {
		a.println(`--- paragma: not in memory. After fixing it: pg error record --from-last --cause "..." --fix "..."`)
	}
	a.printFileRules(s, raw)
}

// afterSuccess reminds of the rules and decisions about the files changed
// in the work tree (git status) that were not shown in the last hours: the
// moment to catch a rule before the work is called done. Silent on any
// problem and after 1.5 s.
func (a *app) afterSuccess() {
	s, err := a.main()
	if err != nil {
		return
	}
	done := make(chan []string, 1)
	go func() { done <- a.ruleReminders(s) }()
	select {
	case lines := <-done:
		if len(lines) > 0 {
			a.println("--- paragma: rules for the files you changed")
			for _, l := range lines {
				a.println(l)
			}
		}
	case <-time.After(1500 * time.Millisecond):
	}
	a.progressReminder(s)
}

// progressReminder follows green tests in a project with open steps: mark
// what is finished and leave a handoff before stopping. Once in a while,
// not after every run.
func (a *app) progressReminder(s *store) {
	f := projectNote(s, s.project)
	if f == nil {
		return
	}
	ts, _ := s.db.Tasks(f.ID)
	if index.CountTasks(ts, 1, 1<<30).Open == 0 {
		return
	}
	sh := loadShown(s)
	key := "progress:" + s.project
	if sh.recent(key, 20*time.Minute) {
		return
	}
	open := index.CountTasks(ts, 1, 1<<30).Open
	next := ""
	if nt, _ := a.nextTask(s, f); nt != nil {
		next = fmt.Sprintf(" (next: ^%s %s)", nt.ID, textutil.OneLine(nt.Text, 60))
	}
	a.printf("--- paragma: %s has %d open steps%s; finished one: pg done <id>; before your final answer: pg finish \"<summary>\"\n", s.project, open, next)
	sh.mark(key)
	a.saveShown(s, sh)
}

func (a *app) ruleReminders(s *store) []string {
	c := exec.Command("git", "status", "--porcelain", "--untracked-files=all")
	c.Dir = a.workDir()
	out, err := c.Output()
	if err != nil {
		return nil
	}
	var paths []string
	for _, l := range strings.Split(string(out), "\n") {
		if len(l) < 4 {
			continue
		}
		p := strings.TrimSpace(l[3:])
		if i := strings.Index(p, " -> "); i >= 0 {
			p = p[i+4:]
		}
		p = strings.Trim(p, `"`)
		if strings.HasSuffix(p, "/") || strings.HasPrefix(p, ".") {
			continue
		}
		paths = append(paths, md.NormRef(p))
		if len(paths) == 30 {
			break
		}
	}
	if len(paths) == 0 {
		return nil
	}
	sh := loadShown(s)
	var lines []string
	add := func(ad, why, text string) bool {
		if sh.recent("remind:"+ad, 6*time.Hour) || text == "" {
			return false
		}
		lines = append(lines, fmt.Sprintf("%s (%s): %s", ad, why, text))
		sh.mark("remind:" + ad)
		return len(lines) == 4
	}
	for _, h := range a.relatedNotes(s, paths, 1.5, false) {
		full := false
		switch h.f.Type {
		case "rule", "decision":
			full = add(h.addr, h.why, a.oneLineAt(s, h))
		default:
			// a section about the file that holds rule callouts
			for _, r := range a.rulesInSection(s, h.f, h.line) {
				if full = add(r.addr, h.why, a.oneLineAt(s, r)); full {
					break
				}
			}
		}
		if full {
			break
		}
	}
	if len(lines) > 0 {
		a.saveShown(s, sh)
	}
	return lines
}

// rulesInSection returns the rule callouts in the innermost section
// containing line.
func (a *app) rulesInSection(s *store, f *index.File, line int) []relHit {
	rows, err := s.db.Query(`SELECT COALESCE(m.block_id,''), m.line FROM marks m JOIN sections x ON x.id = m.section_id
		WHERE m.file_id = ? AND m.kind = 'rule' AND x.id = (SELECT id FROM sections WHERE file_id = ? AND line_start <= ? AND line_end >= ? ORDER BY line_end - line_start LIMIT 1)
		ORDER BY m.line`, f.ID, f.ID, line, line)
	if err != nil {
		return nil
	}
	defer rows.Close()
	var out []relHit
	for rows.Next() {
		var b string
		var l int
		rows.Scan(&b, &l)
		ad := fmt.Sprintf("%s:%d", f.Addr(), l)
		if b != "" {
			ad = f.Addr() + "^" + b
		}
		out = append(out, relHit{f: f, addr: ad, line: l})
	}
	return out
}

// oneLineAt is the text of a related hit's line (a rule callout or the
// first lines of a decision section), on one line.
func (a *app) oneLineAt(s *store, h relHit) string {
	lines, err := fileLines(s, h.f)
	if err != nil || h.line < 1 || h.line > len(lines) {
		return ""
	}
	t := strings.TrimSpace(strings.TrimLeft(lines[h.line-1], " >"))
	t = strings.TrimSpace(strings.TrimPrefix(t, "[!rule]"))
	if i := strings.LastIndex(t, " ^"); i >= 0 && !strings.Contains(t[i+2:], " ") {
		t = t[:i] // the block id
	}
	return textutil.OneLine(strings.TrimSpace(t), 220)
}

// printFileRules shows the rules and decisions about the source files named
// in failing output (at most two lines): what the code there must respect.
func (a *app) printFileRules(s *store, lines []string) {
	var paths []string
	seen := map[string]bool{}
	for _, l := range lines {
		for _, r := range md.CodeRefs(" " + l + " ") {
			if strings.HasPrefix(r, "pkg:") || strings.HasSuffix(r, "/") || seen[r] {
				continue
			}
			seen[r] = true
			paths = append(paths, a.codePath(r))
		}
	}
	if len(paths) == 0 {
		return
	}
	n := 0
	for _, h := range a.relatedNotes(s, paths, 2, false) {
		if h.f.Type != "rule" && h.f.Type != "decision" {
			continue
		}
		a.printf("rule for this file: %s (%s)\n", h.addr, h.why)
		if n++; n == 2 {
			break
		}
	}
}

func maxInt(a, b int) int {
	if a > b {
		return a
	}
	return b
}

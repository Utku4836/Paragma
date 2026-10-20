package cli

// pg finish: the end of a piece of work in one call. It closes the steps
// named with --done, shows the changed files with the rules and decisions
// that apply to them, says whether the tests ran after the last change,
// lists what is still open, and writes the handoff for the next
// conversation.

import (
	"fmt"
	"sort"
	"strings"
	"time"

	"github.com/Utku4836/Paragma/internal/addr"
	"github.com/Utku4836/Paragma/internal/index"
	"github.com/Utku4836/Paragma/internal/md"
	"github.com/Utku4836/Paragma/internal/textutil"
	"github.com/spf13/cobra"
)

func (a *app) finishCmd() *cobra.Command {
	var done []string
	c := &cobra.Command{
		Use:   "finish [summary|-] [--done <id>,...]",
		Short: "End of the work in one call: close steps, check changes against the rules, test status, handoff",
		Long: `Run it before your final answer. It closes the steps given with --done,
lists the files you changed with the rules and decisions about each, says
whether pg run ran the tests after your last change, lists the steps still
open, and writes the project's handoff (the summary, with the progress) for
whoever continues in another conversation.`,
		Example: `pg finish "Refund added with the 30-day window; README section still open" --done t3f2,t3f4`,
		Args:    cobra.ArbitraryArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			summary, err := a.inputText(args, false)
			if err != nil {
				return err
			}
			s, err := a.main()
			if err != nil {
				return err
			}
			if ids := splitList(done); len(ids) > 0 {
				if err := a.changeTasks(ids, markDone(false)); err != nil {
					return err
				}
			}
			if err := a.relatedChanges(s); err != nil {
				a.println("(not a git work tree: changed files unknown)")
			}
			a.printTestStatus(s)
			f := projectNote(s, s.project)
			if f == nil {
				a.println(`no current project: nothing to hand over (pg start <name> --goal "..." records work)`)
				return nil
			}
			ts, _ := s.db.Tasks(f.ID)
			var open []string
			for _, t := range ts {
				if !t.Closed() && t.ID != "" {
					open = append(open, fmt.Sprintf("^%s [%s] %s", t.ID, t.State, textutil.OneLine(t.Text, 80)))
				}
			}
			if len(open) > 0 {
				a.printf("still open in %s (close finished ones with pg done <id>):\n", s.project)
				for _, o := range open[:min(len(open), 8)] {
					a.println("  " + o)
				}
			}
			text := a.mechanicalHandoff(s, f)
			if summary != "" {
				text = summary + "\n" + text
			}
			if err := a.writeHandoff(s, f, text); err != nil {
				return err
			}
			a.printf("next conversation: pg resume %s\n", s.project)
			return nil
		},
	}
	c.Flags().StringSliceVar(&done, "done", nil, "steps to mark done first (ids, comma separated)")
	return c
}

// markDone is the change pg done makes to a task line.
func markDone(force bool) func(s *store, n *note, t *addr.Target, tl *md.TaskLine) error {
	return func(s *store, n *note, t *addr.Target, tl *md.TaskLine) error {
		if open := openChildren(n, t); len(open) > 0 && !force {
			return fail(ExitValidation, "%s has open subtasks: %s; finish them or add --force", t.Addr(), strings.Join(open, ", "))
		}
		tl.State = 'x'
		tl.Set("done", today())
		tl.Set("blocked", "")
		return nil
	}
}

// printTestStatus says how the last pg run went and whether files changed
// after it.
func (a *app) printTestStatus(s *store) {
	h := loadHistory(s)
	var cmd string
	var last *testRun
	for c := range h.Runs {
		if r := h.last(c); last == nil || r.Time > last.Time {
			cmd, last = c, r
		}
	}
	if last == nil {
		if tc := testCommand(a.workDir()); tc != "" {
			a.printf("tests: not run through pg yet: pg run -- %s\n", tc)
		}
		return
	}
	when := clockOf(last.Time)
	if t, err := time.Parse(time.RFC3339, last.Time); err == nil && time.Since(t) > 24*time.Hour {
		when = t.Local().Format("2006-01-02 15:04")
	}
	state := "passed"
	if last.Exit != 0 {
		state = fmt.Sprintf("FAILED (exit %d)", last.Exit)
		if len(last.Failing) > 0 {
			state += ": " + strings.Join(last.Failing[:min(len(last.Failing), 5)], ", ")
		}
	}
	line := fmt.Sprintf("tests: pg run -- %s %s at %s", cmd, state, when)
	if _, files := a.worktreeState(); files != nil {
		if changed := changedBetween(last.Files, files); len(changed) > 0 {
			sort.Strings(changed)
			line += fmt.Sprintf("; changed since: %s, run them again", strings.Join(changed[:min(len(changed), 5)], ", "))
		}
	}
	a.println(line)
}

// writeHandoff replaces the Handoff section of project note f.
func (a *app) writeHandoff(s *store, f *index.File, text string) error {
	return a.withLock(s, func() error {
		n, err := a.load(s, f)
		if err != nil {
			return err
		}
		block := append([]string{"Checkpoint " + stamp() + "."}, splitText(text)...)
		sec := findSection(s, f, "handoff")
		if sec == nil {
			a.appendToSection(n, f, "handoff")(block)
		} else {
			if to := lastLine(n.lines, sec.Line+1, sec.BodyEnd); to > sec.Line {
				n.lines = replaceRange(n.lines, sec.Line+1, to, nil)
			}
			n.lines, _ = place(n.lines, sec.Line, block)
		}
		if err := a.save(n); err != nil {
			return err
		}
		a.ok(f.Addr() + "#" + a.sec(s, "handoff"))
		return nil
	})
}

// logBlock is a time-stamped log entry for text.
func logBlock(text string) []string {
	parts := splitText(text)
	block := []string{"- " + today() + " " + clock() + " " + parts[0]}
	for _, p := range parts[1:] {
		block = append(block, "  "+p)
	}
	return block
}

// recentHandoff reports whether the project's handoff was written in the
// last quarter of an hour (then it is not replaced by a mechanical one).
func (a *app) recentHandoff(s *store, f *index.File) bool {
	h := a.sectionBody(s, f, "handoff")
	if len(h) == 0 || !strings.HasPrefix(h[0], "Checkpoint ") {
		return false
	}
	t, err := time.ParseInLocation("2006-01-02 15:04", strings.TrimSuffix(strings.TrimPrefix(h[0], "Checkpoint "), "."), time.Local)
	return err == nil && time.Since(t) < 15*time.Minute
}

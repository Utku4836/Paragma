package cli

import (
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/Utku-Private/Paragma/internal/index"
	"github.com/Utku-Private/Paragma/internal/md"
	"github.com/Utku-Private/Paragma/internal/textutil"
	"github.com/Utku-Private/Paragma/internal/ulid"
	"github.com/spf13/cobra"
)

// ---- finding sessions ----

func isSession(f *index.File) bool { return f.Type == "session" }

// sessionsOn returns active sessions on a branch, newest first. Sessions
// without a branch in their front matter count as the default branch.
func (a *app) sessionsOn(s *store, branch string, includeClosed bool) []*index.File {
	var out []*index.File
	for _, f := range s.res.Files() {
		if !isSession(f) || (!includeClosed && (f.Inactive() || f.Status == "closed")) {
			continue
		}
		b := f.Branch
		if b == "" {
			b = "main"
		}
		if branch == "" || b == branch {
			out = append(out, f)
		}
	}
	sort.SliceStable(out, func(i, j int) bool { return out[i].Created+out[i].Path > out[j].Created+out[j].Path })
	return out
}

// findSession resolves a session by address, file name or title words.
func (a *app) findSession(s *store, name string) (*index.File, error) {
	name = strings.TrimSpace(strings.TrimPrefix(name, "[["))
	name = strings.TrimSuffix(name, "]]")
	var all []*index.File
	for _, f := range s.res.Files() {
		if isSession(f) {
			all = append(all, f)
		}
	}
	want := textutil.Fold(name)
	var exact, partial []*index.File
	for _, f := range all {
		base := textutil.Fold(filepath.Base(f.Addr()))
		switch {
		case textutil.Fold(f.Addr()) == want || base == want || textutil.Fold(f.Title) == want:
			exact = append(exact, f)
		case strings.HasSuffix(base, "-"+slug(name)) || strings.Contains(base, slug(name)) || strings.Contains(textutil.Fold(f.Title), want):
			partial = append(partial, f)
		}
	}
	pick := exact
	if len(pick) == 0 {
		pick = partial
	}
	// prefer active ones, then the newest
	sort.SliceStable(pick, func(i, j int) bool {
		ai, aj := !pick[i].Inactive() && pick[i].Status != "closed", !pick[j].Inactive() && pick[j].Status != "closed"
		if ai != aj {
			return ai
		}
		return pick[i].Path > pick[j].Path
	})
	if len(pick) == 0 {
		var names []string
		for _, f := range all {
			names = append(names, filepath.Base(f.Addr()))
		}
		msg := "no session matches " + name
		if c := textutil.Closest(name, names, 3); len(c) > 0 {
			msg += " — did you mean: " + strings.Join(c, ", ")
		} else {
			msg += " (pg session list)"
		}
		return nil, fail(ExitNotFound, "%s", msg)
	}
	return pick[0], nil
}

// currentSession is the session writes go to: $PARAGMA_SESSION, else the
// one recorded for the active branch. nil when there is none.
func (a *app) currentSession(s *store) *index.File {
	if env := os.Getenv("PARAGMA_SESSION"); env != "" {
		if f, err := a.findSession(s, env); err == nil {
			return f
		}
	}
	branch, _ := s.v.Branch()
	st := s.v.LoadState()
	if name := st.Sessions[branch]; name != "" {
		if f, err := s.res.ResolveFile(name); err == nil && isSession(f) && !f.Inactive() && f.Status != "closed" {
			return f
		}
	}
	return nil
}

func (a *app) setCurrent(s *store, f *index.File, branch string) error {
	if a.dryRun {
		return nil
	}
	st := s.v.LoadState()
	if f == nil {
		delete(st.Sessions, branch)
	} else {
		st.Sessions[branch] = f.Addr()
	}
	return s.v.SaveState(st)
}

// sessionArg resolves an optional session name argument, defaulting to the
// current session.
func (a *app) sessionArg(s *store, args []string) (*index.File, error) {
	if len(args) > 0 && strings.TrimSpace(strings.Join(args, " ")) != "" {
		return a.findSession(s, strings.Join(args, " "))
	}
	if f := a.currentSession(s); f != nil {
		return f, nil
	}
	branch, _ := s.v.Branch()
	if ss := a.sessionsOn(s, branch, false); len(ss) > 0 {
		return ss[0], nil // the newest active session on the branch
	}
	return nil, fail(ExitNotFound, "no active session on branch %s (pg session start <title>)", branch)
}

// ---- section text helpers ----

// sectionBody returns the non-blank lines of a session section.
func (a *app) sectionBody(s *store, f *index.File, key string) []string {
	sec := findSection(s, f, key)
	if sec == nil {
		return nil
	}
	lines, err := fileLines(s, f)
	if err != nil {
		return nil
	}
	var out []string
	for _, l := range sliceLines(lines, sec.Line+1, sec.BodyEnd) {
		if !isBlank(l) {
			out = append(out, l)
		}
	}
	return out
}

// appendToSection appends block at the end of section key in n, creating
// the section (at the end of the note) if it is missing.
func (a *app) appendToSection(n *note, f *index.File, key string) func(block []string) {
	return func(block []string) {
		sec := findSection(n.s, f, key)
		if sec == nil {
			n.lines, _ = place(n.lines, lastLine(n.lines, 1, len(n.lines)), append([]string{"## " + a.sec(n.s, key), ""}, block...))
			return
		}
		n.lines, _ = place(n.lines, lastLine(n.lines, sec.Line, sec.BodyEnd), block)
	}
}

// ---- commands ----

func (a *app) sessionCmd() *cobra.Command {
	c := &cobra.Command{
		Use:   "session",
		Short: "Long-running work that survives new conversations: start plan status log checkpoint resume fork list switch close",
	}
	c.AddCommand(a.sessionStartCmd(), a.sessionPlanCmd(), a.sessionStatusCmd(), a.sessionLogCmd(),
		a.sessionCheckpointCmd(), a.sessionResumeCmd(), a.sessionListCmd(), a.sessionSwitchCmd(),
		a.sessionForkCmd(), a.sessionCloseCmd())
	return c
}

func (a *app) sessionStartCmd() *cobra.Command {
	var goal string
	c := &cobra.Command{
		Use:   "start <title>",
		Short: "Open a session note (Goal, Plan, Notes, Handoff) and make it current",
		Example: `pg session start "Auth refactor" --goal "Move from cookies to JWT + refresh tokens"
pg session plan "Extract current flow" "Refresh endpoint" "Migration"`,
		Args: cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			title := strings.Join(args, " ")
			s, err := a.main()
			if err != nil {
				return err
			}
			branch, _ := s.v.Branch()
			if goal == "" {
				goal = title
			}
			lines := a.sessionTemplate(s, title, branch, goal, nil, "")
			rel := freePath(s, "work/sessions/"+today()+"-"+slug(title)+".md")
			return a.withLock(s, func() error {
				n := newNote(s, rel, lines)
				if err := a.save(n); err != nil {
					return err
				}
				if !a.dryRun {
					f, err := s.res.ResolveFile(n.addr())
					if err != nil {
						return fail(ExitInternal, "%v", err)
					}
					if err := a.setCurrent(s, f, branch); err != nil {
						return fail(ExitInternal, "%v", err)
					}
				}
				a.ok(n.addr())
				a.println(`next: pg session plan "step 1" "step 2" … (one task per argument)`)
				return nil
			})
		},
	}
	c.Flags().StringVar(&goal, "goal", "", "one or two sentences: what done looks like")
	return c
}

func (a *app) sessionTemplate(s *store, title, branch, goal string, handoff []string, forkedFrom string) []string {
	fm := []string{"---", "id: " + ulid.New(), "type: session", "layer: working", "status: active", "title: " + yamlString(title), "branch: " + yamlString(branch), "created: " + stamp()}
	if forkedFrom != "" {
		fm = append(fm, `forked_from: "[[`+forkedFrom+`]]"`)
	}
	lines := append(fm, "---", "# "+title, "", "## "+a.sec(s, "goal"))
	lines = append(lines, splitText(goal)...)
	lines = append(lines, "", "## "+a.sec(s, "plan"), "", "## "+a.sec(s, "notes"), "", "## "+a.sec(s, "handoff"))
	if len(handoff) > 0 {
		lines = append(lines, handoff...)
	}
	return lines
}

func (a *app) sessionPlanCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "plan <task>... | -",
		Short: "Add tasks to the current session's Plan: one per argument, or one per stdin line",
		Long: `Add tasks to the Plan of the current session. Each argument is one task;
with "-" (or --file) each line is one task and indented lines become
subtasks. Prints the new task ids.`,
		Example: `pg session plan "Extract current flow" "Refresh endpoint" "Migration"
pg session plan --file plan.md`,
		Args: cobra.MinimumNArgs(0),
		RunE: func(cmd *cobra.Command, args []string) error {
			items, err := a.taskItems(args, true)
			if err != nil {
				return err
			}
			s, err := a.main()
			if err != nil {
				return err
			}
			f, err := a.sessionArg(s, nil)
			if err != nil {
				return err
			}
			return a.addTasks(s, f, "plan", items, taskFlags{})
		},
	}
}

func (a *app) sessionLogCmd() *cobra.Command {
	return &cobra.Command{
		Use:     "log <text|->",
		Short:   "Add a time-stamped line to the current session's Notes",
		Example: `pg session log "middleware is called from two places, see [[architecture#Middleware]]"`,
		Args:    cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			text, err := a.inputText(args, true)
			if err != nil {
				return err
			}
			s, err := a.main()
			if err != nil {
				return err
			}
			f, err := a.sessionArg(s, nil)
			if err != nil {
				return err
			}
			return a.withLock(s, func() error {
				n, err := a.load(s, f)
				if err != nil {
					return err
				}
				parts := splitText(text)
				block := []string{"- " + clock() + " " + parts[0]}
				for _, p := range parts[1:] {
					block = append(block, "  "+p)
				}
				a.appendToSection(n, f, "notes")(block)
				if err := a.save(n); err != nil {
					return err
				}
				a.ok(f.Addr() + "#" + a.sec(s, "notes"))
				return nil
			})
		},
	}
}

func (a *app) sessionCheckpointCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "checkpoint [text|-]",
		Short: "Rewrite the Handoff section: where things stand and what to do next",
		Long: `Replace the session's Handoff with a dated note for whoever continues
(you after context compaction, or a new conversation). Without text pg
writes a mechanical one: progress, open tasks, last note.`,
		Example: `pg session checkpoint "Refresh tests red because of timezone, see [[jwt-exp-timezone]]. Next: t004."`,
		Args:    cobra.ArbitraryArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			text, err := a.inputText(args, false)
			if err != nil {
				return err
			}
			s, err := a.main()
			if err != nil {
				return err
			}
			f, err := a.sessionArg(s, nil)
			if err != nil {
				return err
			}
			if text == "" {
				text = a.mechanicalHandoff(s, f)
			}
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
		},
	}
}

func (a *app) mechanicalHandoff(s *store, f *index.File) string {
	ts, _ := s.db.Tasks(f.ID)
	tc := index.CountTasks(ts, 1, 1<<30)
	var open []string
	for _, t := range ts {
		if !t.Closed() && t.ID != "" {
			open = append(open, fmt.Sprintf("^%s [%s] %s", t.ID, t.State, t.Text))
		}
	}
	out := fmt.Sprintf("Progress %d/%d.", tc.Done, tc.Total)
	if len(open) > 0 {
		out += " Open: " + strings.Join(open[:min(len(open), 5)], "; ") + "."
	}
	if notes := a.sectionBody(s, f, "notes"); len(notes) > 0 {
		out += " Last note: " + strings.TrimSpace(strings.TrimPrefix(notes[len(notes)-1], "-"))
	}
	return out
}

func (a *app) sessionStatusCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "status [session]",
		Short: "Goal, progress, open tasks and last handoff of a session (default: current)",
		Args:  cobra.ArbitraryArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			f, err := a.sessionArg(s, args)
			if err != nil {
				return err
			}
			return a.printSession(s, f, false)
		},
	}
}

func (a *app) sessionResumeCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "resume [session]",
		Short: "Everything needed to continue in a new conversation, in one call (≤400 tokens)",
		Long: `Print what a fresh conversation needs to carry on: project and branch, the
session's goal, progress, open tasks, the last handoff and recent notes,
plus other active sessions on the branch. With a name it also makes that
session current.`,
		Args: cobra.ArbitraryArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			branch, _ := s.v.Branch()
			f, err := a.sessionArg(s, args)
			if err != nil {
				// nothing to resume: say what exists instead
				a.printf("project: %s  branch: %s\n", projectName(s), branch)
				a.println("no active session on this branch")
				if ss := a.sessionsOn(s, "", false); len(ss) > 0 {
					a.println("active sessions elsewhere:")
					for _, x := range ss[:min(len(ss), 5)] {
						a.printf("  %s  branch:%s\n", x.Addr(), x.Branch)
					}
				}
				a.println("next: pg session start <title> | pg outline")
				return nil
			}
			if len(args) > 0 && a.currentSession(s) != f {
				b := f.Branch
				if b == "" {
					b = branch
				}
				if err := a.setCurrent(s, f, b); err != nil {
					return fail(ExitInternal, "%v", err)
				}
			}
			return a.printSession(s, f, true)
		},
	}
}

// printSession renders status (short) or resume (full).
func (a *app) printSession(s *store, f *index.File, full bool) error {
	branch, git := s.v.Branch()
	ts, _ := s.db.Tasks(f.ID)
	tc := index.CountTasks(ts, 1, 1<<30)
	goal := strings.Join(a.sectionBody(s, f, "goal"), " ")
	handoff := a.sectionBody(s, f, "handoff")
	notes := a.sectionBody(s, f, "notes")

	if a.jsonOut {
		var tasks []map[string]any
		for _, t := range ts {
			tasks = append(tasks, map[string]any{"id": t.ID, "state": t.State, "text": t.Text, "line": t.Line, "parent": t.Parent})
		}
		return a.emitJSON(map[string]any{"project": projectName(s), "branch": branch, "session": f.Addr(), "title": f.Title,
			"goal": goal, "done": tc.Done, "total": tc.Total, "tasks": tasks, "handoff": strings.Join(handoff, "\n"), "notes": notes})
	}
	if full {
		src := "pg"
		if git {
			src = "git"
		}
		a.printf("project: %s  branch: %s (%s)\n", projectName(s), branch, src)
	}
	a.println(join("session: "+f.Addr(), `"`+f.Title+`"`, "started "+f.Date()))
	if goal != "" {
		a.println("goal: " + textutil.OneLine(goal, 300))
	}
	var inProg, blocked, asks []string
	for _, t := range ts {
		switch t.State {
		case "/":
			inProg = append(inProg, t.ID)
		case "!":
			blocked = append(blocked, t.ID)
		case "?":
			asks = append(asks, t.ID)
		}
	}
	prog := fmt.Sprintf("progress: %d/%d done", tc.Done, tc.Total)
	if len(inProg) > 0 {
		prog += "  in progress: " + strings.Join(inProg, ",")
	}
	if len(blocked) > 0 {
		prog += "  blocked: " + strings.Join(blocked, ",")
	}
	if len(asks) > 0 {
		prog += "  waiting on user: " + strings.Join(asks, ",")
	}
	a.println(prog)
	// open tasks, indented like the plan
	var open []string
	for _, t := range ts {
		if t.Closed() {
			continue
		}
		id := "^" + t.ID
		if t.ID == "" {
			id = fmt.Sprintf(":%d", t.Line)
		}
		line := fmt.Sprintf("  %s [%s] %s", id, t.State, t.Text)
		if t.Parent != "" {
			line = fmt.Sprintf("    %s [%s] %s", id, t.State, t.Text)
		}
		if t.Priority != "" {
			line += " !" + t.Priority
		}
		if t.Due != "" {
			line += " due:" + t.Due
		}
		open = append(open, line)
	}
	maxOpen := 8
	if full {
		maxOpen = 15
	}
	if len(open) > 0 {
		a.println("open:")
		for _, l := range open[:min(len(open), maxOpen)] {
			a.println(l)
		}
		if len(open) > maxOpen {
			a.printf("  … %d more: pg task list --in %s\n", len(open)-maxOpen, f.Addr())
		}
	}
	if len(handoff) > 0 {
		h := strings.Join(handoff, " ")
		if !full {
			h = textutil.OneLine(h, 160)
		} else {
			h = textutil.OneLine(h, 600)
		}
		a.println("handoff: " + h)
	}
	if full {
		if len(notes) > 0 {
			a.println("recent notes:")
			for _, l := range notes[max(0, len(notes)-3):] {
				a.println("  " + textutil.OneLine(l, 160))
			}
		}
		var others []string
		for _, x := range a.sessionsOn(s, branch, false) {
			if x.ID != f.ID {
				others = append(others, fmt.Sprintf("%s (%s)", filepath.Base(x.Addr()), x.Date()))
			}
		}
		if len(others) > 0 {
			a.println("other active sessions on branch: " + strings.Join(others[:min(len(others), 4)], ", "))
		}
		if nt, nf := a.nextTask(s, f); nt != nil {
			a.printf("next: pg task start %s  (%s)\n", nt.ID, textutil.OneLine(nt.Text, 80))
			_ = nf
		} else if tc.Total == 0 {
			a.println(`next: pg session plan "step 1" "step 2" …`)
		}
	}
	return nil
}

func (a *app) sessionListCmd() *cobra.Command {
	var all bool
	c := &cobra.Command{
		Use:   "list",
		Short: "Active sessions (all branches) with progress; * marks the current one",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			cur := a.currentSession(s)
			ss := a.sessionsOn(s, "", all)
			if len(ss) == 0 {
				a.println("no results")
				return errNoResults
			}
			tcs, _ := s.db.TaskCountsByFile()
			for _, f := range ss {
				mark := " "
				if cur != nil && cur.ID == f.ID {
					mark = "*"
				}
				st := ""
				if f.Inactive() || f.Status == "closed" {
					st = "closed"
				}
				b := f.Branch
				if b == "" {
					b = "main"
				}
				tc := tcs[f.ID]
				a.println(join(mark+" "+f.Addr(), meta("branch:"+b, st), fmt.Sprintf("tasks %d/%d", tc.Done, tc.Total), f.Date(), `"`+f.Title+`"`))
			}
			return nil
		},
	}
	c.Flags().BoolVar(&all, "all", false, "include closed and archived sessions")
	return c
}

func (a *app) sessionSwitchCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "switch <session>",
		Short: "Make another session current for its branch",
		Args:  cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			f, err := a.findSession(s, strings.Join(args, " "))
			if err != nil {
				return err
			}
			b := f.Branch
			if b == "" {
				b, _ = s.v.Branch()
			}
			if err := a.setCurrent(s, f, b); err != nil {
				return fail(ExitInternal, "%v", err)
			}
			a.ok(f.Addr())
			return nil
		},
	}
}

func (a *app) sessionForkCmd() *cobra.Command {
	return &cobra.Command{
		Use:     "fork <session> <new title>",
		Short:   "Start a new session on the same branch from another one's goal and handoff",
		Example: `pg session fork auth-refactor "Try a server-side session store instead"`,
		Args:    cobra.MinimumNArgs(2),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			src, err := a.findSession(s, args[0])
			if err != nil {
				return err
			}
			title := strings.Join(args[1:], " ")
			branch := src.Branch
			if branch == "" {
				branch, _ = s.v.Branch()
			}
			goal := strings.Join(a.sectionBody(s, src, "goal"), "\n")
			if goal == "" {
				goal = title
			}
			handoff := a.sectionBody(s, src, "handoff")
			if len(handoff) == 0 {
				handoff = []string{"Forked from [[" + filepath.Base(src.Addr()) + "]] " + stamp() + "."}
			}
			lines := a.sessionTemplate(s, title, branch, goal, handoff, filepath.Base(src.Addr()))
			rel := freePath(s, "work/sessions/"+today()+"-"+slug(title)+".md")
			return a.withLock(s, func() error {
				n := newNote(s, rel, lines)
				if err := a.save(n); err != nil {
					return err
				}
				if !a.dryRun {
					f, err := s.res.ResolveFile(n.addr())
					if err == nil {
						a.setCurrent(s, f, branch)
					}
				}
				a.ok(n.addr())
				return nil
			})
		},
	}
}

func (a *app) sessionCloseCmd() *cobra.Command {
	var summary string
	var keep bool
	c := &cobra.Command{
		Use:     "close [session] [--summary TEXT|-]",
		Short:   "Close a session: summary to the journal, note to archive/",
		Example: `pg session close --summary "JWT refresh shipped; migration still blocked on DB access"`,
		Args:    cobra.ArbitraryArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			if summary == "-" {
				t, err := a.inputText([]string{"-"}, true)
				if err != nil {
					return err
				}
				summary = t
			}
			s, err := a.main()
			if err != nil {
				return err
			}
			f, err := a.sessionArg(s, args)
			if err != nil {
				return err
			}
			ts, _ := s.db.Tasks(f.ID)
			tc := index.CountTasks(ts, 1, 1<<30)
			if summary == "" {
				summary = fmt.Sprintf("%d/%d tasks done", tc.Done, tc.Total)
			}
			return a.withLock(s, func() error {
				n, err := a.load(s, f)
				if err != nil {
					return err
				}
				n.lines = setFront(n.lines, "status", "closed")
				n.lines = setFront(n.lines, "closed", stamp())
				if err := a.save(n); err != nil {
					return err
				}
				j, err := a.journalAppend(s, fmt.Sprintf("session [[%s]] closed (%d/%d): %s", filepath.Base(f.Addr()), tc.Done, tc.Total, textutil.OneLine(summary, 400)))
				if err != nil {
					return err
				}
				b := f.Branch
				if b == "" {
					b, _ = s.v.Branch()
				}
				if cur := a.currentSession(s); cur != nil && cur.ID == f.ID {
					a.setCurrent(s, nil, b)
				}
				if !keep {
					g, err := s.res.ResolveFile(f.Addr())
					if err == nil {
						if err := a.move(s, g, "archive/"+g.Path, false); err != nil {
							return err
						}
					}
				} else {
					a.ok(f.Addr())
				}
				if tc.Done < tc.Total {
					a.printf("note: %d open tasks stay in the archived session; move them first with pg task move <id> <addr> if they continue\n", tc.Total-tc.Done)
				}
				a.ok(j)
				return nil
			})
		},
	}
	c.Flags().StringVar(&summary, "summary", "", "what was achieved (\"-\" reads stdin)")
	c.Flags().BoolVar(&keep, "keep", false, "leave the note in work/ instead of moving it to archive/")
	return c
}

// journalAppend adds "- HH:MM text" under today's heading in
// episodes/journal/YYYY-MM.md and returns the day's address.
func (a *app) journalAppend(s *store, text string) (string, error) {
	month := now().Format("2006-01")
	rel := "episodes/journal/" + month + ".md"
	day := today()
	entry := "- " + clock() + " " + text
	var n *note
	if f, err := s.res.ResolveFile(strings.TrimSuffix(rel, ".md")); err == nil && f.Path == rel {
		if n, err = a.load(s, f); err != nil {
			return "", err
		}
	} else {
		n = newNote(s, rel, []string{"---", "type: journal", "layer: episodic", "---", "# " + month})
	}
	at := -1
	for i, l := range n.lines {
		if strings.TrimSpace(l) == "## "+day {
			at = i + 1
		}
	}
	if at < 0 {
		n.lines, _ = place(n.lines, lastLine(n.lines, 1, len(n.lines)), []string{"## " + day, "", entry})
	} else {
		end := len(n.lines)
		for i := at; i < len(n.lines); i++ {
			if isHeadingLine(n.lines[i]) {
				end = i
				break
			}
		}
		n.lines, _ = place(n.lines, lastLine(n.lines, at, end), []string{entry})
	}
	if err := a.save(n); err != nil {
		return "", err
	}
	return strings.TrimSuffix(rel, ".md") + "#" + day, nil
}

func projectName(s *store) string {
	if s.v.Global {
		return "global"
	}
	if p := vaultRegistryName(s); p != "" {
		return p
	}
	return filepath.Base(s.v.Base)
}

var _ = md.SplitLines

package cli

import (
	"fmt"
	"os"
	"regexp"
	"sort"
	"strings"

	"github.com/Utku4836/Paragma/internal/addr"
	"github.com/Utku4836/Paragma/internal/index"
	"github.com/Utku4836/Paragma/internal/md"
	"github.com/Utku4836/Paragma/internal/space"
	"github.com/spf13/cobra"
)

// Checkbox tasks with ids.

// taskItem is one task to add; level 0 is top level, 1 a subtask, ...
type taskItem struct {
	text  string
	state byte
	level int
}

type taskFlags struct {
	priority, due string
	tags          []string
}

// taskItems turns arguments into tasks. With each=true every argument is a
// task (session plan); otherwise the arguments are one task's words. "-" or
// --file read one task per line, indentation making subtasks.
func (a *app) taskItems(args []string, each bool) ([]taskItem, error) {
	fromLines := a.textFile != "" || (len(args) == 1 && args[0] == "-")
	if !fromLines {
		if len(args) == 0 {
			return nil, fail(ExitUsage, "no task text given")
		}
		if each {
			var out []taskItem
			for _, x := range args {
				if t := strings.TrimSpace(x); t != "" {
					text, st := stripMarker(t)
					out = append(out, taskItem{text: text, state: st})
				}
			}
			return out, nil
		}
		text, st := stripMarker(strings.Join(args, " "))
		return []taskItem{{text: text, state: st}}, nil
	}
	raw, err := a.inputText(args, true)
	if err != nil {
		return nil, err
	}
	var out []taskItem
	var stack []int // indent widths of open levels
	for _, l := range splitText(raw) {
		if isBlank(l) {
			continue
		}
		w := 0
		for _, r := range l {
			if r == ' ' {
				w++
			} else if r == '\t' {
				w += 4
			} else {
				break
			}
		}
		for len(stack) > 0 && stack[len(stack)-1] >= w {
			stack = stack[:len(stack)-1]
		}
		level := len(stack)
		stack = append(stack, w)
		text, st := stripMarker(strings.TrimSpace(l))
		out = append(out, taskItem{text: text, state: st, level: level})
	}
	return out, nil
}

// stripMarker removes a leading "- [ ] ", "- ", "* ", "1. " and returns the
// state given by a checkbox (space when none).
func stripMarker(s string) (string, byte) {
	if tl, ok := md.ParseTaskLine(s); ok {
		return strings.TrimSpace(strings.TrimPrefix(s, tl.Indent+tl.Marker+" ["+string(tl.State)+"]")), tl.State
	}
	for _, p := range []string{"- ", "* ", "+ "} {
		if strings.HasPrefix(s, p) {
			return strings.TrimSpace(s[len(p):]), ' '
		}
	}
	i := 0
	for i < len(s) && s[i] >= '0' && s[i] <= '9' {
		i++
	}
	if i > 0 && i+1 < len(s) && (s[i] == '.' || s[i] == ')') && s[i+1] == ' ' {
		return strings.TrimSpace(s[i+2:]), ' '
	}
	return s, ' '
}

// taskLines renders items as new task lines with fresh ids, indented by
// indent plus one level per item level.
func taskLines(s *store, items []taskItem, fl taskFlags, indent, marker string) (block, ids []string) {
	taken := map[string]bool{}
	for _, it := range items {
		tl := md.TaskLine{Indent: indent + strings.Repeat("    ", it.level), Marker: marker, State: it.state}
		if parsed, ok := md.ParseTaskLine("- [ ] " + it.text); ok {
			tl.Task = parsed.Task
		}
		if tl.ID == "" {
			tl.ID = newBlockID(s, "t", taken)
		}
		applyTaskFlags(&tl, fl)
		block = append(block, tl.String())
		ids = append(ids, tl.ID)
	}
	return block, ids
}

// writeTasks adds items to f as new tasks; put inserts the rendered lines
// into the note. It prints one address per task.
func (a *app) writeTasks(s *store, f *index.File, items []taskItem, fl taskFlags, indent, marker string, put func(n *note, block []string)) error {
	return a.withLock(s, func() error {
		n, err := a.load(s, f)
		if err != nil {
			return err
		}
		block, ids := taskLines(s, items, fl, indent, marker)
		put(n, block)
		if err := a.save(n); err != nil {
			return err
		}
		for _, id := range ids {
			a.ok(f.Addr() + "^" + id)
		}
		return nil
	})
}

// addTasks appends tasks at the end of section key of f (created if
// missing).
func (a *app) addTasks(s *store, f *index.File, key string, items []taskItem, fl taskFlags) error {
	return a.writeTasks(s, f, items, fl, "", "-", func(n *note, block []string) { a.appendToSection(n, f, key)(block) })
}

func applyTaskFlags(tl *md.TaskLine, fl taskFlags) {
	if fl.priority != "" {
		tl.Priority = strings.TrimPrefix(strings.ToLower(fl.priority), "!")
	}
	if fl.due != "" {
		tl.Set("due", fl.due)
	}
	for _, t := range fl.tags {
		t = strings.ToLower(strings.TrimPrefix(t, "#"))
		dup := false
		for _, x := range tl.Tags {
			if x == t {
				dup = true
			}
		}
		if !dup && t != "" {
			tl.Tags = append(tl.Tags, t)
		}
	}
}

// findTask resolves "t3f2", "^t3f2" or "note^t3f2" to a task line.
func (a *app) findTask(s *store, id string) (*addr.Target, error) {
	// marks shells and habits put around an id: %t3f2% (cmd), "t3f2", #t3f2
	raw := strings.Trim(strings.TrimSpace(id), "%$'\"`,;")
	if strings.HasPrefix(raw, "#") && reBareID.MatchString(raw[1:]) {
		raw = raw[1:]
	}
	if !strings.ContainsAny(raw, "^#:@") {
		raw = "^" + raw
	}
	p, err := addr.Parse(raw)
	if err != nil {
		return nil, err
	}
	if p.Block == "" {
		return nil, fail(ExitUsage, "%s is not a task id (task ids look like t3f2)", id)
	}
	t, err := s.res.Resolve(p)
	if err != nil {
		return nil, err
	}
	return t, nil
}

// reBareID is a block id written without its note: t3f2, r004.
var reBareID = regexp.MustCompile(`^[A-Za-z][\w-]{1,15}$`)

// changeTasks applies fn to each task id, each in its own locked edit, and
// prints "ok <addr> [state]" per task.
func (a *app) changeTasks(ids []string, fn func(s *store, n *note, t *addr.Target, tl *md.TaskLine) error) error {
	s, err := a.main()
	if err != nil {
		return err
	}
	var last *index.File
	var lastState byte
	for _, id := range ids {
		t, err := a.findTask(s, id)
		if err != nil {
			return err
		}
		err = a.withLock(s, func() error {
			n, err := a.load(s, t.File)
			if err != nil {
				return err
			}
			tl, ok := md.ParseTaskLine(n.lines[t.From-1])
			if !ok {
				return fail(ExitValidation, "%s is not a task line", t.Addr())
			}
			if err := fn(s, n, t, &tl); err != nil {
				return err
			}
			n.lines[t.From-1] = tl.String()
			if err := a.save(n); err != nil {
				return err
			}
			a.ok(fmt.Sprintf("%s [%c]", t.Addr(), tl.State))
			lastState = tl.State
			return nil
		})
		if err != nil {
			return err
		}
		last = t.File
	}
	// after finishing, blocking or parking a task, say what to do next, in
	// full: the task line with its subtasks and notes
	if last != nil && !a.dryRun && strings.ContainsRune("xX-!?", rune(lastState)) {
		if f, err := s.res.ResolveFile(last.Addr()); err == nil {
			if nt, _ := a.nextTask(s, f); nt != nil {
				a.printf("next: %s^%s [%s] %s\n", f.Addr(), nt.ID, nt.State, nt.Text)
				if lines, err := fileLines(s, f); err == nil && nt.End > nt.Line {
					for _, l := range sliceLines(lines, nt.Line+1, min(nt.End, nt.Line+8)) {
						if !isBlank(l) {
							a.println("  " + strings.TrimSpace(l))
						}
					}
				}
			} else if ts, _ := s.db.Tasks(f.ID); len(ts) > 0 {
				tc := index.CountTasks(ts, 1, 1<<30)
				if tc.Open == 0 {
					a.printf("all %d tasks in %s are done\n", tc.Total, f.Addr())
				}
			}
		}
	}
	return nil
}

// openChildren lists open subtasks inside a task's block.
func openChildren(n *note, t *addr.Target) []string {
	var open []string
	for i := t.From + 1; i <= t.To && i <= len(n.lines); i++ {
		if c, ok := md.ParseTaskLine(n.lines[i-1]); ok && !closedState(c.State) {
			id := c.ID
			if id == "" {
				id = fmt.Sprintf("L%d", i)
			}
			open = append(open, id)
		}
	}
	return open
}

func closedState(b byte) bool { return b == 'x' || b == 'X' || b == '-' }

func (a *app) taskStateCmd(use, short, state string, extra func(c *cobra.Command) func(tl *md.TaskLine, n *note, t *addr.Target) error) *cobra.Command {
	c := &cobra.Command{
		Use:   use + " <id>...",
		Short: short,
		Args:  cobra.MinimumNArgs(1),
	}
	var apply func(tl *md.TaskLine, n *note, t *addr.Target) error
	if extra != nil {
		apply = extra(c)
	}
	c.RunE = func(cmd *cobra.Command, args []string) error {
		return a.changeTasks(args, func(s *store, n *note, t *addr.Target, tl *md.TaskLine) error {
			tl.State = state[0]
			if state != "x" {
				tl.Set("done", "")
			}
			if apply != nil {
				return apply(tl, n, t)
			}
			return nil
		})
	}
	return c
}

// taskDoneCmd builds "done" (both pg task done and pg done).
func (a *app) taskDoneCmd() *cobra.Command {
	var force bool
	done := a.taskStateCmd("done", "Mark tasks done [x] with today's date; prints the next task in full", "x", func(c *cobra.Command) func(*md.TaskLine, *note, *addr.Target) error {
		c.Flags().BoolVar(&force, "force", false, "close even with open subtasks")
		return func(tl *md.TaskLine, n *note, t *addr.Target) error {
			if open := openChildren(n, t); len(open) > 0 && !force {
				return fail(ExitValidation, "%s has open subtasks: %s; finish them or add --force", t.Addr(), strings.Join(open, ", "))
			}
			tl.Set("done", today())
			tl.Set("blocked", "")
			return nil
		}
	})
	done.Example = "pg done t3f4\npg done t3f4 t3f5"
	return done
}

func (a *app) taskCmd() *cobra.Command {
	c := &cobra.Command{
		Use:   "task",
		Short: "Checkbox tasks with ids: add list show start done block cancel ask note move edit next claim",
	}
	done := a.taskDoneCmd()
	start := a.taskStateCmd("start", "Mark tasks in progress [/]", "/", func(c *cobra.Command) func(*md.TaskLine, *note, *addr.Target) error {
		return func(tl *md.TaskLine, n *note, t *addr.Target) error { tl.Set("blocked", ""); return nil }
	})
	var by string
	block := a.taskStateCmd("block", "Mark tasks blocked [!] with the reason", "!", func(c *cobra.Command) func(*md.TaskLine, *note, *addr.Target) error {
		c.Flags().StringVar(&by, "by", "", "what blocks it (text or [[note]])")
		return func(tl *md.TaskLine, n *note, t *addr.Target) error {
			if by == "" {
				return fail(ExitUsage, "--by is required: say what blocks the task")
			}
			tl.Set("blocked", by)
			return nil
		}
	})
	block.Example = `pg task block t3f5 --by "no DB access in prod"`
	var why string
	cancel := a.taskStateCmd("cancel", "Cancel tasks [-] with a reason", "-", func(c *cobra.Command) func(*md.TaskLine, *note, *addr.Target) error {
		c.Flags().StringVar(&why, "why", "", "why it is dropped")
		return func(tl *md.TaskLine, n *note, t *addr.Target) error {
			if why != "" {
				tl.Set("why", why)
			}
			return nil
		}
	})
	var q string
	ask := a.taskStateCmd("ask", "Mark tasks as waiting for the user's decision [?]", "?", func(c *cobra.Command) func(*md.TaskLine, *note, *addr.Target) error {
		c.Flags().StringVar(&q, "q", "", "the question to answer")
		return func(tl *md.TaskLine, n *note, t *addr.Target) error {
			if q != "" {
				tl.Set("q", q)
			}
			return nil
		}
	})
	ask.Example = `pg task ask t3f8 --q "Redis or Postgres for the session store?"`
	reopen := a.taskStateCmd("reopen", "Mark tasks open [ ] again", " ", nil)

	c.AddCommand(a.taskAddCmd(), a.taskListCmd(), a.taskShowCmd(), start, done, block, cancel, ask, reopen,
		a.taskNoteCmd(), a.taskMoveCmd(), a.taskEditCmd(), a.taskNextCmd(), a.taskClaimCmd())
	return c
}

func (a *app) taskAddCmd() *cobra.Command {
	var under, to string
	var fl taskFlags
	c := &cobra.Command{
		Use:   "add <text> | -",
		Short: "Add a task (default: the current project's Plan); prints its id",
		Long: `Add a task. It goes to the current project's Plan; --to picks a note or heading and
--under nests it below another task. With "-" or --file every line is a
task and indented lines become subtasks.`,
		Example: `pg task add "Write refresh endpoint" -p p1 --due 2026-10-20
pg task add "Tests" --under t3f2
pg task add - < tasks.txt`,
		Args: cobra.MinimumNArgs(0),
		RunE: func(cmd *cobra.Command, args []string) error {
			items, err := a.taskItems(args, false)
			if err != nil {
				return err
			}
			s, err := a.main()
			if err != nil {
				return err
			}
			switch {
			case under != "":
				pt, err := a.findTask(s, under)
				if err != nil {
					return err
				}
				lines, err := fileLines(s, pt.File)
				if err != nil {
					return err
				}
				parent, ok := md.ParseTaskLine(lines[pt.From-1])
				if !ok {
					return fail(ExitValidation, "%s is not a task", pt.Addr())
				}
				// right below the parent's block, one level deeper
				return a.writeTasks(s, pt.File, items, fl, parent.Indent+"    ", parent.Marker, func(n *note, block []string) {
					n.lines = replaceRange(n.lines, pt.To+1, pt.To, block)
				})
			case to != "":
				ts, t, err := a.resolve(to)
				if err != nil {
					return err
				}
				return a.writeTasks(ts, t.File, items, fl, "", "-", func(n *note, block []string) {
					after := lastLine(n.lines, 1, len(n.lines))
					if t.Kind == addr.KindSection && !t.Section.IsTitle {
						after = lastLine(n.lines, t.Section.Line, t.Section.BodyEnd)
					}
					n.lines, _ = place(n.lines, after, block)
				})
			}
			f, err := a.noteForWrite(s)
			if err != nil {
				return err
			}
			return a.addTasks(s, f, "plan", items, fl)
		},
	}
	c.Flags().StringVar(&under, "under", "", "parent task id: add as subtask")
	c.Flags().StringVar(&to, "to", "", "note or heading to add the task to")
	c.Flags().StringVarP(&fl.priority, "priority", "p", "", "p0 (highest) ... p3")
	c.Flags().StringVar(&fl.due, "due", "", "due date YYYY-MM-DD")
	c.Flags().StringSliceVar(&fl.tags, "tag", nil, "tag(s)")
	return c
}

var stateFilters = map[string]string{
	"open":     " /",
	"todo":     " ",
	"doing":    "/",
	"blocked":  "!",
	"question": "?",
	"ask":      "?",
	"done":     "xX",
	"cancel":   "-",
	"active":   " /!?",
	"all":      " /!?xX-",
}

func (a *app) taskListCmd() *cobra.Command {
	var states []string
	var in string
	var all bool
	c := &cobra.Command{
		Use:   "list",
		Short: "Tasks across the space, the current project first (default: not done, not cancelled)",
		Example: `pg task list
pg task list --state blocked,question
pg task list --in auth-refactor --state all`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			want := ""
			for _, st := range splitList(states) {
				f, ok := stateFilters[strings.ToLower(st)]
				if !ok {
					return fail(ExitUsage, "--state takes open, todo, doing, blocked, question, done, cancel, active, all")
				}
				want += f
			}
			if want == "" {
				want = stateFilters["active"]
			}
			s, err := a.main()
			if err != nil {
				return err
			}
			var files []*index.File
			from, to := 0, 1<<30
			if in != "" {
				_, t, err := a.resolve(in)
				if err != nil {
					return err
				}
				files = []*index.File{t.File}
				if t.Kind != addr.KindFile {
					from, to = t.From, t.To
				}
			} else {
				// the current project's tasks first, then the rest of the space
				for _, mine := range []bool{true, false} {
					for _, f := range s.res.Files() {
						if (all || !f.Inactive()) && (s.project != "" && space.ProjectOf(f.Path) == s.project) == mine {
							files = append(files, f)
						}
					}
				}
			}
			type row struct {
				f *index.File
				t index.TaskRow
			}
			var rows []row
			for _, f := range files {
				ts, err := s.db.Tasks(f.ID)
				if err != nil {
					return fail(ExitInternal, "%v", err)
				}
				for _, t := range ts {
					if t.Line >= from && t.Line <= to && strings.Contains(want, t.State) {
						rows = append(rows, row{f, t})
					}
				}
			}
			if a.jsonOut {
				var out []map[string]any
				for _, r := range rows {
					out = append(out, map[string]any{"addr": taskAddr(r.f, r.t), "state": r.t.State, "text": r.t.Text, "priority": r.t.Priority, "due": r.t.Due, "parent": r.t.Parent})
				}
				return a.emitJSON(out)
			}
			if len(rows) == 0 {
				a.println("no results")
				return errNoResults
			}
			bw := a.newBudget(a.budgetOr(s.v.Cfg.Budget))
			for i, r := range rows {
				if !bw.line(taskLineOut(r.f, r.t)) {
					a.printf("... %d more; narrow with --in <note> or --state\n", len(rows)-i)
					break
				}
			}
			return nil
		},
	}
	c.Flags().StringSliceVar(&states, "state", nil, "open, todo, doing, blocked, question, done, cancel, active (default), all")
	c.Flags().StringVar(&in, "in", "", "only tasks in this note or heading")
	c.Flags().BoolVar(&all, "all", false, "include archived notes")
	return c
}

func taskAddr(f *index.File, t index.TaskRow) string {
	if t.ID != "" {
		return f.Addr() + "^" + t.ID
	}
	return fmt.Sprintf("%s:%d", f.Addr(), t.Line)
}

func taskLineOut(f *index.File, t index.TaskRow) string {
	parts := []string{taskAddr(f, t), "[" + t.State + "]", t.Text}
	if t.Priority != "" {
		parts = append(parts, "!"+t.Priority)
	}
	if t.Due != "" {
		parts = append(parts, "due:"+t.Due)
	}
	if t.Parent != "" {
		parts = append(parts, "under:"+t.Parent)
	}
	return strings.Join(parts, "  ")
}

func (a *app) taskShowCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "show <id>",
		Short: "A task with its subtasks and notes",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			t, err := a.findTask(s, args[0])
			if err != nil {
				return err
			}
			return a.read(s, t, true, false, false)
		},
	}
}

// nextTask picks the best task to work on in f: in progress before todo,
// leaves before parents, higher priority, earlier due date, then document
// order; blocked, waiting and claimed-by-others tasks are skipped.
func (a *app) nextTask(s *store, f *index.File) (*index.TaskRow, *index.File) {
	ts, err := s.db.Tasks(f.ID)
	if err != nil {
		return nil, nil
	}
	me := os.Getenv("PARAGMA_AGENT")
	claimed := a.claims(s, f)
	hasOpenChild := map[string]bool{}
	for _, t := range ts {
		if t.Parent != "" && !t.Closed() && t.State != "-" {
			hasOpenChild[t.Parent] = true
		}
	}
	var cands []index.TaskRow
	for _, t := range ts {
		if t.State != " " && t.State != "/" {
			continue
		}
		if t.ID != "" && hasOpenChild[t.ID] {
			continue
		}
		if by := claimed[t.Line]; by != "" && by != me {
			continue
		}
		cands = append(cands, t)
	}
	if len(cands) == 0 {
		return nil, nil
	}
	prio := func(p string) string {
		if p == "" {
			return "p9"
		}
		return p
	}
	due := func(d string) string {
		if d == "" {
			return "9999"
		}
		return d
	}
	inProgress := map[string]bool{}
	for _, t := range ts {
		if t.State == "/" && t.ID != "" {
			inProgress[t.ID] = true
		}
	}
	// work already started comes first: an in-progress task, or a subtask of one
	started := func(t index.TaskRow) bool { return t.State == "/" || inProgress[t.Parent] }
	sort.SliceStable(cands, func(i, j int) bool {
		x, y := cands[i], cands[j]
		if started(x) != started(y) {
			return started(x)
		}
		if prio(x.Priority) != prio(y.Priority) {
			return prio(x.Priority) < prio(y.Priority)
		}
		if due(x.Due) != due(y.Due) {
			return due(x.Due) < due(y.Due)
		}
		return x.Line < y.Line
	})
	return &cands[0], f
}

// claims maps task line -> @by(...) owner for a file.
func (a *app) claims(s *store, f *index.File) map[int]string {
	out := map[int]string{}
	lines, err := fileLines(s, f)
	if err != nil {
		return out
	}
	for i, l := range lines {
		if !strings.Contains(l, "@by(") {
			continue
		}
		if tl, ok := md.ParseTaskLine(l); ok {
			out[i+1] = tl.Get("by")
		}
	}
	return out
}

func (a *app) taskNextCmd() *cobra.Command {
	var in string
	c := &cobra.Command{
		Use:   "next",
		Short: "The task to do next: the current project's note first, then its other notes",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			var scope []*index.File
			if in != "" {
				_, t, err := a.resolve(in)
				if err != nil {
					return err
				}
				scope = append(scope, t.File)
			} else {
				if f := projectNote(s, s.project); f != nil {
					scope = append(scope, f)
				}
				// then every other note of the current project with open tasks
				tcs, _ := s.db.TaskCountsByFile()
				for _, f := range s.res.Files() {
					if s.project != "" && space.ProjectOf(f.Path) == s.project && !f.Inactive() && tcs[f.ID].Open > 0 && (len(scope) == 0 || f.ID != scope[0].ID) {
						scope = append(scope, f)
					}
				}
			}
			for _, f := range scope {
				if t, _ := a.nextTask(s, f); t != nil {
					if a.jsonOut {
						return a.emitJSON(map[string]any{"addr": taskAddr(f, *t), "state": t.State, "text": t.Text})
					}
					a.println(taskLineOut(f, *t))
					if t.State == " " && t.ID != "" {
						a.printf("next: pg task start %s\n", t.ID)
					}
					return nil
				}
			}
			a.println("no results")
			return errNoResults
		},
	}
	c.Flags().StringVar(&in, "in", "", "only look in this note")
	return c
}

func (a *app) taskNoteCmd() *cobra.Command {
	return &cobra.Command{
		Use:     "note <id> <text|->",
		Short:   "Add an indented note line under a task",
		Example: `pg task note t3f2 "fails only outside CI: timezone"`,
		Args:    cobra.MinimumNArgs(2),
		RunE: func(cmd *cobra.Command, args []string) error {
			text, err := a.inputText(args[1:], true)
			if err != nil {
				return err
			}
			s, err := a.main()
			if err != nil {
				return err
			}
			t, err := a.findTask(s, args[0])
			if err != nil {
				return err
			}
			return a.withLock(s, func() error {
				n, err := a.load(s, t.File)
				if err != nil {
					return err
				}
				tl, ok := md.ParseTaskLine(n.lines[t.From-1])
				if !ok {
					return fail(ExitValidation, "%s is not a task", t.Addr())
				}
				var block []string
				for i, l := range splitText(text) {
					if i == 0 {
						block = append(block, tl.Indent+"    - "+l)
					} else {
						block = append(block, tl.Indent+"      "+l)
					}
				}
				n.lines = replaceRange(n.lines, t.To+1, t.To, block)
				if err := a.save(n); err != nil {
					return err
				}
				a.ok(t.Addr())
				return nil
			})
		},
	}
}

func (a *app) taskMoveCmd() *cobra.Command {
	return &cobra.Command{
		Use:     "move <id> <addr>",
		Short:   "Move a task (with its subtasks) to another note or heading",
		Example: "pg task move t3f9 payments#Plan",
		Args:    cobra.ExactArgs(2),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			t, err := a.findTask(s, args[0])
			if err != nil {
				return err
			}
			_, dst, err := a.resolve(args[1])
			if err != nil {
				return err
			}
			if dst.File.ID == t.File.ID && dst.From >= t.From && dst.From <= t.To {
				return fail(ExitUsage, "cannot move a task into itself")
			}
			return a.withLock(s, func() error {
				src, err := a.load(s, t.File)
				if err != nil {
					return err
				}
				block := append([]string(nil), src.lines[t.From-1:t.To]...)
				base := len(block[0]) - len(strings.TrimLeft(block[0], " \t"))
				for i, l := range block {
					cut := min(base, len(l)-len(strings.TrimLeft(l, " \t")))
					block[i] = l[cut:]
				}
				dn := src
				if dst.File.ID != t.File.ID {
					if dn, err = a.load(s, dst.File); err != nil {
						return err
					}
				}
				// insertion point in the destination before anything is removed
				var after int
				switch dst.Kind {
				case addr.KindFile:
					after = lastLine(dn.lines, 1, len(dn.lines))
				case addr.KindSection:
					after = lastLine(dn.lines, dst.Section.Line, dst.Section.BodyEnd)
				default:
					after = dst.To
				}
				src.lines = replaceRange(src.lines, t.From, t.To, nil)
				if dn == src && after >= t.From {
					after -= t.To - t.From + 1
				}
				dn.lines, _ = place(dn.lines, after, block)
				if dn != src {
					if err := a.save(src); err != nil {
						return err
					}
				}
				if err := a.save(dn); err != nil {
					return err
				}
				id := strings.TrimPrefix(args[0], "^")
				if i := strings.LastIndex(id, "^"); i >= 0 {
					id = id[i+1:]
				}
				a.ok(dst.File.Addr() + "^" + id)
				return nil
			})
		},
	}
}

func (a *app) taskEditCmd() *cobra.Command {
	var text string
	var fl taskFlags
	var untag []string
	c := &cobra.Command{
		Use:     "edit <id>",
		Short:   "Change a task's text, priority, due date or tags",
		Example: `pg task edit t3f2 --due 2026-10-25 -p p0`,
		Args:    cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			return a.changeTasks(args, func(s *store, n *note, t *addr.Target, tl *md.TaskLine) error {
				if text != "" {
					tl.Text = text
				}
				applyTaskFlags(tl, fl)
				if len(untag) > 0 {
					drop := map[string]bool{}
					for _, u := range splitList(untag) {
						drop[strings.ToLower(strings.TrimPrefix(u, "#"))] = true
					}
					var keep []string
					for _, x := range tl.Tags {
						if !drop[x] {
							keep = append(keep, x)
						}
					}
					tl.Tags = keep
				}
				return nil
			})
		},
	}
	c.Flags().StringVar(&text, "text", "", "new task text")
	c.Flags().StringVarP(&fl.priority, "priority", "p", "", "p0 ... p3")
	c.Flags().StringVar(&fl.due, "due", "", "due date YYYY-MM-DD")
	c.Flags().StringSliceVar(&fl.tags, "tag", nil, "add tag(s)")
	c.Flags().StringSliceVar(&untag, "untag", nil, "remove tag(s)")
	return c
}

func (a *app) taskClaimCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "claim <id>...",
		Short: "Claim tasks for this agent ($PARAGMA_AGENT or --as) so parallel agents skip them in task next",
		Args:  cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			_ = s
			me := os.Getenv("PARAGMA_AGENT")
			if me == "" {
				return fail(ExitValidation, "set PARAGMA_AGENT=<a name for this agent> to claim tasks")
			}
			return a.changeTasks(args, func(s *store, n *note, t *addr.Target, tl *md.TaskLine) error {
				if by := tl.Get("by"); by != "" && by != me {
					return fail(ExitConflict, "%s is claimed by %s", t.Addr(), by)
				}
				tl.Set("by", me)
				if tl.State == ' ' {
					tl.State = '/'
				}
				return nil
			})
		},
	}
}

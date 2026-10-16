package cli

import (
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"github.com/Utku-Private/Paragma/internal/index"
	"github.com/Utku-Private/Paragma/internal/md"
	"github.com/Utku-Private/Paragma/internal/space"
	"github.com/Utku-Private/Paragma/internal/textutil"
	"github.com/Utku-Private/Paragma/internal/ulid"
	"github.com/spf13/cobra"
)

// A project is a folder at the top of a space. Its project note,
// <project>/<project>.md, holds the goal, decisions, plan, log and handoff;
// everything else in the folder is the agent's to organise.

// projectNote returns the project's note, nil when the folder has none.
func projectNote(s *store, p string) *index.File {
	if p == "" {
		return nil
	}
	if f, err := s.res.ResolveFile(p + "/" + p); err == nil && f.Path == p+"/"+p+".md" {
		return f
	}
	return nil
}

// projectFolder maps a name the agent typed ("Payments flow", "payments")
// to a project folder name.
func projectFolder(name string) string {
	name = strings.TrimSpace(name)
	if space.ValidName(name) && strings.ToLower(name) == name {
		return name
	}
	return slug(name)
}

// findProject resolves a project name to an existing folder.
func (a *app) findProject(s *store, name string) (string, error) {
	ps := s.v.Projects()
	want := textutil.Fold(projectFolder(name))
	for _, p := range ps {
		if textutil.Fold(p) == want || textutil.Fold(p) == textutil.Fold(name) {
			return p, nil
		}
	}
	// the project note's title
	for _, p := range ps {
		if f := projectNote(s, p); f != nil && textutil.Fold(f.Title) == textutil.Fold(name) {
			return p, nil
		}
	}
	msg := fmt.Sprintf("no project %q in space %s", name, s.v.Name)
	if c := textutil.Closest(name, ps, 3); len(c) > 0 {
		msg += " - did you mean: " + strings.Join(c, ", ")
	} else if len(ps) > 0 {
		msg += " (projects: " + strings.Join(ps, ", ") + ")"
	}
	return "", fail(ExitNotFound, "%s; new project: pg start %s", msg, projectFolder(name))
}

// projectTemplate is a new project note.
func (a *app) projectTemplate(s *store, title, goal string, decisions []string) []string {
	lines := []string{"---", "id: " + ulid.New(), "type: project", "layer: working", "status: active",
		"title: " + yamlString(title), "created: " + stamp(), "---", "# " + title, "", "## " + a.sec(s, "goal")}
	lines = append(lines, splitText(goal)...)
	lines = append(lines, "", "## "+a.sec(s, "decisions"))
	for _, d := range decisions {
		lines = append(lines, decisionLine(d)...)
	}
	return append(lines, "", "## "+a.sec(s, "plan"), "", "## "+a.sec(s, "notes"), "", "## "+a.sec(s, "handoff"))
}

func decisionLine(text string) []string {
	parts := splitText(text)
	out := []string{"- " + today() + " " + strings.TrimSpace(parts[0])}
	for _, p := range parts[1:] {
		out = append(out, "  "+p)
	}
	return out
}

// noteForWrite returns the current project's note, creating a minimal one
// when the folder exists without it.
func (a *app) noteForWrite(s *store) (*index.File, error) {
	p, err := a.writeProject(s)
	if err != nil {
		return nil, err
	}
	if f := projectNote(s, p); f != nil {
		return f, nil
	}
	if a.dryRun {
		return nil, fail(ExitUsage, "project %s has no %s.md yet; run once without --dry-run", p, p)
	}
	n := newNote(s, p+"/"+p+".md", a.projectTemplate(s, p, p, nil))
	if err := a.save(n); err != nil {
		return nil, err
	}
	if f := projectNote(s, p); f != nil {
		return f, nil
	}
	return nil, fail(ExitInternal, "could not create %s/%s.md", p, p)
}

// sectionBody returns the non-blank lines of a section.
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

// appendTo adds block to section key of the current project's note.
func (a *app) appendTo(key string, block []string) error {
	s, err := a.main()
	if err != nil {
		return err
	}
	f, err := a.noteForWrite(s)
	if err != nil {
		return err
	}
	return a.withLock(s, func() error {
		n, err := a.load(s, f)
		if err != nil {
			return err
		}
		a.appendToSection(n, f, key)(block)
		if err := a.save(n); err != nil {
			return err
		}
		a.ok(f.Addr() + "#" + a.sec(s, key))
		return nil
	})
}

// ---- commands ----

func (a *app) startCmd() *cobra.Command {
	var goal, title string
	var steps, decisions []string
	c := &cobra.Command{
		Use:   "start <project>",
		Short: "Start a project: its folder and note with goal, steps and decisions in one call; it becomes current here",
		Long: `Create the project folder <project>/ and its note <project>/<project>.md
(Goal, Decisions, Plan, Log, Handoff) and make it the current project of
this directory. Every --step becomes a task, every --decision a dated line.
Inside the folder organise notes as you like (pg new <project>/<path> ...).
If the project exists, nothing is created: continue it with pg resume.`,
		Example: `pg start payments --goal "Card payments with Stripe" --step "Webhook endpoint" --step "Tests" --decision "Discount is applied before VAT"`,
		Args:    cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			name := strings.Join(args, " ")
			p := projectFolder(name)
			if !space.ValidName(p) {
				return fail(ExitUsage, "bad project name %q", name)
			}
			s, err := a.main()
			if err != nil {
				return err
			}
			if s.v.HasProject(p) {
				msg := fmt.Sprintf("project %s already exists", p)
				if f := projectNote(s, p); f != nil {
					ts, _ := s.db.Tasks(f.ID)
					tc := index.CountTasks(ts, 1, 1<<30)
					msg += fmt.Sprintf(" (%d/%d tasks, updated %s)", tc.Done, tc.Total, f.Date())
				}
				return fail(ExitConflict, "%s; continue it: pg resume %s", msg, p)
			}
			if title == "" {
				title = name
				if title == p {
					title = strings.ToUpper(p[:1]) + p[1:]
				}
			}
			if goal == "" {
				goal = title
			}
			rel := p + "/" + p + ".md"
			return a.withLock(s, func() error {
				n := newNote(s, rel, a.projectTemplate(s, title, goal, decisions))
				if h := a.gitHead(); h != "" {
					n.lines = setFront(n.lines, "base", h) // where the work began, for pg resume
				}
				if len(steps) > 0 {
					taken := map[string]bool{}
					var block []string
					for _, st := range steps {
						text, state := stripMarker(strings.TrimSpace(st))
						tl := md.TaskLine{Marker: "-", State: state}
						if parsed, ok := md.ParseTaskLine("- [ ] " + text); ok {
							tl.Task = parsed.Task
						}
						tl.ID = newBlockID(s, "t", taken)
						block = append(block, tl.String())
					}
					for i, l := range n.lines {
						if l == "## "+a.sec(s, "plan") {
							n.lines, _ = place(n.lines, i+1, block)
							break
						}
					}
				}
				if err := a.save(n); err != nil {
					return err
				}
				if err := a.setProject(s, p); err != nil {
					return err
				}
				a.ok(n.addr() + "  (current project here)")
				if !a.dryRun {
					if f := projectNote(s, p); f != nil {
						ts, _ := s.db.Tasks(f.ID)
						for _, t := range ts {
							a.printf("  ^%s [%s] %s\n", t.ID, t.State, t.Text)
						}
					}
				}
				a.println(`next: pg done <id> after each step | pg log "<finding>" | pg decide "<decision>" | your notes: pg new note "<title>" --dir <folder>`)
				return nil
			})
		},
	}
	c.Flags().StringVar(&goal, "goal", "", "one or two sentences: what done looks like")
	c.Flags().StringVar(&title, "title", "", "display title (default: the name)")
	c.Flags().StringArrayVar(&steps, "step", nil, "a plan step; repeat for each (each becomes a task)")
	c.Flags().StringArrayVar(&decisions, "decision", nil, "a decision already made (e.g. by the user); repeatable")
	return c
}

func (a *app) resumeCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "resume [project]",
		Short: "Switch to a project and print everything needed to continue it, in one call",
		Long: `Make the project current in this directory and print its goal, all
decisions, open tasks, the last handoff, recent log lines and the notes in
its folder with their cost. Without a name: the current project, or the
list of projects.`,
		Example: "pg resume payments",
		Args:    cobra.ArbitraryArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			p := s.project
			if len(args) > 0 {
				if p, err = a.findProject(s, strings.Join(args, " ")); err != nil {
					return err
				}
			}
			if p == "" {
				ps := s.v.Projects()
				if len(ps) == 1 {
					p = ps[0]
				} else {
					a.printf("no current project here (space %s)\n", s.v.Name)
					a.printProjectList(s, 10)
					a.println("next: pg resume <project> | pg start <project> --goal \"...\"")
					return nil
				}
			}
			if p != s.project {
				if err := a.setProject(s, p); err != nil {
					return err
				}
			}
			return a.printProject(s, p, true)
		},
	}
}

func (a *app) planCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "plan <task>... | -",
		Short: "Add tasks to the current project's Plan: one per argument (or per stdin line)",
		Long: `Add tasks to the Plan of the current project. Each argument is one task;
with "-" (or --file) each line is one task and indented lines become
subtasks. Prints the new task ids.`,
		Example: `pg plan "Extract current flow" "Refresh endpoint" "Migration"`,
		Args:    cobra.MinimumNArgs(0),
		RunE: func(cmd *cobra.Command, args []string) error {
			items, err := a.taskItems(args, true)
			if err != nil {
				return err
			}
			s, err := a.main()
			if err != nil {
				return err
			}
			f, err := a.noteForWrite(s)
			if err != nil {
				return err
			}
			return a.addTasks(s, f, "plan", items, taskFlags{})
		},
	}
}

func (a *app) logCmd() *cobra.Command {
	return &cobra.Command{
		Use:     "log <text|->",
		Short:   "Add a time-stamped line to the current project's Log",
		Example: `pg log "middleware is called from two places, see [[architecture#Middleware]]"`,
		Args:    cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			text, err := a.inputText(args, true)
			if err != nil {
				return err
			}
			parts := splitText(text)
			block := []string{"- " + today() + " " + clock() + " " + parts[0]}
			for _, p := range parts[1:] {
				block = append(block, "  "+p)
			}
			return a.appendTo("notes", block)
		},
	}
}

func (a *app) decideCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "decide <text|->",
		Short: "Record a decision (the user's or yours) in the current project's Decisions",
		Long: `Add a dated line to the project's Decisions. pg resume always prints
every decision in full, so this is the place for what must not be lost:
choices the user made, constraints, agreed behaviour. For a decision with
context and alternatives that outlives the project use pg decision record.`,
		Example: `pg decide "Discount is applied before VAT (user)"`,
		Args:    cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			text, err := a.inputText(args, true)
			if err != nil {
				return err
			}
			return a.appendTo("decisions", decisionLine(text))
		},
	}
}

func (a *app) checkpointCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "checkpoint [text|-]",
		Short: "Rewrite the current project's Handoff: where things stand and what comes next",
		Long: `Replace the Handoff section with a dated note for whoever continues (a
new conversation). Without text pg writes a mechanical one: progress, open
tasks, last log line.`,
		Example: `pg checkpoint "Signature tests red because of the clock; next: t004"`,
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
			f, err := a.noteForWrite(s)
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
		out += " Last log: " + strings.TrimSpace(strings.TrimPrefix(notes[len(notes)-1], "-"))
	}
	return out
}

// printProject renders status (short) or resume (full).
func (a *app) printProject(s *store, p string, full bool) error {
	f := projectNote(s, p)
	var ts []index.TaskRow
	var goal string
	var decisions, handoff, notes []string
	if f != nil {
		ts, _ = s.db.Tasks(f.ID)
		goal = strings.Join(a.sectionBody(s, f, "goal"), " ")
		decisions = a.sectionBody(s, f, "decisions")
		handoff = a.sectionBody(s, f, "handoff")
		notes = a.sectionBody(s, f, "notes")
	}
	tc := index.CountTasks(ts, 1, 1<<30)
	if a.jsonOut {
		var tasks []map[string]any
		for _, t := range ts {
			tasks = append(tasks, map[string]any{"id": t.ID, "state": t.State, "text": t.Text, "line": t.Line, "parent": t.Parent})
		}
		out := map[string]any{"space": s.v.Name, "project": p, "goal": goal, "decisions": decisions, "done": tc.Done,
			"total": tc.Total, "tasks": tasks, "handoff": strings.Join(handoff, "\n"), "log": notes}
		if f != nil {
			out["note"] = f.Addr()
			out["title"] = f.Title
		}
		return a.emitJSON(out)
	}
	head := []string{"project: " + p, "(space " + s.v.Name + ")"}
	if f != nil {
		if textutil.Fold(f.Title) != textutil.Fold(p) {
			head = append(head, `"`+f.Title+`"`)
		}
		if f.Status == "closed" {
			head = append(head, "[closed]")
		}
		head = append(head, "started "+f.Date())
	}
	head = append(head, fmt.Sprintf("tasks %d/%d", tc.Done, tc.Total))
	a.println(strings.Join(head, "  "))
	if f == nil {
		a.printf("no %s/%s.md yet (it is created on the first pg plan/log/decide)\n", p, p)
	}
	if goal != "" {
		a.println("goal: " + goal)
	}
	if len(decisions) > 0 {
		a.println("decisions:")
		for _, d := range decisions {
			a.println("  " + strings.TrimSpace(d))
		}
	}
	var open []string
	for _, t := range ts {
		if t.Closed() {
			continue
		}
		id := "^" + t.ID
		if t.ID == "" {
			id = fmt.Sprintf(":%d", t.Line)
		}
		indent := "  "
		if t.Parent != "" {
			indent = "    "
		}
		line := fmt.Sprintf("%s%s [%s] %s", indent, id, t.State, t.Text)
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
		maxOpen = 30
	}
	if full {
		var done []string
		for _, t := range ts {
			if t.Closed() {
				done = append(done, fmt.Sprintf("^%s [%s] %s", t.ID, t.State, textutil.OneLine(t.Text, 120)))
			}
		}
		if len(done) > 0 {
			a.println("done: " + strings.Join(done, "; "))
		}
	}
	if len(open) > 0 {
		a.println("open:")
		for _, l := range open[:min(len(open), maxOpen)] {
			a.println(l)
		}
		if len(open) > maxOpen {
			a.printf("  ... %d more: pg task list --in %s\n", len(open)-maxOpen, f.Addr())
		}
	}
	if len(handoff) > 0 {
		h := strings.Join(handoff, " ")
		if !full {
			h = textutil.OneLine(h, 200)
		}
		a.println("handoff: " + h)
	}
	if !full {
		return nil
	}
	if len(notes) > 0 {
		k := min(len(notes), 12)
		if k == len(notes) {
			a.println("log:")
		} else {
			a.printf("log (last %d of %d; all: pg read %s#%s):\n", k, len(notes), f.Addr(), a.sec(s, "notes"))
		}
		for _, l := range notes[len(notes)-k:] {
			a.println("  " + strings.TrimSpace(l))
		}
	}
	a.printProjectFiles(s, p, f)
	if f != nil {
		if lines, err := fileLines(s, f); err == nil {
			a.printCodeChanges(frontValue(lines, "base"))
		}
	}
	if f != nil {
		if nt, _ := a.nextTask(s, f); nt != nil {
			a.printf("next: ^%s %s  (pg done %s when finished)\n", nt.ID, textutil.OneLine(nt.Text, 100), nt.ID)
		} else if tc.Total == 0 {
			a.println(`next: pg plan "step 1" "step 2" ...`)
		}
		if len(notes) <= 12 {
			a.println("(this is the whole project note; no need to read it)")
		}
	}
	return nil
}

// printProjectFiles lists the notes in a project folder (other than the
// project note) with their cost; big folders are summarised per directory.
func (a *app) printProjectFiles(s *store, p string, skip *index.File) {
	var fs []*index.File
	for _, f := range s.res.Files() {
		if space.ProjectOf(f.Path) == p && (skip == nil || f.ID != skip.ID) && !f.Inactive() {
			fs = append(fs, f)
		}
	}
	if len(fs) == 0 {
		return
	}
	short := func(f *index.File) string { return strings.TrimPrefix(f.Addr(), p+"/") }
	total := 0
	for _, f := range fs {
		total += f.Tokens
	}
	if len(fs) <= 12 {
		var parts []string
		for _, f := range fs {
			parts = append(parts, short(f)+" "+textutil.FmtTokens(f.Tokens))
		}
		a.printf("notes in %s/ (%d, %s): %s\n", p, len(fs), textutil.FmtTokens(total), strings.Join(parts, ", "))
		return
	}
	type dir struct{ n, tok int }
	dirs := map[string]*dir{}
	for _, f := range fs {
		d := short(f)
		if i := strings.Index(d, "/"); i >= 0 {
			d = d[:i] + "/"
		}
		if dirs[d] == nil {
			dirs[d] = &dir{}
		}
		dirs[d].n++
		dirs[d].tok += f.Tokens
	}
	names := make([]string, 0, len(dirs))
	for d := range dirs {
		names = append(names, d)
	}
	sort.Strings(names)
	var parts []string
	for _, d := range names {
		if strings.HasSuffix(d, "/") {
			parts = append(parts, fmt.Sprintf("%s %d notes %s", d, dirs[d].n, textutil.FmtTokens(dirs[d].tok)))
		} else {
			parts = append(parts, d+" "+textutil.FmtTokens(dirs[d].tok))
		}
	}
	a.printf("notes in %s/ (%d, %s): %s\n", p, len(fs), textutil.FmtTokens(total), strings.Join(parts, ", "))
}

// printProjectList prints one line per project, the current one marked.
func (a *app) printProjectList(s *store, limit int) {
	tcs, _ := s.db.TaskCountsByFile()
	type row struct {
		p, line, date string
	}
	var rows []row
	counts := map[string]int{}
	for _, f := range s.res.Files() {
		counts[space.ProjectOf(f.Path)]++
	}
	for _, p := range s.v.Projects() {
		mark := " "
		if p == s.project {
			mark = "*"
		}
		parts := []string{mark + " " + p, fmt.Sprintf("%d notes", counts[p])}
		date := ""
		if f := projectNote(s, p); f != nil {
			tc := tcs[f.ID]
			if tc.Total > 0 {
				parts = append(parts, fmt.Sprintf("tasks %d/%d", tc.Done, tc.Total))
			}
			if f.Status == "closed" {
				parts = append(parts, "[closed]")
			}
			date = f.Date()
			if g := a.sectionBody(s, f, "goal"); len(g) > 0 {
				parts = append(parts, `"`+textutil.OneLine(strings.Join(g, " "), 70)+`"`)
			}
		}
		rows = append(rows, row{p, strings.Join(parts, "  "), date})
	}
	sort.SliceStable(rows, func(i, j int) bool { return rows[i].date > rows[j].date })
	for i, r := range rows {
		if i == limit {
			a.printf("  ... %d more: pg project list\n", len(rows)-limit)
			break
		}
		a.println(r.line)
	}
}

func (a *app) projectCmd() *cobra.Command {
	c := &cobra.Command{
		Use:   "project",
		Short: "Projects of the space: list status close (start/resume/plan/log/decide/checkpoint/done are top-level)",
	}
	list := &cobra.Command{
		Use:   "list",
		Short: "Projects in this space: notes, tasks, goal; * marks the current one",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			if len(s.v.Projects()) == 0 {
				a.println("no projects yet; pg start <name> --goal \"...\"")
				return errNoResults
			}
			a.printProjectList(s, 1000)
			return nil
		},
	}
	status := &cobra.Command{
		Use:   "status [project]",
		Short: "Goal, decisions, open tasks and handoff of a project (default: current); does not switch",
		Args:  cobra.ArbitraryArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			p := s.project
			if len(args) > 0 {
				if p, err = a.findProject(s, strings.Join(args, " ")); err != nil {
					return err
				}
			}
			if p == "" {
				if p, err = a.writeProject(s); err != nil {
					return err
				}
			}
			return a.printProject(s, p, false)
		},
	}
	var summary string
	closeC := &cobra.Command{
		Use:   "close [project] [--summary TEXT|-]",
		Short: "Close a project: summary to its journal, its decisions to decision records",
		Long: `Mark the project note closed, write the summary to the project's
journal and turn each line of its Decisions into a decision record
(<project>/decisions/), so pg context and pg related keep finding them.`,
		Example: `pg project close --summary "Stripe payments shipped; refunds left for later"`,
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
			p := s.project
			if len(args) > 0 {
				if p, err = a.findProject(s, strings.Join(args, " ")); err != nil {
					return err
				}
			}
			if p == "" {
				if p, err = a.writeProject(s); err != nil {
					return err
				}
			}
			f := projectNote(s, p)
			if f == nil {
				return fail(ExitNotFound, "project %s has no note to close", p)
			}
			ts, _ := s.db.Tasks(f.ID)
			tc := index.CountTasks(ts, 1, 1<<30)
			if summary == "" {
				summary = fmt.Sprintf("%d/%d tasks done", tc.Done, tc.Total)
			}
			decisions := a.sectionBody(s, f, "decisions")
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
				made := 0
				for _, d := range decisions {
					text := strings.TrimSpace(strings.TrimPrefix(strings.TrimSpace(d), "-"))
					if len(text) > 11 && isDatePrefix(text[:10]) {
						text = strings.TrimSpace(text[10:])
					}
					if text == "" || strings.HasPrefix(d, " ") {
						continue
					}
					if _, err := a.writeDecision(s, p, decisionTitle(text), "From project [["+p+"]].", text, "", ""); err != nil {
						return err
					}
					made++
				}
				j, err := a.journalAppend(s, p, fmt.Sprintf("project [[%s]] closed (%d/%d): %s", p, tc.Done, tc.Total, textutil.OneLine(summary, 400)))
				if err != nil {
					return err
				}
				if tc.Done < tc.Total {
					a.printf("note: %d open tasks stay in %s\n", tc.Total-tc.Done, f.Addr())
				}
				a.ok(fmt.Sprintf("%s [closed]  %d decision records", f.Addr(), made))
				a.ok(j)
				return nil
			})
		},
	}
	closeC.Flags().StringVar(&summary, "summary", "", "what was achieved (\"-\" reads stdin)")
	c.AddCommand(list, status, closeC)
	return c
}

// decisionTitle is a short title for a one-line decision.
func decisionTitle(text string) string {
	w := strings.Fields(text)
	if len(w) > 8 {
		w = w[:8]
	}
	return strings.TrimRight(strings.Join(w, " "), ".,;:")
}

func isDatePrefix(s string) bool {
	_, err := time.Parse("2006-01-02", s)
	return err == nil
}

// ---- spaces ----

func (a *app) spaceCmd() *cobra.Command {
	c := &cobra.Command{
		Use:   "space",
		Short: "Spaces (separate memories, like Obsidian vaults): list new use",
	}
	list := &cobra.Command{
		Use:   "list",
		Short: "Spaces with their project and note counts; * marks the one used here",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			for _, name := range space.List(a.home()) {
				mark := " "
				if name == s.v.Name {
					mark = "*"
				}
				sp, err := space.Open(a.home(), name)
				if err != nil {
					continue
				}
				ps := sp.Projects()
				a.printf("%s %s  %d projects%s\n", mark, name, len(ps), projectsHint(ps))
			}
			a.printf("home: %s\n", filepath.ToSlash(a.home()))
			return nil
		},
	}
	newC := &cobra.Command{
		Use:     "new <name>",
		Short:   "Create an empty space",
		Example: "pg space new work",
		Args:    cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			name := args[0]
			if space.Exists(a.home(), name) {
				return fail(ExitConflict, "space %s already exists; use it here: pg space use %s", name, name)
			}
			if _, err := space.Create(a.home(), name); err != nil {
				return fail(ExitUsage, "%v", err)
			}
			a.ok("space " + name)
			a.printf("next: pg space use %s (use it in this directory) | pg --space %s <command>\n", name, name)
			return nil
		},
	}
	use := &cobra.Command{
		Use:   "use <name>",
		Short: "Use this space in the current directory (the git work tree root) from now on",
		Long: `Map the current directory (the root of its git work tree) to a space;
pg run anywhere below it uses that space. "pg space use --default <name>"
changes the space used where nothing is mapped.`,
		Example: "pg space use work",
		Args:    cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			name := args[0]
			if !space.Exists(a.home(), name) {
				return fail(ExitNotFound, "no space %s (pg space list; pg space new %s)", name, name)
			}
			def, _ := cmd.Flags().GetBool("default")
			reg := space.LoadRegistry(a.home())
			if def {
				reg.Default = name
			} else {
				reg.Touch(a.workDir()).Space = name
			}
			if err := reg.Save(); err != nil {
				return fail(ExitInternal, "%v", err)
			}
			if def {
				a.ok("default space " + name)
			} else {
				a.ok(fmt.Sprintf("space %s for %s", name, filepath.ToSlash(a.workDir())))
			}
			return nil
		},
	}
	use.Flags().Bool("default", false, "set the default space instead")
	c.AddCommand(list, newC, use)
	return c
}

func projectsHint(ps []string) string {
	if len(ps) == 0 {
		return ""
	}
	if len(ps) > 6 {
		return ": " + strings.Join(ps[:6], ", ") + ", ..."
	}
	return ": " + strings.Join(ps, ", ")
}

// journalAppend adds "- HH:MM text" under today's heading in the project's
// journal/YYYY-MM.md and returns the day's address.
func (a *app) journalAppend(s *store, p, text string) (string, error) {
	return a.journalAppendAt(s, p, now(), text)
}

// journalAppendAt writes the entry under the day of when.
func (a *app) journalAppendAt(s *store, p string, when time.Time, text string) (string, error) {
	month := when.Format("2006-01")
	rel := "journal/" + month + ".md"
	if p != "" {
		rel = p + "/" + rel
	}
	day := when.Format("2006-01-02")
	entry := "- " + when.Format("15:04") + " " + text
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

var _ = os.Getenv

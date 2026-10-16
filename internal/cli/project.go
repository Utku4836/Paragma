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
	"github.com/Utku-Private/Paragma/internal/textutil"
	"github.com/Utku-Private/Paragma/internal/ulid"
	"github.com/Utku-Private/Paragma/internal/vault"
	"github.com/spf13/cobra"
)

func vaultRegistryName(s *store) string {
	if p := vault.LoadRegistry().ByPath(s.v.Root); p != nil {
		return p.Name
	}
	return ""
}

// ---- project ----

func (a *app) projectCmd() *cobra.Command {
	c := &cobra.Command{
		Use:   "project",
		Short: "Vaults you work in: new list switch info archive",
	}
	var path, lang string
	newC := &cobra.Command{
		Use:   "new <name>",
		Short: "Create a project vault (in --path, or ~/.paragma/projects/<name> for work without a repo) and make it active",
		Example: `pg project new blog --path ~/code/blog
pg project new thesis-notes`,
		Args: cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			name := args[0]
			reg := vault.LoadRegistry()
			if p := reg.Find(name); p != nil {
				return fail(ExitConflict, "project %s exists at %s (pg project switch %s)", name, filepath.ToSlash(p.Path), name)
			}
			var root string
			if path != "" {
				abs, err := filepath.Abs(path)
				if err != nil {
					return fail(ExitUsage, "%v", err)
				}
				root = filepath.Join(abs, vault.DirName)
			} else {
				root = filepath.Join(vault.GlobalRoot(), "projects", slug(name))
			}
			v, _, err := vault.Init(root, ulid.New, today(), lang)
			if err != nil {
				return fail(ExitInternal, "%v", err)
			}
			if path != "" {
				if _, err := vault.IgnoreInRepo(v.Base); err != nil {
					return fail(ExitInternal, ".gitignore: %v", err)
				}
			}
			p := reg.Register(name, v.Root)
			reg.Active = p.Name
			if err := reg.Save(); err != nil {
				return fail(ExitInternal, "%v", err)
			}
			a.ok(fmt.Sprintf("project %s at %s (active)", p.Name, filepath.ToSlash(v.Root)))
			return nil
		},
	}
	newC.Flags().StringVar(&path, "path", "", "project directory (the vault goes in <path>/.paragma)")
	newC.Flags().StringVar(&lang, "lang", "en", "section heading language: en | tr")

	list := &cobra.Command{
		Use:   "list",
		Short: "Registered projects: name, path, last used; * = active, > = the one in use here",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			reg := vault.LoadRegistry()
			if len(reg.Projects) == 0 {
				a.println("no results")
				return errNoResults
			}
			here := ""
			if v, err := vault.Find(a.vaultFlag, ""); err == nil {
				here = v.Root
			}
			for _, p := range reg.Projects {
				m := " "
				if strings.EqualFold(p.Name, reg.Active) {
					m = "*"
				}
				if here != "" && samePathCLI(p.Path, here) {
					m = ">"
				}
				missing := ""
				if _, err := os.Stat(filepath.Join(p.Path, "config.toml")); err != nil {
					missing = "[missing]"
				}
				a.println(join(m+" "+p.Name, filepath.ToSlash(p.Path), p.LastUsed, missing))
			}
			return nil
		},
	}
	switchC := &cobra.Command{
		Use:   "switch <name>",
		Short: "Make a project active (used whenever you are not inside a project directory)",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			reg := vault.LoadRegistry()
			p := reg.Find(args[0])
			if p == nil {
				var names []string
				for _, x := range reg.Projects {
					names = append(names, x.Name)
				}
				return fail(ExitNotFound, "no project %s (known: %s)", args[0], strings.Join(names, ", "))
			}
			reg.Active = p.Name
			p.LastUsed = today()
			if err := reg.Save(); err != nil {
				return fail(ExitInternal, "%v", err)
			}
			a.ok("project " + p.Name)
			if cwd, _ := os.Getwd(); cwd != "" {
				if v, err := vault.Find("", "project"); err == nil && !samePathCLI(v.Root, p.Path) {
					a.printf("note: inside %s the local vault %s still wins\n", filepath.ToSlash(cwd), filepath.ToSlash(v.Root))
				}
			}
			return nil
		},
	}
	info := &cobra.Command{
		Use:   "info",
		Short: "The project in use: path, branch, sessions, tasks, notes",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			branch, git := s.v.Branch()
			src := "pg"
			if git {
				src = "git"
			}
			files := s.res.Files()
			tcs, _ := s.db.TaskCountsByFile()
			open := 0
			for _, tc := range tcs {
				open += tc.Open
			}
			a.printf("project: %s  vault: %s\n", projectName(s), s.v.Display())
			a.printf("branch: %s (%s)  notes: %d  open tasks: %d\n", branch, src, len(files), open)
			if cur := a.currentSession(s); cur != nil {
				a.printf("current session: %s\n", cur.Addr())
			}
			var bs []string
			for _, b := range a.branches(s) {
				bs = append(bs, b.name)
			}
			if len(bs) > 0 {
				a.println("branches: " + strings.Join(bs, ", "))
			}
			return nil
		},
	}
	archive := &cobra.Command{
		Use:   "archive <name>",
		Short: "Drop a project from the registry (its files stay)",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			reg := vault.LoadRegistry()
			if !reg.Remove(args[0]) {
				return fail(ExitNotFound, "no project %s", args[0])
			}
			if err := reg.Save(); err != nil {
				return fail(ExitInternal, "%v", err)
			}
			a.ok("unregistered " + args[0])
			return nil
		},
	}
	c.AddCommand(newC, list, switchC, info, archive)
	return c
}

func samePathCLI(a, b string) bool {
	x, _ := filepath.Abs(a)
	y, _ := filepath.Abs(b)
	return strings.EqualFold(filepath.Clean(x), filepath.Clean(y))
}

// ---- branch ----

func branchRel(name string) string {
	return "work/branches/" + strings.ReplaceAll(slugBranch(name), "/", "--") + ".md"
}

func slugBranch(name string) string {
	var b strings.Builder
	for _, r := range strings.TrimSpace(name) {
		switch {
		case r == '/' || r == '-' || r == '_' || r == '.':
			b.WriteRune(r)
		case r == ' ':
			b.WriteRune('-')
		default:
			if s := slug(string(r)); s != "note" {
				b.WriteString(s)
			} else if r >= '0' && r <= '9' {
				b.WriteRune(r)
			}
		}
	}
	return b.String()
}

// branchFile returns the branch note for a branch, if there is one.
func (a *app) branchFile(s *store, name string) *index.File {
	for _, f := range s.res.Files() {
		if f.Type == "branch" && !f.Inactive() && (f.Branch == name || (f.Branch == "" && f.Path == branchRel(name))) {
			return f
		}
	}
	return nil
}

type branchInfo struct {
	name     string
	file     *index.File
	sessions []*index.File
	done     int
	total    int
}

// branches lists every branch pg knows about: branch notes, sessions'
// branches and the active branch.
func (a *app) branches(s *store) []*branchInfo {
	by := map[string]*branchInfo{}
	get := func(n string) *branchInfo {
		if by[n] == nil {
			by[n] = &branchInfo{name: n}
		}
		return by[n]
	}
	cur, _ := s.v.Branch()
	get(cur)
	tcs, _ := s.db.TaskCountsByFile()
	for _, f := range s.res.Files() {
		if f.Inactive() {
			continue
		}
		switch {
		case f.Type == "branch":
			n := f.Branch
			if n == "" {
				n = strings.ReplaceAll(strings.TrimSuffix(filepath.Base(f.Path), ".md"), "--", "/")
			}
			b := get(n)
			b.file = f
			b.done += tcs[f.ID].Done
			b.total += tcs[f.ID].Total
		case isSession(f) && f.Status != "closed":
			n := f.Branch
			if n == "" {
				n = "main"
			}
			b := get(n)
			b.sessions = append(b.sessions, f)
			b.done += tcs[f.ID].Done
			b.total += tcs[f.ID].Total
		}
	}
	var out []*branchInfo
	for _, b := range by {
		out = append(out, b)
	}
	sort.Slice(out, func(i, j int) bool { return out[i].name < out[j].name })
	return out
}

func (a *app) branchTemplate(s *store, name, goal string) []string {
	fm := []string{"---", "id: " + ulid.New(), "type: branch", "layer: working", "status: active", "title: " + yamlString(name), "branch: " + yamlString(name), "created: " + stamp(), "---"}
	lines := append(fm, "# "+name, "", "## "+a.sec(s, "bgoal"))
	if goal != "" {
		lines = append(lines, splitText(goal)...)
	}
	for _, k := range []string{"status", "decisions", "questions", "tasks"} {
		lines = append(lines, "", "## "+a.sec(s, k))
	}
	return lines
}

// ensureBranchFile returns the branch note, creating it if needed.
func (a *app) ensureBranchFile(s *store, name, goal string) (*index.File, bool, error) {
	if f := a.branchFile(s, name); f != nil {
		return f, false, nil
	}
	n := newNote(s, freePath(s, branchRel(name)), a.branchTemplate(s, name, goal))
	if err := a.save(n); err != nil {
		return nil, false, err
	}
	if a.dryRun {
		return nil, true, nil
	}
	f, err := s.res.ResolveFile(n.addr())
	return f, true, err
}

func (a *app) branchCmd() *cobra.Command {
	c := &cobra.Command{
		Use:   "branch",
		Short: "Work streams inside a project (follow git branches): new list switch status merge close",
	}
	var goal string
	newC := &cobra.Command{
		Use:     "new <name>",
		Short:   "Create a branch note (Goal, Status, Decisions, Open questions, Tasks); without git it also becomes active",
		Example: `pg branch new "payment-flow" --goal "Card payments via Stripe"`,
		Args:    cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			name := args[0]
			return a.withLock(s, func() error {
				f, created, err := a.ensureBranchFile(s, name, goal)
				if err != nil {
					return err
				}
				if !created {
					return fail(ExitConflict, "branch %s already has a note: %s", name, f.Addr())
				}
				_, git := s.v.Branch()
				if !git && !a.dryRun {
					st := s.v.LoadState()
					st.Branch = name
					if err := s.v.SaveState(st); err != nil {
						return fail(ExitInternal, "%v", err)
					}
				}
				a.ok(strings.TrimSuffix(branchRel(name), ".md"))
				if git {
					a.printf("note: git decides the active branch here; run git switch -c %s to work on it\n", name)
				}
				return nil
			})
		},
	}
	newC.Flags().StringVar(&goal, "goal", "", "what the branch is for")

	list := &cobra.Command{
		Use:   "list",
		Short: "Branches with open tasks and active sessions; * = active",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			cur, _ := s.v.Branch()
			for _, b := range a.branches(s) {
				m := " "
				if b.name == cur {
					m = "*"
				}
				note := ""
				if b.file != nil {
					note = b.file.Addr()
				}
				a.println(join(m+" "+b.name, fmt.Sprintf("tasks %d/%d", b.done, b.total), fmt.Sprintf("sessions %d", len(b.sessions)), note))
			}
			return nil
		},
	}
	switchC := &cobra.Command{
		Use:   "switch <name>",
		Short: "Change the active branch (only when git does not decide it)",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			if b, git := s.v.Branch(); git {
				return fail(ExitValidation, "git decides the branch here (now %s); run git switch %s", b, args[0])
			}
			st := s.v.LoadState()
			st.Branch = args[0]
			if err := s.v.SaveState(st); err != nil {
				return fail(ExitInternal, "%v", err)
			}
			a.ok("branch " + args[0])
			return nil
		},
	}
	status := &cobra.Command{
		Use:   "status [name]",
		Short: "Goal, progress, open questions, recent decisions and sessions of a branch",
		Args:  cobra.MaximumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			name, git := s.v.Branch()
			if len(args) == 1 {
				name, git = args[0], false
			}
			var b *branchInfo
			for _, x := range a.branches(s) {
				if x.name == name {
					b = x
				}
			}
			if b == nil {
				return fail(ExitNotFound, "no branch %s (pg branch list)", name)
			}
			src := ""
			if git {
				src = " (git)"
			}
			a.printf("branch: %s%s  tasks %d/%d  sessions %d\n", name, src, b.done, b.total, len(b.sessions))
			if b.file != nil {
				for _, part := range []struct{ key, label string }{{"bgoal", "goal"}, {"status", "status"}, {"questions", "open questions"}, {"decisions", "decisions"}} {
					if body := a.sectionBody(s, b.file, part.key); len(body) > 0 {
						a.println(part.label + ": " + textutil.OneLine(strings.Join(body, " "), 240))
					}
				}
			} else {
				a.printf("no branch note yet (pg branch new %s --goal ...)\n", name)
			}
			for _, f := range b.sessions {
				a.println("  session " + f.Addr())
			}
			return nil
		},
	}
	var into string
	merge := &cobra.Command{
		Use:     "merge <name> [--into main]",
		Short:   "Merge a branch's memory: summary to journal, decisions to episodes/decisions, open tasks to the target branch, notes to archive",
		Example: "pg branch merge feat/auth --into main",
		Args:    cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			return a.withLock(s, func() error { return a.mergeBranch(s, args[0], into, false) })
		},
	}
	merge.Flags().StringVar(&into, "into", "main", "branch that receives open tasks")
	var discard bool
	closeC := &cobra.Command{
		Use:     "close <name> --discard",
		Short:   "Abandon a branch: journal line, its notes and sessions to archive/",
		Example: "pg branch close spike-redis --discard",
		Args:    cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			if !discard {
				return fail(ExitUsage, "close throws the branch's open work away; pass --discard, or keep it with pg branch merge %s", args[0])
			}
			s, err := a.main()
			if err != nil {
				return err
			}
			return a.withLock(s, func() error { return a.mergeBranch(s, args[0], "", true) })
		},
	}
	closeC.Flags().BoolVar(&discard, "discard", false, "confirm abandoning the branch")
	c.AddCommand(newC, list, switchC, status, merge, closeC)
	return c
}

// mergeBranch implements branch merge (discard=false) and branch close
// --discard.
func (a *app) mergeBranch(s *store, name, into string, discard bool) error {
	var b *branchInfo
	for _, x := range a.branches(s) {
		if x.name == name {
			b = x
		}
	}
	if b == nil || (b.file == nil && len(b.sessions) == 0) {
		return fail(ExitNotFound, "no branch %s with notes or sessions (pg branch list)", name)
	}
	if !discard && into == name {
		return fail(ExitUsage, "cannot merge %s into itself", name)
	}
	if a.dryRun {
		a.printf("would %s branch %s: %d sessions, tasks %d/%d\n", map[bool]string{true: "discard", false: "merge"}[discard], name, len(b.sessions), b.done, b.total)
		return nil
	}
	goal := ""
	var decisions []string
	if b.file != nil {
		goal = strings.Join(a.sectionBody(s, b.file, "bgoal"), " ")
		for _, l := range a.sectionBody(s, b.file, "decisions") {
			if t := strings.TrimSpace(l); strings.HasPrefix(t, "- ") || strings.HasPrefix(t, "* ") {
				decisions = append(decisions, strings.TrimSpace(t[2:]))
			}
		}
	}
	var report []string
	if discard {
		j, err := a.journalAppend(s, fmt.Sprintf("branch %s discarded (%d/%d tasks done). %s", name, b.done, b.total, goal))
		if err != nil {
			return err
		}
		report = append(report, "journal "+j)
	} else {
		// open tasks move to the target branch note
		var moved []string
		srcs := append([]*index.File{}, b.sessions...)
		if b.file != nil {
			srcs = append(srcs, b.file)
		}
		for _, f := range srcs {
			n, err := a.load(s, f)
			if err != nil {
				return err
			}
			var keep []string
			for i := 0; i < len(n.lines); i++ {
				tl, ok := md.ParseTaskLine(n.lines[i])
				if !ok || closedState(tl.State) || tl.Indent != "" {
					keep = append(keep, n.lines[i])
					continue
				}
				moved = append(moved, strings.TrimLeft(n.lines[i], " \t"))
				for i+1 < len(n.lines) && (strings.HasPrefix(n.lines[i+1], " ") || strings.HasPrefix(n.lines[i+1], "\t")) && !isBlank(n.lines[i+1]) {
					i++
					moved = append(moved, n.lines[i])
				}
			}
			n.lines = keep
			if err := a.save(n); err != nil {
				return err
			}
		}
		if len(moved) > 0 {
			tf, _, err := a.ensureBranchFile(s, into, "")
			if err != nil {
				return err
			}
			tn, err := a.load(s, tf)
			if err != nil {
				return err
			}
			a.appendToSection(tn, tf, "tasks")(moved)
			if err := a.save(tn); err != nil {
				return err
			}
			report = append(report, fmt.Sprintf("%d tasks -> %s", countTaskLines(moved), tf.Addr()))
		}
		if len(decisions) > 0 {
			title := "Branch " + name
			rel := freePath(s, fmt.Sprintf("episodes/decisions/%04d-%s.md", nextDecisionNumber(s), slug(name)))
			lines := a.template(s, "decision", fmt.Sprintf("%s: %s", strings.TrimSuffix(filepath.Base(rel), ".md")[:4], title), nil, "")
			lines = fillSection(lines, "## "+a.sec(s, "context"), []string{textutil.OneLine(goal, 400) + " (merged from branch " + name + " into " + into + ")"})
			var ds []string
			for _, d := range decisions {
				ds = append(ds, "- "+d)
			}
			lines = fillSection(lines, "## "+a.sec(s, "decision"), ds)
			n := newNote(s, rel, lines)
			if err := a.save(n); err != nil {
				return err
			}
			report = append(report, "decision "+n.addr())
		}
		j, err := a.journalAppend(s, fmt.Sprintf("branch %s merged into %s (%d/%d tasks done). %s", name, into, b.done, b.total, goal))
		if err != nil {
			return err
		}
		report = append(report, "journal "+j)
	}
	// archive the branch note and its sessions
	archived := 0
	var paths []string
	for _, f := range b.sessions {
		paths = append(paths, f.Addr())
	}
	if b.file != nil {
		paths = append(paths, b.file.Addr())
	}
	for _, p := range paths {
		f, err := s.res.ResolveFile(p)
		if err != nil {
			continue
		}
		if err := a.archiveQuiet(s, f); err != nil {
			return err
		}
		archived++
	}
	report = append(report, fmt.Sprintf("%d notes archived", archived))
	st := s.v.LoadState()
	delete(st.Sessions, name)
	if st.Branch == name {
		st.Branch = into
	}
	s.v.SaveState(st)
	verb := "merged"
	if discard {
		verb = "discarded"
	}
	a.ok(fmt.Sprintf("%s %s: %s", verb, name, strings.Join(report, ", ")))
	return nil
}

func countTaskLines(ls []string) int {
	n := 0
	for _, l := range ls {
		if _, ok := md.ParseTaskLine(l); ok {
			n++
		}
	}
	return n
}

// fillSection puts body right under the heading line in lines.
func fillSection(lines []string, heading string, body []string) []string {
	for i, l := range lines {
		if l == heading {
			out := append([]string(nil), lines[:i+1]...)
			out = append(out, body...)
			return append(out, lines[i+1:]...)
		}
	}
	return lines
}

// archiveQuiet archives without printing (used by merges).
func (a *app) archiveQuiet(s *store, f *index.File) error {
	saved := a.stdout
	var sink strings.Builder
	a.stdout = newDiscard(&sink)
	err := a.archiveFile(s, f)
	a.stdout.Flush()
	a.stdout = saved
	return err
}

var _ = time.Now

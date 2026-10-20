package cli

import (
	"errors"
	"fmt"
	"path"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"time"

	"github.com/Utku4836/Paragma/internal/addr"
	"github.com/Utku4836/Paragma/internal/index"
	"github.com/Utku4836/Paragma/internal/space"
	"github.com/Utku4836/Paragma/internal/textutil"
	"github.com/Utku4836/Paragma/internal/ulid"
	"github.com/spf13/cobra"
)

// Space administration: status, reindex, doctor and the space commands.

func (a *app) statusCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "status",
		Short: "Space, current project, counts and index health",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			files := s.res.Files()
			layers := map[string]int{}
			tokens := 0
			fmErr := 0
			for _, f := range files {
				layers[f.Layer]++
				tokens += f.Tokens
				if f.FMError != "" {
					fmErr++
				}
			}
			db := s.db
			secs := db.Count(`SELECT COUNT(*) FROM sections`)
			tasksAll := db.Count(`SELECT COUNT(*) FROM tasks`)
			tasksOpen := db.Count(`SELECT COUNT(*) FROM tasks WHERE state NOT IN ('x','X','-')`)
			links, _ := db.AllLinks()
			broken := 0
			for _, l := range links {
				if !linkResolves(s.res, l) {
					broken++
				}
			}
			if a.jsonOut {
				return a.emitJSON(map[string]any{
					"space": s.v.Name, "root": s.v.Root, "project": s.project, "projects": s.v.Projects(), "files": len(files), "layers": layers,
					"sections": secs, "tokens": tokens, "tasks_open": tasksOpen, "tasks": tasksAll,
					"links": len(links), "broken_links": broken, "frontmatter_errors": fmErr,
				})
			}
			cur := s.project
			if cur == "" {
				cur = "none (pg resume <project>)"
			}
			a.printf("space %s  %s  projects %d  current project here: %s\n", s.v.Name, filepath.ToSlash(s.v.Root), len(s.v.Projects()), cur)
			a.printf("files %d (working %d, episodic %d, semantic %d, other %d)  sections %d  %s\n",
				len(files), layers["working"], layers["episodic"], layers["semantic"],
				len(files)-layers["working"]-layers["episodic"]-layers["semantic"], secs, textutil.FmtTokens(tokens))
			a.printf("tasks %d open / %d  links %d (%d broken)  frontmatter errors %d\n", tasksOpen, tasksAll, len(links), broken, fmErr)
			return nil
		},
	}
}

func (a *app) reindexCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "reindex",
		Short: "Rebuild the space's index from the Markdown files",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			v, err := space.Find(a.home(), a.spaceFlag, a.workDir())
			if err != nil {
				if errors.Is(err, space.ErrNoSpace) {
					return fail(ExitNoSpace, "%v", err)
				}
				return fail(ExitInternal, "%v", err)
			}
			start := time.Now()
			db, err := index.Reset(v)
			if err != nil {
				return fail(ExitInternal, "%v", err)
			}
			defer db.Close()
			st, err := db.Sync()
			if err != nil {
				return fail(ExitInternal, "sync: %v", err)
			}
			secs := db.Count(`SELECT COUNT(*) FROM sections`)
			if a.jsonOut {
				return a.emitJSON(map[string]any{"space": v.Name, "files": st.Files, "sections": secs, "ms": time.Since(start).Milliseconds()})
			}
			a.printf("ok reindexed space %s: %d files, %d sections in %dms\n", v.Name, st.Files, secs, time.Since(start).Milliseconds())
			return nil
		},
	}
}

func linkResolves(r *addr.Resolver, l index.LinkRow) bool {
	p, err := addr.Parse(l.Target)
	if err != nil {
		return false
	}
	if p.File == "" && p.ULID == "" && p.Block == "" {
		return false
	}
	if p.File == "" && p.ULID == "" {
		// same-file reference: [[#Heading]] or [[^blk]]
		f, err := r.DB.FileByID(l.SrcFile)
		if err != nil {
			return false
		}
		p.File = f.Addr()
	}
	_, err = r.Resolve(p)
	return err == nil
}

func (a *app) doctorCmd() *cobra.Command {
	var fix bool
	c := &cobra.Command{
		Use:   "doctor [--fix]",
		Short: "Check the space: front matter errors, duplicate ids, broken links, missing ids",
		Long: `List problems in the space, one per line with its address. --fix adds
missing ids. Exit 6 when problems remain.`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			var problems []string
			var missing []*index.File
			ids := map[string][]string{}
			for _, f := range s.res.Files() {
				if f.FMError != "" {
					problems = append(problems, fmt.Sprintf("%s  front matter does not parse: %s", f.Addr(), textutil.OneLine(f.FMError, 120)))
				}
				if f.ULID != "" {
					ids[f.ULID] = append(ids[f.ULID], f.Addr())
				} else if f.Type != "journal" && f.FMError == "" {
					missing = append(missing, f)
				}
			}
			for id, as := range ids {
				if len(as) > 1 {
					problems = append(problems, fmt.Sprintf("@%s  same id in %s", id, strings.Join(as, ", ")))
				}
			}
			var names []string
			for _, f := range s.res.Files() {
				names = append(names, path.Base(f.Addr()))
			}
			links, _ := s.db.AllLinks()
			for _, l := range links {
				if linkResolves(s.res, l) {
					continue
				}
				if src, err := s.db.FileByID(l.SrcFile); err == nil {
					p := fmt.Sprintf("%s:%d  broken link [[%s]]", src.Addr(), l.Line, l.Target)
					if c := textutil.Closest(strings.SplitN(l.Target, "#", 2)[0], names, 1); len(c) > 0 {
						p += " (did you mean " + c[0] + "?)"
					}
					problems = append(problems, p)
				}
			}
			// code references to files git has renamed since the note was written
			type stale struct {
				f         *index.File
				line      int
				old, path string
			}
			var stales []stale
			if moved := a.renames(); len(moved) > 0 {
				refs, _ := s.db.CodeRefs()
				for _, r := range refs {
					if cur, ok := moved[r.Ref]; ok {
						if f, err := s.db.FileByID(r.FileID); err == nil && !f.Inactive() {
							stales = append(stales, stale{f, r.Line, r.Ref, cur})
						}
					}
				}
			}
			if fix && len(stales) > 0 {
				err := a.withLock(s, func() error {
					byFile := map[int64][]stale{}
					for _, x := range stales {
						byFile[x.f.ID] = append(byFile[x.f.ID], x)
					}
					for _, xs := range byFile {
						g, err := s.res.ResolveFile(xs[0].f.Addr())
						if err != nil {
							continue
						}
						n, err := a.load(s, g)
						if err != nil {
							return err
						}
						for _, x := range xs {
							if x.line >= 1 && x.line <= len(n.lines) {
								re := regexp.MustCompile("(?i)" + regexp.QuoteMeta(x.old))
								n.lines[x.line-1] = re.ReplaceAllString(n.lines[x.line-1], x.path)
							}
						}
						if err := a.save(n); err != nil {
							return err
						}
					}
					return nil
				})
				if err != nil {
					return err
				}
				a.ok(fmt.Sprintf("updated %d references to renamed files", len(stales)))
				stales = nil
			}
			for _, x := range stales {
				problems = append(problems, fmt.Sprintf("%s:%d  names %s, renamed to %s (pg doctor --fix updates it)", x.f.Addr(), x.line, x.old, x.path))
			}
			if fix && len(missing) > 0 {
				err := a.withLock(s, func() error {
					for _, f := range missing {
						n, err := a.load(s, f)
						if err != nil {
							return err
						}
						n.lines = setFront(n.lines, "id", ulid.New())
						if err := a.save(n); err != nil {
							return err
						}
					}
					return nil
				})
				if err != nil {
					return err
				}
				a.ok(fmt.Sprintf("added ids to %d notes", len(missing)))
				missing = nil
			}
			for _, f := range missing {
				problems = append(problems, fmt.Sprintf("%s  no id (pg doctor --fix adds one)", f.Addr()))
			}
			sort.Strings(problems)
			if len(problems) == 0 {
				a.println("ok no problems")
				return nil
			}
			bw := a.newBudget(a.budgetOr(s.v.Cfg.Budget))
			for i, p := range problems {
				if !bw.line(p) {
					a.printf("... %d more\n", len(problems)-i)
					break
				}
			}
			return &Error{Code: ExitValidation}
		},
	}
	c.Flags().BoolVar(&fix, "fix", false, "add missing ids")
	return c
}

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

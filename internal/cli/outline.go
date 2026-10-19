package cli

import (
	"fmt"
	"sort"
	"strings"

	"github.com/Utku4836/Paragma/internal/addr"
	"github.com/Utku4836/Paragma/internal/index"
	"github.com/Utku4836/Paragma/internal/space"
	"github.com/Utku4836/Paragma/internal/textutil"
	"github.com/spf13/cobra"
)

// Maps: the notes of the space, the headings of a note.

func (a *app) outlineCmd() *cobra.Command {
	var depth int
	c := &cobra.Command{
		Use:     "outline [addr]",
		Aliases: []string{"ls", "map", "toc"},
		Short:   "Map of the space (notes by project), or the heading skeleton of one note",
		Long: `Without an address: a map of every note by layer with its token cost.
With an address: the heading skeleton of a note (or of one heading's
subtree) with the cost of reading each heading. Use it before pg read on
anything large.`,
		Example: "pg outline\npg outline architecture\npg outline architecture#Katmanlar --depth 3",
		Args:    cobra.ArbitraryArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			if len(args) == 0 {
				return a.spaceMap()
			}
			s, t, err := a.resolve(strings.Join(args, " ")) // an unquoted "note#Two words" arrives split
			if err != nil {
				return err
			}
			if t.Kind == addr.KindBlock || t.Kind == addr.KindLines {
				return fail(ExitUsage, "outline works on notes and headings; use: pg read %s", args[0])
			}
			return a.outline(s, t, depth)
		},
	}
	c.Flags().IntVar(&depth, "depth", 0, "deepest heading level to show (0 = all)")
	return c
}

type outlineRow struct {
	Addr   string `json:"addr"`
	Level  int    `json:"level"`
	Tokens int    `json:"tokens"`
	Lines  string `json:"lines"`
	Kinds  string `json:"kinds,omitempty"`
	Tasks  string `json:"tasks,omitempty"`
}

func (a *app) outline(s *store, t *addr.Target, depth int) error {
	f := t.File
	secs, err := s.db.Sections(f.ID)
	if err != nil {
		return fail(ExitInternal, "%v", err)
	}
	tasks, _ := s.db.Tasks(f.ID)
	var rows []outlineRow
	var shown []*index.Section
	for _, sec := range secs {
		if t.Kind == addr.KindSection && (sec.Line < t.Section.Line || sec.Line > t.Section.End) {
			continue
		}
		if depth > 0 && sec.Level > depth {
			continue
		}
		shown = append(shown, sec)
		r := outlineRow{Addr: f.Addr(), Level: sec.Level, Tokens: sec.SubTokens, Lines: fmt.Sprintf("%d-%d", sec.Line, sec.End)}
		if sec.Path != "" {
			r.Addr += "#" + sec.Path
		}
		if sec.Kinds != "" {
			r.Kinds = "[!" + strings.ReplaceAll(sec.Kinds, " ", "] [!") + "]"
		}
		if tc := index.CountTasks(tasks, sec.Line, sec.End); tc.Total > 0 {
			r.Tasks = fmt.Sprintf("tasks %d/%d", tc.Done, tc.Total)
		}
		rows = append(rows, r)
	}
	total := f.Tokens
	if t.Kind == addr.KindSection {
		total = t.Section.SubTokens
	}
	if a.jsonOut {
		return a.emitJSON(map[string]any{"addr": t.Addr(), "title": f.Title, "layer": f.Layer, "type": f.Type,
			"status": f.Status, "date": f.Date(), "tokens": total, "headings": rows})
	}
	date := ""
	if f.Verified != "" {
		date = "verified " + f.Date()
	} else if d := f.Date(); d != "" {
		date = d
	}
	a.println(join(t.Addr(), a.fileMeta(s, f), date, textutil.FmtTokens(total)))
	bw := a.newBudget(a.budgetOr(0))
	for i, r := range rows {
		sec := shown[i]
		var label string
		if sec.Level == 0 {
			label = fmt.Sprintf("(intro) %s:%s", f.Addr(), r.Lines)
		} else {
			label = strings.Repeat("#", sec.Level) + " " + sec.Path
		}
		if !bw.line(join(label, textutil.FmtTokens(r.Tokens), r.Kinds, r.Tasks)) {
			a.printf("... %d more headings; narrow with: pg outline %s#<Heading> or --depth 2\n", len(rows)-i, f.Addr())
			break
		}
	}
	if len(rows) == 0 {
		a.println("(no headings)")
	}
	return nil
}

// spaceMap prints every note grouped by project, the current project first.
// When that does not fit the budget it falls back to one line per folder.
func (a *app) spaceMap() error {
	s, err := a.main()
	if err != nil {
		return err
	}
	files := s.res.Files()
	tcs, err := s.db.TaskCountsByFile()
	if err != nil {
		return fail(ExitInternal, "%v", err)
	}
	total := 0
	for _, f := range files {
		total += f.Tokens
	}
	if a.jsonOut {
		var out []map[string]any
		for _, f := range files {
			out = append(out, map[string]any{"addr": f.Addr(), "project": space.ProjectOf(f.Path), "title": f.Title, "layer": f.Layer, "type": f.Type,
				"status": f.Status, "tokens": f.Tokens, "tasks_open": tcs[f.ID].Open})
		}
		return a.emitJSON(map[string]any{"space": s.v.Name, "notes": len(files), "tokens": total, "files": out})
	}
	if len(files) == 0 {
		a.printf("space %s is empty; start a project: pg start <name> --goal \"...\"\n", s.v.Name)
		return nil
	}

	byProject := map[string][]*index.File{}
	for _, f := range files {
		byProject[space.ProjectOf(f.Path)] = append(byProject[space.ProjectOf(f.Path)], f)
	}
	order := s.v.Projects()
	sort.SliceStable(order, func(i, j int) bool { return order[i] == s.project && order[j] != s.project })
	order = append(order, "")
	var lines []string
	for _, p := range order {
		fs := byProject[p]
		if len(fs) == 0 {
			continue
		}
		name := p + "/"
		if p == "" {
			name = "(space root)"
		} else if p == s.project {
			name += "  (current)"
		}
		lines = append(lines, name)
		for _, f := range fs {
			typ := f.Type
			if typ == "note" {
				typ = ""
			}
			if f.Inactive() {
				typ = join(typ, "[inactive]")
			}
			lines = append(lines, "  "+join(f.Addr(), typ, textutil.FmtTokens(f.Tokens), taskCount(tcs[f.ID]), titleIfDifferent(f)))
		}
	}
	a.printf("space %s  %d notes  %s\n", s.v.Name, len(files), textutil.FmtTokens(total))
	budget := a.budgetOr(s.v.Cfg.Budget)
	cost := 0
	for _, l := range lines {
		cost += textutil.Tokens(l) + 1
	}
	if cost <= budget {
		for _, l := range lines {
			a.println(l)
		}
		a.println("next: pg outline <note> | pg context <topic> | pg read <addr>")
		return nil
	}

	// too many notes: summarise per folder
	type dir struct {
		notes, tokens, open int
	}
	dirs := map[string]*dir{}
	for _, f := range files {
		d := "."
		if i := strings.LastIndex(f.Path, "/"); i >= 0 {
			d = f.Path[:i]
		}
		if dirs[d] == nil {
			dirs[d] = &dir{}
		}
		dirs[d].notes++
		dirs[d].tokens += f.Tokens
		dirs[d].open += tcs[f.ID].Open
	}
	names := make([]string, 0, len(dirs))
	for d := range dirs {
		names = append(names, d)
	}
	sort.Strings(names)
	bw := a.newBudget(budget)
	for _, d := range names {
		x := dirs[d]
		open := ""
		if x.open > 0 {
			open = fmt.Sprintf("open tasks %d", x.open)
		}
		if !bw.line(join(d+"/", fmt.Sprintf("%d notes", x.notes), textutil.FmtTokens(x.tokens), open)) {
			break
		}
	}
	a.println("next: pg find --type <t> | pg context <topic> | pg search <words>")
	return nil
}

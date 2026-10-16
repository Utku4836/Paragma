package cli

import (
	"fmt"
	"sort"
	"strings"

	"github.com/Utku-Private/Paragma/internal/index"
	"github.com/Utku-Private/Paragma/internal/space"
	"github.com/Utku-Private/Paragma/internal/textutil"
)

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
			tasks := ""
			if tc := tcs[f.ID]; tc.Total > 0 {
				tasks = fmt.Sprintf("tasks %d/%d", tc.Done, tc.Total)
			}
			typ := f.Type
			if typ == "note" {
				typ = ""
			}
			if f.Inactive() {
				typ = join(typ, "[inactive]")
			}
			title := ""
			base := f.Addr()[strings.LastIndex(f.Addr(), "/")+1:]
			if textutil.Fold(f.Title) != textutil.Fold(base) {
				title = `"` + f.Title + `"`
			}
			lines = append(lines, "  "+join(f.Addr(), typ, textutil.FmtTokens(f.Tokens), tasks, title))
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

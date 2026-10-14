package cli

import (
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/Utku-Private/Paragma/internal/index"
	"github.com/Utku-Private/Paragma/internal/textutil"
)

// notePath is the note's file path as the agent's shell sees it: relative
// to the working directory when possible, so it can be cited or opened.
func notePath(s *store, f *index.File) string {
	p := filepath.Join(s.v.Root, filepath.FromSlash(f.Path))
	if cwd, err := os.Getwd(); err == nil {
		if rel, err := filepath.Rel(cwd, p); err == nil && !strings.HasPrefix(rel, "..") {
			return filepath.ToSlash(rel)
		}
	}
	return filepath.ToSlash(p)
}

// vaultMap prints every note grouped by layer. When that does not fit the
// budget it falls back to one line per directory.
func (a *app) vaultMap() error {
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
			out = append(out, map[string]any{"addr": f.Addr(), "title": f.Title, "layer": f.Layer, "type": f.Type,
				"status": f.Status, "tokens": f.Tokens, "tasks_open": tcs[f.ID].Open})
		}
		return a.emitJSON(map[string]any{"vault": s.v.Root, "notes": len(files), "tokens": total, "files": out})
	}
	if len(files) == 0 {
		a.printf("vault %s is empty; add notes under knowledge/, episodes/ or work/\n", s.v.Display())
		return nil
	}

	byLayer := map[string][]*index.File{}
	for _, f := range files {
		l := f.Layer
		if f.Inactive() {
			l = "inactive"
		}
		byLayer[l] = append(byLayer[l], f)
	}
	order := []string{"semantic", "episodic", "working", "inactive", ""}
	var lines []string
	for _, l := range order {
		fs := byLayer[l]
		if len(fs) == 0 {
			continue
		}
		name := l
		if name == "" {
			name = "other"
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
			title := ""
			base := f.Addr()[strings.LastIndex(f.Addr(), "/")+1:]
			if textutil.Fold(f.Title) != textutil.Fold(base) {
				title = `"` + f.Title + `"`
			}
			lines = append(lines, "  "+join(f.Addr(), typ, textutil.FmtTokens(f.Tokens), tasks, title))
		}
	}
	a.printf("vault %s  %d notes  %s\n", s.v.Display(), len(files), textutil.FmtTokens(total))
	budget := a.budgetOr(s.v.Cfg.Budget)
	cost := 0
	for _, l := range lines {
		cost += textutil.Tokens(l) + 1
	}
	if cost <= budget {
		for _, l := range lines {
			a.println(l)
		}
		a.println("next: pg outline <note> | pg search <words> | pg read <addr>")
		return nil
	}

	// too many notes: summarise per directory
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
	a.println("next: pg find --layer <l> | pg find --type <t> | pg search <words>")
	return nil
}

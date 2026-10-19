package cli

import (
	"fmt"
	"path"
	"regexp"
	"slices"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/Utku4836/Paragma/internal/errsig"
	"github.com/Utku4836/Paragma/internal/index"
	"github.com/Utku4836/Paragma/internal/space"
	"github.com/Utku4836/Paragma/internal/textutil"
	"github.com/spf13/cobra"
)

// Finding notes: full-text search, metadata filters, regular expressions.

type searchFlags struct {
	layers, types, statuses, tags, kinds []string
	all, raw                             bool
	limit, offset                        int
}

func (a *app) searchCmd() *cobra.Command {
	var sf searchFlags
	c := &cobra.Command{
		Use:     "search <query>",
		Aliases: []string{"query", "lookup"},
		Short:   "Section-level full-text search: address, snippet, token cost",
		Long: `Search notes at heading level. Each hit is an address you can pass to
pg read, a one-line snippet and the cost of reading it. Words are matched
case- and accent-insensitively; prefixes and code fragments (ECONNREFUSED,
busy_timeout) also match. Superseded/archived notes sink unless --all.
Exit code 1 with "no results" means memory has nothing on it.`,
		Example: `pg search "database locked"
pg search jwt refresh --layer semantic
pg search timeout --type error --tag sqlite`,
		Args: cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			return a.search(strings.Join(args, " "), sf)
		},
	}
	f := c.Flags()
	f.StringSliceVar(&sf.layers, "layer", nil, "working|episodic|semantic (comma list)")
	f.StringSliceVar(&sf.types, "type", nil, "note|rule|decision|error|project|concept|journal")
	f.StringSliceVar(&sf.statuses, "status", nil, "active|resolved|superseded|archived")
	f.StringSliceVar(&sf.tags, "tag", nil, "only notes with this tag")
	f.StringSliceVar(&sf.kinds, "kind", nil, "only sections holding this callout (rule, warning, decision...)")
	f.BoolVar(&sf.all, "all", false, "rank superseded and archived notes normally")
	f.BoolVar(&sf.raw, "fts", false, "pass the query to SQLite FTS5 unchanged (AND/OR/NOT, \"phrases\", prefix*)")
	f.IntVar(&sf.limit, "limit", 10, "max results")
	f.IntVar(&sf.offset, "offset", 0, "skip this many results (paging)")
	return c
}

func (a *app) search(q string, sf searchFlags) error {
	layers, err := normLayers(sf.layers)
	if err != nil {
		return err
	}
	s, err := a.main()
	if err != nil {
		return err
	}
	hits, err := s.db.Search(index.SearchOpts{
		Query: q, Layers: layers, Types: splitList(sf.types), Statuses: splitList(sf.statuses),
		Tags: splitList(sf.tags), Kinds: splitList(sf.kinds), All: sf.all, Raw: sf.raw,
	})
	if err != nil {
		if sf.raw {
			return fail(ExitUsage, "bad FTS5 query: %v", err)
		}
		return fail(ExitInternal, "%v", err)
	}
	for i := range hits {
		if s.project != "" && space.ProjectOf(hits[i].File.Path) == s.project {
			hits[i].Score *= 1.15 // the current project's notes first among equals
		}
	}
	sort.SliceStable(hits, func(i, j int) bool { return hits[i].Score > hits[j].Score })
	total := len(hits)
	hits = hits[min(sf.offset, len(hits)):]
	if sf.limit > 0 && len(hits) > sf.limit {
		hits = hits[:sf.limit]
	}
	if len(hits) == 0 {
		if sf.offset == 0 {
			recordMiss(s, "search", q)
		}
		if a.jsonOut {
			a.emitJSON([]any{})
		} else {
			a.println("no results")
			a.searchHint(s, q)
		}
		return errNoResults
	}
	if err := s.db.Hydrate(hits); err != nil {
		return fail(ExitInternal, "%v", err)
	}

	if a.jsonOut {
		var out []map[string]any
		for _, h := range hits {
			out = append(out, map[string]any{
				"addr": hitAddr(h), "type": h.File.Type, "layer": h.File.Layer, "status": h.File.Status,
				"date": h.File.Date(), "tokens": h.Sec.SubTokens, "snippet": textutil.OneLine(h.Snippet, 0),
				"score": float64(int(h.Score*100)) / 100, "match": h.Via, "stale": h.File.Stale(s.v.Cfg.StaleDays),
			})
		}
		return a.emitJSON(map[string]any{"total": total, "offset": sf.offset, "results": out})
	}

	bw := a.newBudget(a.budgetOr(s.v.Cfg.Budget))
	shown := 0
	for _, h := range hits {
		head := join(hitAddr(h), meta(append(noteFlags(s, h.File), bangKinds(h.Sec.Kinds))...), h.File.Date(), textutil.FmtTokens(h.Sec.SubTokens))
		snip := textutil.OneLine(h.Snippet, 160)
		cost := textutil.Tokens(head) + 1
		if snip != "" {
			cost += textutil.Tokens(snip) + 2
		}
		if !bw.fits(cost) && shown > 0 {
			break
		}
		bw.line(head)
		if snip != "" {
			bw.line("  " + snip)
		}
		shown++
	}
	next := sf.offset + shown
	switch {
	case next < total:
		a.printf("%d of %d shown; more: pg search %s --offset %d\n", shown, total, shellQuote(q), next)
	case total == 1:
		a.println("1 result")
	default:
		a.printf("%d results\n", total)
	}
	return nil
}

// bangKinds renders callout kinds inside a meta bracket: "!rule !warning".
func bangKinds(k string) string {
	if k == "" {
		return ""
	}
	return "!" + strings.ReplaceAll(k, " ", " !")
}

func hitAddr(h index.Hit) string {
	if h.Sec.Path == "" || h.Sec.IsTitle {
		return h.File.Addr()
	}
	return h.File.Addr() + "#" + h.Sec.Path
}

var layerAlias = map[string]string{"work": "working", "w": "working", "episode": "episodic", "episodes": "episodic", "e": "episodic", "knowledge": "semantic", "s": "semantic"}

func normLayers(ls []string) ([]string, error) {
	out := make([]string, 0, len(ls))
	for _, l := range splitList(ls) {
		l = strings.ToLower(l)
		if a, ok := layerAlias[l]; ok {
			l = a
		}
		switch l {
		case "working", "episodic", "semantic":
		default:
			return nil, fail(ExitUsage, "--layer must be working, episodic or semantic (got %q)", l)
		}
		out = append(out, l)
	}
	return out, nil
}

type findFlags struct {
	types, layers, statuses, tags []string
	since, until, title, sortBy   string
	openTasks                     bool
	limit                         int
}

func (a *app) findCmd() *cobra.Command {
	var ff findFlags
	c := &cobra.Command{
		Use:   "find",
		Short: "List notes by metadata: type, layer, tag, status, date",
		Long: `List notes by metadata instead of content. Dates compare against
updated/verified/created (frontmatter) or the file time; --since takes 7d,
2w, 3m or 2026-10-01.`,
		Example: `pg find --type decision --tag auth
pg find --layer working --open-tasks
pg find --since 7d --sort updated`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error { return a.find(ff) },
	}
	f := c.Flags()
	f.StringSliceVar(&ff.types, "type", nil, "note|rule|decision|error|project|concept|journal")
	f.StringSliceVar(&ff.layers, "layer", nil, "working|episodic|semantic")
	f.StringSliceVar(&ff.statuses, "status", nil, "active|resolved|superseded|archived")
	f.StringSliceVar(&ff.tags, "tag", nil, "has this tag (comma list = any)")
	f.StringVar(&ff.since, "since", "", "date on or after: 7d, 2w, 3m, 2026-10-01")
	f.StringVar(&ff.until, "until", "", "date on or before")
	f.StringVar(&ff.title, "title", "", "title or path contains this text")
	f.BoolVar(&ff.openTasks, "open-tasks", false, "only notes with open tasks")
	f.StringVar(&ff.sortBy, "sort", "path", "path|date|tokens")
	f.IntVar(&ff.limit, "limit", 30, "max results")
	return c
}

var reRel = regexp.MustCompile(`^(\d+)([dwmy])$`)

func parseDate(s string) (string, error) {
	if s == "" {
		return "", nil
	}
	if m := reRel.FindStringSubmatch(s); m != nil {
		n, _ := strconv.Atoi(m[1])
		t := time.Now()
		switch m[2] {
		case "d":
			t = t.AddDate(0, 0, -n)
		case "w":
			t = t.AddDate(0, 0, -7*n)
		case "m":
			t = t.AddDate(0, -n, 0)
		case "y":
			t = t.AddDate(-n, 0, 0)
		}
		return t.Format("2006-01-02"), nil
	}
	if _, err := time.Parse("2006-01-02", s); err != nil {
		return "", fail(ExitUsage, "bad date %q (use 7d, 2w, 3m or 2026-10-01)", s)
	}
	return s, nil
}

func (a *app) find(ff findFlags) error {
	layers, err := normLayers(ff.layers)
	if err != nil {
		return err
	}
	since, err := parseDate(ff.since)
	if err != nil {
		return err
	}
	until, err := parseDate(ff.until)
	if err != nil {
		return err
	}
	switch ff.sortBy {
	case "path", "date", "tokens":
	default:
		return fail(ExitUsage, "--sort must be path, date or tokens")
	}
	s, err := a.main()
	if err != nil {
		return err
	}
	set := func(xs []string) map[string]bool {
		m := map[string]bool{}
		for _, x := range splitList(xs) {
			m[strings.ToLower(strings.TrimPrefix(x, "#"))] = true
		}
		return m
	}
	types, stats, tags, lset := set(ff.types), set(ff.statuses), set(ff.tags), set(layers)
	ftags, err := s.db.FileTags()
	if err != nil {
		return fail(ExitInternal, "%v", err)
	}
	tcs, err := s.db.TaskCountsByFile()
	if err != nil {
		return fail(ExitInternal, "%v", err)
	}
	title := textutil.Fold(ff.title)
	var rows []*index.File
	for _, f := range s.res.Files() {
		if len(types) > 0 && !types[f.Type] || len(lset) > 0 && !lset[f.Layer] || len(stats) > 0 && !stats[f.Status] {
			continue
		}
		if len(tags) > 0 && !slices.ContainsFunc(ftags[f.ID], func(t string) bool { return tags[t] }) {
			continue
		}
		if d := f.Date(); since != "" && d < since || until != "" && (d == "" || d > until) {
			continue
		}
		if title != "" && !strings.Contains(textutil.Fold(f.Title), title) && !strings.Contains(textutil.Fold(f.Path), title) {
			continue
		}
		if ff.openTasks && tcs[f.ID].Open == 0 {
			continue
		}
		rows = append(rows, f)
	}
	switch ff.sortBy {
	case "date":
		sort.SliceStable(rows, func(i, j int) bool { return rows[i].Date() > rows[j].Date() })
	case "tokens":
		sort.SliceStable(rows, func(i, j int) bool { return rows[i].Tokens > rows[j].Tokens })
	}
	total := len(rows)
	if total == 0 {
		if a.jsonOut {
			a.emitJSON([]any{})
		} else {
			a.println("no results")
		}
		return errNoResults
	}
	if ff.limit > 0 && len(rows) > ff.limit {
		rows = rows[:ff.limit]
	}
	if a.jsonOut {
		var out []map[string]any
		for _, f := range rows {
			tc := tcs[f.ID]
			out = append(out, map[string]any{"addr": f.Addr(), "title": f.Title, "type": f.Type, "layer": f.Layer,
				"status": f.Status, "date": f.Date(), "tokens": f.Tokens, "tasks_open": tc.Open, "tasks": tc.Total})
		}
		return a.emitJSON(map[string]any{"total": total, "results": out})
	}
	bw := a.newBudget(a.budgetOr(s.v.Cfg.Budget))
	shown := 0
	for _, f := range rows {
		if !bw.line(join(f.Addr(), a.fileMeta(s, f), f.Date(), textutil.FmtTokens(f.Tokens), taskCount(tcs[f.ID]), titleIfDifferent(f))) {
			break
		}
		shown++
	}
	if shown < total {
		a.printf("%d of %d shown; narrow with --type/--tag/--since or raise --limit/--budget\n", shown, total)
	} else {
		a.printf("%d notes\n", total)
	}
	return nil
}

// taskCount renders "tasks 3/5", or "" for a note without tasks.
func taskCount(tc index.TaskCount) string {
	if tc.Total == 0 {
		return ""
	}
	return fmt.Sprintf("tasks %d/%d", tc.Done, tc.Total)
}

// titleIfDifferent is the note's quoted title when its file name does not
// already say it.
func titleIfDifferent(f *index.File) string {
	if textutil.Fold(f.Title) == textutil.Fold(path.Base(f.Addr())) {
		return ""
	}
	return `"` + f.Title + `"`
}

func (a *app) grepCmd() *cobra.Command {
	var ignoreCase, all bool
	var layers []string
	c := &cobra.Command{
		Use:   "grep <regex>",
		Short: "Raw regular expression search over note lines; prints note:line addresses",
		Long: `Search note text line by line with a Go regular expression. Use it for
exact strings (identifiers, config keys) where pg search's word matching is
too loose. Archived notes are skipped unless --all.`,
		Example: `pg grep "busy_timeout"
pg grep -i "ECONN\w+" --layer episodic`,
		Args: cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			pat := strings.Join(args, " ")
			if ignoreCase {
				pat = "(?i)" + pat
			}
			re, err := regexp.Compile(pat)
			if err != nil {
				return fail(ExitUsage, "bad regex: %v", err)
			}
			ls, err := normLayers(layers)
			if err != nil {
				return err
			}
			want := map[string]bool{}
			for _, l := range ls {
				want[l] = true
			}
			s, err := a.main()
			if err != nil {
				return err
			}
			bw := a.newBudget(a.budgetOr(s.v.Cfg.Budget))
			hits := 0
			for _, f := range s.res.Files() {
				if (!all && f.Inactive()) || (len(want) > 0 && !want[f.Layer]) {
					continue
				}
				lines, err := fileLines(s, f)
				if err != nil {
					continue
				}
				for i, l := range lines {
					if !re.MatchString(l) {
						continue
					}
					hits++
					if bw.cut {
						continue
					}
					if !bw.line(fmt.Sprintf("%s:%d  %s", f.Addr(), i+1, textutil.OneLine(l, 160))) {
						continue
					}
				}
			}
			if hits == 0 {
				a.println("no results")
				return errNoResults
			}
			if bw.cut {
				a.printf("... %d matches in all; narrow the pattern or --layer\n", hits)
			}
			return nil
		},
	}
	c.Flags().BoolVarP(&ignoreCase, "ignore-case", "i", false, "case-insensitive")
	c.Flags().BoolVar(&all, "all", false, "include archived and superseded notes")
	c.Flags().StringSliceVar(&layers, "layer", nil, "working|episodic|semantic")
	return c
}

// searchHint tells the agent how to try again after an empty search.
func (a *app) searchHint(s *store, q string) {
	if !s.v.Cfg.Hints {
		return
	}
	var tips []string
	if len(strings.Fields(q)) >= 3 {
		tips = append(tips, "fewer words")
	}
	tips = append(tips, "synonyms or the other language your notes use")
	if len(errsig.Lines(q, 1)) > 0 {
		tips = append(tips, "for an error message: pg error find")
	}
	line := "try: " + strings.Join(tips, "; ")
	if topics := a.topTags(s, 8); len(topics) > 0 {
		line += "; topics: " + strings.Join(topics, ", ")
	}
	a.println(line)
}

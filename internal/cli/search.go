package cli

import (
	"fmt"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/Utku-Private/Paragma/internal/index"
	"github.com/Utku-Private/Paragma/internal/space"
	"github.com/Utku-Private/Paragma/internal/textutil"
	"github.com/spf13/cobra"
)

func splitList(vals []string) []string {
	var out []string
	for _, v := range vals {
		for _, p := range strings.Split(v, ",") {
			if p = strings.TrimSpace(p); p != "" {
				out = append(out, p)
			}
		}
	}
	return out
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

// ---- search ----

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

type hitOut struct {
	index.Hit
	s      *store
	prefix string
	stale  bool
}

func (a *app) search(q string, sf searchFlags) error {
	layers, err := normLayers(sf.layers)
	if err != nil {
		return err
	}
	stores, err := a.allStores()
	if err != nil {
		return err
	}
	opts := index.SearchOpts{
		Query: q, Layers: layers, Types: splitList(sf.types), Statuses: splitList(sf.statuses),
		Tags: splitList(sf.tags), Kinds: splitList(sf.kinds), All: sf.all, Raw: sf.raw,
	}
	var hits []hitOut
	for _, s := range stores {
		hs, err := s.db.Search(opts)
		if err != nil {
			if sf.raw {
				return fail(ExitUsage, "bad FTS5 query: %v", err)
			}
			return fail(ExitInternal, "%v", err)
		}
		for _, h := range hs {
			if s.project != "" && space.ProjectOf(h.File.Path) == s.project {
				h.Score *= 1.15 // the current project's notes first among equals
			}
			hits = append(hits, hitOut{Hit: h, s: s, prefix: a.prefix(s), stale: h.File.Stale(s.v.Cfg.StaleDays)})
		}
	}
	sort.SliceStable(hits, func(i, j int) bool { return hits[i].Score > hits[j].Score })
	total := len(hits)
	if sf.offset > 0 {
		if sf.offset >= len(hits) {
			hits = nil
		} else {
			hits = hits[sf.offset:]
		}
	}
	if sf.limit > 0 && len(hits) > sf.limit {
		hits = hits[:sf.limit]
	}
	if len(hits) == 0 {
		if sf.offset == 0 {
			for _, st := range stores {
				recordMiss(st, "search", q)
			}
		}
		if a.jsonOut {
			a.emitJSON([]any{})
		} else {
			a.println("no results")
			a.searchHint(stores[0], q)
		}
		return errNoResults
	}
	for _, st := range stores {
		var batch []index.Hit
		var pos []int
		for i, h := range hits {
			if h.s == st {
				batch = append(batch, h.Hit)
				pos = append(pos, i)
			}
		}
		if err := st.db.Hydrate(batch); err != nil {
			return fail(ExitInternal, "%v", err)
		}
		for j, i := range pos {
			hits[i].Hit = batch[j]
		}
	}

	if a.jsonOut {
		var out []map[string]any
		for _, h := range hits {
			out = append(out, map[string]any{
				"addr": h.prefix + hitAddr(h.Hit), "type": h.File.Type, "layer": h.File.Layer, "status": h.File.Status,
				"date": h.File.Date(), "tokens": h.Sec.SubTokens, "snippet": textutil.OneLine(h.Snippet, 0),
				"score": float64(int(h.Score*100)) / 100, "match": h.Via, "stale": h.stale,
			})
		}
		return a.emitJSON(map[string]any{"total": total, "offset": sf.offset, "results": out})
	}

	bw := a.newBudget(a.budgetOr(a.defaultBudget(stores)))
	shown := 0
	for _, h := range hits {
		typ := h.File.Type
		if typ == "note" {
			typ = ""
		}
		st := h.File.Status
		if st == "active" {
			st = ""
		}
		seen := ""
		if h.File.Seen > 0 {
			seen = fmt.Sprintf("seen:%d", h.File.Seen)
		}
		stale := ""
		if h.stale {
			stale = "stale?"
		}
		head := join(h.prefix+hitAddr(h.Hit), meta(typ, st, seen, stale, bangKinds(h.Sec.Kinds)), h.File.Date(), textutil.FmtTokens(h.Sec.SubTokens))
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
	if next < total {
		a.printf("%d of %d shown; more: pg search %s --offset %d\n", shown, total, shellQuote(q), next)
	} else if total == 1 {
		a.println("1 result")
	} else {
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

func (a *app) defaultBudget(stores []*store) int {
	if len(stores) > 0 {
		return stores[0].v.Cfg.Budget
	}
	return 800
}

func hitAddr(h index.Hit) string {
	if h.Sec.Path == "" || h.Sec.IsTitle {
		return h.File.Addr()
	}
	return h.File.Addr() + "#" + h.Sec.Path
}

// ---- find ----

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
	stores, err := a.allStores()
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
	types, stats, tags := set(ff.types), set(ff.statuses), set(ff.tags)
	lset := map[string]bool{}
	for _, l := range layers {
		lset[l] = true
	}
	type row struct {
		f      *index.File
		s      *store
		prefix string
		tc     index.TaskCount
	}
	var rows []row
	for _, s := range stores {
		ftags, err := s.db.FileTags()
		if err != nil {
			return fail(ExitInternal, "%v", err)
		}
		tcs, err := s.db.TaskCountsByFile()
		if err != nil {
			return fail(ExitInternal, "%v", err)
		}
		for _, f := range s.res.Files() {
			if len(types) > 0 && !types[f.Type] || len(lset) > 0 && !lset[f.Layer] || len(stats) > 0 && !stats[f.Status] {
				continue
			}
			if len(tags) > 0 {
				ok := false
				for _, t := range ftags[f.ID] {
					if tags[t] {
						ok = true
						break
					}
				}
				if !ok {
					continue
				}
			}
			d := f.Date()
			if since != "" && d < since || until != "" && (d == "" || d > until) {
				continue
			}
			if ff.title != "" {
				q := textutil.Fold(ff.title)
				if !strings.Contains(textutil.Fold(f.Title), q) && !strings.Contains(textutil.Fold(f.Path), q) {
					continue
				}
			}
			tc := tcs[f.ID]
			if ff.openTasks && tc.Open == 0 {
				continue
			}
			rows = append(rows, row{f, s, a.prefix(s), tc})
		}
	}
	switch ff.sortBy {
	case "date":
		sort.SliceStable(rows, func(i, j int) bool { return rows[i].f.Date() > rows[j].f.Date() })
	case "tokens":
		sort.SliceStable(rows, func(i, j int) bool { return rows[i].f.Tokens > rows[j].f.Tokens })
	case "path", "":
	default:
		return fail(ExitUsage, "--sort must be path, date or tokens")
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
		for _, r := range rows {
			out = append(out, map[string]any{"addr": r.prefix + r.f.Addr(), "title": r.f.Title, "type": r.f.Type, "layer": r.f.Layer,
				"status": r.f.Status, "date": r.f.Date(), "tokens": r.f.Tokens, "tasks_open": r.tc.Open, "tasks": r.tc.Total})
		}
		return a.emitJSON(map[string]any{"total": total, "results": out})
	}
	bw := a.newBudget(a.budgetOr(a.defaultBudget(stores)))
	shown := 0
	for _, r := range rows {
		tasks := ""
		if r.tc.Total > 0 {
			tasks = fmt.Sprintf("tasks %d/%d", r.tc.Done, r.tc.Total)
		}
		title := ""
		base := r.f.Addr()[strings.LastIndex(r.f.Addr(), "/")+1:]
		if textutil.Fold(r.f.Title) != textutil.Fold(base) {
			title = strconviQuote(r.f.Title)
		}
		if !bw.line(join(r.prefix+r.f.Addr(), a.fileMeta(r.s, r.f), r.f.Date(), textutil.FmtTokens(r.f.Tokens), tasks, title)) {
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

func strconviQuote(s string) string { return `"` + s + `"` }

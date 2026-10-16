package cli

import (
	"fmt"
	"os"
	"path"
	"path/filepath"
	"regexp"
	"sort"
	"strings"

	"github.com/Utku-Private/Paragma/internal/addr"
	"github.com/Utku-Private/Paragma/internal/index"
	"github.com/Utku-Private/Paragma/internal/md"
	"github.com/Utku-Private/Paragma/internal/textutil"
	"github.com/spf13/cobra"
)

// ---- code <-> memory: related ----

// relHit is a note (or a block/section of one) that mentions a code path.
type relHit struct {
	f     *index.File
	addr  string
	line  int
	score float64
	why   string
}

// typeRank orders note types by how binding they are for code changes.
var typeRank = map[string]int{"rule": 0, "decision": 1, "error": 2, "concept": 3, "note": 3, "branch": 4, "session": 5, "project": 5, "journal": 6}

// codePath turns a path given on the command line into the form notes use:
// relative to the work tree root, lower case, forward slashes.
func (a *app) codePath(p string) string {
	if abs, err := filepath.Abs(p); err == nil {
		if rel, err := filepath.Rel(a.workDir(), abs); err == nil && !strings.HasPrefix(rel, "..") {
			p = rel
		}
	}
	return md.NormRef(p)
}

// refScore says how strongly a code reference in a note points at path.
func refScore(ref, p string) (float64, string) {
	dir := path.Dir(p)
	switch {
	case ref == p:
		return 3, "names " + p
	case strings.Contains(ref, "/") && !strings.HasSuffix(ref, "/") && (strings.HasSuffix(p, "/"+ref) || strings.HasSuffix(ref, "/"+p)):
		return 2.5, "names " + p
	case !strings.Contains(ref, "/") && !strings.HasPrefix(ref, "pkg:") && ref == path.Base(p):
		return 2, "names " + ref
	case strings.HasSuffix(ref, "/") && (strings.HasPrefix(p, ref) || strings.Contains(p, "/"+ref)):
		return 2, "covers " + ref
	case dir != "." && ref == "pkg:"+path.Base(dir):
		return 1.5, "package " + path.Base(dir)
	case dir != "." && strings.Contains(ref, "/") && !strings.HasSuffix(ref, "/") && path.Dir(ref) == dir:
		return 1, "same directory " + dir
	}
	return 0, ""
}

// relatedNotes finds notes that mention the given code paths. Hits inside
// a block (a rule, a task) are reported per block, others per section.
func (a *app) relatedNotes(s *store, paths []string, minScore float64, includeInactive bool) []relHit {
	refs, err := s.db.CodeRefs()
	if err != nil {
		return nil
	}
	best := map[string]relHit{}
	secCache := map[int64]*index.Section{}
	for _, r := range refs {
		var sc float64
		var why string
		for _, p := range paths {
			if x, w := refScore(r.Ref, p); x > sc {
				sc, why = x, w
			}
		}
		if sc < minScore {
			continue
		}
		f, err := s.db.FileByID(r.FileID)
		if err != nil || (!includeInactive && f.Inactive()) {
			continue
		}
		ad := f.Addr()
		if b := s.db.BlockAt(f.ID, r.Line); b != "" {
			ad += "^" + b
		} else if r.SectionID > 0 {
			sec, ok := secCache[r.SectionID]
			if !ok {
				sec, _ = s.db.SectionByID(r.SectionID)
				secCache[r.SectionID] = sec
			}
			if sec != nil && sec.Path != "" && !sec.IsTitle {
				ad += "#" + sec.Path
			}
		}
		if cur, ok := best[ad]; !ok || sc > cur.score {
			best[ad] = relHit{f: f, addr: ad, line: r.Line, score: sc, why: why}
		}
	}
	var out []relHit
	for _, h := range best {
		out = append(out, h)
	}
	sort.SliceStable(out, func(i, j int) bool {
		if out[i].score != out[j].score {
			return out[i].score > out[j].score
		}
		ri, rj := typeRank[out[i].f.Type], typeRank[out[j].f.Type]
		if ri != rj {
			return ri < rj
		}
		return out[i].addr < out[j].addr
	})
	return out
}

func (a *app) relatedCmd() *cobra.Command {
	var loose, all bool
	c := &cobra.Command{
		Use:   "related <code path>...",
		Short: "Rules, decisions and past errors about a source file, before you change it",
		Long: `List the memory that mentions a code file: rules, decisions and recorded
errors that name it, its directory or its package (payments.Refund counts
for payments/...). Run it before editing a file. --loose also lists notes
about files in the same directory.`,
		Example: "pg related payments/refund.go\npg related internal/db/open.go cmd/migrate/main.go",
		Args:    cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			var paths []string
			for _, p := range args {
				paths = append(paths, a.codePath(p))
			}
			min := 1.5
			if loose {
				min = 1
			}
			hits := a.relatedNotes(s, paths, min, all)
			if a.jsonOut {
				var out []map[string]any
				for _, h := range hits {
					out = append(out, map[string]any{"addr": h.addr, "type": h.f.Type, "status": h.f.Status, "why": h.why, "score": h.score})
				}
				if out == nil {
					out = []map[string]any{}
				}
				return a.emitJSON(out)
			}
			if len(hits) == 0 {
				a.println("no results")
				return errNoResults
			}
			bw := a.newBudget(a.budgetOr(s.v.Cfg.Budget))
			cache := map[int64][]string{}
			for i, h := range hits {
				lines, ok := cache[h.f.ID]
				if !ok {
					lines, _ = fileLines(s, h.f)
					cache[h.f.ID] = lines
				}
				text := ""
				if h.line >= 1 && h.line <= len(lines) {
					text = strings.TrimSpace(strings.TrimLeft(lines[h.line-1], " >-*"))
				}
				typ := h.f.Type
				if typ == "note" {
					typ = ""
				}
				if !bw.line(join(h.addr, meta(typ, statusWord(h.f)), h.why)) || (text != "" && !bw.line("  "+textutil.OneLine(text, 150))) {
					a.printf("… %d more; pg related %s --json for all\n", len(hits)-i, strings.Join(args, " "))
					break
				}
			}
			return nil
		},
	}
	c.Flags().BoolVar(&loose, "loose", false, "also notes about other files in the same directory")
	c.Flags().BoolVar(&all, "all", false, "include archived and superseded notes")
	return c
}

func statusWord(f *index.File) string {
	if f.Status == "active" || f.Status == "" {
		return ""
	}
	return f.Status
}

// ---- context pack ----

type ctxItem struct {
	f     *index.File
	addr  string
	from  int // line range to print
	to    int
	score float64
	why   string
}

// keyRange is the part of a note that carries its point: an error's fix, a
// decision's decision, a session's goal, else the note's opening text.
func (a *app) keyRange(s *store, f *index.File) (string, int, int) {
	key := map[string]string{"error": "fix", "decision": "decision", "session": "goal", "project": "goal", "branch": "bgoal"}[f.Type]
	if key != "" {
		if sec := findSection(s, f, key); sec != nil {
			return f.Addr() + "#" + sec.Path, sec.Line, sec.BodyEnd
		}
	}
	secs, _ := s.res.Sections(f)
	for _, sec := range secs {
		if sec.Level == 0 || sec.IsTitle {
			if sec.BodyEnd > sec.Line || sec.Level == 0 {
				return f.Addr(), sec.Line, sec.BodyEnd
			}
		}
	}
	if len(secs) > 1 {
		return f.Addr() + "#" + secs[1].Path, secs[1].Line, secs[1].BodyEnd
	}
	return f.Addr(), f.BodyLine, f.NLines
}

func (a *app) contextCmd() *cobra.Command {
	var budget int
	c := &cobra.Command{
		Use:   "context <topic|addr|code path>",
		Short: "One call for everything memory knows about a topic: best sections plus linked fixes, decisions and rules",
		Long: `Collect what memory knows about a topic in one output, within a token
budget: the best search hits, the notes they link to (fixes, decisions,
superseded and promoted notes), and for a code path the rules and errors
about that file. Use it instead of several search + read calls. Items that
did not fit are listed at the end with their cost.`,
		Example: `pg context "refund window"
pg context payments/refund.go --budget 400
pg context sqlite-database-locked`,
		Args: cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			q := strings.Join(args, " ")
			s, err := a.main()
			if err != nil {
				return err
			}
			if budget <= 0 {
				budget = a.budgetOr(1000)
			}
			items := a.gatherContext(s, q)
			if len(items) == 0 {
				recordMiss(s, "context", q)
				a.println("no results")
				a.searchHint(s, q)
				return errNoResults
			}
			return a.printContext(s, q, items, budget)
		},
	}
	c.Flags().IntVar(&budget, "budget", 0, "token budget for the topic part of the pack (default 1000)")
	return c
}

// gatherContext ranks the sections worth reading for q.
func (a *app) gatherContext(s *store, q string) []ctxItem {
	cand := map[string]*ctxItem{}
	add := func(it ctxItem) {
		if cur, ok := cand[it.addr]; ok {
			if it.score > cur.score {
				cur.score, cur.why = it.score, it.why
			} else {
				cur.score += it.score * 0.2 // reached twice: a little more relevant
			}
			return
		}
		cand[it.addr] = &it
	}
	seedFiles := map[int64]float64{}

	// 1. an exact address
	if p, err := addr.Parse(q); err == nil && !strings.Contains(q, " ") {
		if t, err := s.res.Resolve(p); err == nil {
			ad, from, to := t.Addr(), t.From, t.To
			if t.Kind == addr.KindFile {
				ad, from, to = a.keyRange(s, t.File)
			}
			add(ctxItem{f: t.File, addr: ad, from: from, to: to, score: 10, why: "asked"})
			seedFiles[t.File.ID] = 10
		}
	}
	// 2. a code path: notes about that file
	if len(md.CodeRefs(" "+q+" ")) > 0 && !strings.Contains(strings.TrimSpace(q), " ") {
		for _, h := range a.relatedNotes(s, []string{a.codePath(q)}, 1.5, false) {
			from, to := h.line, h.line
			if t, err := s.res.Resolve(mustParse(h.addr)); err == nil {
				from, to = t.From, t.To
			}
			add(ctxItem{f: h.f, addr: h.addr, from: from, to: to, score: 3 + h.score, why: h.why})
			seedFiles[h.f.ID] = max(seedFiles[h.f.ID], 3)
		}
	}
	// 3. full-text hits
	hits, _ := s.db.Search(index.SearchOpts{Query: q, Max: 40})
	if len(hits) > 0 {
		hits = hits[:min(len(hits), 8)]
		s.db.Hydrate(hits)
		top := hits[0].Score
		for _, h := range hits {
			if h.Sec == nil {
				continue
			}
			sc := 5 * h.Score / top
			seedFiles[h.File.ID] = max(seedFiles[h.File.ID], sc)
			// a section of rules: take the individual rules that match,
			// not the whole list
			if strings.Contains(" "+h.Sec.Kinds+" ", " rule ") {
				if rules := a.matchingBlocks(s, h.File, h.Sec.Line, h.Sec.BodyEnd, q); len(rules) > 0 {
					for i, r := range rules {
						add(ctxItem{f: h.File, addr: r.addr, from: r.from, to: r.to, score: sc * (1 - 0.05*float64(i)), why: "rule matches"})
					}
					continue
				}
			}
			ad := hitAddr(h)
			from, to := h.Sec.Line, h.Sec.BodyEnd
			if h.Sec.IsTitle || h.Sec.Path == "" {
				ad, from, to = a.keyRange(s, h.File)
			}
			add(ctxItem{f: h.File, addr: ad, from: from, to: to, score: sc, why: "matches"})
		}
	}
	// 4. one hop along links from the strongest seeds
	type seed struct {
		id int64
		sc float64
	}
	var seeds []seed
	for id, sc := range seedFiles {
		seeds = append(seeds, seed{id, sc})
	}
	sort.Slice(seeds, func(i, j int) bool { return seeds[i].sc > seeds[j].sc })
	for _, sd := range seeds[:min(len(seeds), 4)] {
		f, err := s.db.FileByID(sd.id)
		if err != nil {
			continue
		}
		for _, e := range a.edges(s, f) {
			if e.f.ID == f.ID || e.f.Type == "journal" {
				continue
			}
			w := 0.35
			if e.kind != "wiki" && e.kind != "md" && e.kind != "embed" {
				w = 0.55 // typed links (fixes, supersedes, promoted, from) carry more
			}
			ad, from, to := e.addr, e.from, e.to
			if e.whole {
				ad, from, to = a.keyRange(s, e.f)
			}
			add(ctxItem{f: e.f, addr: ad, from: from, to: to, score: sd.sc * w, why: e.kind + " " + e.dir + " " + f.Addr()})
		}
	}
	var out []ctxItem
	for _, it := range cand {
		if it.f.Inactive() {
			it.score *= 0.3
		}
		// a whole section of rules, however it was reached: keep only the
		// rules that match the topic
		if it.f.Type == "rule" && strings.Contains(it.addr, "#") {
			if rules := a.matchingBlocks(s, it.f, it.from, it.to, q); len(rules) > 0 {
				for i, r := range rules {
					if _, dup := cand[r.addr]; !dup {
						out = append(out, ctxItem{f: it.f, addr: r.addr, from: r.from, to: r.to, score: it.score * (1 - 0.05*float64(i)), why: it.why})
					}
				}
				continue
			}
		}
		out = append(out, *it)
	}
	sort.SliceStable(out, func(i, j int) bool {
		if out[i].score != out[j].score {
			return out[i].score > out[j].score
		}
		return out[i].addr < out[j].addr
	})
	return out
}

type blockRange struct {
	addr     string
	from, to int
}

// matchingBlocks returns the ^id blocks in [from,to] of f whose text shares
// a word with q, best overlap first.
func (a *app) matchingBlocks(s *store, f *index.File, from, to int, q string) []blockRange {
	rows, err := s.db.Query(`SELECT block_id, line, line_end FROM blocks WHERE file_id=? AND line>=? AND line<=? ORDER BY line`, f.ID, from, to)
	if err != nil {
		return nil
	}
	type cand struct {
		blockRange
		hits int
	}
	var cs []cand
	var bs []blockRange
	for rows.Next() {
		var b blockRange
		var id string
		rows.Scan(&id, &b.from, &b.to)
		b.addr = f.Addr() + "^" + id
		bs = append(bs, b)
	}
	rows.Close()
	lines, err := fileLines(s, f)
	if err != nil {
		return nil
	}
	words := strings.Fields(textutil.Fold(q))
	for _, b := range bs {
		text := textutil.Fold(strings.Join(sliceLines(lines, b.from, b.to), " "))
		n := 0
		for _, w := range words {
			if len(w) >= 3 && strings.Contains(text, w) {
				n++
			}
		}
		if n > 0 {
			cs = append(cs, cand{b, n})
		}
	}
	sort.SliceStable(cs, func(i, j int) bool { return cs[i].hits > cs[j].hits })
	var out []blockRange
	for _, c := range cs[:min(len(cs), 4)] {
		out = append(out, c.blockRange)
	}
	return out
}

func mustParse(s string) addr.Addr {
	p, _ := addr.Parse(s)
	return p
}

// ---- link graph ----

// edge is one link between notes, seen from a note.
type edge struct {
	f        *index.File
	addr     string // target (out) or source (in) address
	from, to int
	whole    bool   // the link points at the whole note
	kind     string // wiki, fixes, supersedes, ...
	dir      string // "to" (out) or "from" (in)
}

// edges returns the resolved outgoing and incoming links of a note.
func (a *app) edges(s *store, f *index.File) []edge {
	var out []edge
	ls, _ := s.db.LinksFrom(f.ID, 1, max(f.NLines, 1))
	for _, l := range ls {
		t, err := resolveLink(s.res, f, l.Target)
		if err != nil {
			continue
		}
		out = append(out, edge{f: t.File, addr: t.Addr(), from: t.From, to: t.To, whole: t.Kind == addr.KindFile, kind: l.Kind, dir: "to"})
	}
	whole := &addr.Target{Kind: addr.KindFile, File: f, From: 1, To: f.NLines}
	bl, _ := a.backlinks(s, whole)
	for _, b := range bl {
		src, err := s.res.ResolveFile(strings.SplitN(strings.SplitN(b.Addr, "#", 2)[0], "^", 2)[0])
		if err != nil {
			continue
		}
		e := edge{f: src, addr: b.Addr, from: b.Line, to: b.Line, kind: b.Kind, dir: "from"}
		if t, err := s.res.Resolve(mustParse(b.Addr)); err == nil {
			e.from, e.to = t.From, t.To
			if t.Kind == addr.KindFile {
				e.whole = true
			}
		}
		out = append(out, e)
	}
	return out
}

func (a *app) neighborsCmd(use string) *cobra.Command {
	var depth int
	c := &cobra.Command{
		Use:   use + " <addr>",
		Short: "The link graph around a note as text: what it points to (→) and what points to it (←)",
		Long: `Walk the link graph from a note up to --depth hops and print it as an
indented tree. Each line: direction (→ links to, ← linked from), the
address, its type, the link kind (wiki, fixes, supersedes, promoted ...)
and the cost of reading it.`,
		Example: "pg neighbors sqlite-database-locked\npg neighbors auth-refactor --depth 2",
		Args:    cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, t, err := a.resolve(strings.Join(args, " "))
			if err != nil {
				return err
			}
			depth = max(1, min(depth, 3))
			bw := a.newBudget(a.budgetOr(s.v.Cfg.Budget))
			bw.line(join(t.File.Addr(), a.fileMeta(s, t.File), textutil.FmtTokens(t.File.Tokens)))
			visited := map[int64]bool{t.File.ID: true}
			nodes := 0
			var walk func(f *index.File, d int, indent string)
			walk = func(f *index.File, d int, indent string) {
				if d > depth || bw.cut {
					return
				}
				es := a.edges(s, f)
				sort.SliceStable(es, func(i, j int) bool {
					if es[i].dir != es[j].dir {
						return es[i].dir == "to"
					}
					return typeRank[es[i].f.Type] < typeRank[es[j].f.Type]
				})
				seenHere := map[string]bool{}
				for _, e := range es {
					if e.f.ID == f.ID || seenHere[e.dir+e.addr] {
						continue
					}
					seenHere[e.dir+e.addr] = true
					arrow := "→"
					if e.dir == "from" {
						arrow = "←"
					}
					typ := e.f.Type
					if typ == "note" {
						typ = ""
					}
					again := ""
					if visited[e.f.ID] {
						again = "(above)"
					}
					if !bw.line(indent + join(arrow+" "+e.addr, meta(typ, statusWord(e.f)), e.kind, textutil.FmtTokens(e.f.Tokens), again)) {
						return
					}
					nodes++
					if !visited[e.f.ID] {
						visited[e.f.ID] = true
						walk(e.f, d+1, indent+"  ")
					}
				}
			}
			walk(t.File, 1, "  ")
			if nodes == 0 {
				a.println("  (no links; pg related, pg search or pg link can connect it)")
			}
			if bw.cut {
				a.println("… cut by the budget; lower --depth or raise --budget")
			}
			return nil
		},
	}
	c.Flags().IntVar(&depth, "depth", 1, "hops to follow (1-3)")
	return c
}

func (a *app) graphCmd() *cobra.Command {
	c := a.neighborsCmd("graph")
	var text bool
	c.Flags().BoolVar(&text, "text", true, "print the graph as text (the full-screen view is not built yet)")
	c.Short = "Same as pg neighbors: the link graph around a note, as text for agents"
	return c
}

func (a *app) orphansCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "orphans",
		Short: "Notes with no links in or out (candidates to link, merge or archive)",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			var out []string
			for _, f := range s.res.Files() {
				if f.Inactive() || f.Type == "journal" || f.Type == "session" || f.Type == "branch" || f.Type == "project" {
					continue
				}
				if len(a.edges(s, f)) == 0 {
					out = append(out, join(f.Addr(), a.fileMeta(s, f), textutil.FmtTokens(f.Tokens)))
				}
			}
			if len(out) == 0 {
				a.println("no results")
				return errNoResults
			}
			bw := a.newBudget(a.budgetOr(s.v.Cfg.Budget))
			for i, l := range out {
				if !bw.line(l) {
					a.printf("… %d more\n", len(out)-i)
					break
				}
			}
			a.println("next: pg related / pg search to find a home, then pg link <note> <addr>")
			return nil
		},
	}
}

func (a *app) brokenCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "broken",
		Short: "Links that point nowhere, with the closest existing note",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			links, _ := s.db.AllLinks()
			var names []string
			for _, f := range s.res.Files() {
				names = append(names, filepath.Base(f.Addr()))
			}
			n := 0
			for _, l := range links {
				if linkResolves(s.res, l) {
					continue
				}
				src, err := s.db.FileByID(l.SrcFile)
				if err != nil {
					continue
				}
				hint := ""
				if c := textutil.Closest(strings.SplitN(l.Target, "#", 2)[0], names, 1); len(c) > 0 {
					hint = "did you mean " + c[0]
				}
				a.println(join(fmt.Sprintf("%s:%d", src.Addr(), l.Line), "[["+l.Target+"]]", hint))
				n++
			}
			if n == 0 {
				a.println("no results")
				return errNoResults
			}
			return nil
		},
	}
}

// suggestLinks prints notes a freshly written note should probably link to.
func (a *app) suggestLinks(s *store, newAddr, query string) {
	if !s.v.Cfg.Hints || a.dryRun || a.quiet {
		return
	}
	hits, err := s.db.Search(index.SearchOpts{Query: query, Types: []string{"rule", "decision", "error"}, Max: 20})
	if err != nil {
		return
	}
	var picks []string
	seen := map[int64]bool{}
	for _, h := range hits {
		if h.File.Addr() == newAddr || seen[h.File.ID] || h.File.Inactive() {
			continue
		}
		seen[h.File.ID] = true
		picks = append(picks, h.File.Addr())
		if len(picks) == 3 {
			break
		}
	}
	if len(picks) > 0 {
		a.printf("related: %s (pg link %s <addr> --kind relates)\n", strings.Join(picks, ", "), newAddr)
	}
}

var _ = os.Getenv

var reTrailingID = regexp.MustCompile(`\s\^[A-Za-z0-9][\w-]*\s*$`)

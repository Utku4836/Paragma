package cli

import (
	"fmt"
	"sort"
	"strings"

	"github.com/Utku4836/Paragma/internal/addr"
	"github.com/Utku4836/Paragma/internal/index"
	"github.com/Utku4836/Paragma/internal/textutil"
	"github.com/spf13/cobra"
)

// The link graph: outgoing links, backlinks, neighbours, orphans.

// resolveLink resolves a link target written in file src.
func resolveLink(r *addr.Resolver, src *index.File, target string) (*addr.Target, error) {
	p, err := addr.Parse(target)
	if err != nil {
		return nil, err
	}
	if p.File == "" && p.ULID == "" {
		if p.Block == "" && p.Heading == "" {
			return nil, fail(ExitNotFound, "empty link")
		}
		p.File = src.Addr() // [[#Heading]] / [[^blk]] point into the same note
	}
	return r.Resolve(p)
}

func (a *app) linksCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "links <addr>",
		Short: "Outgoing links of a note or heading, typed, with broken ones flagged",
		Long: `Outgoing [[links]] of a note or heading. Each line starts with the
resolved target address (or the raw target if broken), then the link kind:
wiki, embed, md, or a typed key such as fixes:: / supersedes::.`,
		Example: "pg links sqlite-database-locked\npg links architecture#Middleware",
		Args:    cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, t, err := a.resolve(strings.Join(args, " ")) // an unquoted "note#Two words" arrives split
			if err != nil {
				return err
			}
			from, to := t.From, t.To
			if t.Kind == addr.KindFile {
				from = 1 // frontmatter links count for whole-note queries
			}
			ls, err := s.db.LinksFrom(t.File.ID, from, to)
			if err != nil {
				return fail(ExitInternal, "%v", err)
			}
			type out struct {
				Addr   string `json:"addr"`
				Kind   string `json:"kind"`
				Lines  []int  `json:"lines"`
				Broken bool   `json:"broken,omitempty"`
			}
			var rows []*out
			idx := map[string]*out{}
			for _, l := range ls {
				o := &out{Kind: l.Kind}
				if tt, err := resolveLink(s.res, t.File, l.Target); err == nil {
					o.Addr = tt.Addr()
				} else {
					o.Addr, o.Broken = l.Target, true
				}
				key := o.Addr + "\x00" + o.Kind
				if prev, ok := idx[key]; ok {
					prev.Lines = append(prev.Lines, l.Line)
					continue
				}
				o.Lines = []int{l.Line}
				idx[key] = o
				rows = append(rows, o)
			}
			if a.jsonOut {
				if rows == nil {
					rows = []*out{}
				}
				return a.emitJSON(rows)
			}
			if len(rows) == 0 {
				a.println("no results")
				return errNoResults
			}
			bw := a.newBudget(a.budgetOr(s.v.Cfg.Budget))
			for i, o := range rows {
				k := o.Kind
				if o.Broken {
					k += " broken"
				}
				if !bw.line(join(o.Addr, "["+k+"]", lineList(o.Lines))) {
					a.printf("... %d more links\n", len(rows)-i)
					break
				}
			}
			return nil
		},
	}
}

func lineList(ls []int) string {
	parts := make([]string, len(ls))
	for i, l := range ls {
		parts[i] = fmt.Sprint(l)
	}
	return "L" + strings.Join(parts, ",")
}

// fileKeys lists every folded key a link may use to point at f.
func fileKeys(f *index.File) []string {
	k := index.FileKey(f.Path)
	keys := []string{k}
	for {
		i := strings.Index(k, "/")
		if i < 0 {
			break
		}
		k = k[i+1:]
		keys = append(keys, k)
	}
	if f.ULID != "" {
		keys = append(keys, textutil.Fold("@"+f.ULID))
	}
	return keys
}

// backlink is one incoming link.
type backlink struct {
	Addr    string `json:"addr"`
	Kind    string `json:"kind"`
	Line    int    `json:"line"`
	Context string `json:"context"`
}

// backlinks returns the links that land on target t (inside its range when t
// is a heading, block or line range).
func (a *app) backlinks(s *store, t *addr.Target) ([]backlink, error) {
	ls, err := s.db.LinksTo(fileKeys(t.File))
	if err != nil {
		return nil, fail(ExitInternal, "%v", err)
	}
	var rows []backlink
	secCache := map[int64]*index.Section{}
	for _, l := range ls {
		src, err := s.db.FileByID(l.SrcFile)
		if err != nil {
			continue
		}
		tt, err := resolveLink(s.res, src, l.Target)
		if err != nil || tt.File.ID != t.File.ID {
			continue
		}
		if t.Kind != addr.KindFile {
			// a heading/block/range query matches links that land inside it
			if tt.Kind == addr.KindFile || tt.From < t.From || tt.From > t.To {
				continue
			}
		}
		srcAddr := src.Addr()
		if l.SrcSection > 0 {
			sec, ok := secCache[l.SrcSection]
			if !ok {
				sec, _ = s.db.SectionByID(l.SrcSection)
				secCache[l.SrcSection] = sec
			}
			if sec != nil && sec.Path != "" && !sec.IsTitle {
				srcAddr += "#" + sec.Path
			}
		}
		rows = append(rows, backlink{srcAddr, l.Kind, l.Line, l.Context})
	}
	return rows, nil
}

func (a *app) backlinksCmd() *cobra.Command {
	return &cobra.Command{
		Use:     "backlinks <addr>",
		Short:   "Incoming links to a note or heading, with the linking line",
		Example: "pg backlinks architecture#Middleware\npg backlinks sqlite-database-locked",
		Args:    cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, t, err := a.resolve(strings.Join(args, " ")) // an unquoted "note#Two words" arrives split
			if err != nil {
				return err
			}
			rows, err := a.backlinks(s, t)
			if err != nil {
				return err
			}
			if a.jsonOut {
				if rows == nil {
					rows = []backlink{}
				}
				return a.emitJSON(rows)
			}
			if len(rows) == 0 {
				a.println("no results")
				return errNoResults
			}
			bw := a.newBudget(a.budgetOr(s.v.Cfg.Budget))
			for i, r := range rows {
				if !bw.line(join(r.Addr, "["+r.Kind+"]", fmt.Sprintf("L%d", r.Line))) || !bw.line("  "+textutil.OneLine(r.Context, 140)) {
					a.printf("... %d more backlinks\n", len(rows)-i)
					break
				}
			}
			return nil
		},
	}
}

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

func (a *app) neighborsCmd() *cobra.Command {
	var depth int
	c := &cobra.Command{
		Use:     "neighbors <addr>",
		Aliases: []string{"graph"},
		Short:   "The link graph around a note as text: what it points to (->) and what points to it (<-)",
		Long: `Walk the link graph from a note up to --depth hops and print it as an
indented tree. Each line: direction (-> links to, <- linked from), the
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
					arrow := "->"
					if e.dir == "from" {
						arrow = "<-"
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
				a.println("... cut by the budget; lower --depth or raise --budget")
			}
			return nil
		},
	}
	c.Flags().IntVar(&depth, "depth", 1, "hops to follow (1-3)")
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
				if f.Inactive() || f.Type == "journal" || f.Type == "project" {
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
					a.printf("... %d more\n", len(out)-i)
					break
				}
			}
			a.println("next: pg related / pg search to find a home, then pg link <note> <addr>")
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

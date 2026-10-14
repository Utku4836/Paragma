package cli

import (
	"fmt"
	"strings"

	"github.com/Utku-Private/Paragma/internal/addr"
	"github.com/Utku-Private/Paragma/internal/index"
	"github.com/Utku-Private/Paragma/internal/textutil"
	"github.com/spf13/cobra"
)

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
		Args:    cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, t, err := a.resolve(args[0])
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
					o.Addr = a.prefix(s) + tt.Addr()
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
					a.printf("… %d more links\n", len(rows)-i)
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

func (a *app) backlinksCmd() *cobra.Command {
	return &cobra.Command{
		Use:     "backlinks <addr>",
		Short:   "Incoming links to a note or heading, with the linking line",
		Example: "pg backlinks architecture#Middleware\npg backlinks sqlite-database-locked",
		Args:    cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, t, err := a.resolve(args[0])
			if err != nil {
				return err
			}
			ls, err := s.db.LinksTo(fileKeys(t.File))
			if err != nil {
				return fail(ExitInternal, "%v", err)
			}
			// same-file links ([[#Heading]]) are keyed by the file's own path
			type out struct {
				Addr    string `json:"addr"`
				Kind    string `json:"kind"`
				Line    int    `json:"line"`
				Context string `json:"context"`
			}
			var rows []out
			secCache := map[int64]*index.Section{}
			fileCache := map[int64]*index.File{}
			for _, l := range ls {
				src := fileCache[l.SrcFile]
				if src == nil {
					if src, err = s.db.FileByID(l.SrcFile); err != nil {
						continue
					}
					fileCache[l.SrcFile] = src
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
					sec := secCache[l.SrcSection]
					if sec == nil {
						sec, _ = s.db.SectionByID(l.SrcSection)
						secCache[l.SrcSection] = sec
					}
					if sec != nil && sec.Path != "" {
						srcAddr += "#" + sec.Path
					}
				}
				rows = append(rows, out{a.prefix(s) + srcAddr, l.Kind, l.Line, l.Context})
			}
			if a.jsonOut {
				if rows == nil {
					rows = []out{}
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
					a.printf("… %d more backlinks\n", len(rows)-i)
					break
				}
			}
			return nil
		},
	}
}

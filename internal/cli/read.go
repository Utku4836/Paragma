package cli

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/Utku-Private/Paragma/internal/addr"
	"github.com/Utku-Private/Paragma/internal/index"
	"github.com/Utku-Private/Paragma/internal/md"
	"github.com/Utku-Private/Paragma/internal/textutil"
	"github.com/spf13/cobra"
)

// fileLines reads a note from disk, split exactly like the parser splits it.
func fileLines(s *store, f *index.File) ([]string, error) {
	b, err := os.ReadFile(filepath.Join(s.v.Root, filepath.FromSlash(f.Path)))
	if err != nil {
		return nil, fail(ExitInternal, "read %s: %v", f.Path, err)
	}
	return md.SplitLines(b), nil
}

func sliceLines(lines []string, from, to int) []string {
	from = max(from, 1)
	to = min(to, len(lines))
	if from > to {
		return nil
	}
	return lines[from-1 : to]
}

// fileMeta renders "[layer type status stale?]" for a file.
func (a *app) fileMeta(s *store, f *index.File) string {
	typ := f.Type
	if typ == "note" {
		typ = ""
	}
	st := f.Status
	if st == "active" {
		st = ""
	}
	stale := ""
	if f.Stale(s.v.Cfg.StaleDays) {
		stale = "stale?"
	}
	seen := ""
	if f.Seen > 0 {
		seen = fmt.Sprintf("seen:%d", f.Seen)
	}
	return meta(f.Layer, typ, st, seen, stale)
}

// ---- outline ----

func (a *app) outlineCmd() *cobra.Command {
	var depth int
	c := &cobra.Command{
		Use:     "outline [addr]",
		Aliases: []string{"ls", "map", "tree", "toc"},
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

// ---- read ----

func (a *app) readCmd() *cobra.Command {
	var full, withMeta, numbers bool
	c := &cobra.Command{
		Use:     "read <addr>...",
		Aliases: []string{"show", "cat", "view", "get", "open"},
		Short:   "Print only the addressed notes, headings, blocks or lines",
		Long: `Print the addressed part of a note. A heading includes its sub-headings.
Several addresses can be read in one call; each gets its own header line
(address, file:lines to cite, token cost, hash). If a target is bigger than
read_limit (config, default 1500 tokens) pg shows its outline instead, or
cuts it and prints the line range to continue with; --full disables that.`,
		Example: "pg read architecture#Middleware\npg read rules^r003 jwt-exp-timezone#Çözüm\npg read stack:10-24\npg read rules --full",
		Args:    cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			args = a.regroupArgs(args)
			if len(args) == 1 {
				s, t, err := a.resolve(args[0])
				if err != nil {
					return err
				}
				return a.read(s, t, full, withMeta, numbers)
			}
			// several targets: keep going past a bad address, report it in
			// place so the output stays in the order asked for
			var failed error
			for i, arg := range args {
				if i > 0 && !a.jsonOut {
					a.println("")
				}
				s, t, err := a.resolve(arg)
				if err == nil {
					err = a.read(s, t, full, withMeta, numbers)
				}
				if err != nil {
					a.println("error: " + err.Error())
					failed = err
				}
			}
			if failed != nil {
				var e *Error
				if errors.As(failed, &e) {
					return &Error{Code: e.Code}
				}
				return &Error{Code: ExitInternal}
			}
			return nil
		},
	}
	c.Flags().BoolVar(&full, "full", false, "never replace a large target with its outline or cut it")
	c.Flags().BoolVar(&withMeta, "meta", false, "include the frontmatter when reading a whole note")
	c.Flags().BoolVarP(&numbers, "numbers", "n", false, "prefix lines with their line number")
	return c
}

func (a *app) read(s *store, t *addr.Target, full, withMeta, numbers bool) error {
	lines, err := fileLines(s, t.File)
	if err != nil {
		return err
	}
	from, to := t.From, t.To
	if t.Kind == addr.KindFile && withMeta {
		from = 1
	}
	body := sliceLines(lines, from, to)
	text := strings.Join(body, "\n")
	tokens := textutil.Tokens(text)
	address := t.Addr()

	if a.jsonOut {
		return a.emitJSON(map[string]any{"addr": address, "from": from, "to": to, "tokens": tokens, "hash": t.File.Hash, "text": text})
	}
	limit := a.budgetOr(s.v.Cfg.ReadLimit)
	header := join(address, fmt.Sprintf("L%d-%d", from, to), textutil.FmtTokens(tokens), "hash:"+t.File.Hash)

	if full || tokens <= limit {
		a.println(header)
		a.printLines(body, from, numbers)
		if s.v.Cfg.Hints && (t.Kind == addr.KindFile || t.Kind == addr.KindSection) {
			if bl, err := a.backlinks(s, t); err == nil && len(bl) > 0 {
				a.printf("next: pg backlinks %s (%d links in)\n", address, len(bl))
			}
		}
		return nil
	}

	// too big: an outline if there is structure below, otherwise a cut
	if t.Kind == addr.KindFile || t.Kind == addr.KindSection {
		secs, err := s.db.Sections(t.File.ID)
		if err != nil {
			return fail(ExitInternal, "%v", err)
		}
		var kids []*index.Section
		for _, sec := range secs {
			if sec.Line > from && sec.Line <= to && sec.Level > 0 && (t.Section == nil || sec.Level > t.Section.Level) {
				kids = append(kids, sec)
			}
		}
		if len(kids) > 0 {
			a.println(header)
			a.printf("too large (%s > %s): outline below; read one heading, or add --full\n", textutil.FmtTokens(tokens), textutil.FmtTokens(limit))
			bw := a.newBudget(400)
			for i, k := range kids {
				if !bw.line(join(strings.Repeat("#", k.Level)+" "+t.File.Addr()+"#"+k.Path, textutil.FmtTokens(k.SubTokens), kindsLabel(k.Kinds))) {
					a.printf("... %d more; pg outline %s\n", len(kids)-i, address)
					break
				}
			}
			return nil
		}
	}
	// cut at the budget and say where to continue
	used := 0
	end := from - 1
	for i, l := range body {
		used += textutil.Tokens(l) + 1
		if used > limit && i > 0 {
			break
		}
		end = from + i
	}
	a.println(header)
	a.printLines(sliceLines(lines, from, end), from, numbers)
	rest := textutil.Tokens(strings.Join(sliceLines(lines, end+1, to), "\n"))
	a.printf("... cut at L%d (%s more); next: pg read %s%s:%d-%d\n", end, textutil.FmtTokens(rest), a.prefix(s), t.File.Addr(), end+1, to)
	return nil
}

func kindsLabel(k string) string {
	if k == "" {
		return ""
	}
	return "[!" + strings.ReplaceAll(k, " ", "] [!") + "]"
}

func (a *app) printLines(lines []string, from int, numbers bool) {
	for i, l := range lines {
		if numbers {
			a.printf("%d| %s\n", from+i, l)
		} else {
			a.println(l)
		}
	}
}

// ---- peek ----

func (a *app) peekCmd() *cobra.Command {
	var n int
	c := &cobra.Command{
		Use:     "peek <addr>",
		Short:   "First lines of a target plus how much is left",
		Example: "pg peek rules --lines 5\npg peek architecture#Middleware",
		Args:    cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, t, err := a.resolve(strings.Join(args, " ")) // an unquoted "note#Two words" arrives split
			if err != nil {
				return err
			}
			lines, err := fileLines(s, t.File)
			if err != nil {
				return err
			}
			body := sliceLines(lines, t.From, t.To)
			var shown []string
			last := t.From - 1
			for i, l := range body {
				if len(shown) >= n {
					break
				}
				last = t.From + i
				if strings.TrimSpace(l) != "" {
					shown = append(shown, l)
				}
			}
			rest := textutil.Tokens(strings.Join(sliceLines(lines, last+1, t.To), "\n"))
			address := t.Addr()
			if a.jsonOut {
				return a.emitJSON(map[string]any{"addr": address, "lines": shown, "rest_tokens": rest})
			}
			a.println(join(address, fmt.Sprintf("L%d-%d", t.From, t.To), textutil.FmtTokens(textutil.Tokens(strings.Join(body, "\n")))))
			for _, l := range shown {
				a.println(l)
			}
			if last < t.To {
				a.printf("... %s more: pg read %s\n", textutil.FmtTokens(rest), address)
			}
			return nil
		},
	}
	c.Flags().IntVar(&n, "lines", 5, "number of non-blank lines to show")
	return c
}

// regroupArgs re-joins addresses the shell split at spaces. Agents often
// write `pg read note#Kök neden` without quotes; an argument holding '#'
// that does not resolve is extended with the following arguments until the
// joined text does.
func (a *app) regroupArgs(args []string) []string {
	var out []string
	for i := 0; i < len(args); i++ {
		cur := args[i]
		if strings.Contains(cur, "#") && i+1 < len(args) {
			if _, _, err := a.resolve(cur); err != nil {
				for j := i + 1; j < len(args) && j <= i+6; j++ {
					cand := strings.Join(args[i:j+1], " ")
					if _, _, err := a.resolve(cand); err == nil {
						cur, i = cand, j
						break
					}
				}
			}
		}
		out = append(out, cur)
	}
	return out
}

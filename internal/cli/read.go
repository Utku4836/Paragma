package cli

import (
	"errors"
	"fmt"
	"strings"

	"github.com/Utku4836/Paragma/internal/addr"
	"github.com/Utku4836/Paragma/internal/index"
	"github.com/Utku4836/Paragma/internal/textutil"
	"github.com/spf13/cobra"
)

// Reading addressed parts of notes.

func (a *app) readCmd() *cobra.Command {
	var full, withMeta, numbers bool
	c := &cobra.Command{
		Use:     "read <addr>...",
		Aliases: []string{"show", "cat", "view", "get", "open"},
		Short:   "Print only the addressed notes, headings, blocks or lines; or one function of a source file",
		Long: `Print the addressed part of a note. A heading includes its sub-headings.
Several addresses can be read in one call; each gets its own header line
(address, file:lines to cite, token cost, hash). If a target is bigger than
read_limit (config, default 1500 tokens) pg shows its outline instead, or
cuts it and prints the line range to continue with; --full disables that.
A source file works too: file.go#Name prints one declaration (Cart.Total
for a method, with the comment above it), file.go:10-30 a line range.`,
		Example: "pg read architecture#Middleware\npg read rules^r003 jwt-exp-timezone#Çözüm\npg read stack:10-24\npg read payments/payments.go#Charge cart/cart.go#Cart.Total",
		Args:    cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			args = a.regroupArgs(args)
			if len(args) == 1 {
				if ct, ok := a.codeArg(args[0]); ok {
					return a.readCode(ct, numbers)
				}
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
				var err error
				if ct, ok := a.codeArg(arg); ok {
					err = a.readCode(ct, numbers)
				} else {
					var s *store
					var t *addr.Target
					if s, t, err = a.resolve(arg); err == nil {
						err = a.read(s, t, full, withMeta, numbers)
					}
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
	a.printf("... cut at L%d (%s more); next: pg read %s:%d-%d\n", end, textutil.FmtTokens(rest), t.File.Addr(), end+1, to)
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

// regroupArgs re-joins addresses the shell split at spaces. Agents often
// write `pg read note#Kök neden` without quotes; an argument holding '#'
// that does not resolve is extended with the following arguments until the
// joined text does.
func (a *app) regroupArgs(args []string) []string {
	var out []string
	for i := 0; i < len(args); i++ {
		cur := args[i]
		if _, ok := a.codeArg(cur); ok {
			out = append(out, cur)
			continue
		}
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

// fileMeta renders "[layer type status seen:N stale?]" for a file.
func (a *app) fileMeta(s *store, f *index.File) string {
	return meta(append([]string{f.Layer}, noteFlags(s, f)...)...)
}

// noteFlags are the parts of a note's meta bracket that are worth showing:
// its type (not "note"), status (not "active"), how often an error was seen
// and whether it is stale.
func noteFlags(s *store, f *index.File) []string {
	var out []string
	if f.Type != "note" {
		out = append(out, f.Type)
	}
	if f.Status != "active" {
		out = append(out, f.Status)
	}
	if f.Seen > 0 {
		out = append(out, fmt.Sprintf("seen:%d", f.Seen))
	}
	if f.Stale(s.v.Cfg.StaleDays) {
		out = append(out, "stale?")
	}
	return out
}

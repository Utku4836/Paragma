package cli

import (
	"bufio"
	"fmt"
	"io"
	"regexp"
	"strings"

	"github.com/Utku-Private/Paragma/internal/addr"
	"github.com/Utku-Private/Paragma/internal/md"
	"github.com/Utku-Private/Paragma/internal/textutil"
	"github.com/spf13/cobra"
)

func newDiscard(w io.Writer) *bufio.Writer { return bufio.NewWriter(w) }

var markKinds = []string{"pin", "important", "stale", "verify", "question", "warning"}

// markLine is the line a mark is written on: the heading of a section, the
// first line of a block or range, the title (or first body line) of a note.
func markLine(n *note, t *addr.Target) int {
	switch t.Kind {
	case addr.KindSection:
		return t.Section.Line
	case addr.KindFile:
		for i := t.From; i <= len(n.lines); i++ {
			if !isBlank(n.lines[i-1]) {
				return i
			}
		}
		return t.From
	}
	return t.From
}

// addToken appends " token" to a line, before a trailing ^block-id so the
// id stays last.
func addToken(line, token string) string {
	if strings.Contains(line, " "+token) {
		return line
	}
	if m := regexp.MustCompile(`\s\^[A-Za-z0-9][\w-]*\s*$`).FindStringIndex(line); m != nil && !strings.HasPrefix(strings.TrimSpace(line), "#") {
		return line[:m[0]] + " " + token + line[m[0]:]
	}
	return strings.TrimRight(line, " ") + " " + token
}

func (a *app) markCmd() *cobra.Command {
	var as, noteText string
	c := &cobra.Command{
		Use:   "mark <addr> --as <kind>",
		Short: "Flag a block, heading or note: pin, important, stale, verify, question, warning",
		Long: `Flag something so it can be found with pg marks and is labelled in
search results. Writes a #kind tag (and @note(...) with --note) on the
target's first line or heading.`,
		Example: `pg mark architecture#Deploy --as stale --note "moved to fly.io"
pg mark rules^r004 --as pin`,
		Args: cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			kind := strings.ToLower(strings.TrimPrefix(as, "#"))
			okKind := false
			for _, k := range markKinds {
				okKind = okKind || k == kind
			}
			if !okKind {
				return fail(ExitUsage, "--as must be one of %s", strings.Join(markKinds, ", "))
			}
			s, t, err := a.resolve(strings.Join(args, " "))
			if err != nil {
				return err
			}
			return a.withLock(s, func() error {
				n, err := a.load(s, t.File)
				if err != nil {
					return err
				}
				i := markLine(n, t)
				line := addToken(n.lines[i-1], "#"+kind)
				if noteText != "" {
					line = addToken(line, "@note("+strings.ReplaceAll(noteText, ")", "]")+")")
				}
				n.lines[i-1] = line
				if err := a.save(n); err != nil {
					return err
				}
				a.ok(fmt.Sprintf("%s [%s]", t.Addr(), kind))
				return nil
			})
		},
	}
	c.Flags().StringVar(&as, "as", "", strings.Join(markKinds, " | "))
	c.Flags().StringVar(&noteText, "note", "", "short reason, stored as @note(...)")
	return c
}

func (a *app) highlightCmd() *cobra.Command {
	var text string
	c := &cobra.Command{
		Use:     "highlight <addr> --text TEXT",
		Short:   "Wrap exact text inside the target in ==highlight==",
		Example: `pg highlight rules^r001 --text "busy_timeout"`,
		Args:    cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			if text == "" {
				return fail(ExitUsage, "--text is required")
			}
			s, t, err := a.resolve(strings.Join(args, " "))
			if err != nil {
				return err
			}
			return a.withLock(s, func() error {
				n, err := a.load(s, t.File)
				if err != nil {
					return err
				}
				for i := t.From; i <= t.To && i <= len(n.lines); i++ {
					l := n.lines[i-1]
					if j := strings.Index(l, text); j >= 0 && !strings.Contains(l, "=="+text+"==") {
						n.lines[i-1] = l[:j] + "==" + text + "==" + l[j+len(text):]
						if err := a.save(n); err != nil {
							return err
						}
						a.ok(fmt.Sprintf("%s:%d", t.File.Addr(), i))
						return nil
					}
				}
				return fail(ExitNotFound, "not found in %s: %q", t.Addr(), text)
			})
		},
	}
	c.Flags().StringVar(&text, "text", "", "exact text to highlight")
	return c
}

func (a *app) calloutCmd() *cobra.Command {
	var kind string
	c := &cobra.Command{
		Use:   "callout <addr> --kind KIND",
		Short: "Turn a block or line range into a > [!kind] callout (rule, warning, decision, pin, ...)",
		Example: `pg callout stack^b3 --kind warning
pg callout architecture:30-31 --kind rule`,
		Args: cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			if kind == "" {
				return fail(ExitUsage, "--kind is required (rule, warning, decision, question, pin, stale)")
			}
			s, t, err := a.resolve(strings.Join(args, " "))
			if err != nil {
				return err
			}
			if t.Kind == addr.KindFile || t.Kind == addr.KindSection {
				return fail(ExitUsage, "callout takes a block (note^id) or a line range (note:10-12)")
			}
			return a.withLock(s, func() error {
				n, err := a.load(s, t.File)
				if err != nil {
					return err
				}
				var block []string
				for i := t.From; i <= t.To; i++ {
					l := strings.TrimLeft(n.lines[i-1], " \t")
					l = strings.TrimPrefix(strings.TrimPrefix(l, "> "), ">")
					if i == t.From {
						if tl, ok := md.ParseTaskLine(l); ok {
							l = strings.TrimSpace(strings.TrimPrefix(l, tl.Indent+tl.Marker+" ["+string(tl.State)+"]"))
						} else {
							l, _ = stripMarker(l)
						}
						l = "[!" + strings.ToLower(kind) + "] " + l
					}
					block = append(block, "> "+l)
				}
				n.lines = replaceRange(n.lines, t.From, t.To, block)
				if err := a.save(n); err != nil {
					return err
				}
				a.ok(t.Addr())
				return nil
			})
		},
	}
	c.Flags().StringVar(&kind, "kind", "", "callout type")
	return c
}

var reMarkTokens = regexp.MustCompile(`\s+(?:#(?:pin|important|stale|verify|question|warning)\b|@note\([^)]*\))`)
var reHighlight = regexp.MustCompile(`==([^=]+)==`)

func (a *app) unmarkCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "unmark <addr>",
		Short: "Remove mark tags, @note(...) and ==highlights== from the target",
		Args:  cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, t, err := a.resolve(strings.Join(args, " "))
			if err != nil {
				return err
			}
			return a.withLock(s, func() error {
				n, err := a.load(s, t.File)
				if err != nil {
					return err
				}
				from := t.From
				if t.Kind == addr.KindSection {
					from = t.Section.Line
				}
				for i := from; i <= t.To && i <= len(n.lines); i++ {
					n.lines[i-1] = reHighlight.ReplaceAllString(reMarkTokens.ReplaceAllString(n.lines[i-1], ""), "$1")
				}
				if err := a.save(n); err != nil {
					return err
				}
				a.ok(t.Addr())
				return nil
			})
		},
	}
}

func (a *app) marksCmd() *cobra.Command {
	var as []string
	c := &cobra.Command{
		Use:     "marks",
		Short:   "Everything flagged: mark tags and callouts, with the marked line",
		Example: "pg marks\npg marks --as stale,verify",
		Args:    cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			want := map[string]bool{}
			for _, k := range splitList(as) {
				want[strings.ToLower(k)] = true
			}
			rows, err := s.db.Query(`SELECT file_id, COALESCE(block_id,''), line, kind, COALESCE(note,'') FROM marks ORDER BY file_id, line`)
			if err != nil {
				return fail(ExitInternal, "%v", err)
			}
			type mk struct {
				fid        int64
				block      string
				line       int
				kind, note string
			}
			var ms []mk
			for rows.Next() {
				var m mk
				rows.Scan(&m.fid, &m.block, &m.line, &m.kind, &m.note)
				if len(want) == 0 || want[m.kind] {
					ms = append(ms, m)
				}
			}
			rows.Close()
			if len(ms) == 0 {
				a.println("no results")
				return errNoResults
			}
			bw := a.newBudget(a.budgetOr(s.v.Cfg.Budget))
			cache := map[int64][]string{}
			for i, m := range ms {
				f, err := s.db.FileByID(m.fid)
				if err != nil || f.Inactive() {
					continue
				}
				lines, ok := cache[m.fid]
				if !ok {
					lines, _ = fileLines(s, f)
					cache[m.fid] = lines
				}
				at := fmt.Sprintf("%s:%d", f.Addr(), m.line)
				if m.block != "" {
					at = f.Addr() + "^" + m.block
				}
				text := ""
				if m.line >= 1 && m.line <= len(lines) {
					text = textutil.OneLine(reMarkTokens.ReplaceAllString(lines[m.line-1], ""), 110)
				}
				note := ""
				if m.note != "" {
					note = "(" + m.note + ")"
				}
				if !bw.line(join(at, "["+m.kind+"]", note, text)) {
					a.printf("... %d more; filter with --as\n", len(ms)-i)
					break
				}
			}
			return nil
		},
	}
	c.Flags().StringSliceVar(&as, "as", nil, "only these kinds (mark tags or callout types)")
	return c
}

package cli

import (
	"fmt"
	"regexp"
	"slices"
	"strings"

	"github.com/Utku4836/Paragma/internal/addr"
	"github.com/Utku4836/Paragma/internal/textutil"
	"github.com/spf13/cobra"
)

// Marks: flags, callouts and highlights on parts of notes.

var markKinds = []string{"pin", "important", "stale", "verify", "question", "warning"}

var (
	reMarkTokens = regexp.MustCompile(`\s+(?:#(?:pin|important|stale|verify|question|warning)\b|@note\([^)]*\))`)
	reHighlight  = regexp.MustCompile(`==([^=]+)==`)
)

func (a *app) markCmd() *cobra.Command {
	var as, noteText, callout, highlight string
	var remove bool
	c := &cobra.Command{
		Use:   "mark <addr> --as KIND | --callout KIND | --highlight TEXT | --remove",
		Short: "Flag a note, heading or block (pin, stale, warning...), make a callout, highlight text, or remove marks",
		Long: `Mark part of a note. Exactly one of:
  --as KIND        a #kind tag on the target's first line or heading (` + strings.Join(markKinds, ", ") + `);
                   --note adds a short reason as @note(...)
  --callout KIND   turn a block or line range into a > [!kind] callout (rule, warning, decision...)
  --highlight TEXT wrap that exact text in ==highlight==
  --remove         drop mark tags, @note(...) and ==highlights== from the target
pg marks lists everything flagged.`,
		Example: `pg mark architecture#Deploy --as stale --note "moved to fly.io"
pg mark stack^b3 --callout warning
pg mark rules^r001 --highlight "busy_timeout"
pg mark architecture#Deploy --remove`,
		Args: cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			given := 0
			for _, set := range []bool{as != "", callout != "", highlight != "", remove} {
				if set {
					given++
				}
			}
			if given != 1 {
				return fail(ExitUsage, "give exactly one of --as, --callout, --highlight, --remove")
			}
			var edit func(n *note, t *addr.Target) (string, error)
			switch {
			case as != "":
				kind := strings.ToLower(strings.TrimPrefix(as, "#"))
				if !slices.Contains(markKinds, kind) {
					return fail(ExitUsage, "--as must be one of %s", strings.Join(markKinds, ", "))
				}
				edit = func(n *note, t *addr.Target) (string, error) { return markTag(n, t, kind, noteText) }
			case callout != "":
				edit = func(n *note, t *addr.Target) (string, error) { return markCallout(n, t, callout) }
			case highlight != "":
				edit = func(n *note, t *addr.Target) (string, error) { return markHighlight(n, t, highlight) }
			default:
				edit = unmark
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
				done, err := edit(n, t)
				if err != nil {
					return err
				}
				if err := a.save(n); err != nil {
					return err
				}
				a.ok(done)
				return nil
			})
		},
	}
	c.Flags().StringVar(&as, "as", "", strings.Join(markKinds, " | "))
	c.Flags().StringVar(&noteText, "note", "", "with --as: short reason, stored as @note(...)")
	c.Flags().StringVar(&callout, "callout", "", "callout type: rule, warning, decision, question, pin...")
	c.Flags().StringVar(&highlight, "highlight", "", "exact text to wrap in ==highlight==")
	c.Flags().BoolVar(&remove, "remove", false, "remove marks and highlights")
	return c
}

// markTag writes #kind (and @note(...)) on the target's heading or first line.
func markTag(n *note, t *addr.Target, kind, noteText string) (string, error) {
	i := t.From
	switch t.Kind {
	case addr.KindSection:
		i = t.Section.Line
	case addr.KindFile:
		for i <= len(n.lines) && isBlank(n.lines[i-1]) {
			i++
		}
		if i > len(n.lines) {
			i = t.From
		}
	}
	line := addToken(n.lines[i-1], "#"+kind)
	if noteText != "" {
		line = addToken(line, "@note("+strings.ReplaceAll(noteText, ")", "]")+")")
	}
	n.lines[i-1] = line
	return fmt.Sprintf("%s [%s]", t.Addr(), kind), nil
}

// addToken appends " token" to a line, before a trailing ^block-id so the
// id stays last.
func addToken(line, token string) string {
	if strings.Contains(line, " "+token) {
		return line
	}
	if m := reTrailingID.FindStringIndex(line); m != nil && !strings.HasPrefix(strings.TrimSpace(line), "#") {
		return line[:m[0]] + " " + token + line[m[0]:]
	}
	return strings.TrimRight(line, " ") + " " + token
}

// markCallout turns a block or line range into a > [!kind] callout.
func markCallout(n *note, t *addr.Target, kind string) (string, error) {
	if t.Kind == addr.KindFile || t.Kind == addr.KindSection {
		return "", fail(ExitUsage, "--callout takes a block (note^id) or a line range (note:10-12)")
	}
	var block []string
	for i := t.From; i <= t.To; i++ {
		l := strings.TrimLeft(n.lines[i-1], " \t")
		l = strings.TrimPrefix(strings.TrimPrefix(l, "> "), ">")
		if i == t.From {
			l, _ = stripMarker(l)
			l = "[!" + strings.ToLower(kind) + "] " + l
		}
		block = append(block, "> "+l)
	}
	n.lines = replaceRange(n.lines, t.From, t.To, block)
	return t.Addr(), nil
}

// markHighlight wraps the first occurrence of text in the target.
func markHighlight(n *note, t *addr.Target, text string) (string, error) {
	for i := t.From; i <= t.To && i <= len(n.lines); i++ {
		l := n.lines[i-1]
		if j := strings.Index(l, text); j >= 0 && !strings.Contains(l, "=="+text+"==") {
			n.lines[i-1] = l[:j] + "==" + text + "==" + l[j+len(text):]
			return fmt.Sprintf("%s:%d", t.File.Addr(), i), nil
		}
	}
	return "", fail(ExitNotFound, "not found in %s: %q", t.Addr(), text)
}

// unmark drops mark tags, @note(...) and ==highlights== from the target.
func unmark(n *note, t *addr.Target) (string, error) {
	from := t.From
	if t.Kind == addr.KindSection {
		from = t.Section.Line
	}
	for i := from; i <= t.To && i <= len(n.lines); i++ {
		n.lines[i-1] = reHighlight.ReplaceAllString(reMarkTokens.ReplaceAllString(n.lines[i-1], ""), "$1")
	}
	return t.Addr(), nil
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

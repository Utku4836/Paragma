package cli

import (
	"fmt"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/Utku-Private/Paragma/internal/addr"
	"github.com/Utku-Private/Paragma/internal/index"
	"github.com/Utku-Private/Paragma/internal/textutil"
	"github.com/spf13/cobra"
)

// ---- decision record ----

func (a *app) decisionCmd() *cobra.Command {
	c := &cobra.Command{
		Use:   "decision",
		Short: "Architecture decision records (ADR): decision record",
	}
	var ctx, dec, alts, cons, supersedes, tags string
	rec := &cobra.Command{
		Use:   "record <title> --context TEXT --decision TEXT",
		Short: "Write a numbered decision record (context, decision, alternatives, consequences)",
		Example: `pg decision record "WAL mode for SQLite" --context "parallel tests locked the DB" --decision "open with WAL and busy_timeout=5000" --alternatives "serial tests: 3x slower CI"
pg decision record "Postgres for sessions" --decision "..." --supersedes 0007-redis-sessions`,
		Args: cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			title := strings.Join(args, " ")
			if dec == "" {
				return fail(ExitUsage, "--decision is required: what was decided")
			}
			s, err := a.main()
			if err != nil {
				return err
			}
			var tagList []string
			for _, t := range splitList([]string{tags}) {
				tagList = append(tagList, strings.TrimPrefix(t, "#"))
			}
			num := fmt.Sprintf("%04d", nextDecisionNumber(s))
			name := num + "-" + slug(title)
			lines := a.template(s, "decision", num+": "+title, tagList, "")
			for _, kv := range []struct{ key, text string }{{"context", ctx}, {"decision", dec}, {"options", alts}, {"outcome", cons}} {
				if kv.text != "" {
					lines = fillSection(lines, "## "+a.sec(s, kv.key), splitText(kv.text))
				}
			}
			var old *index.File
			if supersedes != "" {
				_, t, err := a.resolve(supersedes)
				if err != nil {
					return err
				}
				old = t.File
				lines = append(lines, "", "supersedes:: [["+shortRef(s, old)+"]]")
			}
			rel := freePath(s, "episodes/decisions/"+name+".md")
			return a.withLock(s, func() error {
				n := newNote(s, rel, lines)
				if err := a.save(n); err != nil {
					return err
				}
				if old != nil {
					if err := a.markSuperseded(s, old, strings.TrimSuffix(rel[strings.LastIndex(rel, "/")+1:], ".md")); err != nil {
						return err
					}
				}
				a.ok(n.addr())
				return nil
			})
		},
	}
	f := rec.Flags()
	f.StringVar(&ctx, "context", "", "why a decision was needed")
	f.StringVar(&dec, "decision", "", "what was decided")
	f.StringVar(&alts, "alternatives", "", "options considered and why not")
	f.StringVar(&cons, "consequences", "", "what follows from it")
	f.StringVar(&supersedes, "supersedes", "", "earlier decision this replaces")
	f.StringVar(&tags, "tags", "", "comma separated tags")
	c.AddCommand(rec)
	return c
}

// markSuperseded sets status: superseded and superseded_by on an old note.
func (a *app) markSuperseded(s *store, old *index.File, by string) error {
	g, err := s.res.ResolveFile(old.Addr())
	if err != nil {
		return err
	}
	n, err := a.load(s, g)
	if err != nil {
		return err
	}
	n.lines = setFront(n.lines, "status", "superseded")
	n.lines = setFront(n.lines, "superseded_by", strconv.Quote("[["+by+"]]"))
	n.lines = setFront(n.lines, "updated", today())
	return a.save(n)
}

func (a *app) journalCmd() *cobra.Command {
	return &cobra.Command{
		Use:     "journal <text|->",
		Short:   "Add a time-stamped line to this month's journal",
		Example: `pg journal "v0.3 released; migration 0014 ran in prod"`,
		Args:    cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			text, err := a.inputText(args, true)
			if err != nil {
				return err
			}
			s, err := a.main()
			if err != nil {
				return err
			}
			return a.withLock(s, func() error {
				j, err := a.journalAppend(s, textutil.OneLine(text, 2000))
				if err != nil {
					return err
				}
				a.ok(j)
				return nil
			})
		},
	}
}

// ---- rules ----

var reRuleID = regexp.MustCompile(`\^r(\d+)\b`)

// rulesFile returns knowledge/rules.md, creating it if needed.
func (a *app) rulesFile(s *store) (*index.File, error) {
	if f, err := s.res.ResolveFile("knowledge/rules"); err == nil {
		return f, nil
	}
	title := map[string]string{"tr": "Kurallar"}[s.v.Cfg.Lang]
	if title == "" {
		title = "Rules"
	}
	n := newNote(s, "knowledge/rules.md", a.template(s, "note", title, nil, ""))
	n.lines = setFront(n.lines, "type", "rule")
	if err := a.save(n); err != nil {
		return nil, err
	}
	return s.res.ResolveFile("knowledge/rules")
}

func nextRuleID(lines []string) string {
	maxN := 0
	for _, l := range lines {
		for _, m := range reRuleID.FindAllStringSubmatch(l, -1) {
			if n, _ := strconv.Atoi(m[1]); n > maxN {
				maxN = n
			}
		}
	}
	return fmt.Sprintf("r%03d", maxN+1)
}

// addRule appends "> [!rule] text ^rNNN" to a section of rules.md and
// returns the new rule's address.
func (a *app) addRule(s *store, text, section, from string) (string, error) {
	f, err := a.rulesFile(s)
	if err != nil {
		return "", err
	}
	n, err := a.load(s, f)
	if err != nil {
		return "", err
	}
	id := nextRuleID(n.lines)
	block := []string{"> [!rule] " + textutil.OneLine(text, 600) + " ^" + id}
	if from != "" {
		block = append(block, "> from [["+from+"]]")
	}
	if section == "" {
		section = map[string]string{"tr": "Genel"}[s.v.Cfg.Lang]
		if section == "" {
			section = "General"
		}
	}
	var target *index.Section
	secs, _ := s.res.Sections(f)
	for _, sec := range secs {
		if !sec.IsTitle && sec.Level > 0 && foldName(sec.Path) == foldName(section) {
			target = sec
		}
	}
	if target == nil {
		n.lines, _ = place(n.lines, lastLine(n.lines, 1, len(n.lines)), append([]string{"## " + section, ""}, block...))
	} else {
		n.lines, _ = place(n.lines, lastLine(n.lines, target.Line, target.End), block)
	}
	if err := a.save(n); err != nil {
		return "", err
	}
	return f.Addr() + "^" + id, nil
}

func (a *app) ruleCmd() *cobra.Command {
	c := &cobra.Command{
		Use:   "rule",
		Short: "Project rules (knowledge/rules.md): rule add, rule list",
	}
	var section string
	add := &cobra.Command{
		Use:     "add <rule text>",
		Short:   "Add a rule as a > [!rule] callout with an id, under a section",
		Example: `pg rule add "Open SQLite with WAL and busy_timeout=5000" --section Database`,
		Args:    cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			text, err := a.inputText(args, true)
			if err != nil {
				return err
			}
			s, err := a.main()
			if err != nil {
				return err
			}
			return a.withLock(s, func() error {
				ad, err := a.addRule(s, text, section, "")
				if err != nil {
					return err
				}
				a.ok(ad)
				return nil
			})
		},
	}
	add.Flags().StringVar(&section, "section", "", "heading in rules.md (created if missing)")

	var in string
	list := &cobra.Command{
		Use:     "list",
		Short:   "Every rule in the vault, one line each: address, section, text",
		Example: "pg rule list\npg rule list --section Money",
		Args:    cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			rows, err := s.db.Query(`SELECT m.file_id, COALESCE(m.block_id,''), m.line, COALESCE(s.heading_path,'') FROM marks m LEFT JOIN sections s ON s.id = m.section_id WHERE m.kind = 'rule' ORDER BY m.file_id, m.line`)
			if err != nil {
				return fail(ExitInternal, "%v", err)
			}
			type r struct {
				fid   int64
				block string
				line  int
				sec   string
			}
			var rs []r
			for rows.Next() {
				var x r
				rows.Scan(&x.fid, &x.block, &x.line, &x.sec)
				rs = append(rs, x)
			}
			rows.Close()
			bw := a.newBudget(a.budgetOr(max(s.v.Cfg.Budget, 1500)))
			cache := map[int64][]string{}
			shown := 0
			for i, x := range rs {
				f, err := s.db.FileByID(x.fid)
				if err != nil || f.Inactive() {
					continue
				}
				sec := x.sec
				if j := strings.LastIndex(sec, "/"); j >= 0 {
					sec = sec[j+1:]
				}
				if in != "" && !strings.Contains(textutil.Fold(sec), textutil.Fold(in)) {
					continue
				}
				lines, ok := cache[x.fid]
				if !ok {
					lines, _ = fileLines(s, f)
					cache[x.fid] = lines
				}
				text := ""
				if x.line >= 1 && x.line <= len(lines) {
					text = strings.TrimSpace(strings.TrimPrefix(strings.TrimLeft(lines[x.line-1], " >"), "[!rule]"))
					text = strings.TrimSpace(reRuleID.ReplaceAllString(text, ""))
					text = strings.TrimSpace(strings.TrimSuffix(text, "^"+x.block))
				}
				at := fmt.Sprintf("%s:%d", f.Addr(), x.line)
				if x.block != "" {
					at = f.Addr() + "^" + x.block
				}
				if !bw.line(join(at, "["+sec+"]", textutil.OneLine(text, 220))) {
					a.printf("… %d more; filter with --section\n", len(rs)-i)
					break
				}
				shown++
			}
			if shown == 0 {
				a.println("no results")
				return errNoResults
			}
			return nil
		},
	}
	list.Flags().StringVar(&in, "section", "", "only rules under this heading")
	c.AddCommand(add, list)
	return c
}

func (a *app) promoteCmd() *cobra.Command {
	var text, section string
	c := &cobra.Command{
		Use:   "promote <addr>",
		Short: "Turn a verified fix or note into a project rule, linked both ways",
		Long: `Make a rule from an episodic fix (or any heading/block) that proved
itself. The rule goes to knowledge/rules.md with "from [[source]]"; the
source gets promoted:: [[rules^rNNN]]. Use --text when the source is longer
than one rule sentence.`,
		Example: `pg promote sqlite-database-locked#Fix --text "Open SQLite with WAL and busy_timeout=5000" --section Database`,
		Args:    cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, t, err := a.resolve(strings.Join(args, " "))
			if err != nil {
				return err
			}
			if text == "" {
				lines, err := fileLines(s, t.File)
				if err != nil {
					return err
				}
				from := t.From
				if t.Kind == addr.KindSection {
					from = t.Section.Line + 1
				}
				var parts []string
				for _, l := range sliceLines(lines, from, t.To) {
					if l = strings.TrimSpace(strings.TrimLeft(l, "->*+ ")); l != "" && !isHeadingLine(l) {
						parts = append(parts, l)
					}
				}
				text = strings.Join(parts, " ")
				if len([]rune(text)) > 300 {
					return fail(ExitUsage, "%s is too long for one rule (%d chars); give the rule with --text", t.Addr(), len([]rune(text)))
				}
				if text == "" {
					return fail(ExitUsage, "%s is empty; give the rule with --text", t.Addr())
				}
			}
			srcRef := shortRef(s, t.File) + strings.TrimPrefix(t.Addr(), t.File.Addr())
			return a.withLock(s, func() error {
				ruleAddr, err := a.addRule(s, text, section, srcRef)
				if err != nil {
					return err
				}
				g, err := s.res.ResolveFile(t.File.Addr())
				if err != nil {
					return err
				}
				n, err := a.load(s, g)
				if err != nil {
					return err
				}
				link := "promoted:: [[" + strings.TrimPrefix(ruleAddr, "knowledge/") + "]]"
				n.lines, _ = place(n.lines, lastLine(n.lines, 1, len(n.lines)), []string{link})
				if err := a.save(n); err != nil {
					return err
				}
				a.ok(ruleAddr)
				return nil
			})
		},
	}
	c.Flags().StringVar(&text, "text", "", "the rule sentence (default: the target's text)")
	c.Flags().StringVar(&section, "section", "", "heading in rules.md")
	return c
}

func (a *app) supersedeCmd() *cobra.Command {
	return &cobra.Command{
		Use:     "supersede <old> <new>",
		Short:   "Mark a note as replaced by another: old sinks in search, both linked",
		Example: "pg supersede 0003-journal-mode 0007-wal-mode",
		Args:    cobra.ExactArgs(2),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, oldT, err := a.resolve(args[0])
			if err != nil {
				return err
			}
			_, newT, err := a.resolve(args[1])
			if err != nil {
				return err
			}
			if oldT.File.ID == newT.File.ID {
				return fail(ExitUsage, "a note cannot supersede itself")
			}
			return a.withLock(s, func() error {
				if err := a.markSuperseded(s, oldT.File, shortRef(s, newT.File)); err != nil {
					return err
				}
				g, err := s.res.ResolveFile(newT.File.Addr())
				if err != nil {
					return err
				}
				n, err := a.load(s, g)
				if err != nil {
					return err
				}
				n.lines, _ = place(n.lines, lastLine(n.lines, 1, len(n.lines)), []string{"supersedes:: [[" + shortRef(s, oldT.File) + "]]"})
				if err := a.save(n); err != nil {
					return err
				}
				a.ok(oldT.File.Addr() + " [superseded by " + newT.File.Addr() + "]")
				return nil
			})
		},
	}
}

func (a *app) verifyCmd() *cobra.Command {
	return &cobra.Command{
		Use:     "verify <addr>...",
		Short:   "You checked it and it is still true: set verified to today",
		Example: "pg verify stack architecture",
		Args:    cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			for _, arg := range args {
				s, t, err := a.resolve(arg)
				if err != nil {
					return err
				}
				err = a.withLock(s, func() error {
					n, err := a.load(s, t.File)
					if err != nil {
						return err
					}
					n.lines = setFront(n.lines, "verified", today())
					if err := a.save(n); err != nil {
						return err
					}
					a.ok(t.File.Addr() + " verified " + today())
					return nil
				})
				if err != nil {
					return err
				}
			}
			return nil
		},
	}
}

func (a *app) staleCmd() *cobra.Command {
	var days int
	c := &cobra.Command{
		Use:     "stale",
		Short:   "Semantic notes not verified for a while (default: stale_days from config)",
		Example: "pg stale\npg stale --days 30",
		Args:    cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			if days <= 0 {
				days = s.v.Cfg.StaleDays
			}
			type row struct {
				f   *index.File
				age int
			}
			var rows []row
			for _, f := range s.res.Files() {
				if !f.Stale(days) {
					continue
				}
				d := f.Verified
				if d == "" {
					d = f.Updated
				}
				age := 0
				if t, err := time.Parse("2006-01-02", d[:min(len(d), 10)]); err == nil {
					age = int(time.Since(t).Hours() / 24)
				}
				rows = append(rows, row{f, age})
			}
			if len(rows) == 0 {
				a.println("no results")
				return errNoResults
			}
			sort.Slice(rows, func(i, j int) bool { return rows[i].age > rows[j].age })
			for _, r := range rows {
				a.println(join(r.f.Addr(), fmt.Sprintf("verified %s (%dd ago)", r.f.Date(), r.age), textutil.FmtTokens(r.f.Tokens)))
			}
			a.println("next: check it, then pg verify <addr> | pg mark <addr> --as stale")
			return nil
		},
	}
	c.Flags().IntVar(&days, "days", 0, "older than this many days")
	return c
}

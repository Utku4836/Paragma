package cli

import (
	"fmt"
	"regexp"
	"strconv"
	"strings"

	"github.com/Utku4836/Paragma/internal/addr"
	"github.com/Utku4836/Paragma/internal/index"
	"github.com/Utku4836/Paragma/internal/space"
	"github.com/Utku4836/Paragma/internal/textutil"
	"github.com/spf13/cobra"
)

// Project rules (<project>/rules.md) and promoting a fix to a rule.

var reRuleID = regexp.MustCompile(`\^r(\d+)\b`)

// rulesFile returns <project>/rules.md, creating it if needed.
func (a *app) rulesFile(s *store, p string) (*index.File, error) {
	if f, err := s.res.ResolveFile(p + "/rules"); err == nil && f.Path == p+"/rules.md" {
		return f, nil
	}
	title := map[string]string{"tr": "Kurallar"}[s.v.Cfg.Lang]
	if title == "" {
		title = "Rules"
	}
	n := newNote(s, p+"/rules.md", a.template(s, "note", title, nil, ""))
	n.lines = setFront(n.lines, "type", "rule")
	if err := a.save(n); err != nil {
		return nil, err
	}
	return s.res.ResolveFile(p + "/rules")
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

// addRule appends "> [!rule] text ^rNNN" to a section of project p's
// rules.md and returns the new rule's address.
func (a *app) addRule(s *store, p, text, section, from string) (string, error) {
	f, err := a.rulesFile(s, p)
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
		Short: "Project rules (<project>/rules.md): rule add, rule list",
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
			p, err := a.writeProject(s)
			if err != nil {
				return err
			}
			return a.withLock(s, func() error {
				ad, err := a.addRule(s, p, text, section, "")
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
		Short:   "Every rule in the space, one line each: address, section, text",
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
					a.printf("... %d more; filter with --section\n", len(rs)-i)
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
itself. The rule goes to the project's rules.md with "from [[source]]"; the
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
				ruleAddr, err := a.promote(s, t.File, srcRef, text, section)
				if err != nil {
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

// promote makes text a rule of src's project, citing srcRef, and links src
// to the rule with promoted:: (the caller holds the lock).
func (a *app) promote(s *store, src *index.File, srcRef, text, section string) (string, error) {
	p := space.ProjectOf(src.Path)
	if p == "" {
		var err error
		if p, err = a.writeProject(s); err != nil {
			return "", err
		}
	}
	ruleAddr, err := a.addRule(s, p, text, section, srcRef)
	if err != nil {
		return "", err
	}
	g, err := s.res.ResolveFile(src.Addr()) // the index changed: resolve again
	if err != nil {
		return "", err
	}
	n, err := a.load(s, g)
	if err != nil {
		return "", err
	}
	n.lines, _ = place(n.lines, lastLine(n.lines, 1, len(n.lines)), []string{"promoted:: [[" + ruleAddr + "]]"})
	return ruleAddr, a.save(n)
}

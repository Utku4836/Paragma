package cli

import (
	"fmt"
	"regexp"
	"sort"
	"strings"
	"time"

	"github.com/Utku4836/Paragma/internal/addr"
	"github.com/Utku4836/Paragma/internal/index"
	"github.com/Utku4836/Paragma/internal/textutil"
	"github.com/Utku4836/Paragma/internal/ulid"
	"github.com/spf13/cobra"
)

func (a *app) grepCmd() *cobra.Command {
	var ignoreCase, all bool
	var layers []string
	c := &cobra.Command{
		Use:   "grep <regex>",
		Short: "Raw regular expression search over note lines; prints note:line addresses",
		Long: `Search note text line by line with a Go regular expression. Use it for
exact strings (identifiers, config keys) where pg search's word matching is
too loose. Archived notes are skipped unless --all.`,
		Example: `pg grep "busy_timeout"
pg grep -i "ECONN\w+" --layer episodic`,
		Args: cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			pat := strings.Join(args, " ")
			if ignoreCase {
				pat = "(?i)" + pat
			}
			re, err := regexp.Compile(pat)
			if err != nil {
				return fail(ExitUsage, "bad regex: %v", err)
			}
			ls, err := normLayers(layers)
			if err != nil {
				return err
			}
			want := map[string]bool{}
			for _, l := range ls {
				want[l] = true
			}
			s, err := a.main()
			if err != nil {
				return err
			}
			bw := a.newBudget(a.budgetOr(s.v.Cfg.Budget))
			hits := 0
			for _, f := range s.res.Files() {
				if (!all && f.Inactive()) || (len(want) > 0 && !want[f.Layer]) {
					continue
				}
				lines, err := fileLines(s, f)
				if err != nil {
					continue
				}
				for i, l := range lines {
					if !re.MatchString(l) {
						continue
					}
					hits++
					if bw.cut {
						continue
					}
					if !bw.line(fmt.Sprintf("%s:%d  %s", f.Addr(), i+1, textutil.OneLine(l, 160))) {
						continue
					}
				}
			}
			if hits == 0 {
				a.println("no results")
				return errNoResults
			}
			if bw.cut {
				a.printf("... %d matches in all; narrow the pattern or --layer\n", hits)
			}
			return nil
		},
	}
	c.Flags().BoolVarP(&ignoreCase, "ignore-case", "i", false, "case-insensitive")
	c.Flags().BoolVar(&all, "all", false, "include archived and superseded notes")
	c.Flags().StringSliceVar(&layers, "layer", nil, "working|episodic|semantic")
	return c
}

func (a *app) recentCmd() *cobra.Command {
	var since string
	var limit int
	c := &cobra.Command{
		Use:     "recent",
		Short:   "Notes changed lately, newest first",
		Example: "pg recent\npg recent --since 3d",
		Args:    cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			var cutoff time.Time
			if since != "" {
				d, err := parseDate(since)
				if err != nil {
					return err
				}
				cutoff, _ = time.ParseInLocation("2006-01-02", d, time.Local)
			}
			var fs []*index.File
			for _, f := range s.res.Files() {
				if f.Mtime > 0 && (cutoff.IsZero() || time.Unix(0, f.Mtime).After(cutoff)) {
					fs = append(fs, f)
				}
			}
			if len(fs) == 0 {
				a.println("no results")
				return errNoResults
			}
			sort.Slice(fs, func(i, j int) bool { return fs[i].Mtime > fs[j].Mtime })
			for _, f := range fs[:min(len(fs), limit)] {
				a.println(join(f.Addr(), a.fileMeta(s, f), "changed "+ago(time.Unix(0, f.Mtime)), textutil.FmtTokens(f.Tokens)))
			}
			return nil
		},
	}
	c.Flags().StringVar(&since, "since", "", "only changes since: 3d, 2w, 2026-10-01")
	c.Flags().IntVar(&limit, "limit", 15, "max notes")
	return c
}

func ago(t time.Time) string {
	d := time.Since(t)
	switch {
	case d < time.Minute:
		return "just now"
	case d < time.Hour:
		return fmt.Sprintf("%dm ago", int(d.Minutes()))
	case d < 48*time.Hour:
		return fmt.Sprintf("%dh ago", int(d.Hours()))
	}
	return fmt.Sprintf("%dd ago", int(d.Hours()/24))
}

func (a *app) doctorCmd() *cobra.Command {
	var fix bool
	c := &cobra.Command{
		Use:   "doctor [--fix]",
		Short: "Check the space: front matter errors, duplicate ids, broken links, missing ids",
		Long: `List problems in the vault, one per line with its address. --fix adds
missing ids. Exit 6 when problems remain.`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			var problems []string
			var missing []*index.File
			ids := map[string][]string{}
			for _, f := range s.res.Files() {
				if f.FMError != "" {
					problems = append(problems, fmt.Sprintf("%s  front matter does not parse: %s", f.Addr(), textutil.OneLine(f.FMError, 120)))
				}
				if f.ULID != "" {
					ids[f.ULID] = append(ids[f.ULID], f.Addr())
				} else if f.Type != "journal" && f.FMError == "" {
					missing = append(missing, f)
				}
			}
			for id, as := range ids {
				if len(as) > 1 {
					problems = append(problems, fmt.Sprintf("@%s  same id in %s", id, strings.Join(as, ", ")))
				}
			}
			links, _ := s.db.AllLinks()
			for _, l := range links {
				if !linkResolves(s.res, l) {
					src, err := s.db.FileByID(l.SrcFile)
					if err == nil {
						problems = append(problems, fmt.Sprintf("%s:%d  broken link [[%s]]", src.Addr(), l.Line, l.Target))
					}
				}
			}
			if fix && len(missing) > 0 {
				err := a.withLock(s, func() error {
					for _, f := range missing {
						n, err := a.load(s, f)
						if err != nil {
							return err
						}
						n.lines = setFront(n.lines, "id", ulid.New())
						if err := a.save(n); err != nil {
							return err
						}
					}
					return nil
				})
				if err != nil {
					return err
				}
				a.ok(fmt.Sprintf("added ids to %d notes", len(missing)))
				missing = nil
			}
			for _, f := range missing {
				problems = append(problems, fmt.Sprintf("%s  no id (pg doctor --fix adds one)", f.Addr()))
			}
			sort.Strings(problems)
			if len(problems) == 0 {
				a.println("ok no problems")
				return nil
			}
			bw := a.newBudget(a.budgetOr(s.v.Cfg.Budget))
			for i, p := range problems {
				if !bw.line(p) {
					a.printf("... %d more\n", len(problems)-i)
					break
				}
			}
			return &Error{Code: ExitValidation}
		},
	}
	c.Flags().BoolVar(&fix, "fix", false, "add missing ids")
	return c
}

var _ = addr.KindFile

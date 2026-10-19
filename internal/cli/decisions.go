package cli

import (
	"fmt"
	"strconv"
	"strings"

	"github.com/Utku4836/Paragma/internal/index"
	"github.com/spf13/cobra"
)

// Decision records (ADRs) and superseding one note by another.

func (a *app) decisionCmd() *cobra.Command {
	c := &cobra.Command{
		Use:   "decision",
		Short: "Architecture decision records (ADR): decision record",
	}
	var ctx, dec, alts, cons, supersedes, tags, keywords string
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
			p, err := a.writeProject(s)
			if err != nil {
				return err
			}
			var tagList []string
			for _, t := range splitList([]string{tags}) {
				tagList = append(tagList, strings.TrimPrefix(t, "#"))
			}
			rel, lines := a.decisionNote(s, p, title, tagList, keywords, ctx, dec, alts, cons)
			var old *index.File
			if supersedes != "" {
				_, t, err := a.resolve(supersedes)
				if err != nil {
					return err
				}
				old = t.File
				lines = append(lines, "", "supersedes:: [["+shortRef(s, old)+"]]")
			}
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
				a.suggestLinks(s, n.addr(), title+" "+ctx+" "+dec)
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
	f.StringVar(&keywords, "keywords", "", "comma separated search words: synonyms, other language")
	c.AddCommand(rec)
	return c
}

// decisionNote builds a numbered decision record for project p.
func (a *app) decisionNote(s *store, p, title string, tags []string, keywords, ctx, dec, alts, cons string) (string, []string) {
	num := fmt.Sprintf("%04d", nextDecisionNumber(s, p))
	lines := withKeywords(a.template(s, "decision", num+": "+title, tags, ""), keywords)
	for _, kv := range []struct{ key, text string }{{"context", ctx}, {"decision", dec}, {"options", alts}, {"outcome", cons}} {
		if kv.text != "" {
			lines = fillSection(lines, "## "+a.sec(s, kv.key), splitText(kv.text))
		}
	}
	return freePath(s, p+"/decisions/"+num+"-"+slug(title)+".md"), lines
}

// writeDecision saves a decision record (the caller holds the lock).
func (a *app) writeDecision(s *store, p, title, ctx, dec, alts, cons string) (string, error) {
	rel, lines := a.decisionNote(s, p, title, nil, "", ctx, dec, alts, cons)
	n := newNote(s, rel, lines)
	if err := a.save(n); err != nil {
		return "", err
	}
	return n.addr(), nil
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

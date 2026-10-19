package cli

import (
	"strings"

	"github.com/Utku4836/Paragma/internal/textutil"
	"github.com/spf13/cobra"
)

// The project journal: dated, time-stamped lines, one file per month.

func (a *app) journalCmd() *cobra.Command {
	return &cobra.Command{
		Use:     "journal <text|->",
		Short:   "Add a time-stamped line to the current project's journal (this month)",
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
			p, err := a.writeProject(s)
			if err != nil {
				return err
			}
			return a.withLock(s, func() error {
				j, err := a.journalAppend(s, p, textutil.OneLine(text, 2000))
				if err != nil {
					return err
				}
				a.ok(j)
				return nil
			})
		},
	}
}

// journalAppend adds "- HH:MM text" under today's heading in the project's
// journal/YYYY-MM.md and returns the day's address.
func (a *app) journalAppend(s *store, p, text string) (string, error) {
	when := now()
	month := when.Format("2006-01")
	rel := "journal/" + month + ".md"
	if p != "" {
		rel = p + "/" + rel
	}
	day := when.Format("2006-01-02")
	entry := "- " + when.Format("15:04") + " " + text
	var n *note
	if f, err := s.res.ResolveFile(strings.TrimSuffix(rel, ".md")); err == nil && f.Path == rel {
		if n, err = a.load(s, f); err != nil {
			return "", err
		}
	} else {
		n = newNote(s, rel, []string{"---", "type: journal", "layer: episodic", "---", "# " + month})
	}
	at := -1
	for i, l := range n.lines {
		if strings.TrimSpace(l) == "## "+day {
			at = i + 1
		}
	}
	if at < 0 {
		n.lines, _ = place(n.lines, lastLine(n.lines, 1, len(n.lines)), []string{"## " + day, "", entry})
	} else {
		end := len(n.lines)
		for i := at; i < len(n.lines); i++ {
			if isHeadingLine(n.lines[i]) {
				end = i
				break
			}
		}
		n.lines, _ = place(n.lines, lastLine(n.lines, at, end), []string{entry})
	}
	if err := a.save(n); err != nil {
		return "", err
	}
	return strings.TrimSuffix(rel, ".md") + "#" + day, nil
}

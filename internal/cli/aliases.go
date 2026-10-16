package cli

import (
	"strings"

	"github.com/spf13/cobra"
)

// guessedCmds are hidden commands for names agents tried in test runs
// ("pg tasks") or are likely to try: each runs the command it means, so a
// wrong guess costs no extra round trip through pg help.
func (a *app) guessedCmds() []*cobra.Command {
	sub := func(name string, parent *cobra.Command, path ...string) *cobra.Command {
		c, _, err := parent.Find(path)
		if err != nil || c == parent {
			return nil
		}
		parent.RemoveCommand(c)
		c.Use = name + strings.TrimPrefix(c.Use, strings.Fields(c.Use)[0])
		c.Aliases = nil
		c.Hidden = true
		return c
	}
	var out []*cobra.Command
	for _, c := range []*cobra.Command{
		sub("tasks", a.taskCmd(), "list"),
		sub("todo", a.taskCmd(), "list"),
		sub("next", a.taskCmd(), "next"),
		sub("projects", a.projectCmd(), "list"),
		sub("spaces", a.spaceCmd(), "list"),
		sub("rules", a.ruleCmd(), "list"),
		sub("close", a.projectCmd(), "close"),
	} {
		if c != nil {
			out = append(out, c)
		}
	}
	return out
}

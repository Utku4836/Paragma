package cli

import (
	"errors"
	"fmt"
	"io"
	"os"
	"strings"

	"github.com/spf13/cobra"
)

// Version is set at build time with -ldflags "-X .../internal/cli.Version=...".
var Version = "0.1.0-dev (Paragma)"

// Execute runs pg and returns the process exit code.
func Execute(args []string, stdout, stderr io.Writer, stdin io.Reader) int {
	a := newApp(stdout, stdin)
	root := a.rootCmd()
	root.SetArgs(args)
	root.SetOut(a.stdout)
	root.SetErr(stderr)
	err := root.Execute()
	a.close()
	if err == nil {
		return ExitOK
	}
	var e *Error
	if errors.As(err, &e) {
		if e.Msg != "" {
			fmt.Fprintln(stderr, "error: "+e.Msg)
		}
		return e.Code
	}
	// cobra parse errors: unknown flag, wrong arg count, unknown command
	msg := err.Error()
	fmt.Fprintln(stderr, "error: "+msg)
	if cmd, _, ferr := root.Find(args); ferr == nil && cmd != root {
		fmt.Fprintf(stderr, "usage: %s (pg help %s)\n", cmd.UseLine(), cmd.Name())
	} else {
		fmt.Fprintln(stderr, "see: pg help")
	}
	return ExitUsage
}

func (a *app) rootCmd() *cobra.Command {
	root := &cobra.Command{
		Use:           "pg",
		Short:         "Paragma: agent memory",
		SilenceUsage:  true,
		SilenceErrors: true,
		Version:       Version,
	}
	root.CompletionOptions.DisableDefaultCmd = true
	pf := root.PersistentFlags()
	pf.BoolVar(&a.jsonOut, "json", false, "structured output")
	pf.IntVar(&a.budget, "budget", 0, "cap output at N tokens")
	pf.StringVar(&a.vaultFlag, "vault", "", "vault path (default: nearest .paragma, else ~/.paragma)")
	pf.StringVar(&a.scope, "scope", "", "project|global|all")
	pf.BoolVar(&a.quiet, "quiet", false, "print addresses / exit code only")

	root.PersistentPreRunE = func(cmd *cobra.Command, args []string) error {
		switch a.scope {
		case "", "project", "global", "all":
		default:
			return fail(ExitUsage, "--scope must be project, global or all")
		}
		return nil
	}

	root.AddCommand(
		a.initCmd(), a.statusCmd(), a.reindexCmd(),
		a.searchCmd(), a.findCmd(),
		a.outlineCmd(), a.readCmd(), a.peekCmd(),
		a.linksCmd(), a.backlinksCmd(),
	)
	root.SetHelpCommand(a.helpCmd(root))
	root.SetHelpFunc(func(c *cobra.Command, _ []string) { a.printHelp(c) })
	root.SetFlagErrorFunc(func(c *cobra.Command, err error) error { return err })
	return root
}

// groups is the help index printed by pg help.
var groups = []struct{ name, cmds string }{
	{"read", "outline search read peek links backlinks find"},
	{"admin", "init status reindex"},
}

func (a *app) helpCmd(root *cobra.Command) *cobra.Command {
	return &cobra.Command{
		Use:   "help [command]",
		Short: "Help for pg or one command",
		RunE: func(cmd *cobra.Command, args []string) error {
			if len(args) == 0 {
				a.printHelp(root)
				return nil
			}
			c, _, err := root.Find(args)
			if err != nil || c == root {
				return fail(ExitUsage, "unknown command %q (see: pg help)", strings.Join(args, " "))
			}
			a.printHelp(c)
			return nil
		},
	}
}

func (a *app) printHelp(c *cobra.Command) {
	if c.Parent() == nil {
		a.println("pg - agent memory (Paragma). Notes are Markdown under .paragma/; pg returns only the part you need.")
		a.println("start: pg outline (map of notes) -> pg search <words> -> pg read <addr> [<addr>...] (copy addresses from output)")
		a.println("addresses: note | note#Heading | note#Up/Down | note^block | note:10-24 | @id | ~note (global)")
		a.println("commands (pg help <command>):")
		for _, g := range groups {
			a.printf("  %-6s %s\n", g.name, g.cmds)
		}
		a.println("rule: read memory through pg, not by opening .paragma files; pg read prints file:lines for citing")
		a.println("flags: --json --budget N --vault PATH --scope project|global|all")
		a.println("exit: 0 ok, 1 no results, 2 usage, 3 not found, 5 no vault, 7 internal")
		return
	}
	a.printf("usage: %s\n", c.UseLine())
	if c.Long != "" {
		a.println(strings.TrimSpace(c.Long))
	} else {
		a.println(c.Short)
	}
	if fs := c.LocalNonPersistentFlags(); fs.HasAvailableFlags() {
		a.println("flags:")
		a.stdout.WriteString(fs.FlagUsages())
	}
	if c.Example != "" {
		a.println("examples:")
		for _, l := range strings.Split(strings.TrimSpace(c.Example), "\n") {
			a.println("  " + strings.TrimSpace(l))
		}
	}
}

func init() {
	// cobra prints "Error:" itself unless silenced; we also want mousetrap off
	// so pg can be launched from Explorer-less contexts on Windows.
	cobra.MousetrapHelpText = ""
	_ = os.Stderr
}

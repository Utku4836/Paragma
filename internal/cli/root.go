package cli

import (
	"errors"
	"fmt"
	"io"
	"os"
	"regexp"
	"strings"

	"github.com/spf13/cobra"
	"github.com/spf13/pflag"
)

// Version is set at build time with -ldflags "-X .../internal/cli.Version=...".
var Version = "0.1.2-dev (Paragma)"

// Execute runs pg and returns the process exit code.
func Execute(args []string, stdout, stderr io.Writer, stdin io.Reader) int {
	a := newApp(stdout, stdin)
	a.rawErr = stderr
	root := a.rootCmd()
	unprotect(root)
	root.SetArgs(protectArgs(args))
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
		if e.Code == ExitUsage && e.Msg != "" { // not the exit code of a command pg run ran
			if cmd, _, ferr := root.Find(args); ferr == nil && cmd != root && cmd.Example != "" {
				fmt.Fprintf(stderr, "example: %s\n", strings.TrimSpace(strings.SplitN(cmd.Example, "\n", 2)[0]))
			}
		}
		return e.Code
	}
	// cobra parse errors: unknown flag, wrong arg count, unknown command
	msg := err.Error()
	fmt.Fprintln(stderr, "error: "+msg)
	if cmd, _, ferr := root.Find(args); ferr == nil && cmd != root {
		// the correct form right away, so the agent does not need pg help
		fmt.Fprintf(stderr, "usage: %s\n", cmd.UseLine())
		if cmd.Example != "" {
			fmt.Fprintf(stderr, "example: %s\n", strings.TrimSpace(strings.SplitN(cmd.Example, "\n", 2)[0]))
		}
		if cmd.HasAvailableSubCommands() {
			var names []string
			for _, sc := range cmd.Commands() {
				if sc.IsAvailableCommand() && sc.Name() != "help" {
					names = append(names, sc.Name())
				}
			}
			fmt.Fprintf(stderr, "subcommands: %s\n", strings.Join(names, " "))
		}
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
	pf.StringVar(&a.spaceFlag, "space", "", "space to use (default: the one mapped to this directory, else main)")
	pf.StringVar(&a.homeFlag, "home", "", "Paragma home (default: $PARAGMA_HOME, else ~/.paragma)")
	pf.MarkHidden("home")
	pf.BoolVar(&a.quiet, "quiet", false, "print addresses / exit code only")
	pf.StringVar(&a.ifHash, "if-hash", "", "write only if the note still has this hash (from pg read)")
	pf.BoolVar(&a.dryRun, "dry-run", false, "show what a write would change, write nothing")
	pf.StringVar(&a.textFile, "file", "", "read the text to write from this file (UTF-8 or UTF-16)")

	root.AddCommand(
		a.setupCmd(), a.statusCmd(), a.reindexCmd(), a.spaceCmd(),
		a.startCmd(), a.resumeCmd(), a.planCmd(), a.logCmd(), a.decideCmd(), a.checkpointCmd(), a.taskDoneCmd(),
		a.searchCmd(), a.findCmd(),
		a.outlineCmd(), a.readCmd(), a.peekCmd(),
		a.linksCmd(), a.backlinksCmd(),
		a.newCmd(), a.appendCmd(), a.writeCmd(), a.insertCmd(), a.editCmd(), a.sectionCmd(), a.linkCmd(),
		a.mvCmd(), a.archiveCmd(), a.rmCmd(),
		a.taskCmd(), a.projectCmd(),
		a.markCmd(), a.highlightCmd(), a.calloutCmd(), a.unmarkCmd(), a.marksCmd(),
		a.errorCmd(), a.runCmd(), a.decisionCmd(), a.journalCmd(), a.ruleCmd(), a.promoteCmd(), a.supersedeCmd(),
		a.verifyCmd(), a.staleCmd(), a.primeCmd(), a.indexCmd(), a.skillCmd(), a.installCmd(), a.hookCmd(),
		a.grepCmd(), a.recentCmd(), a.doctorCmd(),
		a.relatedCmd(), a.contextCmd(), a.neighborsCmd("neighbors"), a.graphCmd(), a.orphansCmd(), a.brokenCmd(),
		a.keywordsCmd(), a.missesCmd(), a.dupesCmd(), a.gcCmd(),
	)
	root.AddCommand(a.guessedCmds()...)
	root.SetHelpCommand(a.helpCmd(root))
	root.SetHelpFunc(func(c *cobra.Command, _ []string) { a.printHelp(c) })
	root.SetFlagErrorFunc(func(c *cobra.Command, err error) error { return err })
	return root
}

// groups is the help index printed by pg help.
var groups = []struct{ name, cmds string }{
	{"read", "outline search read peek links backlinks find grep recent index"},
	{"write", "new append write insert edit section link keywords mv archive rm"},
	{"project", "start resume plan done log decide checkpoint project list|status|close"},
	{"task", "task add|list|show|start|done|block|cancel|ask|reopen|note|move|edit|next|claim"},
	{"error", "run  error record|find|seen|reopen  decision record  journal"},
	{"rule", "rule add|list  promote supersede verify stale"},
	{"graph", "related context neighbors graph orphans broken"},
	{"mark", "mark highlight callout unmark marks"},
	{"agent", "setup prime skill"},
	{"admin", "space list|new|use  status reindex doctor dupes gc misses"},
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
				// a group from the pg help index that is not itself a command
				for _, g := range groups {
					if len(args) == 1 && g.name == args[0] {
						a.printGroup(root, g.cmds)
						return nil
					}
				}
				return fail(ExitUsage, "unknown command %q (see: pg help)", strings.Join(args, " "))
			}
			a.printHelp(c)
			return nil
		},
	}
}

func (a *app) printHelp(c *cobra.Command) {
	if c.Parent() == nil {
		a.println("pg - agent memory (Paragma). Notes are Markdown in project folders of a space (~/.paragma/Spaces/<space>/<project>/); pg returns only the part you need.")
		a.println("find: pg context <topic> (everything on a topic, one call) | pg search <words> -> pg read <addr> [<addr>...] (copy addresses from output)")
		a.println("addresses: project/note | note | note#Heading | note#Up/Down | note^block | note:10-24 | @id")
		a.println("commands (pg help <command>):")
		for _, g := range groups {
			a.printf("  %-7s %s\n", g.name, g.cmds)
		}
		a.println(`projects: pg start <name> --goal "..." --step "..." --decision "..." (once) | pg resume <name> (new conversation, or switch) | pg done <id> prints the next task | pg log / pg decide`)
		a.println("rule: read memory through pg, not by opening the files; cite a note by its address (pg read prints its lines as L10-24)")
		a.println("text: as arguments, or - for stdin, or --file F (PowerShell 5.1 pipes drop non-ASCII); writes print ok <addr>")
		a.println("flags: --json --budget N --space NAME")
		a.println("exit: 0 ok, 1 no results, 2 usage, 3 not found, 4 conflict (exists, --if-hash, lock), 5 no such space, 6 refused (e.g. open subtasks), 7 internal")
		return
	}
	if c.HasAvailableSubCommands() {
		// a group: one line per command with its first example
		a.printf("%s - %s\n", c.CommandPath(), c.Short)
		for _, sc := range c.Commands() {
			if !sc.IsAvailableCommand() || sc.Name() == "help" {
				continue
			}
			ex := ""
			if sc.Example != "" {
				ex = "  e.g. " + strings.TrimSpace(strings.SplitN(sc.Example, "\n", 2)[0])
			}
			a.printf("  %-11s %s%s\n", sc.Name(), sc.Short, ex)
		}
		a.printf("details: pg help %s <command>\n", c.Name())
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

// textMark hides a leading "-" from the flag parser. Markdown text such as
// "- item" or "- [ ] task" starts with a dash; without this, pg would read it
// as an unknown shorthand flag and refuse the command.
const textMark = "\x1f"

var reFlag = regexp.MustCompile(`^--?[A-Za-z][\w-]*(=.*)?$`)

func protectArgs(args []string) []string {
	out := make([]string, len(args))
	for i, a := range args {
		if len(a) > 1 && a[0] == '-' && a != "--" && !reFlag.MatchString(a) {
			a = textMark + a
		}
		out[i] = a
	}
	return out
}

// unprotect wraps every command so arguments and string flag values arrive
// without the mark protectArgs added.
func unprotect(c *cobra.Command) {
	if run := c.RunE; run != nil {
		c.RunE = func(cmd *cobra.Command, args []string) error {
			for i := range args {
				args[i] = strings.TrimPrefix(args[i], textMark)
			}
			cmd.Flags().VisitAll(func(f *pflag.Flag) {
				if f.Value.Type() == "string" && strings.HasPrefix(f.Value.String(), textMark) {
					f.Value.Set(strings.TrimPrefix(f.Value.String(), textMark))
				}
			})
			return run(cmd, args)
		}
	}
	for _, sc := range c.Commands() {
		unprotect(sc)
	}
}

// printGroup lists the commands of a help group, one line each with its
// first example.
func (a *app) printGroup(root *cobra.Command, cmds string) {
	for _, name := range strings.Fields(cmds) {
		name = strings.SplitN(name, "|", 2)[0]
		c, _, err := root.Find([]string{name})
		if err != nil || c == root {
			continue
		}
		if c.HasAvailableSubCommands() {
			a.printf("  %-10s %s (pg help %s)\n", c.Name(), c.Short, c.Name())
			continue
		}
		ex := ""
		if c.Example != "" {
			ex = "  e.g. " + strings.TrimSpace(strings.SplitN(c.Example, "\n", 2)[0])
		}
		a.printf("  %-10s %s%s\n", c.Name(), c.Short, ex)
	}
}

package cli

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/Utku4836/Paragma/internal/agents"
	"github.com/Utku4836/Paragma/internal/space"
	"github.com/spf13/cobra"
	"golang.org/x/term"
)

// pg setup connects Paragma to the agents installed on this computer: for
// every conversation (the agent's global instructions file) or for one
// conversation (the instructions file in that conversation's folder).
// Interactive in a terminal; scriptable with arguments.

func (a *app) setupCmd() *cobra.Command {
	var global, list bool
	var dir string
	c := &cobra.Command{
		Use:   "setup [agent]",
		Short: "Connect Paragma to your coding agents (Claude Code, Codex, Antigravity, OpenCode)",
		Long: `Without arguments, in a terminal: pick an installed agent, then either
every conversation (its global instructions file) or one of its
conversations (the instructions file in that conversation's folder).
With arguments: pg setup <agent> --global, or pg setup <agent> --dir <folder>.
Re-running replaces Paragma's section; the rest of the file is kept.`,
		Example: "pg setup\npg setup codex --global\npg setup claude --dir .\npg setup --list",
		Args:    cobra.MaximumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			if list {
				return a.setupList()
			}
			if len(args) == 0 {
				if !term.IsTerminal(int(os.Stdin.Fd())) || !term.IsTerminal(int(os.Stdout.Fd())) {
					a.println("pg setup is interactive; in a script use: pg setup <agent> --global | --dir <folder>")
					return a.setupList()
				}
				return a.setupTUI()
			}
			ag := agents.Find(args[0])
			if ag == nil {
				var ids []string
				for _, k := range agents.Known() {
					ids = append(ids, k.ID)
				}
				return fail(ExitUsage, "unknown agent %q (supported: %s)", args[0], strings.Join(ids, ", "))
			}
			var path string
			switch {
			case global && dir == "":
				path = ag.GlobalFile
			case dir != "" && !global:
				abs, err := filepath.Abs(dir)
				if err != nil {
					return fail(ExitUsage, "%v", err)
				}
				path = filepath.Join(abs, ag.ProjectFile)
			default:
				return fail(ExitUsage, "say where: --global (every conversation) or --dir <folder> (conversations in that folder)")
			}
			if a.dryRun {
				a.printf("would write Paragma's section to %s\n", filepath.ToSlash(path))
				return nil
			}
			a.ensureSkills()
			changed, err := installSection(path)
			if err != nil {
				return fail(ExitInternal, "%v", err)
			}
			state := "already up to date"
			if changed {
				state = "written"
			}
			a.ok(fmt.Sprintf("%s: %s (%s)", ag.Name, filepath.ToSlash(path), state))
			return nil
		},
	}
	c.Flags().BoolVar(&global, "global", false, "every conversation of the agent (its global instructions file)")
	c.Flags().StringVar(&dir, "dir", "", "conversations in this folder (the instructions file there)")
	c.Flags().BoolVar(&list, "list", false, "list the supported agents, where they read instructions, whether installed")
	return c
}

func (a *app) setupList() error {
	inst := map[string]bool{}
	for _, ag := range agents.Installed() {
		inst[ag.ID] = true
	}
	for _, ag := range agents.Known() {
		state := "not found"
		if inst[ag.ID] {
			state = "found"
			if hasSection(ag.GlobalFile) {
				state = "found, set up globally"
			}
		}
		a.printf("%-12s %-12s global: %s  folder: %s  (%s)\n", ag.ID, ag.Name, filepath.ToSlash(ag.GlobalFile), ag.ProjectFile, state)
	}
	return nil
}

func hasSection(path string) bool {
	b, err := os.ReadFile(path)
	return err == nil && strings.Contains(string(b), markStart)
}

const (
	markStart = "<!-- paragma:start -->"
	markEnd   = "<!-- paragma:end -->"
)

// installSection writes the memory section into an instruction file
// between markers, replacing an earlier version.
func installSection(path string) (bool, error) {
	section := markStart + "\n## Memory (Paragma)\n\n" + skillBody + markEnd + "\n"
	old, err := os.ReadFile(path)
	if err != nil && !os.IsNotExist(err) {
		return false, err
	}
	text := string(old)
	if i := strings.Index(text, markStart); i >= 0 {
		if j := strings.Index(text, markEnd); j > i {
			end := j + len(markEnd)
			if end < len(text) && text[end] == '\n' {
				end++
			}
			if text[i:end] == section {
				return false, nil
			}
			text = text[:i] + section + text[end:]
			return true, space.WriteFileAtomic(path, []byte(text))
		}
	}
	if text != "" && !strings.HasSuffix(text, "\n") {
		text += "\n"
	}
	if text != "" {
		text += "\n"
	}
	return true, space.WriteFileAtomic(path, []byte(text+section))
}

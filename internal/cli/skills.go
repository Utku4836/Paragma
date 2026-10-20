package cli

// What agents are told. The core (skills/core.md) goes into each agent's
// instructions file through pg setup and is read in every conversation; it
// is short and says when to use pg. The skills (skills/<name>.md) are read
// when needed with pg skill <name>; pg keeps a copy of each in
// <home>/skills/paragma-<name>/ (SKILL.md, and REFERENCE.md made from the
// help of the commands it names) so they can be opened or linked into an
// agent's own skills folder.

import (
	"bytes"
	"crypto/sha256"
	"embed"
	"encoding/hex"
	"os"
	"path/filepath"
	"strings"

	"github.com/Utku4836/Paragma/internal/space"
	"github.com/spf13/cobra"
)

//go:embed skills/*.md
var skillFiles embed.FS

// skillBody is the core every agent reads.
var skillBody = mustSkill("core")

// skillSet are the skills and the commands each one's reference covers.
var skillSet = []struct{ name, cmds string }{
	{"coding", "tree outline read related run error doctor"},
	{"projects", "start resume done plan decide log finish checkpoint task mv new tree project"},
	{"knowledge", "context search read find grep rule decision promote supersede keywords links backlinks neighbors"},
}

func mustSkill(name string) string {
	b, err := skillFiles.ReadFile("skills/" + name + ".md")
	if err != nil {
		panic(err)
	}
	return string(b)
}

func (a *app) skillCmd() *cobra.Command {
	var ref bool
	c := &cobra.Command{
		Use:   "skill [coding|projects|knowledge] [--ref]",
		Short: "Print Paragma's instructions for agents: the core, or a skill (--ref: every flag of its commands)",
		Long: `Without a name: the core instructions (the section pg setup writes) and
the list of skills. With a name: that skill; --ref adds the full help of
the commands it covers. Copies live in <home>/skills/paragma-<name>/.`,
		Example: "pg skill coding\npg skill projects --ref",
		Args:    cobra.MaximumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			a.ensureSkills()
			if len(args) == 0 {
				a.stdout.WriteString(skillBody)
				a.println("")
				a.printf("skills (in %s):\n", filepath.ToSlash(filepath.Join(a.home(), "skills")))
				for _, sk := range skillSet {
					a.printf("  pg skill %-10s %s\n", sk.name, skillDescription(sk.name))
				}
				return nil
			}
			name := strings.TrimPrefix(strings.ToLower(args[0]), "paragma-")
			for _, sk := range skillSet {
				if sk.name != name {
					continue
				}
				a.stdout.WriteString(stripFront(mustSkill(name)))
				if ref {
					a.println("")
					a.stdout.WriteString(a.skillReference(sk.cmds))
				}
				return nil
			}
			return fail(ExitUsage, "no skill %q (coding, projects, knowledge)", args[0])
		},
	}
	c.Flags().BoolVar(&ref, "ref", false, "add the full help of the skill's commands")
	return c
}

// skillDescription is the description line of a skill's front matter.
func skillDescription(name string) string {
	for _, l := range strings.Split(mustSkill(name), "\n") {
		if d, ok := strings.CutPrefix(l, "description: "); ok {
			return d
		}
	}
	return ""
}

func stripFront(s string) string {
	if strings.HasPrefix(s, "---\n") {
		if i := strings.Index(s[4:], "\n---\n"); i >= 0 {
			return strings.TrimLeft(s[4+i+5:], "\n")
		}
	}
	return s
}

// skillReference is the help of the given commands, as pg help prints it.
func (a *app) skillReference(cmds string) string {
	var buf bytes.Buffer
	tmp := newApp(&buf, nil)
	root := tmp.rootCmd()
	buf.WriteString("# Reference\n")
	for _, name := range strings.Fields(cmds) {
		c, _, err := root.Find([]string{name})
		if err != nil || c == root {
			continue
		}
		tmp.printf("\n## pg %s\n", name)
		tmp.printHelp(c)
		if c.HasAvailableSubCommands() {
			for _, sc := range c.Commands() {
				if sc.IsAvailableCommand() && sc.Name() != "help" {
					tmp.printf("\n### pg %s %s\n", name, sc.Name())
					tmp.printHelp(sc)
				}
			}
		}
	}
	tmp.stdout.Flush()
	return buf.String()
}

// ensureSkills writes the skills to <home>/skills when this pg's copies
// differ from the files there (a first run, or a new version of pg).
func (a *app) ensureSkills() {
	if a.dryRun {
		return
	}
	dir := filepath.Join(a.home(), "skills")
	sum := sha256.New()
	for _, sk := range skillSet {
		sum.Write([]byte(mustSkill(sk.name)))
	}
	stamp := Version + " " + hex.EncodeToString(sum.Sum(nil)[:8])
	if b, err := os.ReadFile(filepath.Join(dir, ".version")); err == nil && string(b) == stamp {
		return
	}
	for _, sk := range skillSet {
		d := filepath.Join(dir, "paragma-"+sk.name)
		if os.MkdirAll(d, 0o755) != nil {
			return
		}
		space.WriteFileAtomic(filepath.Join(d, "SKILL.md"), []byte(mustSkill(sk.name)))
		space.WriteFileAtomic(filepath.Join(d, "REFERENCE.md"), []byte(a.skillReference(sk.cmds)))
	}
	space.WriteFileAtomic(filepath.Join(dir, ".version"), []byte(stamp))
}

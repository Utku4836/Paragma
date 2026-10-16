package cli

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/Utku-Private/Paragma/internal/index"
	"github.com/Utku-Private/Paragma/internal/md"
	"github.com/Utku-Private/Paragma/internal/space"
	"github.com/Utku-Private/Paragma/internal/textutil"
	"github.com/spf13/cobra"
)

// skillBody teaches an agent the core loop. It is the single source for
// pg skill, the SKILL.md written by pg install claude and the AGENTS.md /
// GEMINI.md section; everything else is learnt from pg help when needed.
const skillBody = `Memory lives in ` + "`pg`" + ` (Paragma): project notes, rules, decisions, past errors and their fixes. Pull only what you need, through pg; do not open its files.

START
- New conversation: ` + "`pg prime`" + ` (what memory holds, its projects). Continue earlier work: ` + "`pg resume <project>`" + `.
- New multi-step work: ` + "`pg start <project> --goal \"...\" --step \"...\" --step \"...\" --decision \"<what the user decided>\"`" + `, once; the project stays current.

READ
- Tests and builds through ` + "`pg run -- <cmd>`" + `: on failure it prints the known fix; on success, rules for the files you changed.
- A topic or task: ` + "`pg context \"<topic>\"`" + `: every matching rule, decision and fix in one call, and a last line saying what else exists.
- Before changing a file: ` + "`pg related <path>`" + `. An error seen elsewhere: ` + "`pg error find \"<error line>\"`" + `.
- A specific fact: ` + "`pg search \"<words>\"`" + `, then ` + "`pg read <addr>`" + `.

WRITE (goes to the current project)
- Step finished: ` + "`pg done <id>`" + ` (prints the next step). Finding: ` + "`pg log \"...\"`" + `. Decision from the user: ` + "`pg decide \"...\"`" + `.
- Your own notes: ` + "`pg new note \"<title>\" --dir <folder>`" + `; organise the project folder as you like.
- Fixed a non-obvious error: ` + "`pg error record --from-last --cause \"...\" --fix \"...\"`" + `. A recorded fix worked again: ` + "`pg error seen <addr>`" + `.
- Architectural choice: ` + "`pg decision record \"<title>\" --decision \"...\"`" + `. Rule from the user: ` + "`pg rule add \"<rule>\"`" + `.

RULES
- Exit code 1 means memory has nothing on it: solve it yourself, then record it. Do not record trivia.
- Long text: ` + "`-`" + ` and stdin, or ` + "`--file PATH`" + `. Anything else: ` + "`pg help`" + `.
`

const skillDescription = "Project memory (Paragma). Use at the start of a conversation, when an error may have been seen before, when you need a project rule or past decision, to resume long work, or to record a fix, decision, task or rule."

func skillFile() string {
	return "---\nname: paragma\ndescription: " + skillDescription + "\n---\n" + skillBody
}

// ---- prime ----

func (a *app) primeText(s *store) string {
	var b strings.Builder
	files := s.res.Files()
	counts := map[string]int{}
	active := 0
	for _, f := range files {
		if f.Inactive() {
			continue
		}
		active++
		counts[f.Type]++
	}
	rules := s.db.Count(`SELECT COUNT(*) FROM marks m JOIN files f ON f.id = m.file_id WHERE m.kind = 'rule' AND f.path NOT LIKE '%archive/%' AND COALESCE(f.status,'') NOT IN ('superseded','archived')`)
	fmt.Fprintf(&b, "[paragma] space: %s  %d notes: %d errors, %d decisions, %d rules\n",
		s.v.Name, active, counts["error"], counts["decision"], rules)
	ps := s.v.Projects()
	if len(ps) > 0 {
		perProject := map[string]int{}
		for _, f := range files {
			if !f.Inactive() {
				perProject[space.ProjectOf(f.Path)]++
			}
		}
		tcs, _ := s.db.TaskCountsByFile()
		var parts []string
		for _, p := range ps {
			x := fmt.Sprintf("%s (%d notes", p, perProject[p])
			if f := projectNote(s, p); f != nil {
				if tc := tcs[f.ID]; tc.Total > 0 {
					x += fmt.Sprintf(", tasks %d/%d", tc.Done, tc.Total)
				}
				if f.Status == "closed" {
					x += ", closed"
				}
			}
			x += ")"
			if p == s.project {
				x = "*" + x
			}
			parts = append(parts, x)
		}
		if len(parts) > 8 {
			parts = append(parts[:8], fmt.Sprintf("... %d more: pg project list", len(parts)-8))
		}
		fmt.Fprintf(&b, "projects: %s\n", strings.Join(parts, ", "))
	}
	if topics := a.topTags(s, 10); len(topics) > 0 {
		fmt.Fprintf(&b, "topics: %s\n", strings.Join(topics, ", "))
	}
	if f := projectNote(s, s.project); f != nil {
		ts, _ := s.db.Tasks(f.ID)
		tc := index.CountTasks(ts, 1, 1<<30)
		line := fmt.Sprintf("current project here: %s  tasks %d/%d", s.project, tc.Done, tc.Total)
		if h := a.sectionBody(s, f, "handoff"); len(h) > 0 && strings.HasPrefix(h[0], "Checkpoint ") {
			line += ", " + strings.TrimSuffix(strings.ToLower(h[0][:1])+h[0][1:], ".")
		}
		b.WriteString(line + "\n")
		b.WriteString("next: pg resume " + s.project + " (to continue it) | pg context <topic> | pg run -- <test cmd> | pg start <new project>\n")
	} else if len(ps) > 0 {
		b.WriteString("next: pg resume <project> (to continue one) | pg context <topic> | pg run -- <test cmd> | pg start <new project>\n")
	} else {
		b.WriteString("memory is empty; next: pg start <project> --goal \"...\" | pg run -- <test cmd>\n")
	}
	return b.String()
}

// topTags are the most used tags, mark tags left out.
func (a *app) topTags(s *store, n int) []string {
	rows, err := s.db.Query(`SELECT t.tag, COUNT(*) c FROM tags t JOIN files f ON f.id = t.file_id WHERE f.path NOT LIKE '%archive/%' GROUP BY t.tag ORDER BY c DESC, t.tag LIMIT ?`, n+len(md.MarkTags))
	if err != nil {
		return nil
	}
	defer rows.Close()
	var out []string
	for rows.Next() {
		var t string
		var c int
		rows.Scan(&t, &c)
		if md.MarkTags[t] || len(out) >= n {
			continue
		}
		out = append(out, t)
	}
	return out
}

func (a *app) primeCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "prime",
		Short: "Start-of-conversation summary: what memory holds, its projects, the current one (<=150 tokens)",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			a.forgetShown(s) // a new conversation: rules may be printed again
			a.stdout.WriteString(a.primeText(s))
			return nil
		},
	}
}

// ---- index ----

func (a *app) indexCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "index",
		Short: "Write the space's .pg/INDEX.md (topics, counts, recent notes, projects) and print it",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			text := a.indexText(s)
			if !a.dryRun {
				if err := space.WriteFileAtomic(s.v.Meta("INDEX.md"), []byte(text)); err != nil {
					return fail(ExitInternal, "%v", err)
				}
			}
			a.stdout.WriteString(text)
			return nil
		},
	}
}

func (a *app) indexText(s *store) string {
	var b strings.Builder
	fmt.Fprintf(&b, "# Index\n\nGenerated by `pg index` on %s; do not edit.\n\n", today())
	layers := map[string]int{}
	files := s.res.Files()
	var recent []*index.File
	for _, f := range files {
		if f.Inactive() {
			continue
		}
		layers[f.Layer]++
		recent = append(recent, f)
	}
	fmt.Fprintf(&b, "## Notes\n\nsemantic %d | episodic %d | working %d\n\n", layers["semantic"], layers["episodic"], layers["working"])
	rows, err := s.db.Query(`SELECT t.tag, COUNT(*) c FROM tags t JOIN files f ON f.id = t.file_id WHERE f.path NOT LIKE '%archive/%' GROUP BY t.tag ORDER BY c DESC, t.tag LIMIT 25`)
	if err == nil {
		var ts []string
		for rows.Next() {
			var t string
			var c int
			rows.Scan(&t, &c)
			if !md.MarkTags[t] {
				ts = append(ts, fmt.Sprintf("%s (%d)", t, c))
			}
		}
		rows.Close()
		if len(ts) > 0 {
			fmt.Fprintf(&b, "## Topics\n\n%s\n\n", strings.Join(ts, ", "))
		}
	}
	sort.Slice(recent, func(i, j int) bool { return recent[i].Mtime > recent[j].Mtime })
	b.WriteString("## Recent\n\n")
	for _, f := range recent[:min(len(recent), 8)] {
		fmt.Fprintf(&b, "- %s: %s\n", f.Addr(), f.Title)
	}
	if ps := s.v.Projects(); len(ps) > 0 {
		b.WriteString("\n## Projects\n\n")
		for _, p := range ps {
			title := p
			if f := projectNote(s, p); f != nil {
				title = f.Title
				if f.Status == "closed" {
					title += " (closed)"
				}
			}
			fmt.Fprintf(&b, "- %s: %s\n", p, title)
		}
	}
	return b.String()
}

// ---- skill / install ----

func (a *app) skillCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "skill",
		Short: "Print the agent instructions (SKILL.md format, under 400 tokens)",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			a.stdout.WriteString(skillFile())
			return nil
		},
	}
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

// installTargets are the agent CLIs pg knows how to set up.
var installTargets = []struct{ name, tier, what string }{
	{"claude", "1 native", "Claude Code: .claude/skills/paragma/SKILL.md; --hooks adds SessionStart (pg prime) and PreCompact (checkpoint)"},
	{"agents-md", "2 instructions", "AGENTS.md section: Antigravity (verified), Codex, pi and other agents that read AGENTS.md"},
	{"gemini", "2 instructions", "GEMINI.md section: Gemini CLI, Antigravity (verified)"},
}

func (a *app) installCmd() *cobra.Command {
	var list, hooks bool
	var file string
	c := &cobra.Command{
		Use:   "install <claude|agents-md|gemini>",
		Short: "Teach an agent CLI to use pg: skill file, instruction file section, optional hooks",
		Long: `Write the agent instructions where a CLI reads them. Tier 1 (claude)
uses the CLI's own skill system and, with --hooks, runs pg prime at session
start and writes a checkpoint before context compaction. Tier 2 adds a
"Memory (Paragma)" section to the instruction file; re-running replaces it.
Every pg feature works without hooks.`,
		Example: "pg install agents-md\npg install claude --hooks\npg install --list",
		Args:    cobra.MaximumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			if list || len(args) == 0 {
				for _, t := range installTargets {
					a.println(join(t.name, "["+t.tier+"]", t.what))
				}
				return nil
			}
			s, err := a.main()
			if err != nil {
				return err
			}
			base := a.workDir()
			_ = s
			switch args[0] {
			case "agents-md", "gemini":
				name := map[string]string{"agents-md": "AGENTS.md", "gemini": "GEMINI.md"}[args[0]]
				p := filepath.Join(base, name)
				if file != "" {
					p = file
				}
				if a.dryRun {
					a.printf("would write the Memory (Paragma) section to %s\n", filepath.ToSlash(p))
					return nil
				}
				changed, err := installSection(p)
				if err != nil {
					return fail(ExitInternal, "%v", err)
				}
				state := "up to date"
				if changed {
					state = "written"
				}
				a.ok(filepath.ToSlash(p) + " (" + state + ")")
				return nil
			case "claude":
				p := filepath.Join(base, ".claude", "skills", "paragma", "SKILL.md")
				if a.dryRun {
					a.printf("would write %s\n", filepath.ToSlash(p))
					return nil
				}
				if err := space.WriteFileAtomic(p, []byte(skillFile())); err != nil {
					return fail(ExitInternal, "%v", err)
				}
				a.ok(filepath.ToSlash(p))
				if hooks {
					sp := filepath.Join(base, ".claude", "settings.json")
					if err := mergeClaudeHooks(sp); err != nil {
						return fail(ExitInternal, "settings.json: %v", err)
					}
					a.ok(filepath.ToSlash(sp) + " (hooks: SessionStart, PreCompact)")
				}
				return nil
			}
			return fail(ExitUsage, "unknown target %q (pg install --list)", args[0])
		},
	}
	c.Flags().BoolVar(&list, "list", false, "list supported agent CLIs and their support tier")
	c.Flags().BoolVar(&hooks, "hooks", false, "claude: also install the SessionStart and PreCompact hooks")
	c.Flags().StringVar(&file, "file", "", "instruction file to write (default AGENTS.md / GEMINI.md in the project)")
	return c
}

// mergeClaudeHooks adds pg's hooks to a Claude Code settings file without
// touching anything else in it.
func mergeClaudeHooks(path string) error {
	cfg := map[string]any{}
	if b, err := os.ReadFile(path); err == nil && len(strings.TrimSpace(string(b))) > 0 {
		if err := json.Unmarshal(b, &cfg); err != nil {
			return fmt.Errorf("not valid JSON, left unchanged: %v", err)
		}
	}
	hooks, _ := cfg["hooks"].(map[string]any)
	if hooks == nil {
		hooks = map[string]any{}
	}
	add := func(event, command string) {
		groups, _ := hooks[event].([]any)
		for _, g := range groups {
			gm, _ := g.(map[string]any)
			hs, _ := gm["hooks"].([]any)
			for _, h := range hs {
				if hm, _ := h.(map[string]any); hm != nil && hm["command"] == command {
					return // already there
				}
			}
		}
		groups = append(groups, map[string]any{"hooks": []any{map[string]any{"type": "command", "command": command, "timeout": 10}}})
		hooks[event] = groups
	}
	add("SessionStart", "pg hook session-start")
	add("PreCompact", "pg hook pre-compact")
	cfg["hooks"] = hooks
	b, err := json.MarshalIndent(cfg, "", "  ")
	if err != nil {
		return err
	}
	return space.WriteFileAtomic(path, append(b, '\n'))
}

// ---- hook ----

func (a *app) hookCmd() *cobra.Command {
	return &cobra.Command{
		Use:    "hook <session-start|pre-compact>",
		Short:  "Entry point for agent CLI hooks (reads the hook JSON on stdin, never fails the agent)",
		Hidden: false,
		Args:   cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			io.Copy(io.Discard, io.LimitReader(a.stdin, 1<<20)) // the event JSON; nothing in it is needed yet
			s, err := a.main()
			if err != nil {
				return nil // no space: stay silent
			}
			switch args[0] {
			case "session-start":
				// plain stdout becomes context for the model
				a.forgetShown(s)
				a.stdout.WriteString(a.primeText(s))
			case "pre-compact":
				f := projectNote(s, s.project)
				if f == nil {
					return nil
				}
				text := "Automatic checkpoint before context compaction. " + a.mechanicalHandoff(s, f)
				_ = a.withLock(s, func() error {
					n, err := a.load(s, f)
					if err != nil {
						return err
					}
					a.appendToSection(n, f, "notes")([]string{"- " + today() + " " + clock() + " " + textutil.OneLine(text, 600)})
					return a.save(n)
				})
			default:
				return fail(ExitUsage, "unknown hook %q (session-start, pre-compact)", args[0])
			}
			return nil
		},
	}
}

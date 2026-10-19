package cli

import (
	"fmt"
	"strings"

	"github.com/Utku4836/Paragma/internal/index"
	"github.com/Utku4836/Paragma/internal/md"
	"github.com/Utku4836/Paragma/internal/space"
	"github.com/spf13/cobra"
)

// What an agent is told: its instructions (pg skill, the section pg setup
// writes) and the start-of-conversation summary (pg prime).

// skillBody teaches an agent the core loop. It is the single source for
// pg skill and the section pg setup writes into an agent's instructions
// file; everything else is learnt from pg help when needed.
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
- Step finished: ` + "`pg done <id>`" + ` (prints the next step). More steps: ` + "`pg plan \"step\" \"step\"`" + `. Finding (also where things are in the code): ` + "`pg log \"...\"`" + `. Decision from the user: ` + "`pg decide \"...\"`" + `.
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

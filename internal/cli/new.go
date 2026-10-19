package cli

import (
	"fmt"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"

	"github.com/Utku4836/Paragma/internal/space"
	"github.com/Utku4836/Paragma/internal/ulid"
	"github.com/spf13/cobra"
)

// Creating notes from templates.

// typeDirs are the default folders inside a project, per note type.
var typeDirs = map[string]string{
	"note":     "",
	"concept":  "concepts",
	"error":    "errors",
	"decision": "decisions",
}

func (a *app) newCmd() *cobra.Command {
	var dir, tags, keywords string
	c := &cobra.Command{
		Use:   "new <type> <title> [text|-]",
		Short: "Create a note from a template: note, concept, error, decision",
		Long: `Create a note in the current project. Templates: error (symptom, root
cause, fix, files), decision (numbered ADR: context, decision, alternatives,
consequences), concept and note (title only). Optional text becomes the
intro paragraph. --dir is a folder inside the project (created as needed):
organise the project as you like. Prints the new address. To start a
project use pg start.`,
		Example: `pg new note "Webhook signature findings" --dir webhook
pg new concept "Refresh token rotation"
pg new decision "Use WAL mode" --tags sqlite`,
		Args: cobra.MinimumNArgs(2),
		RunE: func(cmd *cobra.Command, args []string) error {
			typ := strings.ToLower(args[0])
			d, ok := typeDirs[typ]
			if !ok {
				if typ == "project" {
					return fail(ExitUsage, "use: pg start <project> --goal \"...\"")
				}
				return fail(ExitUsage, "type must be note, concept, error or decision (got %q)", typ)
			}
			if dir != "" {
				d = strings.Trim(filepath.ToSlash(dir), "/")
				if strings.Contains("/"+d+"/", "/../") {
					return fail(ExitUsage, "--dir must stay inside the project")
				}
			}
			title := args[1]
			text, err := a.inputText(args[2:], false)
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
			// --dir may already name the project ("payments/webhook")
			if d == p || strings.HasPrefix(d, p+"/") {
				d = strings.TrimPrefix(strings.TrimPrefix(d, p), "/")
			}
			d = strings.Trim(p+"/"+d, "/")
			name := slug(title)
			if typ == "decision" {
				name = fmt.Sprintf("%04d-%s", nextDecisionNumber(s, p), name)
				title = fmt.Sprintf("%s: %s", name[:4], title)
			}
			var tagList []string
			for _, t := range splitList([]string{tags}) {
				tagList = append(tagList, strings.TrimPrefix(t, "#"))
			}
			rel := freePath(s, d+"/"+name+".md")
			lines := withKeywords(a.template(s, typ, title, tagList, text), keywords)
			return a.withLock(s, func() error {
				n := newNote(s, rel, lines)
				if err := a.save(n); err != nil {
					return err
				}
				a.ok(n.addr())
				if first := a.firstTemplateSection(s, typ); first != "" && !a.dryRun {
					a.printf("next: pg write %s \"...\"\n", shellQuote(n.addr()+"#"+first))
				}
				return nil
			})
		},
	}
	c.Flags().StringVar(&dir, "dir", "", "folder inside the project (default by type: errors/, decisions/, concepts/)")
	c.Flags().StringVar(&tags, "tags", "", "comma separated tags")
	c.Flags().StringVar(&keywords, "keywords", "", "comma separated search words: synonyms, other language")
	return c
}

var reDecisionNum = regexp.MustCompile(`^(\d{4})-`)

// nextDecisionNumber numbers decision records per project.
func nextDecisionNumber(s *store, p string) int {
	maxN := 0
	for _, f := range s.res.Files() {
		if !strings.Contains(f.Path, "decisions/") || space.ProjectOf(f.Path) != p {
			continue
		}
		if m := reDecisionNum.FindStringSubmatch(filepath.Base(f.Path)); m != nil {
			if n, _ := strconv.Atoi(m[1]); n > maxN {
				maxN = n
			}
		}
	}
	return maxN + 1
}

func (a *app) firstTemplateSection(s *store, typ string) string {
	switch typ {
	case "error":
		return a.sec(s, "symptom")
	case "decision":
		return a.sec(s, "context")
	}
	return ""
}

// template builds a new note's lines.
func (a *app) template(s *store, typ, title string, tags []string, text string) []string {
	layer := map[string]string{"note": "semantic", "concept": "semantic", "error": "episodic", "decision": "episodic", "project": "working"}[typ]
	fm := []string{"---", "id: " + ulid.New(), "type: " + typ, "layer: " + layer, "title: " + yamlString(title)}
	switch typ {
	case "error":
		fm = append(fm, "status: active", "seen: 1")
	case "decision":
		fm = append(fm, "status: active")
	}
	if len(tags) > 0 {
		fm = append(fm, "tags: ["+strings.Join(tags, ", ")+"]")
	}
	fm = append(fm, "created: "+today())
	if layer == "semantic" {
		fm = append(fm, "verified: "+today())
	}
	fm = append(fm, "---")
	lines := append(fm, "# "+title)
	if text != "" {
		lines = append(lines, "")
		lines = append(lines, splitText(text)...)
	}
	var secs []string
	switch typ {
	case "error":
		secs = []string{"symptom", "cause", "fix", "files"}
	case "decision":
		secs = []string{"context", "decision", "options", "outcome"}
	}
	for _, k := range secs {
		lines = append(lines, "", "## "+a.sec(s, k))
	}
	return lines
}

// yamlString quotes a value when YAML would misread it.
func yamlString(s string) string {
	if strings.ContainsAny(s, ":#[]{}&*!|>'\"%@`,") || strings.HasPrefix(s, "-") || strings.HasPrefix(s, "?") {
		return strconv.Quote(s)
	}
	return s
}

// keywordList parses "a, b; c" into clean keywords.
func keywordList(s string) []string {
	var out []string
	seen := map[string]bool{}
	for _, k := range strings.FieldsFunc(s, func(r rune) bool { return r == ',' || r == ';' }) {
		k = strings.TrimSpace(k)
		if k != "" && !seen[strings.ToLower(k)] {
			seen[strings.ToLower(k)] = true
			out = append(out, k)
		}
	}
	return out
}

func yamlList(xs []string) string {
	q := make([]string, len(xs))
	for i, x := range xs {
		q[i] = yamlString(x)
	}
	return "[" + strings.Join(q, ", ") + "]"
}

// withKeywords sets the keywords front matter of a new note's lines.
func withKeywords(lines []string, kw string) []string {
	if ks := keywordList(kw); len(ks) > 0 {
		return setFront(lines, "keywords", yamlList(ks))
	}
	return lines
}

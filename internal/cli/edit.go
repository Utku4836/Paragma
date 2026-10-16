package cli

import (
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"

	"github.com/Utku-Private/Paragma/internal/addr"
	"github.com/Utku-Private/Paragma/internal/index"
	"github.com/Utku-Private/Paragma/internal/space"
	"github.com/Utku-Private/Paragma/internal/textutil"
	"github.com/Utku-Private/Paragma/internal/ulid"
	"github.com/spf13/cobra"
)

// sectionNames are the headings pg writes and recognises, per language.
// Recognition accepts every language so a vault can mix them.
var sectionNames = map[string]map[string]string{
	"goal":      {"en": "Goal", "tr": "Hedef"},
	"plan":      {"en": "Plan", "tr": "Plan"},
	"notes":     {"en": "Log", "tr": "Günlük"},
	"handoff":   {"en": "Handoff", "tr": "Devir"},
	"symptom":   {"en": "Symptom", "tr": "Belirti"},
	"cause":     {"en": "Root cause", "tr": "Kök neden"},
	"fix":       {"en": "Fix", "tr": "Çözüm"},
	"files":     {"en": "Files", "tr": "Dosyalar"},
	"context":   {"en": "Context", "tr": "Bağlam"},
	"decision":  {"en": "Decision", "tr": "Karar"},
	"options":   {"en": "Alternatives", "tr": "Alternatifler"},
	"outcome":   {"en": "Consequences", "tr": "Sonuçlar"},
	"bgoal":     {"en": "Goal", "tr": "Amaç"},
	"status":    {"en": "Status", "tr": "Durum"},
	"decisions": {"en": "Decisions", "tr": "Kararlar"},
	"questions": {"en": "Open questions", "tr": "Açık sorular"},
	"tasks":     {"en": "Tasks", "tr": "Görevler"},
}

// extra spellings that count as the same section
var sectionAliases = map[string][]string{
	"goal":    {"Amaç", "Objective"},
	"notes":   {"Notes", "Notlar", "Log"},
	"handoff": {"Checkpoint"},
	"plan":    {"Tasks", "Görevler"},
	"tasks":   {"Plan"},
	"bgoal":   {"Hedef"},
}

func (a *app) sec(s *store, key string) string {
	lang := s.v.Cfg.Lang
	if lang != "tr" {
		lang = "en"
	}
	return sectionNames[key][lang]
}

// findSection returns the top-level section of a file whose heading is one of
// key's spellings (level 2 under a title, or level 1 otherwise).
func findSection(s *store, f *index.File, key string) *index.Section {
	secs, err := s.res.Sections(f)
	if err != nil {
		return nil
	}
	names := map[string]bool{}
	for _, n := range sectionNames[key] {
		names[foldName(n)] = true
	}
	for _, n := range sectionAliases[key] {
		names[foldName(n)] = true
	}
	for _, sec := range secs {
		if sec.IsTitle || sec.Level == 0 {
			continue
		}
		if names[foldName(sec.Path)] {
			return sec
		}
	}
	return nil
}

func foldName(s string) string { return strings.TrimSpace(textutil.Fold(s)) }

// ---- new ----

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
				if typ == "project" || typ == "session" {
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

// nextDecisionNumber numbers decision records per project.
func nextDecisionNumber(s *store, p string) int {
	re := regexp.MustCompile(`^(\d{4})-`)
	maxN := 0
	for _, f := range s.res.Files() {
		if !strings.Contains(f.Path, "decisions/") || space.ProjectOf(f.Path) != p {
			continue
		}
		if m := re.FindStringSubmatch(filepath.Base(f.Path)); m != nil {
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
	layer := map[string]string{"note": "semantic", "concept": "semantic", "error": "episodic", "decision": "episodic", "session": "working", "branch": "working", "project": "working"}[typ]
	fm := []string{"---", "id: " + ulid.New(), "type: " + typ, "layer: " + layer, "title: " + yamlString(title)}
	switch typ {
	case "error":
		fm = append(fm, "status: active", "seen: 1")
	case "decision", "session", "branch":
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

// ---- append / write / insert ----

func (a *app) appendCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "append <addr> <text|->",
		Short: "Add text at the end of a note, heading, block or line range",
		Long: `Add text at the end of the target. For a heading the text goes after the
heading's own paragraphs, before its sub-headings. Long text: pass "-" and
pipe it on stdin, or use --file PATH (PowerShell 5.1 pipes are not UTF-8).`,
		Example: `pg append rules#Database "> [!rule] Open SQLite in WAL mode"
pg append architecture#Middleware --file note.md`,
		Args: cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			addrArgs, textArgs := a.splitAddrArgs(args)
			text, err := a.inputText(textArgs, true)
			if err != nil {
				return err
			}
			s, t, err := a.resolve(addrArgs)
			if err != nil {
				return err
			}
			return a.withLock(s, func() error {
				n, err := a.load(s, t.File)
				if err != nil {
					return err
				}
				after := t.To
				switch t.Kind {
				case addr.KindFile:
					after = len(n.lines)
				case addr.KindSection:
					after = lastLine(n.lines, t.Section.Line, t.Section.BodyEnd)
				}
				n.lines, _ = place(n.lines, after, splitText(text))
				if err := a.save(n); err != nil {
					return err
				}
				a.ok(t.Addr())
				return nil
			})
		},
	}
}

// splitAddrArgs separates the address from the text in "<addr> <text...>"
// commands. Normally the address is the first argument; when an unquoted
// heading with spaces was split by the shell, the longest prefix of the
// arguments that resolves is taken as the address.
func (a *app) splitAddrArgs(args []string) (string, []string) {
	if len(args) <= 1 || !strings.Contains(args[0], "#") {
		return args[0], args[1:]
	}
	if _, _, err := a.resolve(args[0]); err == nil {
		return args[0], args[1:]
	}
	for j := min(len(args)-1, 6); j >= 1; j-- {
		cand := strings.Join(args[:j+1], " ")
		if _, _, err := a.resolve(cand); err == nil {
			return cand, args[j+1:]
		}
	}
	return args[0], args[1:]
}

func (a *app) writeCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "write <addr> <text|->",
		Short: "Replace the body of a heading, a block, a line range or a whole note",
		Long: `Replace the target's text. A heading keeps its heading line and its
sub-headings; only its own paragraphs are replaced. A whole note keeps its
front matter. A block keeps its ^id even if the new text omits it.
Use --if-hash with the hash from pg read to refuse if someone else changed
the note meanwhile.`,
		Example: `pg write stack#Backend "Go 1.24, chi, sqlc"
pg write auth-refactor^t004 "- [ ] Tests for refresh endpoint ^t004"
pg write architecture#Deploy - --if-hash 3f2a9c1e`,
		Args: cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			addrArgs, textArgs := a.splitAddrArgs(args)
			text, err := a.inputText(textArgs, false)
			if err != nil {
				return err
			}
			s, t, err := a.resolve(addrArgs)
			if err != nil {
				return err
			}
			return a.withLock(s, func() error {
				n, err := a.load(s, t.File)
				if err != nil {
					return err
				}
				block := splitText(text)
				switch t.Kind {
				case addr.KindFile:
					body := n.lines[:t.From-1]
					n.lines = append(append([]string(nil), body...), block...)
				case addr.KindSection:
					from, to := t.Section.Line+1, lastLine(n.lines, t.Section.Line+1, t.Section.BodyEnd)
					if to >= from {
						n.lines = replaceRange(n.lines, from, to, nil)
					}
					n.lines, _ = place(n.lines, t.Section.Line, block)
				case addr.KindBlock:
					block = keepBlockID(block, t.Block)
					n.lines = replaceRange(n.lines, t.From, t.To, block)
				case addr.KindLines:
					n.lines = replaceRange(n.lines, t.From, t.To, block)
				}
				if err := a.save(n); err != nil {
					return err
				}
				a.ok(t.Addr())
				return nil
			})
		},
	}
}

// keepBlockID appends ^id to a rewritten block that lost it: on the first
// line of a list item, else on the last line.
func keepBlockID(block []string, id string) []string {
	if len(block) == 0 {
		return block
	}
	for _, l := range block {
		if strings.Contains(l, "^"+id) {
			return block
		}
	}
	out := append([]string(nil), block...)
	i := len(out) - 1
	if isListLine(out[0]) {
		i = 0
	}
	out[i] += " ^" + id
	return out
}

func (a *app) insertCmd() *cobra.Command {
	var before, after bool
	c := &cobra.Command{
		Use:   "insert <addr> --before|--after <text|->",
		Short: "Insert text right before or after a block, heading or line range",
		Example: `pg insert rules^r004 --after "> [!rule] Never log tokens ^r005"
pg insert stack:12 --before "## Runtime"`,
		Args: cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			if before == after {
				return fail(ExitUsage, "give exactly one of --before or --after")
			}
			addrArgs, textArgs := a.splitAddrArgs(args)
			text, err := a.inputText(textArgs, true)
			if err != nil {
				return err
			}
			s, t, err := a.resolve(addrArgs)
			if err != nil {
				return err
			}
			return a.withLock(s, func() error {
				n, err := a.load(s, t.File)
				if err != nil {
					return err
				}
				at := t.To
				if before {
					at = t.From - 1
				}
				n.lines, _ = place(n.lines, at, splitText(text))
				if err := a.save(n); err != nil {
					return err
				}
				a.ok(t.Addr())
				return nil
			})
		},
	}
	c.Flags().BoolVar(&before, "before", false, "insert before the target")
	c.Flags().BoolVar(&after, "after", false, "insert after the target")
	return c
}

// ---- edit (find / replace) ----

func (a *app) editCmd() *cobra.Command {
	var find, repl string
	var all bool
	c := &cobra.Command{
		Use:   "edit <addr> --find TEXT --replace TEXT",
		Short: "Replace exact text inside a note, heading, block or line range",
		Long: `Targeted find/replace limited to the addressed part. Fails with exit 3 if
the text is not there and exit 6 if it occurs more than once (use --all).`,
		Example: `pg edit stack --find "Go 1.22" --replace "Go 1.24"
pg edit rules^r002 --find "mock" --replace "fake" --all`,
		Args: cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			if find == "" {
				return fail(ExitUsage, "--find is required")
			}
			s, t, err := a.resolve(strings.Join(args, " "))
			if err != nil {
				return err
			}
			return a.withLock(s, func() error {
				n, err := a.load(s, t.File)
				if err != nil {
					return err
				}
				from, to := t.From, t.To
				region := strings.Join(n.lines[from-1:to], "\n")
				c := strings.Count(region, find)
				switch {
				case c == 0:
					return fail(ExitNotFound, "not found in %s: %q", t.Addr(), find)
				case c > 1 && !all:
					return fail(ExitValidation, "%q occurs %d times in %s; narrow the address or add --all", find, c, t.Addr())
				}
				region = strings.Replace(region, find, repl, map[bool]int{true: -1, false: 1}[all])
				n.lines = replaceRange(n.lines, from, to, strings.Split(region, "\n"))
				if err := a.save(n); err != nil {
					return err
				}
				a.ok(fmt.Sprintf("%s (%d replaced)", t.Addr(), map[bool]int{true: c, false: 1}[all]))
				return nil
			})
		},
	}
	c.Flags().StringVar(&find, "find", "", "exact text to find")
	c.Flags().StringVar(&repl, "replace", "", "replacement text")
	c.Flags().BoolVar(&all, "all", false, "replace every occurrence")
	return c
}

// ---- section add ----

func (a *app) sectionCmd() *cobra.Command {
	c := &cobra.Command{
		Use:   "section",
		Short: "Heading operations: section add",
	}
	c.AddCommand(&cobra.Command{
		Use:   "add <addr> <heading> [text|-]",
		Short: "Add a heading under a note or heading (at the end of it)",
		Example: `pg section add architecture "Caching"
pg section add architecture#Layers "Queue" "Workers read from Redis streams."`,
		Args: cobra.MinimumNArgs(2),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, t, err := a.resolve(args[0])
			if err != nil {
				return err
			}
			heading := strings.TrimSpace(args[1])
			text, err := a.inputText(args[2:], false)
			if err != nil {
				return err
			}
			if t.Kind == addr.KindBlock || t.Kind == addr.KindLines {
				return fail(ExitUsage, "section add takes a note or a heading address")
			}
			return a.withLock(s, func() error {
				n, err := a.load(s, t.File)
				if err != nil {
					return err
				}
				level, after, parentPath := 2, len(n.lines), ""
				if t.Kind == addr.KindSection && !t.Section.IsTitle {
					level, after, parentPath = min(t.Section.Level+1, 6), t.Section.End, t.Section.Path
				} else if !hasTitle(s, t.File) {
					level = 1
				}
				block := []string{strings.Repeat("#", level) + " " + heading}
				if text != "" {
					block = append(block, "")
					block = append(block, splitText(text)...)
				}
				n.lines, _ = place(n.lines, lastLine(n.lines, 1, after), block)
				if err := a.save(n); err != nil {
					return err
				}
				p := heading
				if parentPath != "" {
					p = parentPath + "/" + heading
				}
				a.ok(t.File.Addr() + "#" + p)
				return nil
			})
		},
	})
	return c
}

func hasTitle(s *store, f *index.File) bool {
	secs, _ := s.res.Sections(f)
	for _, sec := range secs {
		if sec.IsTitle {
			return true
		}
	}
	return false
}

// ---- link ----

func (a *app) linkCmd() *cobra.Command {
	var kind string
	c := &cobra.Command{
		Use:   "link <from-addr> <to-addr>",
		Short: "Add a [[link]] (optionally typed: fixes, supersedes, ...) at the end of the source",
		Example: `pg link 0007-wal-mode sqlite-database-locked --kind fixes
pg link auth-refactor#Notes architecture#Middleware`,
		Args: cobra.ExactArgs(2),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, t, err := a.resolve(args[0])
			if err != nil {
				return err
			}
			_, dst, err := a.resolve(args[1])
			if err != nil {
				return err
			}
			target := shortRef(s, dst.File)
			if sub := strings.TrimPrefix(dst.Addr(), dst.File.Addr()); sub != "" && !strings.HasPrefix(sub, ":") {
				target += sub
			}
			line := "- [[" + target + "]]"
			if kind != "" {
				line = strings.ToLower(kind) + ":: [[" + target + "]]"
			}
			return a.withLock(s, func() error {
				n, err := a.load(s, t.File)
				if err != nil {
					return err
				}
				after := t.To
				switch t.Kind {
				case addr.KindFile:
					after = len(n.lines)
				case addr.KindSection:
					after = lastLine(n.lines, t.Section.Line, t.Section.BodyEnd)
				}
				n.lines, _ = place(n.lines, after, []string{line})
				if err := a.save(n); err != nil {
					return err
				}
				a.ok(t.Addr() + " -> " + target)
				return nil
			})
		},
	}
	c.Flags().StringVar(&kind, "kind", "", "link type written as kind:: [[target]] (fixes, supersedes, ...)")
	return c
}

// shortRef is the shortest name that still resolves to f: its file name if
// unique, else its path.
func shortRef(s *store, f *index.File) string {
	base := filepath.Base(f.Addr())
	if g, err := s.res.ResolveFile(base); err == nil && g.ID == f.ID {
		return base
	}
	return f.Addr()
}

// ---- mv / archive / rm ----

func (a *app) mvCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "mv <addr> <new-name>",
		Short: "Rename or move a note and rewrite the [[links]] that point to it",
		Long: `Rename a note. A bare name keeps the directory; a name with "/" is a path
inside the vault. Every [[link]] to the note is rewritten.`,
		Example: "pg mv auth-notes auth\npg mv scratch knowledge/concepts/token-rotation",
		Args:    cobra.ExactArgs(2),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, t, err := a.resolve(args[0])
			if err != nil {
				return err
			}
			if t.Kind != addr.KindFile {
				return fail(ExitUsage, "mv moves whole notes; give a note address")
			}
			dest := strings.TrimSuffix(filepath.ToSlash(args[1]), ".md")
			if !strings.Contains(dest, "/") {
				dest = filepath.ToSlash(filepath.Dir(t.File.Path)) + "/" + dest
			}
			dest = strings.TrimPrefix(dest, "./") + ".md"
			return a.withLock(s, func() error {
				return a.move(s, t.File, dest, true)
			})
		},
	}
}

// move renames f to dest (vault-relative) and, if rewrite, updates links.
func (a *app) move(s *store, f *index.File, dest string, rewrite bool) error {
	dp := filepath.Join(s.v.Root, filepath.FromSlash(dest))
	if _, err := os.Stat(dp); err == nil {
		return fail(ExitConflict, "%s already exists", strings.TrimSuffix(dest, ".md"))
	}
	// collect links to f before the move, while they still resolve
	type fix struct {
		src *index.File
		old string
	}
	var fixes []fix
	if rewrite {
		ls, err := s.db.LinksTo(fileKeys(f))
		if err != nil {
			return fail(ExitInternal, "%v", err)
		}
		seen := map[string]bool{}
		for _, l := range ls {
			src, err := s.db.FileByID(l.SrcFile)
			if err != nil {
				continue
			}
			tt, err := resolveLink(s.res, src, l.Target)
			if err != nil || tt.File.ID != f.ID {
				continue
			}
			raw := l.Target
			if i := strings.IndexAny(raw, "#^"); i >= 0 {
				raw = raw[:i]
			}
			if raw == "" || seen[src.Path+"\x00"+raw] {
				continue
			}
			seen[src.Path+"\x00"+raw] = true
			fixes = append(fixes, fix{src, raw})
		}
	}
	if a.dryRun {
		a.printf("would move %s -> %s and rewrite %d links\n", f.Addr(), strings.TrimSuffix(dest, ".md"), len(fixes))
		return nil
	}
	if err := os.MkdirAll(filepath.Dir(dp), 0o755); err != nil {
		return fail(ExitInternal, "%v", err)
	}
	if err := os.Rename(filepath.Join(s.v.Root, filepath.FromSlash(f.Path)), dp); err != nil {
		return fail(ExitInternal, "move: %v", err)
	}
	changed := []string{f.Path, dest}
	newRef := strings.TrimSuffix(filepath.Base(dest), ".md")
	if err := a.reindex(s, changed...); err != nil {
		return err
	}
	if g, err := s.res.ResolveFile(newRef); err != nil || g.Path != dest {
		newRef = strings.TrimSuffix(dest, ".md")
	}
	updated := map[string]bool{}
	for _, fx := range fixes {
		srcPath := fx.src.Path
		if srcPath == f.Path {
			srcPath = dest
		}
		g, err := s.res.ResolveFile(strings.TrimSuffix(srcPath, ".md"))
		if err != nil {
			continue
		}
		n, err := a.load(s, g)
		if err != nil {
			return err
		}
		re := regexp.MustCompile(`\[\[` + regexp.QuoteMeta(fx.old) + `([\]|#^])`)
		for i, l := range n.lines {
			n.lines[i] = re.ReplaceAllString(l, "[["+newRef+"$1")
		}
		if err := a.save(n); err != nil {
			return err
		}
		updated[srcPath] = true
	}
	msg := strings.TrimSuffix(dest, ".md")
	if len(updated) > 0 {
		msg += fmt.Sprintf(" (links updated in %d notes)", len(updated))
	}
	a.ok(msg)
	return nil
}

func (a *app) archiveCmd() *cobra.Command {
	return &cobra.Command{
		Use:     "archive <addr>",
		Short:   "Move a note to its project's archive/ (kept, searchable with --all, ranked low)",
		Example: "pg archive old-deploy",
		Args:    cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, t, err := a.resolve(args[0])
			if err != nil {
				return err
			}
			if t.Kind != addr.KindFile {
				return fail(ExitUsage, "archive works on whole notes")
			}
			if space.Archived(t.File.Path) {
				return fail(ExitValidation, "%s is already archived", t.File.Addr())
			}
			return a.withLock(s, func() error { return a.archiveFile(s, t.File) })
		},
	}
}

// archiveFile marks a note archived and moves it under its project's
// archive/, keeping the rest of its path so links by name keep resolving.
func (a *app) archiveFile(s *store, f *index.File) error {
	n, err := a.load(s, f)
	if err != nil {
		return err
	}
	n.lines = setFront(n.lines, "status", "archived")
	if err := a.save(n); err != nil {
		return err
	}
	dest := "archive/" + f.Path
	if p := space.ProjectOf(f.Path); p != "" {
		dest = p + "/archive/" + strings.TrimPrefix(f.Path, p+"/")
	}
	return a.move(s, f, dest, false)
}

func (a *app) rmCmd() *cobra.Command {
	var hard bool
	c := &cobra.Command{
		Use:     "rm <addr> --hard",
		Short:   "Delete a note for good (prefer pg archive)",
		Example: "pg rm scratch --hard",
		Args:    cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			if !hard {
				return fail(ExitUsage, "rm deletes for good; pass --hard, or keep it with: pg archive %s", args[0])
			}
			s, t, err := a.resolve(args[0])
			if err != nil {
				return err
			}
			if t.Kind != addr.KindFile {
				return fail(ExitUsage, "rm deletes whole notes; to remove part of one use pg write")
			}
			bl, _ := a.backlinks(s, t)
			return a.withLock(s, func() error {
				if a.dryRun {
					a.printf("would delete %s (%d links to it would break)\n", t.File.Addr(), len(bl))
					return nil
				}
				if err := os.Remove(filepath.Join(s.v.Root, filepath.FromSlash(t.File.Path))); err != nil {
					return fail(ExitInternal, "%v", err)
				}
				if err := a.reindex(s, t.File.Path); err != nil {
					return err
				}
				msg := "removed " + t.File.Addr()
				if len(bl) > 0 {
					msg += fmt.Sprintf(" (%d links to it are now broken)", len(bl))
				}
				a.ok(msg)
				return nil
			})
		},
	}
	c.Flags().BoolVar(&hard, "hard", false, "really delete the file")
	return c
}

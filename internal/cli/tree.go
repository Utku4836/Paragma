package cli

// pg tree: the structure of the code being worked on, each file with its
// top-level declarations and the memory that names it, so an agent sees
// where things are (and which rules apply there) in one call instead of
// listing and opening files. With --notes: the folders and notes of a
// Paragma project.

import (
	"fmt"
	"os"
	"path"
	"path/filepath"
	"regexp"
	"slices"
	"sort"
	"strings"

	"github.com/Utku4836/Paragma/internal/index"
	"github.com/Utku4836/Paragma/internal/textutil"
	"github.com/spf13/cobra"
)

func (a *app) treeCmd() *cobra.Command {
	var notes bool
	var depth int
	c := &cobra.Command{
		Use:   "tree [dir] [--notes]",
		Short: "Map of the code: folders and files with their declarations and the rules, decisions and errors about each",
		Long: `Without --notes: the files of the work tree (as git sees them, .gitignore
respected; below dir if given), each source file with its top-level
declarations, and [2 rules, 1 error] where memory names that file or
folder (pg related <file> shows those notes). Use it instead of listing
and opening files to find where things are.
With --notes: the folders and notes of the current Paragma project (or of
the project or folder given), with type and size.
Too big for the budget: one line per folder; narrow with pg tree <dir>.`,
		Example: "pg tree\npg tree payments\npg tree --notes\npg tree --notes shopkit/decisions",
		Args:    cobra.MaximumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			arg := ""
			if len(args) > 0 {
				arg = args[0]
			}
			if notes {
				return a.notesTree(s, arg, depth)
			}
			return a.codeTree(s, arg, depth)
		},
	}
	c.Flags().BoolVar(&notes, "notes", false, "the Paragma project's notes instead of the code")
	c.Flags().IntVar(&depth, "depth", 0, "deepest folder level to expand (0 = as the budget allows)")
	return c
}

// ---- the tree, for code and notes alike ----

type treeDir struct {
	name  string
	path  string // slash path from the root of the listing, "" for the root
	dirs  map[string]*treeDir
	files []treeFile
	mark  memMark // notes about the folder itself
	n     int     // files below, recursively
}

type treeFile struct {
	name, path string
	info       string // declarations (code) or type and size (notes)
	mark       memMark
	tokens     int // notes only
}

func newTreeDir(name, p string) *treeDir {
	return &treeDir{name: name, path: p, dirs: map[string]*treeDir{}}
}

func (d *treeDir) add(rel string, f treeFile) {
	parts := strings.Split(rel, "/")
	cur := d
	cur.n++
	for i, part := range parts[:len(parts)-1] {
		sub := cur.dirs[part]
		if sub == nil {
			sub = newTreeDir(part, strings.Join(parts[:i+1], "/"))
			cur.dirs[part] = sub
		}
		cur = sub
		cur.n++
	}
	f.name = parts[len(parts)-1]
	cur.files = append(cur.files, f)
}

func (d *treeDir) sortedDirs() []*treeDir {
	out := make([]*treeDir, 0, len(d.dirs))
	for _, s := range d.dirs {
		out = append(out, s)
	}
	sort.Slice(out, func(i, j int) bool { return out[i].name < out[j].name })
	return out
}

// total sums the memory marks of a folder and everything below it.
func (d *treeDir) total() memMark {
	m := d.mark
	for _, f := range d.files {
		m = m.plus(f.mark)
	}
	for _, s := range d.dirs {
		m = m.plus(s.total())
	}
	return m
}

// render prints the tree; folders at maxDepth (0: none) become one
// summary line.
func (d *treeDir) render(lines *[]string, indent string, depth, maxDepth int, summary func(*treeDir) string) {
	sort.Slice(d.files, func(i, j int) bool { return d.files[i].name < d.files[j].name })
	for _, f := range d.files {
		*lines = append(*lines, indent+join(f.name, f.info, f.mark.String()))
	}
	for _, s := range d.sortedDirs() {
		if maxDepth > 0 && depth >= maxDepth {
			*lines = append(*lines, indent+join(s.name+"/", summary(s), s.total().String()))
			continue
		}
		*lines = append(*lines, indent+join(s.name+"/", s.mark.String()))
		s.render(lines, indent+"  ", depth+1, maxDepth, summary)
	}
}

// printTree prints as deep a tree as fits the budget: the whole of it, or
// folders below some level summarised in one line each.
func (a *app) printTree(root *treeDir, head, next string, maxDepth int, summary func(*treeDir) string) {
	budget := a.budgetOr(1500)
	cost := func(ls []string) int {
		n := 0
		for _, l := range ls {
			n += textutil.Tokens(l) + 1
		}
		return n
	}
	var lines []string
	root.render(&lines, "", 1, maxDepth, summary)
	full := cost(lines)
	cut := false
	for d := 4; full > budget && d >= 1 && (maxDepth == 0 || d < maxDepth); d-- {
		lines = nil
		root.render(&lines, "", 1, d, summary)
		cut = true
		if cost(lines) <= budget {
			break
		}
	}
	a.println(head)
	if cut {
		lines = append(lines, fmt.Sprintf("(deeper folders summarised: the full tree is %s; expand one: %s)", textutil.FmtTokens(full), next))
	}
	bw := a.newBudget(budget + 200)
	for i, l := range lines {
		if !bw.line(l) {
			a.printf("... %d more lines; narrow it: %s\n", len(lines)-i, next)
			break
		}
	}
}

// ---- memory marks ----

// memMark counts the binding notes (rules, decisions, recorded errors) that
// name a file or folder.
type memMark struct{ rules, decisions, errors int }

func (m memMark) plus(o memMark) memMark {
	return memMark{m.rules + o.rules, m.decisions + o.decisions, m.errors + o.errors}
}

func (m memMark) String() string {
	var parts []string
	for _, x := range []struct {
		n    int
		name string
	}{{m.rules, "rule"}, {m.decisions, "decision"}, {m.errors, "error"}} {
		if x.n == 1 {
			parts = append(parts, "1 "+x.name)
		} else if x.n > 1 {
			parts = append(parts, fmt.Sprintf("%d %ss", x.n, x.name))
		}
	}
	if len(parts) == 0 {
		return ""
	}
	return "[" + strings.Join(parts, ", ") + "]"
}

func (m *memMark) count(typ string) {
	switch typ {
	case "rule":
		m.rules++
	case "decision":
		m.decisions++
	case "error":
		m.errors++
	}
}

// codeMarks maps each file and folder of the listing to the active rules,
// decisions and errors whose code references point at it, each note counted
// once per file or folder.
func (a *app) codeMarks(s *store, files []string) (map[string]memMark, map[string]memMark) {
	byFile, byDir := map[string]memMark{}, map[string]memMark{}
	refs, err := s.db.CodeRefs()
	if err != nil {
		return byFile, byDir
	}
	lower := map[string]string{} // lower-case path -> listed path
	byBase := map[string][]string{}
	dirs := map[string]string{}
	for _, f := range files {
		l := strings.ToLower(f)
		lower[l] = f
		byBase[path.Base(l)] = append(byBase[path.Base(l)], l)
		for d := path.Dir(f); d != "."; d = path.Dir(d) {
			dirs[strings.ToLower(d)] = d
		}
	}
	notes := map[int64]*index.File{}
	seen := map[string]bool{}
	hit := func(m map[string]memMark, key string, f *index.File) {
		k := fmt.Sprintf("%s\x00%d", key, f.ID)
		if seen[k] {
			return
		}
		seen[k] = true
		x := m[key]
		x.count(f.Type)
		m[key] = x
	}
	for _, r := range refs {
		f, ok := notes[r.FileID]
		if !ok {
			f, _ = s.db.FileByID(r.FileID)
			notes[r.FileID] = f
		}
		if f == nil || f.Inactive() || (f.Type != "rule" && f.Type != "decision" && f.Type != "error") {
			continue
		}
		ref := r.Ref
		switch {
		case strings.HasPrefix(ref, "pkg:"):
			for d, orig := range dirs {
				if path.Base(d) == ref[4:] {
					hit(byDir, orig, f)
				}
			}
		case strings.HasSuffix(ref, "/"):
			for d, orig := range dirs {
				if d+"/" == ref || strings.HasSuffix(d+"/", "/"+ref) {
					hit(byDir, orig, f)
				}
			}
		default:
			for _, p := range byBase[path.Base(ref)] {
				if p == ref || !strings.Contains(ref, "/") || strings.HasSuffix(p, "/"+ref) || strings.HasSuffix(ref, "/"+p) {
					hit(byFile, lower[p], f)
				}
			}
		}
	}
	return byFile, byDir
}

// ---- code ----

// srcExt are the files whose declarations are listed.
var srcExt = map[string]bool{".go": true, ".py": true, ".js": true, ".mjs": true, ".ts": true, ".tsx": true, ".jsx": true,
	".rs": true, ".java": true, ".kt": true, ".cs": true, ".rb": true, ".php": true, ".swift": true, ".scala": true,
	".c": true, ".h": true, ".cc": true, ".cpp": true, ".hpp": true}

// skipDirs are never listed when there is no git to ask.
var skipDirs = map[string]bool{".git": true, "node_modules": true, "vendor": true, "dist": true, "build": true, "target": true,
	".venv": true, "venv": true, "__pycache__": true, ".idea": true, ".vscode": true, "bin": true, "obj": true}

// workFiles lists the files of the work tree below sub: git's view
// (tracked and untracked, ignored left out), else a walk of the folder.
func (a *app) workFiles(sub string) ([]string, string) {
	root := a.workDir()
	var out []string
	how := "git"
	if _, err := os.Stat(filepath.Join(root, ".git")); err == nil {
		args := []string{"ls-files", "--cached", "--others", "--exclude-standard", "-z"}
		if sub != "" {
			args = append(args, "--", sub)
		}
		for _, f := range strings.Split(a.gitOut(args...), "\x00") {
			if f != "" {
				out = append(out, f)
			}
		}
	} else {
		how = "folder"
		filepath.WalkDir(filepath.Join(root, filepath.FromSlash(sub)), func(p string, e os.DirEntry, err error) error {
			if err != nil {
				return nil
			}
			if e.IsDir() {
				if skipDirs[e.Name()] || (strings.HasPrefix(e.Name(), ".") && p != root) {
					return filepath.SkipDir
				}
				return nil
			}
			if rel, err := filepath.Rel(root, p); err == nil && len(out) < 5000 {
				out = append(out, filepath.ToSlash(rel))
			}
			return nil
		})
	}
	sort.Strings(out)
	return out, how
}

var (
	reGoMethod = regexp.MustCompile(`^func \(\s*\w*\s*\*?(\w+)[^)]*\)\s*(\w+)`)
	reDeclName = regexp.MustCompile(`\b(func|def|function|fn|class|struct|enum|trait|interface|type|const|var|impl)\s+(\w+)`)
	reJavaDecl = regexp.MustCompile(`^public (?:static )?[\w<>\[\]]+ (\w+)\(`)
)

// declNames turns declaration lines into short names: Charge(), Provider,
// Cart.Total().
func declNames(decls []string, max int) string {
	var out []string
	for _, d := range decls {
		if d == "..." {
			continue
		}
		name := ""
		if m := reGoMethod.FindStringSubmatch(d); m != nil {
			name = m[1] + "." + m[2] + "()"
		} else if m := reJavaDecl.FindStringSubmatch(d); m != nil {
			name = m[1] + "()"
		} else if m := reDeclName.FindStringSubmatch(d); m != nil {
			name = m[2]
			switch m[1] {
			case "func", "def", "function", "fn":
				name += "()"
			}
		}
		if name != "" && !slices.Contains(out, name) {
			out = append(out, name)
		}
	}
	if len(out) > max {
		return strings.Join(out[:max], ", ") + fmt.Sprintf(", +%d", len(out)-max)
	}
	return strings.Join(out, ", ")
}

func isTestFile(name string) bool {
	l := strings.ToLower(name)
	return strings.HasSuffix(l, "_test.go") || strings.Contains(l, ".test.") || strings.Contains(l, ".spec.") ||
		strings.HasPrefix(l, "test_") || strings.HasSuffix(l, "_test.py")
}

func (a *app) codeTree(s *store, arg string, maxDepth int) error {
	root := a.workDir()
	sub := ""
	if arg != "" {
		abs, err := filepath.Abs(arg)
		if err != nil {
			return fail(ExitUsage, "%v", err)
		}
		rel, err := filepath.Rel(root, abs)
		if err != nil || strings.HasPrefix(rel, "..") {
			return fail(ExitUsage, "%s is outside the work tree %s", arg, filepath.ToSlash(root))
		}
		if rel != "." {
			sub = filepath.ToSlash(rel)
		}
	}
	files, how := a.workFiles(sub)
	if len(files) == 0 {
		a.printf("no files in %s\n", filepath.ToSlash(filepath.Join(root, filepath.FromSlash(sub))))
		return errNoResults
	}
	byFile, byDir := a.codeMarks(s, files)
	tree := newTreeDir("", "")
	marked := 0
	for _, f := range files {
		info := ""
		name := path.Base(f)
		if srcExt[strings.ToLower(path.Ext(f))] && !isTestFile(name) {
			info = declNames(declsOf(filepath.Join(root, filepath.FromSlash(f)), 24), 8)
		}
		m := byFile[f]
		if m != (memMark{}) {
			marked++
		}
		rel := f
		if sub != "" {
			rel = strings.TrimPrefix(strings.TrimPrefix(f, sub), "/")
		}
		if rel == "" {
			rel = name
		}
		tree.add(rel, treeFile{path: f, info: info, mark: m})
	}
	var setMarks func(d *treeDir)
	setMarks = func(d *treeDir) {
		full := d.path
		if sub != "" && d.path != "" {
			full = sub + "/" + d.path
		} else if sub != "" {
			full = sub
		}
		d.mark = byDir[full]
		for _, c := range d.dirs {
			setMarks(c)
		}
	}
	setMarks(tree)
	if a.jsonOut {
		var out []map[string]any
		for _, f := range files {
			m := byFile[f]
			out = append(out, map[string]any{"path": f, "rules": m.rules, "decisions": m.decisions, "errors": m.errors})
		}
		return a.emitJSON(map[string]any{"root": filepath.ToSlash(root), "files": out})
	}
	where := filepath.ToSlash(root)
	if sub != "" {
		where += "/" + sub
	}
	head := fmt.Sprintf("code: %s  (%d files, %s)", where, len(files), how)
	if marked > 0 || len(byDir) > 0 {
		head += "  [n rules, ...]: memory about that file or folder, see pg related <file>"
	}
	a.printTree(tree, head, "pg tree <dir>", maxDepth, func(d *treeDir) string { return fmt.Sprintf("(%d files)", d.n) })
	return nil
}

// ---- notes ----

func (a *app) notesTree(s *store, arg string, maxDepth int) error {
	prefix := strings.Trim(filepath.ToSlash(arg), "/")
	if prefix == "" {
		prefix = s.project
		if ps := s.v.Projects(); prefix == "" && len(ps) == 1 {
			prefix = ps[0]
		}
	} else if !strings.Contains(prefix, "/") && !s.v.HasProject(prefix) {
		p, err := a.findProject(s, prefix)
		if err != nil {
			return err
		}
		prefix = p
	}
	tree := newTreeDir("", "")
	total, n := 0, 0
	tcs, _ := s.db.TaskCountsByFile()
	for _, f := range s.res.Files() {
		if prefix != "" && f.Path != prefix+".md" && !strings.HasPrefix(f.Path, prefix+"/") {
			continue
		}
		rel := strings.TrimPrefix(strings.TrimPrefix(f.Path, prefix), "/")
		typ := f.Type
		if typ == "note" {
			typ = ""
		}
		if f.Inactive() {
			typ = join(typ, "inactive")
		}
		info := join(meta(typ), textutil.FmtTokens(f.Tokens), taskCount(tcs[f.ID]), titleIfDifferent(f))
		tree.add(strings.TrimSuffix(rel, ".md"), treeFile{path: f.Path, info: info, tokens: f.Tokens})
		total += f.Tokens
		n++
	}
	if n == 0 {
		if prefix == "" {
			a.printf("space %s has no notes yet\n", s.v.Name)
		} else {
			a.printf("no notes in %s\n", prefix)
		}
		return errNoResults
	}
	where := "space " + s.v.Name
	if prefix != "" {
		where = prefix + "/"
		if prefix == s.project {
			where += "  (current project)"
		}
	}
	tok := map[*treeDir]int{}
	var sum func(d *treeDir) int
	sum = func(d *treeDir) int {
		t := 0
		for _, f := range d.files {
			t += f.tokens
		}
		for _, c := range d.dirs {
			t += sum(c)
		}
		tok[d] = t
		return t
	}
	sum(tree)
	head := fmt.Sprintf("notes: %s  (%d notes, %s)", where, n, textutil.FmtTokens(total))
	next := "pg tree --notes <folder>"
	a.printTree(tree, head, next, maxDepth, func(d *treeDir) string {
		return fmt.Sprintf("(%d notes, %s)", d.n, textutil.FmtTokens(tok[d]))
	})
	return nil
}

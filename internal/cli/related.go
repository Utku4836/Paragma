package cli

import (
	"fmt"
	"os"
	"path"
	"path/filepath"
	"sort"
	"strings"

	"github.com/Utku4836/Paragma/internal/index"
	"github.com/Utku4836/Paragma/internal/md"
	"github.com/Utku4836/Paragma/internal/textutil"
	"github.com/spf13/cobra"
)

// From code to memory: the notes that mention a source file.

// relHit is a note (or a block/section of one) that mentions a code path.
type relHit struct {
	f     *index.File
	addr  string
	line  int
	score float64
	why   string
}

// typeRank orders note types by how binding they are for code changes.
var typeRank = map[string]int{"rule": 0, "decision": 1, "error": 2, "concept": 3, "note": 3, "project": 4, "journal": 5}

// codePath turns a path given on the command line into the form notes use:
// relative to the work tree root, lower case, forward slashes.
func (a *app) codePath(p string) string {
	if abs, err := filepath.Abs(p); err == nil {
		if rel, err := filepath.Rel(a.workDir(), abs); err == nil && !strings.HasPrefix(rel, "..") {
			p = rel
		}
	}
	return md.NormRef(p)
}

// refScore says how strongly a code reference in a note points at path.
func refScore(ref, p string) (float64, string) {
	dir := path.Dir(p)
	switch {
	case ref == p:
		return 3, "names " + p
	case strings.Contains(ref, "/") && !strings.HasSuffix(ref, "/") && (strings.HasSuffix(p, "/"+ref) || strings.HasSuffix(ref, "/"+p)):
		return 2.5, "names " + p
	case !strings.Contains(ref, "/") && !strings.HasPrefix(ref, "pkg:") && ref == path.Base(p):
		return 2, "names " + ref
	case strings.HasSuffix(ref, "/") && (strings.HasPrefix(p, ref) || strings.Contains(p, "/"+ref)):
		return 2, "covers " + ref
	case dir != "." && ref == "pkg:"+path.Base(dir):
		return 1.5, "package " + path.Base(dir)
	case dir != "." && strings.Contains(ref, "/") && !strings.HasSuffix(ref, "/") && path.Dir(ref) == dir:
		return 1, "same directory " + dir
	}
	return 0, ""
}

// relatedNotes finds notes that mention the given code paths. Hits inside
// a block (a rule, a task) are reported per block, others per section.
func (a *app) relatedNotes(s *store, paths []string, minScore float64, includeInactive bool) []relHit {
	refs, err := s.db.CodeRefs()
	if err != nil {
		return nil
	}
	best := map[string]relHit{}
	secCache := map[int64]*index.Section{}
	for _, r := range refs {
		var sc float64
		var why string
		for _, p := range paths {
			if x, w := refScore(r.Ref, p); x > sc {
				sc, why = x, w
			}
		}
		if sc < minScore {
			continue
		}
		f, err := s.db.FileByID(r.FileID)
		if err != nil || (!includeInactive && f.Inactive()) {
			continue
		}
		ad := f.Addr()
		if b := s.db.BlockAt(f.ID, r.Line); b != "" {
			ad += "^" + b
		} else if r.SectionID > 0 {
			sec, ok := secCache[r.SectionID]
			if !ok {
				sec, _ = s.db.SectionByID(r.SectionID)
				secCache[r.SectionID] = sec
			}
			if sec != nil && sec.Path != "" && !sec.IsTitle {
				ad += "#" + sec.Path
			}
		}
		if cur, ok := best[ad]; !ok || sc > cur.score {
			best[ad] = relHit{f: f, addr: ad, line: r.Line, score: sc, why: why}
		}
	}
	var out []relHit
	for _, h := range best {
		out = append(out, h)
	}
	// what binds a change comes first: a rule about the package outranks an
	// old error that names the file
	sort.SliceStable(out, func(i, j int) bool {
		ri, rj := typeRank[out[i].f.Type], typeRank[out[j].f.Type]
		if ri != rj {
			return ri < rj
		}
		if out[i].score != out[j].score {
			return out[i].score > out[j].score
		}
		return out[i].addr < out[j].addr
	})
	return out
}

func (a *app) relatedCmd() *cobra.Command {
	var loose, all bool
	c := &cobra.Command{
		Use:   "related [code path]...",
		Short: "Rules, decisions and past errors about a source file, and its git history; without a path: the files you changed",
		Long: `List the memory that mentions a code file: rules and decisions first,
then recorded errors (each with its fix; errors with the same fix folded)
and notes, for the file, its directory or its package (payments.Refund
counts for payments/...), followed by the file's last commits. Run it
before editing a file. --loose also lists notes about files in the same
directory.
Without a path: every file changed in the work tree (git status, and since
the current project started) with the rules and decisions about it; the
check before calling the work done.`,
		Example: "pg related payments/refund.go\npg related internal/db/open.go cmd/migrate/main.go\npg related",
		Args:    cobra.ArbitraryArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			if len(args) == 0 {
				return a.relatedChanges(s)
			}
			var paths []string
			for _, p := range args {
				paths = append(paths, a.codePath(p))
			}
			min := 1.5
			if loose {
				min = 1
			}
			hits := a.relatedNotes(s, paths, min, all)
			if a.jsonOut {
				var out []map[string]any
				for _, h := range hits {
					out = append(out, map[string]any{"addr": h.addr, "type": h.f.Type, "status": h.f.Status, "why": h.why, "score": h.score})
				}
				if out == nil {
					out = []map[string]any{}
				}
				return a.emitJSON(out)
			}
			if len(hits) == 0 {
				a.println("no notes about it in memory")
				a.printHistory(args)
				return errNoResults
			}
			// errors that record the same fix (the same mistake in many places)
			// are one line: the first, with the others counted
			sameFix := map[string]int{}
			firstOf := map[string]string{}
			var shown []relHit
			for _, h := range hits {
				if k := a.fixKey(s, h.f); k != "" {
					if first, ok := firstOf[k]; ok && first != h.f.Addr() {
						sameFix[first]++
						continue
					}
					firstOf[k] = h.f.Addr()
				}
				shown = append(shown, h)
			}
			bw := a.newBudget(a.budgetOr(s.v.Cfg.Budget))
			for i, h := range shown {
				typ := h.f.Type
				if typ == "note" {
					typ = ""
				}
				head := join(h.addr, meta(typ, statusWord(h.f)), h.why)
				if n := sameFix[h.f.Addr()]; n > 0 {
					head += fmt.Sprintf("  (+%d notes with the same fix)", n)
				}
				text := a.relatedText(s, h)
				if !bw.line(head) || (text != "" && !bw.line("  "+text)) {
					a.printf("... %d more; pg related %s --json for all\n", len(shown)-i, strings.Join(args, " "))
					break
				}
			}
			a.printHistory(args)
			return nil
		},
	}
	c.Flags().BoolVar(&loose, "loose", false, "also notes about other files in the same directory")
	c.Flags().BoolVar(&all, "all", false, "include archived and superseded notes")
	return c
}

// printHistory prints the last commits that touched the given files, the
// way git log --oneline does (nothing outside a git work tree).
func (a *app) printHistory(files []string) {
	root := a.workDir()
	var rels []string
	for _, f := range files {
		if abs, err := filepath.Abs(f); err == nil {
			if rel, err := filepath.Rel(root, abs); err == nil && !strings.HasPrefix(rel, "..") {
				rels = append(rels, filepath.ToSlash(rel))
			}
		}
	}
	if len(rels) == 0 {
		return
	}
	out := strings.TrimSpace(a.gitOut(append([]string{"log", "-n", "5", "--date=short", "--format=%ad %h %s", "--"}, rels...)...))
	if out == "" {
		return
	}
	a.println("history (git log):")
	for _, l := range strings.Split(out, "\n") {
		a.println("  " + textutil.OneLine(l, 120))
	}
}

// relatedChanges is pg related without a path: the files changed in the
// work tree, each with the rules and decisions about it.
func (a *app) relatedChanges(s *store) error {
	if _, err := os.Stat(filepath.Join(a.workDir(), ".git")); err != nil {
		return fail(ExitUsage, "not in a git work tree: give the files, pg related <path>...")
	}
	base := ""
	if f := projectNote(s, s.project); f != nil {
		if lines, err := fileLines(s, f); err == nil {
			base = frontValue(lines, "base")
		}
	}
	changed := a.changedSince(base)
	if len(changed) == 0 {
		a.println("no changed files (git status is clean); one file: pg related <path>")
		return nil
	}
	label := "changed in the work tree (git status)"
	if base != "" {
		label = "changed since project " + s.project + " started (git)"
	}
	a.printf("%s: %d files\n", label, len(changed))
	bw := a.newBudget(a.budgetOr(max(s.v.Cfg.Budget, 1200)))
	bound := 0
	printed := map[string]bool{}
	for i, c := range changed {
		if c.status == "D" {
			if !bw.line(fmt.Sprintf("%s %s", c.status, c.path)) {
				break
			}
			continue
		}
		var lines, again []string
		for _, h := range a.relatedNotes(s, []string{a.codePath(c.path)}, 1.5, false) {
			if h.f.Type != "rule" && h.f.Type != "decision" {
				continue
			}
			if printed[h.addr] {
				again = append(again, h.addr)
				continue
			}
			printed[h.addr] = true
			lines = append(lines, "  "+join(h.addr, meta(h.f.Type))+": "+strings.TrimPrefix(a.relatedText(s, h), "decision: "))
			if len(lines) == 4 {
				break
			}
		}
		head := fmt.Sprintf("%s %s", c.status, c.path)
		switch {
		case len(lines) == 0 && len(again) == 0:
			head += "  (no rules or decisions about it)"
		case len(again) > 0:
			head += "  (also " + strings.Join(again, ", ") + ", above)"
		}
		if !bw.line(head) {
			a.printf("... %d more files; one file: pg related <path>\n", len(changed)-i)
			break
		}
		for _, l := range lines {
			bw.line(l)
		}
		bound += len(lines)
	}
	if bound > 0 {
		a.println("check the changes against these before calling the work done")
	}
	return nil
}

// relatedText is what a related hit says, so it needs no pg read: an
// error's fix, a decision's decision, else the line that names the code.
func (a *app) relatedText(s *store, h relHit) string {
	switch h.f.Type {
	case "error":
		if _, fix := a.fixText(s, h.f, 220); fix != "" {
			return "fix: " + fix
		}
	case "decision":
		if d := a.sectionBody(s, h.f, "decision"); len(d) > 0 {
			return "decision: " + textutil.OneLine(strings.Join(d, " "), 220)
		}
	}
	lines, err := fileLines(s, h.f)
	if err != nil || h.line < 1 || h.line > len(lines) {
		return ""
	}
	return textutil.OneLine(cleanLine(strings.TrimLeft(lines[h.line-1], " -*")), 200)
}

func statusWord(f *index.File) string {
	if f.Status == "active" || f.Status == "" {
		return ""
	}
	return f.Status
}

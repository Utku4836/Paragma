package cli

// The task brief: what work on a project needs, printed by pg start and pg
// resume so the agent does not gather it call by call (prime, context,
// related, tree, search, read): the memory about the task, the code it is
// likely to touch, and how to run the tests.

import (
	"encoding/json"
	"os"
	"path"
	"path/filepath"
	"sort"
	"strings"

	"github.com/Utku4836/Paragma/internal/index"
	"github.com/Utku4836/Paragma/internal/textutil"
)

// briefStop are words of task descriptions that say nothing about where
// in memory or code the task lives.
var briefStop = map[string]bool{}

func init() {
	for _, w := range strings.Fields(`the and for with from into that this then than when what which while where
		add adds added make makes use uses using new old all any each every one two three only also must should
		can will would could not are was were been being have has had our your their its his her them they you
		step steps done task tasks work fix fixes bug code test tests write read run file files function method
		support implement change update create remove delete handle get set call return returns value values
		first second last next before after above below other more most less some such like just way part
		ctx context func err error errors nil int int64 string bool byte return package type struct var const
		import true false self none null void async await class interface public private static object
		bir ve ile için olan olarak gibi daha çok`) {
		briefStop[w] = true
	}
}

// taskKeywords picks the words a task is about: the first words of its
// text that are not filler or code syntax and that memory knows at all.
// A task says what it is about first ("refund support for payments").
func taskKeywords(s *store, text string, n int) []string {
	seen := map[string]bool{}
	var out []string
	for _, w := range strings.FieldsFunc(textutil.Fold(text), func(r rune) bool {
		return !(r >= 'a' && r <= 'z' || r >= '0' && r <= '9' || r == '_' || r > 127)
	}) {
		if len(w) < 3 || briefStop[w] || seen[w] {
			continue
		}
		seen[w] = true
		if s.db.Count(`SELECT COUNT(*) FROM sections_fts WHERE sections_fts MATCH ?`, `"`+w+`"*`) > 0 {
			out = append(out, w)
			if len(out) == n {
				break
			}
		}
	}
	return out
}

// printBrief prints the brief for project p (its note is f).
func (a *app) printBrief(s *store, p string, f *index.File) {
	if a.jsonOut || f == nil {
		return
	}
	text := strings.Join(a.sectionBody(s, f, "goal"), " ")
	if ts, err := s.db.Tasks(f.ID); err == nil {
		for _, t := range ts {
			if !t.Closed() {
				text += " " + t.Text
			}
		}
	}
	for _, d := range a.sectionBody(s, f, "decisions") {
		text += " " + d
	}
	keys := taskKeywords(s, text, 4)
	var noteIDs []int64
	if len(keys) > 0 {
		q := strings.Join(keys, " ")
		var items []ctxItem
		for _, it := range a.gatherContext(s, q) {
			if it.f.ID != f.ID { // the project note itself was printed above
				items = append(items, it)
			}
		}
		if len(items) > 0 {
			a.println("")
			a.println("## memory about this task")
			a.ctxSkip = f.ID
			a.printContext(s, q, items, 900)
			a.ctxSkip = 0
			for _, it := range items {
				noteIDs = append(noteIDs, it.f.ID)
			}
		}
	}
	a.printLikelyCode(s, text, keys, noteIDs)
	if cmd := testCommand(a.workDir()); cmd != "" {
		a.printf("tests: pg run -- %s  (on failure it prints the known fix)\n", cmd)
	}
	a.println("when the work is done: pg finish \"<what changed, what is next>\"")
}

// printLikelyCode lists the files of the work tree the task is likely to
// touch: named by the notes about the task, or matching its words in their
// path or declarations; each with its declarations and memory marks.
func (a *app) printLikelyCode(s *store, text string, keys []string, noteIDs []int64) {
	if _, err := os.Stat(filepath.Join(a.workDir(), ".git")); err != nil {
		return
	}
	files, _ := a.workFiles("")
	if len(files) == 0 {
		return
	}
	words := map[string]bool{}
	for _, w := range strings.FieldsFunc(textutil.Fold(text), func(r rune) bool {
		return !(r >= 'a' && r <= 'z' || r >= '0' && r <= '9' || r > 127)
	}) {
		if len(w) >= 4 && !briefStop[w] {
			words[w] = true
		}
	}
	for _, k := range keys {
		words[k] = true
	}
	score := map[string]float64{}
	// named by the notes the brief printed
	if len(noteIDs) > 0 {
		refs, _ := s.db.CodeRefs()
		ids := map[int64]bool{}
		for _, id := range noteIDs {
			ids[id] = true
		}
		for _, r := range refs {
			if !ids[r.FileID] {
				continue
			}
			for _, f := range files {
				if sc, _ := refScore(r.Ref, strings.ToLower(f)); sc >= 2 {
					score[f] += sc
				}
			}
		}
	}
	// the task's words in the path or the declarations
	decls := map[string][]string{}
	root := a.workDir()
	for i, f := range files {
		if !srcExt[strings.ToLower(path.Ext(f))] || isTestFile(path.Base(f)) {
			continue
		}
		low := textutil.Fold(f)
		for w := range words {
			if strings.Contains(low, w) {
				score[f] += 2
			}
		}
		if i < 2000 {
			ds := declsOf(filepath.Join(root, filepath.FromSlash(f)), 40)
			decls[f] = ds
			for _, d := range ds {
				ld := textutil.Fold(d)
				for w := range words {
					if strings.Contains(ld, w) {
						score[f] += 0.5
					}
				}
			}
		}
	}
	var pick []string
	for f, sc := range score {
		if sc >= 1.5 && srcExt[strings.ToLower(path.Ext(f))] {
			pick = append(pick, f)
		}
	}
	if len(pick) == 0 {
		return
	}
	sort.Slice(pick, func(i, j int) bool {
		if score[pick[i]] != score[pick[j]] {
			return score[pick[i]] > score[pick[j]]
		}
		return pick[i] < pick[j]
	})
	pick = pick[:min(len(pick), 6)]
	byFile, _ := a.codeMarks(s, pick)
	a.println("## code likely involved (pg read <file>#<Name> for one declaration, pg tree for the rest)")
	for _, f := range pick {
		ds, ok := decls[f]
		if !ok {
			ds = declsOf(filepath.Join(root, filepath.FromSlash(f)), 40)
		}
		a.println(join("  "+f, declNames(ds, 8), byFile[f].String()))
	}
}

// testCommand guesses how the project's tests are run.
func testCommand(dir string) string {
	has := func(name string) bool {
		_, err := os.Stat(filepath.Join(dir, name))
		return err == nil
	}
	switch {
	case has("go.mod"):
		return "go test ./..."
	case has("Cargo.toml"):
		return "cargo test"
	case has("package.json"):
		var pkg struct {
			Scripts map[string]string `json:"scripts"`
		}
		if b, err := os.ReadFile(filepath.Join(dir, "package.json")); err == nil && json.Unmarshal(b, &pkg) == nil && pkg.Scripts["test"] != "" {
			if has("pnpm-lock.yaml") {
				return "pnpm test"
			}
			if has("yarn.lock") {
				return "yarn test"
			}
			return "npm test"
		}
	case has("pyproject.toml") || has("pytest.ini") || has("setup.cfg") || has("tox.ini"):
		return "pytest"
	case has("pom.xml"):
		return "mvn -q test"
	case has("build.gradle") || has("build.gradle.kts"):
		return "gradle test"
	case has("Makefile"):
		if b, err := os.ReadFile(filepath.Join(dir, "Makefile")); err == nil && strings.Contains(string(b), "\ntest:") {
			return "make test"
		}
	}
	return ""
}

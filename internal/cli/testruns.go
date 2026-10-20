package cli

// What pg run remembers about test runs, per command: when it ran, which
// tests failed, and the state of the work tree (HEAD and the content of the
// changed files). With it pg run tells a new failure from one that was
// there before, a flaky test (failed and passed on the same code) from a
// broken one, and which files changed between a failure and the run that
// fixed it.

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path"
	"path/filepath"
	"regexp"
	"slices"
	"sort"
	"strings"
	"time"

	"github.com/Utku4836/Paragma/internal/errsig"
	"github.com/Utku4836/Paragma/internal/space"
)

type testRun struct {
	Time    string            `json:"time"`
	Exit    int               `json:"exit"`
	Failing []string          `json:"failing,omitempty"`
	Head    string            `json:"head,omitempty"`
	Files   map[string]string `json:"files,omitempty"` // changed file -> content hash
}

type testHistory struct {
	Runs  map[string][]testRun `json:"runs"`  // command -> last runs, oldest first
	Flaky map[string][]string  `json:"flaky"` // command -> tests seen failing and passing on the same code
}

func historyPath(s *store) string { return s.v.Meta("test-runs.json") }

func loadHistory(s *store) *testHistory {
	h := &testHistory{}
	if b, err := os.ReadFile(historyPath(s)); err == nil {
		json.Unmarshal(b, h)
	}
	if h.Runs == nil {
		h.Runs = map[string][]testRun{}
	}
	if h.Flaky == nil {
		h.Flaky = map[string][]string{}
	}
	return h
}

func (h *testHistory) save(s *store) {
	if b, err := json.Marshal(h); err == nil {
		space.WriteFileAtomic(historyPath(s), b)
	}
}

func (h *testHistory) last(cmd string) *testRun {
	rs := h.Runs[cmd]
	if len(rs) == 0 {
		return nil
	}
	return &rs[len(rs)-1]
}

// add records a run (keeping the last ten per command) and learns flaky
// tests: one that failed in a run and passed in another on the same code.
func (h *testHistory) add(cmd string, r testRun) {
	for _, old := range h.Runs[cmd] {
		if old.Head != r.Head || !sameFiles(old.Files, r.Files) {
			continue
		}
		for _, t := range symDiff(old.Failing, r.Failing) {
			if r.Exit == 0 || old.Exit == 0 || slices.Contains(old.Failing, t) != slices.Contains(r.Failing, t) {
				if !slices.Contains(h.Flaky[cmd], t) {
					h.Flaky[cmd] = append(h.Flaky[cmd], t)
				}
			}
		}
	}
	rs := append(h.Runs[cmd], r)
	if len(rs) > 10 {
		rs = rs[len(rs)-10:]
	}
	h.Runs[cmd] = rs
}

func sameFiles(a, b map[string]string) bool {
	if len(a) != len(b) {
		return false
	}
	for k, v := range a {
		if b[k] != v {
			return false
		}
	}
	return true
}

func symDiff(a, b []string) []string {
	var out []string
	for _, x := range a {
		if !slices.Contains(b, x) {
			out = append(out, x)
		}
	}
	for _, x := range b {
		if !slices.Contains(a, x) {
			out = append(out, x)
		}
	}
	return out
}

// worktreeState is HEAD and the content hash of every changed or new file.
func (a *app) worktreeState() (string, map[string]string) {
	root := a.workDir()
	if _, err := os.Stat(filepath.Join(root, ".git")); err != nil {
		return "", nil
	}
	head := strings.TrimSpace(a.gitOut("rev-parse", "HEAD"))
	files := map[string]string{}
	for _, c := range a.changedSince("") {
		if len(files) == 300 {
			break
		}
		b, err := os.ReadFile(filepath.Join(root, filepath.FromSlash(c.path)))
		if err != nil {
			files[c.path] = "deleted"
			continue
		}
		sum := sha256.Sum256(b)
		files[c.path] = hex.EncodeToString(sum[:6])
	}
	return head, files
}

// changedBetween lists the files whose content differs between two states.
func changedBetween(before, after map[string]string) []string {
	var out []string
	for f, h := range after {
		if before[f] != h {
			out = append(out, f)
		}
	}
	for f := range before {
		if _, ok := after[f]; !ok {
			out = append(out, f) // reverted to the committed version
		}
	}
	sort.Strings(out)
	return out
}

// failing test names in the output of the common test runners
var reFailing = []*regexp.Regexp{
	regexp.MustCompile(`(?m)^\s*--- FAIL: (\S+)`),                                  // go test
	regexp.MustCompile(`(?m)^FAILED (\S+?)(?: - |$)`),                              // pytest
	regexp.MustCompile(`(?m)^test (\S+) \.\.\. FAILED`),                            // cargo test
	regexp.MustCompile(`(?m)^\s*[✕×✗]\s+(.+?)(?:\s+\(\d+(?:\.\d+)?\s*m?s\))?\s*$`), // jest, vitest
	regexp.MustCompile(`(?m)^\s*● (.+ › .+)$`),                                     // jest
}

func failingTests(output string) []string {
	var out []string
	for _, re := range reFailing {
		for _, m := range re.FindAllStringSubmatch(output, -1) {
			if t := strings.TrimSpace(m[1]); t != "" && !slices.Contains(out, t) {
				out = append(out, t)
			}
		}
	}
	if len(out) > 50 {
		out = out[:50]
	}
	return out
}

// summarise shortens command output for the agent: all of it when short;
// for a success its start and end; for a failure every error line with a
// little context and the end. ok reports whether lines were left out.
func summarise(output string, failed bool) (string, bool) {
	lines := strings.Split(strings.TrimRight(strings.ReplaceAll(output, "\r\n", "\n"), "\n"), "\n")
	limit := 40
	if failed {
		limit = 150
	}
	if len(lines) <= limit {
		return strings.Join(lines, "\n"), false
	}
	keep := map[int]bool{}
	if failed {
		errLines := map[string]bool{}
		for _, l := range errsig.Lines(output, 60) {
			errLines[l.Raw] = true
		}
		for _, t := range failingTests(output) {
			errLines[t] = true
		}
		for i, l := range lines {
			t := strings.TrimSpace(l)
			hit := errLines[t]
			if !hit {
				for k := range errLines {
					if len(k) > 6 && strings.Contains(t, k) {
						hit = true
						break
					}
				}
			}
			if hit {
				for j := max(0, i-2); j <= min(len(lines)-1, i+3); j++ {
					keep[j] = true
				}
			}
			if len(keep) > 110 {
				break
			}
		}
		for j := len(lines) - 25; j < len(lines); j++ {
			keep[j] = true
		}
	} else {
		for j := 0; j < 5; j++ {
			keep[j] = true
		}
		for j := len(lines) - 15; j < len(lines); j++ {
			keep[j] = true
		}
	}
	var b strings.Builder
	skipped := 0
	for i, l := range lines {
		if !keep[i] {
			skipped++
			continue
		}
		if skipped > 0 {
			fmt.Fprintf(&b, "... (%d lines)\n", skipped)
			skipped = 0
		}
		b.WriteString(l + "\n")
	}
	return strings.TrimRight(b.String(), "\n"), true
}

// affectedArgs narrows a Go test command to the packages with changed Go
// files: ./... becomes ./payments ./cart. Other commands stay as they are.
func (a *app) affectedArgs(args []string) []string {
	if _, err := os.Stat(filepath.Join(a.workDir(), "go.mod")); err != nil {
		return args
	}
	dirs := map[string]bool{}
	for _, c := range a.changedSince("") {
		if strings.HasSuffix(c.path, ".go") && c.status != "D" {
			dirs["./"+path.Dir(c.path)] = true
		}
	}
	if len(dirs) == 0 {
		return args
	}
	var pkgs []string
	for d := range dirs {
		pkgs = append(pkgs, strings.TrimSuffix(d, "/."))
	}
	sort.Strings(pkgs)
	var out []string
	for _, x := range args {
		if x == "./..." {
			out = append(out, pkgs...)
			continue
		}
		out = append(out, x)
	}
	return out
}

func clockOf(stamp string) string {
	if t, err := time.Parse(time.RFC3339, stamp); err == nil {
		return t.Local().Format("15:04")
	}
	return stamp
}

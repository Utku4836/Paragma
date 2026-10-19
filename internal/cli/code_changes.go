package cli

import (
	"bufio"
	"context"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"
	"time"
)

// The code a project has touched, so that a new conversation does not
// rediscover it: pg start records the git commit the work began at (base:),
// pg resume lists the files changed since then with their top-level
// declarations.

// gitOut runs git in the work tree with a short timeout; "" on any error.
func (a *app) gitOut(args ...string) string {
	ctx, cancel := context.WithTimeout(context.Background(), 1500*time.Millisecond)
	defer cancel()
	c := exec.CommandContext(ctx, "git", args...)
	c.Dir = a.workDir()
	out, err := c.Output()
	if err != nil {
		return ""
	}
	return string(out)
}

// gitHead is the commit checked out in the work tree ("" outside git).
func (a *app) gitHead() string {
	if _, err := os.Stat(filepath.Join(a.workDir(), ".git")); err != nil {
		return ""
	}
	return strings.TrimSpace(a.gitOut("rev-parse", "HEAD"))
}

type changedFile struct {
	path, status string // status: A added, M modified, D deleted, R renamed
}

// changedSince lists files changed since base (committed or not) and
// untracked files.
func (a *app) changedSince(base string) []changedFile {
	seen := map[string]bool{}
	var out []changedFile
	add := func(st, p string) {
		p = strings.Trim(strings.TrimSpace(p), `"`)
		if p == "" || seen[p] || strings.HasPrefix(p, ".") {
			return
		}
		seen[p] = true
		out = append(out, changedFile{p, st})
	}
	if base != "" {
		for _, l := range strings.Split(a.gitOut("diff", "--name-status", base), "\n") {
			f := strings.Split(l, "\t")
			if len(f) >= 2 && f[0] != "" {
				add(f[0][:1], f[len(f)-1])
			}
		}
	}
	for _, l := range strings.Split(a.gitOut("status", "--porcelain", "--untracked-files=all"), "\n") {
		if len(l) < 4 {
			continue
		}
		st := strings.TrimSpace(l[:2])
		p := l[3:]
		if i := strings.Index(p, " -> "); i >= 0 {
			p = p[i+4:]
		}
		switch {
		case st == "??":
			st = "A"
		case st != "":
			st = st[:1]
		}
		add(st, p)
	}
	return out
}

// top-level declarations worth listing, per language family
var reDecl = regexp.MustCompile(`^(func |type |var [A-Z]|const [A-Z]|def |class |async def |export (default )?(async )?(function|class|const|interface|type) |function |interface |pub (fn|struct|enum|trait) |fn |struct |enum |impl |public (static )?[\w<>\[\]]+ \w+\()`)

// declsOf returns the top-level declarations of a source file, one line
// each without bodies.
func declsOf(path string, max int) []string {
	f, err := os.Open(path)
	if err != nil {
		return nil
	}
	defer f.Close()
	if st, err := f.Stat(); err != nil || st.Size() > 512*1024 {
		return nil
	}
	var out []string
	sc := bufio.NewScanner(f)
	sc.Buffer(make([]byte, 64*1024), 1024*1024)
	for sc.Scan() {
		l := sc.Text()
		if !reDecl.MatchString(l) {
			continue
		}
		if i := strings.Index(l, " {"); i > 0 {
			l = l[:i] // a one-line body
		}
		l = strings.TrimSpace(strings.TrimRight(strings.TrimSpace(l), "{:"))
		if len(l) > 110 {
			l = l[:107] + "..."
		}
		out = append(out, l)
		if len(out) == max {
			out = append(out, "...")
			break
		}
	}
	return out
}

// printCodeChanges prints what the project changed in the work tree since
// it started: one line per file with its declarations.
func (a *app) printCodeChanges(base string) {
	if _, err := os.Stat(filepath.Join(a.workDir(), ".git")); err != nil {
		return
	}
	ch := a.changedSince(base)
	if len(ch) == 0 {
		return
	}
	label := "code changed in this work tree (git, uncommitted"
	if base != "" {
		label = "code changed since the project started (git"
	}
	a.println(label + "):")
	for i, c := range ch {
		if i == 15 {
			a.printf("  ... %d more: git status\n", len(ch)-15)
			break
		}
		line := fmt.Sprintf("  %s %s", c.status, c.path)
		if c.status != "D" {
			if ds := declsOf(filepath.Join(a.workDir(), filepath.FromSlash(c.path)), 8); len(ds) > 0 {
				line += ": " + strings.Join(ds, "; ")
			}
		}
		a.println(line)
	}
}

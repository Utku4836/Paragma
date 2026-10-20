package cli

// Reading code by part: pg outline <file> lists a source file's top-level
// declarations with their line ranges, pg read <file>#<Name> prints one of
// them (Cart.Total for a method), pg read <file>:10-30 a line range. An
// agent reads the function it needs instead of the whole file.

import (
	"fmt"
	"os"
	"path"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"

	"github.com/Utku4836/Paragma/internal/md"
	"github.com/Utku4836/Paragma/internal/textutil"
)

type codeDecl struct {
	name      string // Charge, Cart.Total, Provider
	sig       string // the declaration line, without its body
	line, end int    // 1-based, inclusive
}

// codeTarget is a source file named on the command line, with the part
// asked for: a declaration (#Name) or a line range (:10-30).
type codeTarget struct {
	abs, rel string
	symbol   string
	from, to int
}

var (
	reCodeRange = regexp.MustCompile(`^(\d+)(?:-(\d+))?$`)
	reGoBlock   = regexp.MustCompile(`^(var|const|type|import)\s*\($`)
)

// codeArg reports whether arg names a source file (and which part of it):
// a known source extension, or any existing file that is not a note.
func (a *app) codeArg(arg string) (*codeTarget, bool) {
	file, sub := arg, ""
	if i := strings.IndexByte(arg, '#'); i >= 0 {
		file, sub = arg[:i], arg[i:]
	} else if i := strings.LastIndexByte(arg, ':'); i > 1 && reCodeRange.MatchString(arg[i+1:]) {
		file, sub = arg[:i], arg[i:]
	}
	ext := strings.ToLower(filepath.Ext(file))
	if file == "" || ext == ".md" {
		return nil, false
	}
	root := a.workDir()
	var abs string
	for _, cand := range []string{file, filepath.Join(root, filepath.FromSlash(file))} {
		if p, err := filepath.Abs(cand); err == nil {
			if st, err := os.Stat(p); err == nil && !st.IsDir() {
				abs = p
				break
			}
		}
	}
	if abs == "" {
		return nil, false // not on disk: maybe a note address after all
	}
	if ext == "" && sub == "" {
		return nil, false // a bare word is a note name more often than a file
	}
	t := &codeTarget{abs: abs, rel: filepath.ToSlash(abs)}
	if rel, err := filepath.Rel(root, abs); err == nil && !strings.HasPrefix(rel, "..") {
		t.rel = filepath.ToSlash(rel)
	}
	switch {
	case strings.HasPrefix(sub, "#"):
		t.symbol = strings.TrimSpace(sub[1:])
	case strings.HasPrefix(sub, ":"):
		m := reCodeRange.FindStringSubmatch(sub[1:])
		t.from, _ = strconv.Atoi(m[1])
		t.to = t.from
		if m[2] != "" {
			t.to, _ = strconv.Atoi(m[2])
		}
	}
	return t, true
}

// codeDecls lists the top-level declarations of a source file with the
// lines each one spans.
func codeDecls(lines []string, ext string) []codeDecl {
	var out []codeDecl
	for i, l := range lines {
		if !reDecl.MatchString(l) {
			continue
		}
		name := declNames([]string{l}, 1)
		if name == "" {
			continue
		}
		name = strings.TrimSuffix(name, "()")
		end := i
		if ext == ".py" {
			end = indentEnd(lines, i)
		} else {
			end = braceEnd(lines, i)
		}
		sig := strings.TrimSpace(l)
		if j := strings.Index(sig, " {"); j > 0 {
			sig = sig[:j]
		}
		out = append(out, codeDecl{name: name, sig: textutil.OneLine(strings.TrimRight(sig, "{:"), 140), line: i + 1, end: end + 1})
	}
	return out
}

// braceEnd is the line where the declaration starting at line i closes:
// its braces (or the parentheses of a Go var/const/type/import block)
// balance, or the declaration was a single line.
func braceEnd(lines []string, i int) int {
	depth, opened := 0, false
	block := reGoBlock.MatchString(strings.TrimSpace(lines[i]))
	for j := i; j < len(lines) && j < i+3000; j++ {
		for _, ch := range codeOnly(lines[j]) {
			switch ch {
			case '{':
				depth++
				opened = true
			case '(':
				depth++
				if block {
					opened = true
				}
			case '}', ')':
				depth--
			}
		}
		if opened && depth <= 0 {
			return j
		}
		if !opened && depth <= 0 {
			t := strings.TrimSpace(codeOnly(lines[j]))
			if t == "" || !strings.ContainsAny(t[len(t)-1:], ",=+-*/|&.(") {
				return j // a one-line declaration
			}
		}
	}
	return i
}

// indentEnd is the last line of a Python block: the line before the next
// line indented no deeper than the declaration.
func indentEnd(lines []string, i int) int {
	ind := len(lines[i]) - len(strings.TrimLeft(lines[i], " \t"))
	end := i
	for j := i + 1; j < len(lines); j++ {
		t := strings.TrimSpace(lines[j])
		if t == "" || strings.HasPrefix(t, "#") {
			continue
		}
		if len(lines[j])-len(strings.TrimLeft(lines[j], " \t")) <= ind {
			break
		}
		end = j
	}
	return end
}

// codeOnly drops string literals and line comments, so braces inside them
// do not count.
func codeOnly(l string) string {
	var b strings.Builder
	var quote rune
	prev := rune(0)
	for _, r := range l {
		switch {
		case quote != 0:
			if r == quote && prev != '\\' {
				quote = 0
			}
		case r == '"' || r == '\'' || r == '`':
			quote = r
		case r == '/' && prev == '/':
			s := b.String()
			return s[:len(s)-1]
		default:
			b.WriteRune(r)
		}
		prev = r
	}
	return b.String()
}

// findDecl finds a declaration by name: exact, then without regard to
// case, then a method by its bare name (Total for Cart.Total); for Python
// also a method inside a class (Cart.total).
func findDecl(decls []codeDecl, lines []string, ext, name string) (codeDecl, bool) {
	for _, pass := range []func(d codeDecl) bool{
		func(d codeDecl) bool { return d.name == name },
		func(d codeDecl) bool { return strings.EqualFold(d.name, name) },
		func(d codeDecl) bool { return strings.EqualFold(d.name[strings.LastIndex(d.name, ".")+1:], name) },
	} {
		for _, d := range decls {
			if pass(d) {
				return d, true
			}
		}
	}
	if cls, meth, ok := strings.Cut(name, "."); ok {
		if c, ok := findDecl(decls, lines, ext, cls); ok {
			re := regexp.MustCompile(`^\s+(async\s+)?(def|fn|func|function|public|private|protected|static)?\b.*\b` + regexp.QuoteMeta(meth) + `\s*\(`)
			for j := c.line; j < c.end && j < len(lines); j++ {
				if re.MatchString(lines[j]) {
					end := braceEnd(lines, j)
					if ext == ".py" {
						end = indentEnd(lines, j)
					}
					return codeDecl{name: name, sig: strings.TrimSpace(lines[j]), line: j + 1, end: end + 1}, true
				}
			}
		}
	}
	return codeDecl{}, false
}

func readSource(t *codeTarget) ([]string, error) {
	b, err := os.ReadFile(t.abs)
	if err != nil {
		return nil, fail(ExitInternal, "read %s: %v", t.rel, err)
	}
	return md.SplitLines(b), nil
}

// readCode prints the part of a source file a codeTarget names.
func (a *app) readCode(t *codeTarget, numbers bool) error {
	lines, err := readSource(t)
	if err != nil {
		return err
	}
	ext := strings.ToLower(path.Ext(t.rel))
	from, to, label := 1, len(lines), t.rel
	switch {
	case t.symbol != "":
		d, ok := findDecl(codeDecls(lines, ext), lines, ext, t.symbol)
		if !ok {
			var names []string
			for _, x := range codeDecls(lines, ext) {
				names = append(names, x.name)
			}
			msg := fmt.Sprintf("no declaration %q in %s", t.symbol, t.rel)
			if c := textutil.Closest(t.symbol, names, 3); len(c) > 0 {
				msg += "; did you mean: " + strings.Join(c, ", ")
			}
			return fail(ExitNotFound, "%s (pg outline %s lists them)", msg, t.rel)
		}
		// the comment right above belongs to it
		for d.line > 1 && strings.HasPrefix(strings.TrimSpace(lines[d.line-2]), "//") {
			d.line--
		}
		from, to, label = d.line, d.end, t.rel+"#"+t.symbol
	case t.from > 0:
		from, to = t.from, min(t.to, len(lines))
		label = fmt.Sprintf("%s:%d-%d", t.rel, from, to)
	}
	body := sliceLines(lines, from, to)
	text := strings.Join(body, "\n")
	if a.jsonOut {
		return a.emitJSON(map[string]any{"addr": label, "from": from, "to": to, "tokens": textutil.Tokens(text), "text": text})
	}
	lr := fmt.Sprintf("L%d-%d", from, to)
	if t.from > 0 {
		lr = "" // the label is the range
	}
	a.println(join(label, lr, textutil.FmtTokens(textutil.Tokens(text))))
	a.printLines(body, from, numbers)
	return nil
}

// outlineCode prints a source file's declarations with their line ranges
// and the memory about the file.
func (a *app) outlineCode(t *codeTarget) error {
	lines, err := readSource(t)
	if err != nil {
		return err
	}
	decls := codeDecls(lines, strings.ToLower(path.Ext(t.rel)))
	if a.jsonOut {
		var out []map[string]any
		for _, d := range decls {
			out = append(out, map[string]any{"name": d.name, "line": d.line, "end": d.end, "sig": d.sig})
		}
		return a.emitJSON(map[string]any{"file": t.rel, "lines": len(lines), "declarations": out})
	}
	a.println(join(t.rel, fmt.Sprintf("%d lines", len(lines)), textutil.FmtTokens(textutil.Tokens(strings.Join(lines, "\n")))))
	for _, d := range decls {
		cost := textutil.Tokens(strings.Join(sliceLines(lines, d.line, d.end), "\n"))
		a.println(join(fmt.Sprintf("  %s#%s", t.rel, d.name), fmt.Sprintf("L%d-%d", d.line, d.end), textutil.FmtTokens(cost), d.sig))
	}
	if len(decls) == 0 {
		a.println("  (no top-level declarations found)")
	}
	if s, err := a.main(); err == nil {
		var notes []string
		for _, h := range a.relatedNotes(s, []string{a.codePath(t.rel)}, 2, false) {
			if h.f.Type == "rule" || h.f.Type == "decision" || h.f.Type == "error" {
				notes = append(notes, h.addr)
			}
			if len(notes) == 4 {
				break
			}
		}
		if len(notes) > 0 {
			a.printf("memory about it: %s (pg related %s)\n", strings.Join(notes, ", "), t.rel)
		}
	}
	return nil
}

// Package cli implements the pg command line. Output follows the
// agent-first contract: every result line starts with a copy-pasteable
// address, carries a token estimate, and plain text is the default.
package cli

import (
	"bufio"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"strings"
	"time"

	"github.com/Utku4836/Paragma/internal/addr"
	"github.com/Utku4836/Paragma/internal/index"
	"github.com/Utku4836/Paragma/internal/space"
	"github.com/Utku4836/Paragma/internal/textutil"
)

// Exit codes (pg help prints them).
const (
	ExitOK         = 0
	ExitNoResults  = 1
	ExitUsage      = 2
	ExitNotFound   = 3
	ExitConflict   = 4
	ExitNoSpace    = 5
	ExitValidation = 6
	ExitInternal   = 7
)

// Error is an error with an exit code. Msg is printed as "error: <Msg>".
type Error = addr.Error

func fail(code int, f string, a ...any) error { return &Error{Code: code, Msg: fmt.Sprintf(f, a...)} }

// errNoResults is returned after "no results" has been printed.
var errNoResults = &Error{Code: ExitNoResults}

type app struct {
	jsonOut   bool
	budget    int
	homeFlag  string
	spaceFlag string
	quiet     bool
	ifHash    string
	dryRun    bool
	textFile  string

	stdout *bufio.Writer
	rawOut io.Writer // unbuffered stdout and stderr, for pg run
	rawErr io.Writer
	stdin  io.Reader

	st *store // opened lazily

	inLock   bool              // inside withLock
	written  map[string]string // notes this process wrote: path -> hash
	holdLock bool              // take the space lock before the command looks anything up
	heldLock string            // the lock file held for the whole command
}

// errStale: a note changed on disk after this command looked it up (another
// pg process wrote it before this one got the lock). Execute runs the
// command again when nothing was written yet.
var errStale = &Error{Code: ExitConflict, Msg: "a note changed while this command ran (another pg process wrote it); run the command again"}

// store is the opened space with its synced index.
type store struct {
	v       *space.Space
	db      *index.DB
	res     *addr.Resolver
	project string // current project in this work directory ("" = none)
}

func newApp(stdout io.Writer, stdin io.Reader) *app {
	return &app{stdout: bufio.NewWriterSize(stdout, 32*1024), rawOut: stdout, rawErr: os.Stderr, stdin: stdin}
}

func (a *app) close() {
	a.stdout.Flush()
	if a.st != nil {
		a.st.db.Close()
	}
}

func (a *app) printf(f string, args ...any) { fmt.Fprintf(a.stdout, f, args...) }

func (a *app) println(s string) { a.stdout.WriteString(s); a.stdout.WriteByte('\n') }

func (a *app) home() string { return space.Home(a.homeFlag) }

func home() string {
	h, _ := os.UserHomeDir()
	return h
}

// workDir is the directory pg remembers the current project for: the git
// work tree root, else the working directory.
func (a *app) workDir() string {
	cwd, err := os.Getwd()
	if err != nil {
		return "."
	}
	return space.WorkDir(cwd)
}

// main opens and syncs the space for this invocation.
func (a *app) main() (*store, error) {
	if a.st != nil {
		return a.st, nil
	}
	t0 := time.Now()
	v, err := space.Find(a.home(), a.spaceFlag, a.workDir())
	if err != nil {
		if errors.Is(err, space.ErrNoSpace) {
			return nil, fail(ExitNoSpace, "%v", err)
		}
		return nil, fail(ExitInternal, "%v", err)
	}
	t1 := time.Now()
	db, err := index.Open(v)
	if err != nil {
		return nil, fail(ExitInternal, "%v", err)
	}
	t2 := time.Now()
	st, err := db.Sync()
	if err != nil {
		db.Close()
		return nil, fail(ExitInternal, "sync: %v", err)
	}
	t3 := time.Now()
	res, err := addr.NewResolver(db)
	if err != nil {
		db.Close()
		return nil, fail(ExitInternal, "%v", err)
	}
	if os.Getenv("PARAGMA_TRACE") != "" {
		fmt.Fprintf(os.Stderr, "trace: find %v open %v sync %v (files %d changed %d) resolver %v\n",
			t1.Sub(t0), t2.Sub(t1), t3.Sub(t2), st.Files, st.Changed, time.Since(t3))
	}
	s := &store{v: v, db: db, res: res}
	s.project = a.currentProject(s)
	res.Prefer = s.project
	a.st = s
	return s, nil
}

// currentProject is $PARAGMA_PROJECT, else the project last started or
// resumed in this work directory, if its folder still exists.
func (a *app) currentProject(s *store) string {
	if p := os.Getenv("PARAGMA_PROJECT"); p != "" && s.v.HasProject(p) {
		return p
	}
	reg := space.LoadRegistry(s.v.Home)
	if p := reg.Dir(a.workDir()).Projects[s.v.Name]; s.v.HasProject(p) {
		return p
	}
	return ""
}

// setProject makes p the current project of this work directory.
func (a *app) setProject(s *store, p string) error {
	s.project = p
	s.res.Prefer = p
	if a.dryRun {
		return nil
	}
	reg := space.LoadRegistry(s.v.Home)
	reg.Touch(a.workDir()).Projects[s.v.Name] = p
	if err := reg.Save(); err != nil {
		return fail(ExitInternal, "%v", err)
	}
	return nil
}

// writeProject is the project a write goes to: the current one, or the only
// project of the space.
func (a *app) writeProject(s *store) (string, error) {
	if s.project != "" {
		return s.project, nil
	}
	ps := s.v.Projects()
	if len(ps) == 1 {
		return ps[0], nil
	}
	if len(ps) == 0 {
		return "", fail(ExitNotFound, "no project yet in space %s; start one: pg start <name> --goal \"...\"", s.v.Name)
	}
	return "", fail(ExitNotFound, "no current project here; pick one: pg resume <name> (projects: %s) or pg start <name>", strings.Join(ps, ", "))
}

// resolve parses and resolves an address.
func (a *app) resolve(raw string) (*store, *addr.Target, error) {
	p, err := addr.Parse(raw)
	if err != nil {
		return nil, nil, err
	}
	s, err := a.main()
	if err != nil {
		return nil, nil, err
	}
	t, err := s.res.Resolve(p)
	if err != nil {
		// a bare task/block id ("t3f2") written without its ^
		if p.File != "" && p.Heading == "" && p.Block == "" && p.From == 0 && !strings.ContainsAny(p.File, "/.") {
			if bt, berr := s.res.Resolve(addr.Addr{Block: p.File}); berr == nil {
				return s, bt, nil
			}
		}
		return nil, nil, err
	}
	return s, t, nil
}

func (a *app) budgetOr(def int) int {
	if a.budget > 0 {
		return a.budget
	}
	return def
}

// budgetWriter prints lines until a token budget runs out.
type budgetWriter struct {
	a      *app
	budget int
	used   int
	cut    bool
}

func (a *app) newBudget(n int) *budgetWriter { return &budgetWriter{a: a, budget: n} }

// line writes s if it fits; it returns false (and remembers the cut) if not.
func (b *budgetWriter) line(s string) bool {
	t := textutil.Tokens(s) + 1
	if b.budget > 0 && b.used+t > b.budget {
		b.cut = true
		return false
	}
	b.used += t
	b.a.println(s)
	return true
}

// fits reports whether n more tokens fit.
func (b *budgetWriter) fits(n int) bool { return b.budget <= 0 || b.used+n <= b.budget }

func shellQuote(s string) string {
	if s != "" && !strings.ContainsAny(s, " \t\"'$`\\|&;<>()*?![]{}#~") {
		return s
	}
	return `"` + strings.ReplaceAll(s, `"`, `\"`) + `"`
}

func meta(parts ...string) string {
	var keep []string
	for _, p := range parts {
		if p != "" {
			keep = append(keep, p)
		}
	}
	if len(keep) == 0 {
		return ""
	}
	return "[" + strings.Join(keep, " ") + "]"
}

func join(parts ...string) string {
	var keep []string
	for _, p := range parts {
		if p != "" {
			keep = append(keep, p)
		}
	}
	return strings.Join(keep, "  ")
}

func (a *app) emitJSON(v any) error {
	enc := json.NewEncoder(a.stdout)
	enc.SetEscapeHTML(false)
	return enc.Encode(v)
}

func (a *app) ok(s string) {
	switch {
	case a.dryRun:
	case a.quiet:
		a.println(strings.Fields(s)[0]) // the address alone
	default:
		a.println("ok " + s)
	}
}

func now() time.Time { return time.Now() }

func today() string { return now().Format("2006-01-02") }

func clock() string { return now().Format("15:04") }

func stamp() string { return now().Format("2006-01-02 15:04") }

// Package cli implements the pg command line. Output follows the
// agent-first contract: every result line starts with a copy-pasteable
// address, carries a token estimate, and plain text is the default.
package cli

import (
	"bufio"
	"errors"
	"fmt"
	"io"
	"os"
	"strings"
	"time"

	"github.com/Utku-Private/Paragma/internal/addr"
	"github.com/Utku-Private/Paragma/internal/index"
	"github.com/Utku-Private/Paragma/internal/textutil"
	"github.com/Utku-Private/Paragma/internal/vault"
)

// Exit codes (see docs/TASARIM.md, "Çıkış kodları").
const (
	ExitOK         = 0
	ExitNoResults  = 1
	ExitUsage      = 2
	ExitNotFound   = 3
	ExitConflict   = 4
	ExitNoVault    = 5
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
	vaultFlag string
	scope     string
	quiet     bool
	ifHash    string
	dryRun    bool
	textFile  string

	stdout *bufio.Writer
	rawOut io.Writer // unbuffered stdout and stderr, for pg run
	rawErr io.Writer
	stdin  io.Reader

	// opened lazily
	stores map[bool]*store // keyed by Global
}

// store is one opened vault with its synced index.
type store struct {
	v   *vault.Vault
	db  *index.DB
	res *addr.Resolver
}

func newApp(stdout io.Writer, stdin io.Reader) *app {
	return &app{stdout: bufio.NewWriterSize(stdout, 32*1024), rawOut: stdout, rawErr: os.Stderr, stdin: stdin, stores: map[bool]*store{}}
}

func (a *app) close() {
	a.stdout.Flush()
	for _, s := range a.stores {
		s.db.Close()
	}
}

func (a *app) printf(f string, args ...any) { fmt.Fprintf(a.stdout, f, args...) }
func (a *app) println(s string)             { a.stdout.WriteString(s); a.stdout.WriteByte('\n') }

// openVault opens and syncs a vault. global selects the global vault
// explicitly; otherwise --scope/--vault and directory discovery decide.
func (a *app) openVault(global bool) (*store, error) {
	scope := a.scope
	if global {
		scope = "global"
	}
	if scope == "all" {
		scope = ""
	}
	t0 := time.Now()
	v, err := vault.Find(a.vaultFlag, scope)
	if err != nil {
		if errors.Is(err, vault.ErrNoVault) {
			return nil, fail(ExitNoVault, "%v", err)
		}
		return nil, fail(ExitInternal, "%v", err)
	}
	if s, ok := a.stores[v.Global]; ok {
		return s, nil
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
	a.stores[v.Global] = s
	return s, nil
}

// main returns the primary store for the current scope.
func (a *app) main() (*store, error) { return a.openVault(false) }

// stores for --scope all: project (if any) then global (if any).
func (a *app) allStores() ([]*store, error) {
	if a.scope != "all" {
		s, err := a.main()
		if err != nil {
			return nil, err
		}
		return []*store{s}, nil
	}
	var out []*store
	saved := a.scope
	a.scope = "project"
	if s, err := a.openVault(false); err == nil {
		out = append(out, s)
	}
	a.scope = saved
	if s, err := a.openVault(true); err == nil && (len(out) == 0 || s != out[0]) {
		out = append(out, s)
	}
	if len(out) == 0 {
		return nil, fail(ExitNoVault, "no vault found (run: pg init)")
	}
	return out, nil
}

// resolve parses and resolves an address, picking the global vault for
// addresses that start with "~".
func (a *app) resolve(raw string) (*store, *addr.Target, error) {
	p, err := addr.Parse(raw)
	if err != nil {
		return nil, nil, err
	}
	s, err := a.openVault(p.Global)
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

// prefix returns the address prefix for results from s ("~" for global
// results when they are mixed with project results).
func (a *app) prefix(s *store) string {
	if s.v.Global && a.scope == "all" {
		return "~"
	}
	if s.v.Global && a.scope != "global" {
		// only reachable via fallback; still unambiguous without ~
		return ""
	}
	return ""
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

func readStdin(r io.Reader) (string, error) {
	b, err := io.ReadAll(r)
	return string(b), err
}

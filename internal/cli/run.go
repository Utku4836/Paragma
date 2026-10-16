package cli

import (
	"encoding/json"
	"errors"
	"io"
	"os/exec"
	"runtime"
	"strings"
	"sync"
	"time"

	"github.com/Utku-Private/Paragma/internal/errsig"
	"github.com/Utku-Private/Paragma/internal/md"
	"github.com/Utku-Private/Paragma/internal/vault"
	"github.com/spf13/cobra"
)

// tailBuffer keeps the last max bytes written to it.
type tailBuffer struct {
	mu  sync.Mutex
	buf []byte
	max int
}

func (t *tailBuffer) Write(p []byte) (int, error) {
	t.mu.Lock()
	defer t.mu.Unlock()
	t.buf = append(t.buf, p...)
	if len(t.buf) > t.max {
		t.buf = t.buf[len(t.buf)-t.max:]
	}
	return len(p), nil
}

func (t *tailBuffer) String() string {
	t.mu.Lock()
	defer t.mu.Unlock()
	return string(t.buf)
}

func (a *app) runCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "run -- <command> [args...]",
		Short: "Run a test/build command; on failure, print the known fix from memory",
		Long: `Run a command with its output passed through unchanged and its exit code
kept. If it fails and memory knows the error (exact or likely match), pg
appends the recorded fix after the output. The failure is saved so that
after fixing it you can record it with: pg error record --from-last ...
A single quoted argument is run through the shell (cmd /C on Windows,
sh -c elsewhere), so pipes work there.`,
		Example: `pg run -- go test ./...
pg run -- npm test
pg run -- "make build 2>&1"`,
		Args:               cobra.MinimumNArgs(1),
		DisableFlagParsing: false,
		RunE: func(cmd *cobra.Command, args []string) error {
			a.stdout.Flush()
			var c *exec.Cmd
			if len(args) == 1 && strings.ContainsAny(args[0], " \t|&<>") {
				if runtime.GOOS == "windows" {
					c = exec.Command("cmd", "/C", args[0])
				} else {
					c = exec.Command("sh", "-c", args[0])
				}
			} else {
				c = exec.Command(args[0], args[1:]...)
			}
			tail := &tailBuffer{max: 256 * 1024}
			c.Stdin = a.stdin
			c.Stdout = io.MultiWriter(a.rawOut, tail)
			c.Stderr = io.MultiWriter(a.rawErr, tail)
			err := c.Run()
			code := 0
			if err != nil {
				var ee *exec.ExitError
				if errors.As(err, &ee) {
					code = ee.ExitCode()
				} else {
					return fail(ExitUsage, "cannot run %s: %v", args[0], err)
				}
			}
			if code == 0 {
				return nil
			}
			a.afterFailure(strings.Join(args, " "), code, tail.String())
			return &Error{Code: code}
		},
	}
}

// afterFailure saves the failed run and prints the known fix, if any. It
// never fails the command for reasons of its own: no vault, a broken index
// or a timeout all mean "print nothing".
func (a *app) afterFailure(command string, code int, output string) {
	s, err := a.openVault(false) // project vault, else the global one
	if err != nil {
		return
	}
	lines := errsig.Lines(output, 30)
	var raw []string
	for _, l := range lines {
		raw = append(raw, l.Raw)
	}
	out := output
	if len(out) > 8000 {
		out = out[len(out)-8000:]
	}
	if b, err := json.MarshalIndent(lastRun{Cmd: command, Exit: code, Time: stamp(), Lines: raw, Output: out}, "", " "); err == nil {
		vault.WriteFileAtomic(lastRunPath(s), b)
	}

	// the project's own record wins; the global vault (errors that are not
	// about one project: toolchain, OS, CI) is the fallback
	done := make(chan []errMatch, 1)
	from := s
	go func() {
		ms := a.matchErrors(s, output, false)
		if len(ms) == 0 && !s.v.Global {
			if g, err := a.openVault(true); err == nil {
				if ms = a.matchErrors(g, output, false); len(ms) > 0 {
					from = g
				}
			}
		}
		done <- ms
	}()
	var ms []errMatch
	select {
	case ms = <-done:
	case <-time.After(2 * time.Second):
	}
	if len(ms) > 0 {
		m := ms[0]
		addr, fix := a.fixText(from, m.f, 700)
		if from != s {
			addr = "~" + addr // global vault
		}
		a.printf("--- paragma: known error (%s match, seen %dx) %s\n", m.level, maxInt(m.f.Seen, 1), addr)
		if fix != "" {
			a.println(fix)
		}
		if len(ms) > 1 {
			var more []string
			for _, x := range ms[1:min(len(ms), 3)] {
				more = append(more, globalMark(from, s)+x.f.Addr())
			}
			a.println("also: " + strings.Join(more, ", "))
		}
		a.printf("if it works: pg error seen %s%s\n", globalMark(from, s), m.f.Addr())
		a.printFileRules(s, raw)
		return
	}
	if s.v.Cfg.Hints && len(lines) > 0 {
		a.println(`--- paragma: not in memory. After fixing it: pg error record --from-last --cause "..." --fix "..."`)
	}
	a.printFileRules(s, raw)
}

// printFileRules shows the rules and decisions about the source files named
// in failing output (at most two lines): what the code there must respect.
func (a *app) printFileRules(s *store, lines []string) {
	var paths []string
	seen := map[string]bool{}
	for _, l := range lines {
		for _, r := range md.CodeRefs(" " + l + " ") {
			if strings.HasPrefix(r, "pkg:") || strings.HasSuffix(r, "/") || seen[r] {
				continue
			}
			seen[r] = true
			paths = append(paths, codePath(s, r))
		}
	}
	if len(paths) == 0 {
		return
	}
	n := 0
	for _, h := range a.relatedNotes(s, paths, 2, false) {
		if h.f.Type != "rule" && h.f.Type != "decision" {
			continue
		}
		a.printf("rule for this file: %s (%s)\n", h.addr, h.why)
		if n++; n == 2 {
			break
		}
	}
}

func maxInt(a, b int) int {
	if a > b {
		return a
	}
	return b
}

// globalMark is "~" when a match came from the global vault.
func globalMark(from, project *store) string {
	if from != project {
		return "~"
	}
	return ""
}

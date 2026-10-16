package cli

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"

	"github.com/Utku-Private/Paragma/internal/errsig"
	"github.com/Utku-Private/Paragma/internal/index"
	"github.com/Utku-Private/Paragma/internal/textutil"
	"github.com/spf13/cobra"
)

// ---- matching ----

// errMatch is a recorded error that matches some output.
type errMatch struct {
	f     *index.File
	level string // exact | likely | related
	line  string // the output line that matched
}

var levelRank = map[string]int{"exact": 0, "likely": 1, "related": 2}

// matchErrors finds recorded errors for command output or an error message.
// exact: the recorded signature equals, or is contained in, an output line
// after normalisation. likely: at least 80% of the signature's words appear
// in one line. related (only when allowRelated): full-text hits among error
// notes. pg run shows exact and likely only, so a wrong guess never lands in
// front of the agent unasked.
func (a *app) matchErrors(s *store, output string, allowRelated bool) []errMatch {
	lines := errsig.Lines(output, 30)
	if len(lines) == 0 && len(strings.TrimSpace(output)) > 0 && len(output) < 400 {
		t := strings.TrimSpace(output)
		lines = []errsig.Line{{Raw: t, Norm: errsig.Normalize(t)}}
	}
	sigs, err := s.db.ErrorSigs()
	if err != nil {
		return nil
	}
	best := map[int64]errMatch{}
	consider := func(fid int64, level, line string) {
		f, err := s.db.FileByID(fid)
		if err != nil {
			return
		}
		if cur, ok := best[fid]; ok && levelRank[cur.level] <= levelRank[level] {
			return
		}
		best[fid] = errMatch{f: f, level: level, line: line}
	}
	for _, l := range lines {
		lw := errsig.Words(l.Norm)
		h := errsig.Hash(l.Norm)
		for _, sg := range sigs {
			sw := errsig.Words(sg.Norm)
			// how much of the output line the signature explains: a short,
			// generic signature ("context deadline exceeded") inside a longer
			// unrelated line must not count as the same error
			covers := errsig.Overlap(lw, sw)
			specific := len(sw) >= 4 || covers >= 0.6
			switch {
			case sg.Hash == h,
				specific && len(sg.Norm) >= 12 && strings.Contains(l.Norm, sg.Norm),
				len(l.Norm) >= 12 && strings.Contains(sg.Norm, l.Norm) && float64(len(l.Norm)) >= 0.6*float64(len(sg.Norm)):
				consider(sg.FileID, "exact", l.Raw)
			case len(sw) >= 3 && errsig.Overlap(sw, lw) >= 0.8 && covers >= 0.6:
				consider(sg.FileID, "likely", l.Raw)
			}
		}
	}
	if allowRelated && len(lines) > 0 {
		var words []string
		for _, l := range lines[:min(len(lines), 3)] {
			words = append(words, errsig.Words(l.Norm)...)
		}
		if len(words) > 0 {
			hits, _ := s.db.Search(index.SearchOpts{Query: strings.Join(words[:min(len(words), 12)], " "), Types: []string{"error"}, Max: 20})
			n := 0
			for _, h := range hits {
				if _, ok := best[h.File.ID]; ok {
					continue
				}
				consider(h.File.ID, "related", lines[0].Raw)
				if n++; n >= 3 {
					break
				}
			}
		}
	}
	var out []errMatch
	for _, m := range best {
		out = append(out, m)
	}
	sort.SliceStable(out, func(i, j int) bool {
		if levelRank[out[i].level] != levelRank[out[j].level] {
			return levelRank[out[i].level] < levelRank[out[j].level]
		}
		if out[i].f.Inactive() != out[j].f.Inactive() {
			return !out[i].f.Inactive()
		}
		return out[i].f.Seen > out[j].f.Seen
	})
	return out
}

// fixText is the Fix section of an error note on one line-ish block.
func (a *app) fixText(s *store, f *index.File, maxRunes int) (string, string) {
	addr := f.Addr()
	if sec := findSection(s, f, "fix"); sec != nil {
		addr += "#" + sec.Path
	}
	body := a.sectionBody(s, f, "fix")
	return addr, textutil.OneLine(strings.Join(body, " "), maxRunes)
}

// ---- last failed run ----

type lastRun struct {
	Cmd    string   `json:"cmd"`
	Exit   int      `json:"exit"`
	Time   string   `json:"time"`
	Lines  []string `json:"error_lines"`
	Output string   `json:"output_tail"`
}

func lastRunPath(s *store) string { return filepath.Join(s.v.Root, "last-run.json") }

func loadLastRun(s *store) (*lastRun, error) {
	b, err := os.ReadFile(lastRunPath(s))
	if err != nil {
		return nil, fail(ExitNotFound, "no failed pg run recorded yet (run the command with: pg run -- <cmd>)")
	}
	var lr lastRun
	if err := json.Unmarshal(b, &lr); err != nil {
		return nil, fail(ExitInternal, "last-run.json: %v", err)
	}
	return &lr, nil
}

// ---- commands ----

func (a *app) errorCmd() *cobra.Command {
	c := &cobra.Command{
		Use:   "error",
		Short: "Error memory: record a fixed error, find the fix for an error, mark a fix as seen again",
	}
	c.AddCommand(a.errorRecordCmd(), a.errorFindCmd(), a.errorSeenCmd(), a.errorReopenCmd())
	return c
}

func (a *app) errorRecordCmd() *cobra.Command {
	var sig, title, symptom, cause, fix, files, tags string
	var fromLast, force, global bool
	c := &cobra.Command{
		Use:   "record --sig TEXT|--from-last --cause TEXT --fix TEXT",
		Short: "Record a fixed error so it is recognised next time",
		Long: `Record an error you fixed. --sig is the error message as it appears
(paths, numbers and hashes are normalised, so the next occurrence matches
even with different values). --from-last takes the signature and output
from the last failed pg run. If the error is already recorded, pg says so
and changes nothing (then use pg error seen, or --new for a separate one).`,
		Example: `pg error record --from-last --cause "CRLF checkout" --fix "normalise line endings in Load"
pg error record --sig "database is locked (5)" --cause "WAL off" --fix "PRAGMA journal_mode=WAL" --files internal/db/open.go`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			proj := s // where pg run saved the last failure
			if global {
				if s, err = a.openVault(true); err != nil {
					return fail(ExitNoVault, "no global vault yet (pg init --global)")
				}
			}
			var lr *lastRun
			if fromLast {
				if lr, err = loadLastRun(proj); err != nil {
					return err
				}
				if sig == "" && len(lr.Lines) > 0 {
					sig = bestSigLine(lr.Lines)
				}
			}
			if sig == "" {
				return fail(ExitUsage, "give --sig \"<error message>\" or --from-last")
			}
			if cause == "" || fix == "" {
				return fail(ExitUsage, "--cause and --fix are required: what was wrong and what fixed it")
			}
			if !force {
				for _, m := range a.matchErrors(s, sig, false) {
					a.printf("exists: %s (%s match, seen:%d); nothing written\n", m.f.Addr(), m.level, max(m.f.Seen, 1))
					a.printf("next: pg error seen %s | pg error reopen %s --note \"...\" | --new to record separately\n", m.f.Addr(), m.f.Addr())
					return nil
				}
			}
			if title == "" {
				title = errorTitle(sig)
			}
			if symptom == "" {
				symptom = "`" + strings.ReplaceAll(textutil.OneLine(sig, 300), "`", "'") + "`"
				if lr != nil {
					symptom = fmt.Sprintf("`%s` failed (exit %d) with:\n\n```\n%s\n```", lr.Cmd, lr.Exit, strings.Join(lr.Lines[:min(len(lr.Lines), 5)], "\n"))
				}
			}
			var tagList []string
			for _, t := range splitList([]string{tags}) {
				tagList = append(tagList, strings.TrimPrefix(t, "#"))
			}
			lines := a.template(s, "error", title, tagList, "")
			lines = setFront(lines, "status", "resolved")
			lines = setFront(lines, "sig", strconv.Quote(sig))
			lines = setFront(lines, "verified", today())
			lines = fillSection(lines, "## "+a.sec(s, "symptom"), splitText(symptom))
			lines = fillSection(lines, "## "+a.sec(s, "cause"), splitText(cause))
			lines = fillSection(lines, "## "+a.sec(s, "fix"), splitText(fix))
			if files != "" {
				var fl []string
				for _, f := range splitList([]string{files}) {
					fl = append(fl, "- "+f)
				}
				lines = fillSection(lines, "## "+a.sec(s, "files"), fl)
			}
			rel := freePath(s, "episodes/errors/"+slug(title)+".md")
			return a.withLock(s, func() error {
				n := newNote(s, rel, lines)
				if err := a.save(n); err != nil {
					return err
				}
				if fromLast && !a.dryRun {
					os.Remove(lastRunPath(proj))
				}
				a.ok(globalPrefix(global) + n.addr())
				a.suggestLinks(s, n.addr(), title+" "+cause)
				return nil
			})
		},
	}
	f := c.Flags()
	f.StringVar(&sig, "sig", "", "the error message (one line)")
	f.BoolVar(&fromLast, "from-last", false, "use the last failed pg run")
	f.StringVar(&title, "title", "", "note title (default: the signature)")
	f.StringVar(&symptom, "symptom", "", "what was observed (default: the signature / failing command)")
	f.StringVar(&cause, "cause", "", "root cause")
	f.StringVar(&fix, "fix", "", "what fixed it")
	f.StringVar(&files, "files", "", "comma separated files involved")
	f.StringVar(&tags, "tags", "", "comma separated tags")
	f.BoolVar(&force, "new", false, "record even if a matching error exists")
	f.BoolVar(&global, "global", false, "record in the global vault: errors that are not about this project (toolchain, OS, CI)")
	return c
}

// bestSigLine picks the most specific error line: one with a message after
// a colon, not a bare FAIL summary.
func bestSigLine(lines []string) string {
	best := lines[0]
	bestScore := -1
	for _, l := range lines {
		score := len(errsig.Words(errsig.Normalize(l)))
		if strings.Contains(l, ": ") {
			score += 3
		}
		if strings.HasPrefix(strings.TrimSpace(l), "---") {
			score -= 5
		}
		if score > bestScore {
			best, bestScore = l, score
		}
	}
	return best
}

func (a *app) errorFindCmd() *cobra.Command {
	var all bool
	c := &cobra.Command{
		Use:   "find <error text>|-",
		Short: "Find the recorded fix for an error message or command output",
		Long: `Look up an error in memory. Pass the error line as arguments, or pipe
whole command output with "-". Matches are exact (same normalised
signature), likely (most words of a recorded signature) or related
(full-text hit). Exit 1: memory has nothing on it.`,
		Example: `pg error find "database is locked (5)"
go test ./... 2>&1 | pg error find -`,
		Args: cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			text, err := a.inputText(args, true)
			if err != nil {
				return err
			}
			s, err := a.main()
			if err != nil {
				return err
			}
			ms := a.matchErrors(s, text, true)
			if !all {
				ms = ms[:min(len(ms), 3)]
			}
			if a.jsonOut {
				var out []map[string]any
				for _, m := range ms {
					addr, fix := a.fixText(s, m.f, 1200)
					out = append(out, map[string]any{"addr": addr, "match": m.level, "seen": m.f.Seen, "status": m.f.Status, "fix": fix, "line": m.line})
				}
				if out == nil {
					out = []map[string]any{}
				}
				return a.emitJSON(out)
			}
			if len(ms) == 0 {
				a.println("no results")
				return errNoResults
			}
			for _, m := range ms {
				addr, fix := a.fixText(s, m.f, 500)
				a.println(join(addr, meta("error", m.f.Status, fmt.Sprintf("seen:%d", max(m.f.Seen, 1)), m.level), textutil.FmtTokens(textutil.Tokens(fix))))
				if fix != "" {
					a.println("  " + fix)
				}
			}
			a.printf("next: pg read %s (cause, files) | pg error seen %s if it works\n", ms[0].f.Addr(), ms[0].f.Addr())
			return nil
		},
	}
	c.Flags().BoolVar(&all, "all", false, "show every match, not just the best three")
	return c
}

func (a *app) errorSeenCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "seen <addr>...",
		Short: "The recorded fix worked again: seen+1, verified today",
		Args:  cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			for _, arg := range args {
				s, t, err := a.resolve(arg)
				if err != nil {
					return err
				}
				err = a.withLock(s, func() error {
					n, err := a.load(s, t.File)
					if err != nil {
						return err
					}
					seen := max(t.File.Seen, 1) + 1
					n.lines = setFront(n.lines, "seen", strconv.Itoa(seen))
					n.lines = setFront(n.lines, "updated", today())
					n.lines = setFront(n.lines, "verified", today())
					if t.File.Status == "" || t.File.Status == "active" {
						n.lines = setFront(n.lines, "status", "resolved")
					}
					if err := a.save(n); err != nil {
						return err
					}
					a.ok(fmt.Sprintf("%s seen:%d", t.File.Addr(), seen))
					if seen >= 3 && !strings.Contains(strings.Join(n.lines, "\n"), "promoted::") {
						fixAddr, _ := a.fixText(s, t.File, 0)
						a.printf("next: seen %d times; make it a rule: pg promote %s\n", seen, shellQuote(fixAddr))
					}
					return nil
				})
				if err != nil {
					return err
				}
			}
			return nil
		},
	}
}

func (a *app) errorReopenCmd() *cobra.Command {
	var noteText string
	c := &cobra.Command{
		Use:     "reopen <addr> --note TEXT",
		Short:   "The recorded fix did not work: status active, note under Fix",
		Example: `pg error reopen jwt-exp-timezone --note "still fails in CI; clock.Fixed not used in handler tests"`,
		Args:    cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			if noteText == "" {
				return fail(ExitUsage, "--note is required: what happened this time")
			}
			s, t, err := a.resolve(args[0])
			if err != nil {
				return err
			}
			return a.withLock(s, func() error {
				n, err := a.load(s, t.File)
				if err != nil {
					return err
				}
				n.lines = setFront(n.lines, "status", "active")
				n.lines = setFront(n.lines, "updated", today())
				a.appendToSection(n, t.File, "fix")([]string{"- reopened " + today() + ": " + noteText})
				if err := a.save(n); err != nil {
					return err
				}
				a.ok(t.File.Addr() + " [active]")
				return nil
			})
		},
	}
	c.Flags().StringVar(&noteText, "note", "", "what happened this time")
	return c
}

// reTitlePrefix matches the tool / file / position prefix of an error
// line: "vet.exe: ", "cart/broken.go:3:28: ", "C:\x\y.go:12: ".
var reTitlePrefix = regexp.MustCompile(`^(?:[\w.\-]+\.(?:exe|go|js|mjs|ts|tsx|py|rs|java|kt|c|cc|cpp|h|rb|cs|php)|[A-Za-z]:[\\/][^\s:]+|[\w.\-]*[/\\][\w./\\\-]+)(?::\d+)*:\s+`)

// errorTitle makes a readable title from an error line.
func errorTitle(sig string) string {
	t := strings.TrimSpace(sig)
	for i := 0; i < 4; i++ {
		u := reTitlePrefix.ReplaceAllString(t, "")
		if u == t || u == "" {
			break
		}
		t = u
	}
	return textutil.OneLine(t, 70)
}

func globalPrefix(global bool) string {
	if global {
		return "~"
	}
	return ""
}

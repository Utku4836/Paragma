package cli

import (
	"bufio"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/Utku-Private/Paragma/internal/errsig"
	"github.com/Utku-Private/Paragma/internal/index"
	"github.com/Utku-Private/Paragma/internal/textutil"
	"github.com/spf13/cobra"
)

// ---- misses: lookups that found nothing ----

const missLog = "misses.log"

// recordMiss appends a lookup that found nothing to .paragma/misses.log
// (local, git-ignored). pg misses turns it into evidence of what memory
// lacks, or of where word search falls short.
func recordMiss(s *store, kind, query string) {
	if s == nil || strings.TrimSpace(query) == "" {
		return
	}
	p := filepath.Join(s.v.Root, missLog)
	if st, err := os.Stat(p); err == nil && st.Size() > 256*1024 {
		// keep the newer half
		if b, err := os.ReadFile(p); err == nil {
			b = b[len(b)/2:]
			if i := strings.IndexByte(string(b), '\n'); i >= 0 {
				b = b[i+1:]
			}
			os.WriteFile(p, b, 0o644)
		}
	}
	f, err := os.OpenFile(p, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o644)
	if err != nil {
		return
	}
	defer f.Close()
	fmt.Fprintf(f, "%s\t%s\t%s\n", now().Format("2006-01-02 15:04"), kind, textutil.OneLine(query, 300))
}

// searchHint tells the agent how to try again after an empty search.
func (a *app) searchHint(s *store, q string) {
	if !s.v.Cfg.Hints {
		return
	}
	var tips []string
	if len(strings.Fields(q)) >= 3 {
		tips = append(tips, "fewer words")
	}
	tips = append(tips, "synonyms or the other language your notes use")
	if len(errsig.Lines(q, 1)) > 0 {
		tips = append(tips, "for an error message: pg error find")
	}
	line := "try: " + strings.Join(tips, "; ")
	if topics := a.topTags(s, 8); len(topics) > 0 {
		line += "; topics: " + strings.Join(topics, ", ")
	}
	a.println(line)
}

func (a *app) missesCmd() *cobra.Command {
	var limit int
	var since string
	c := &cobra.Command{
		Use:   "misses",
		Short: "Lookups that found nothing (search, error find, context), most frequent first",
		Long: `Show what agents looked for and did not find, grouped by the (folded)
query with how often and when last. Repeated misses mean memory lacks a
note, or that the note uses other words: add it, or add --keywords to it.`,
		Example: "pg misses\npg misses --since 7d",
		Args:    cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			cutoff := ""
			if since != "" {
				if cutoff, err = parseDate(since); err != nil {
					return err
				}
			}
			f, err := os.Open(filepath.Join(s.v.Root, missLog))
			if err != nil {
				a.println("no results")
				return errNoResults
			}
			defer f.Close()
			type agg struct {
				q, kind, last string
				n             int
			}
			by := map[string]*agg{}
			sc := bufio.NewScanner(f)
			sc.Buffer(make([]byte, 64*1024), 1024*1024)
			for sc.Scan() {
				parts := strings.SplitN(sc.Text(), "\t", 3)
				if len(parts) != 3 || (cutoff != "" && parts[0][:min(10, len(parts[0]))] < cutoff) {
					continue
				}
				k := parts[1] + "\x00" + textutil.Fold(strings.Join(strings.Fields(parts[2]), " "))
				x := by[k]
				if x == nil {
					x = &agg{q: parts[2], kind: parts[1]}
					by[k] = x
				}
				x.n++
				x.last = parts[0]
			}
			var xs []*agg
			for _, x := range by {
				xs = append(xs, x)
			}
			if len(xs) == 0 {
				a.println("no results")
				return errNoResults
			}
			sort.Slice(xs, func(i, j int) bool {
				if xs[i].n != xs[j].n {
					return xs[i].n > xs[j].n
				}
				return xs[i].last > xs[j].last
			})
			for _, x := range xs[:min(len(xs), limit)] {
				a.println(join(fmt.Sprintf("%dx", x.n), x.kind, strconv.Quote(textutil.OneLine(x.q, 120)), "last "+x.last))
			}
			return nil
		},
	}
	c.Flags().IntVar(&limit, "limit", 20, "max lines")
	c.Flags().StringVar(&since, "since", "", "only misses since: 7d, 2026-10-01")
	return c
}

// ---- keywords ----

// keywordList parses "a, b; c" into clean keywords.
func keywordList(s string) []string {
	var out []string
	seen := map[string]bool{}
	for _, k := range strings.FieldsFunc(s, func(r rune) bool { return r == ',' || r == ';' }) {
		k = strings.TrimSpace(k)
		if k != "" && !seen[strings.ToLower(k)] {
			seen[strings.ToLower(k)] = true
			out = append(out, k)
		}
	}
	return out
}

func yamlList(xs []string) string {
	q := make([]string, len(xs))
	for i, x := range xs {
		q[i] = yamlString(x)
	}
	return "[" + strings.Join(q, ", ") + "]"
}

// withKeywords sets the keywords front matter of a new note's lines.
func withKeywords(lines []string, kw string) []string {
	if ks := keywordList(kw); len(ks) > 0 {
		return setFront(lines, "keywords", yamlList(ks))
	}
	return lines
}

func (a *app) keywordsCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "keywords <addr> <keyword>[, keyword...]",
		Short: "Add search keywords to a note: synonyms, the other language, words a future search will use",
		Long: `Add keywords to a note's front matter. They are searched like the
note's text, so a note written as "database is locked" can also be found
by "kilitlenme" or "sqlite busy". Use it after a search missed a note that
existed, or when recording something under one name that others will
look for under another.`,
		Example: `pg keywords sqlite-database-locked "kilitlenme, sqlite busy, lock contention"`,
		Args:    cobra.MinimumNArgs(2),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, t, err := a.resolve(args[0])
			if err != nil {
				return err
			}
			add := keywordList(strings.Join(args[1:], ","))
			return a.withLock(s, func() error {
				n, err := a.load(s, t.File)
				if err != nil {
					return err
				}
				var have []string
				for _, l := range n.lines {
					if strings.HasPrefix(l, "keywords:") {
						have = keywordList(strings.Trim(strings.TrimSpace(strings.TrimPrefix(l, "keywords:")), "[]"))
						break
					}
				}
				all := keywordList(strings.Join(append(have, add...), ","))
				for i, k := range all {
					all[i] = strings.Trim(k, `"'`)
				}
				n.lines = setFront(n.lines, "keywords", yamlList(all))
				if err := a.save(n); err != nil {
					return err
				}
				a.ok(t.File.Addr() + " keywords: " + strings.Join(all, ", "))
				return nil
			})
		},
	}
}

// ---- dupes ----

var dupeStop = map[string]bool{"that": true, "this": true, "with": true, "from": true, "when": true, "then": true, "into": true, "have": true, "were": true, "after": true, "before": true, "only": true, "also": true, "file": true, "files": true, "symptom": true, "cause": true, "root": true, "fix": true, "decision": true, "context": true, "alternatives": true, "consequences": true}

func wordSet(text string) map[string]bool {
	set := map[string]bool{}
	for _, w := range strings.FieldsFunc(textutil.Fold(text), func(r rune) bool {
		return !(r >= 'a' && r <= 'z' || r >= '0' && r <= '9' || r == '_' || r > 127)
	}) {
		if len(w) >= 4 && !dupeStop[w] {
			set[w] = true
		}
	}
	return set
}

func jaccard(a, b map[string]bool) float64 {
	if len(a) == 0 || len(b) == 0 {
		return 0
	}
	in := 0
	for w := range a {
		if b[w] {
			in++
		}
	}
	return float64(in) / float64(len(a)+len(b)-in)
}

func (a *app) dupesCmd() *cobra.Command {
	var types []string
	var minSim float64
	var limit int
	c := &cobra.Command{
		Use:   "dupes",
		Short: "Notes that say nearly the same thing: candidates to merge or supersede",
		Long: `Compare notes of the same type by their words (and errors also by their
normalised signature) and list the most similar pairs. Merge them, or keep
the better one with: pg supersede <older> <newer>.`,
		Example: "pg dupes\npg dupes --type error --min 0.5",
		Args:    cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			want := map[string]bool{}
			for _, t := range splitList(types) {
				want[t] = true
			}
			sigs, _ := s.db.ErrorSigs()
			sigWords := map[int64][]string{}
			sigHash := map[int64]map[string]bool{}
			for _, sg := range sigs {
				sigWords[sg.FileID] = append(sigWords[sg.FileID], errsig.Words(sg.Norm)...)
				if sigHash[sg.FileID] == nil {
					sigHash[sg.FileID] = map[string]bool{}
				}
				sigHash[sg.FileID][sg.Hash] = true
			}
			sameSig := func(x, y int64) bool {
				for h := range sigHash[x] {
					if sigHash[y][h] {
						return true
					}
				}
				return false
			}
			byType := map[string][]*index.File{}
			for _, f := range s.res.Files() {
				if f.Inactive() || f.Type == "journal" || f.Type == "session" || f.Type == "branch" || (len(want) > 0 && !want[f.Type]) {
					continue
				}
				byType[f.Type] = append(byType[f.Type], f)
			}
			type pair struct {
				x, y *index.File
				sim  float64
				why  string
			}
			var pairs []pair
			for _, fs := range byType {
				sets := make([]map[string]bool, len(fs))
				for i, f := range fs {
					lines, err := fileLines(s, f)
					if err != nil {
						continue
					}
					sets[i] = wordSet(f.Title + " " + strings.Join(sliceLines(lines, f.BodyLine, f.NLines), " "))
				}
				for i := 0; i < len(fs); i++ {
					for j := i + 1; j < len(fs); j++ {
						if sets[i] == nil || sets[j] == nil {
							continue
						}
						if fs[i].Type == "error" && sameSig(fs[i].ID, fs[j].ID) {
							pairs = append(pairs, pair{fs[i], fs[j], 1, "same error signature"})
							continue
						}
						// sets of very different size cannot reach the threshold
						if r := float64(min(len(sets[i]), len(sets[j]))) / float64(max(len(sets[i]), len(sets[j]))); r < minSim {
							continue
						}
						sim, why := jaccard(sets[i], sets[j]), "text"
						if fs[i].Type == "error" {
							wi, wj := sigWords[fs[i].ID], sigWords[fs[j].ID]
							if len(wi) >= 3 && len(wj) >= 3 {
								if o := min(errsig.Overlap(wi, wj), errsig.Overlap(wj, wi)); o >= 0.8 && o > sim {
									sim, why = o, "same error signature"
								}
							}
						}
						if sim >= minSim {
							pairs = append(pairs, pair{fs[i], fs[j], sim, why})
						}
					}
				}
			}
			if len(pairs) == 0 {
				a.println("no results")
				return errNoResults
			}
			sort.Slice(pairs, func(i, j int) bool { return pairs[i].sim > pairs[j].sim })
			for _, p := range pairs[:min(len(pairs), limit)] {
				older, newer := p.x, p.y
				if older.Date() > newer.Date() {
					older, newer = newer, older
				}
				a.println(join(older.Addr()+" ~ "+newer.Addr(), fmt.Sprintf("%.0f%% %s", p.sim*100, p.why), "["+p.x.Type+"]"))
			}
			a.println("next: merge them, or keep the newer: pg supersede <older> <newer>")
			return nil
		},
	}
	c.Flags().StringSliceVar(&types, "type", nil, "only these note types")
	c.Flags().Float64Var(&minSim, "min", 0.6, "similarity threshold 0..1")
	c.Flags().IntVar(&limit, "limit", 20, "max pairs")
	return c
}

// ---- gc ----

func (a *app) gcCmd() *cobra.Command {
	var older string
	var apply bool
	c := &cobra.Command{
		Use:   "gc [--older 180d] [--apply]",
		Short: "Fold old archived sessions into one journal line each and delete them",
		Long: `Archived sessions older than --older become one line in the journal of
the day they ended (goal, progress, last handoff) and their files are
deleted. Without --apply it only lists what it would do.`,
		Example: "pg gc\npg gc --older 90d --apply",
		Args:    cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			cutoff, err := parseDate(older)
			if err != nil {
				return err
			}
			type cand struct {
				f    *index.File
				when time.Time
				line string
			}
			var cs []cand
			for _, f := range s.res.Files() {
				if f.Type != "session" || !strings.HasPrefix(f.Path, "archive/") {
					continue
				}
				d := f.Date()
				if n, err := a.load(s, f); err == nil {
					for _, l := range n.lines {
						if strings.HasPrefix(l, "closed:") {
							d = strings.TrimSpace(strings.Trim(strings.TrimPrefix(l, "closed:"), `"' `))
						}
					}
				}
				if len(d) < 10 || d[:10] >= cutoff {
					continue
				}
				when, err := time.ParseInLocation("2006-01-02", d[:10], time.Local)
				if err != nil {
					continue
				}
				ts, _ := s.db.Tasks(f.ID)
				tc := index.CountTasks(ts, 1, 1<<30)
				goal := strings.Join(a.sectionBody(s, f, "goal"), " ")
				hand := strings.Join(a.sectionBody(s, f, "handoff"), " ")
				line := fmt.Sprintf("session %q (gc): %s; %d/%d tasks done", f.Title, textutil.OneLine(goal, 160), tc.Done, tc.Total)
				if hand != "" {
					line += "; last handoff: " + textutil.OneLine(hand, 200)
				}
				cs = append(cs, cand{f, when.Add(23*time.Hour + 59*time.Minute), line})
			}
			if len(cs) == 0 {
				a.println("no results")
				return errNoResults
			}
			if !apply || a.dryRun {
				for _, c := range cs {
					a.println(join(c.f.Addr(), "ended "+c.when.Format("2006-01-02"), "→ journal, file deleted"))
				}
				a.printf("%d sessions; run with --apply to do it\n", len(cs))
				return nil
			}
			return a.withLock(s, func() error {
				for _, c := range cs {
					if _, err := a.journalAppendAt(s, c.when, c.line); err != nil {
						return err
					}
					if err := os.Remove(filepath.Join(s.v.Root, filepath.FromSlash(c.f.Path))); err != nil {
						return fail(ExitInternal, "%v", err)
					}
					if err := a.reindex(s, c.f.Path); err != nil {
						return err
					}
				}
				a.ok(fmt.Sprintf("folded %d sessions into the journal", len(cs)))
				return nil
			})
		},
	}
	c.Flags().StringVar(&older, "older", "180d", "only sessions that ended before this: 90d, 6m, 2026-01-01")
	c.Flags().BoolVar(&apply, "apply", false, "really fold and delete")
	return c
}

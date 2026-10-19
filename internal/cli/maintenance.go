package cli

import (
	"bufio"
	"fmt"
	"os"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/Utku4836/Paragma/internal/errsig"
	"github.com/Utku4836/Paragma/internal/index"
	"github.com/Utku4836/Paragma/internal/textutil"
	"github.com/spf13/cobra"
)

// Keeping memory healthy: verify, stale, dupes, misses.

func (a *app) verifyCmd() *cobra.Command {
	return &cobra.Command{
		Use:     "verify <addr>...",
		Short:   "You checked it and it is still true: set verified to today",
		Example: "pg verify stack architecture",
		Args:    cobra.MinimumNArgs(1),
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
					n.lines = setFront(n.lines, "verified", today())
					if err := a.save(n); err != nil {
						return err
					}
					a.ok(t.File.Addr() + " verified " + today())
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

func (a *app) staleCmd() *cobra.Command {
	var days int
	c := &cobra.Command{
		Use:     "stale",
		Short:   "Semantic notes not verified for a while (default: stale_days from config)",
		Example: "pg stale\npg stale --days 30",
		Args:    cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			if days <= 0 {
				days = s.v.Cfg.StaleDays
			}
			type row struct {
				f   *index.File
				age int
			}
			var rows []row
			for _, f := range s.res.Files() {
				if !f.Stale(days) {
					continue
				}
				d := f.Verified
				if d == "" {
					d = f.Updated
				}
				age := 0
				if t, err := time.Parse("2006-01-02", d[:min(len(d), 10)]); err == nil {
					age = int(time.Since(t).Hours() / 24)
				}
				rows = append(rows, row{f, age})
			}
			if len(rows) == 0 {
				a.println("no results")
				return errNoResults
			}
			sort.Slice(rows, func(i, j int) bool { return rows[i].age > rows[j].age })
			for _, r := range rows {
				a.println(join(r.f.Addr(), fmt.Sprintf("verified %s (%dd ago)", r.f.Date(), r.age), textutil.FmtTokens(r.f.Tokens)))
			}
			a.println("next: check it, then pg verify <addr> | pg mark <addr> --as stale")
			return nil
		},
	}
	c.Flags().IntVar(&days, "days", 0, "older than this many days")
	return c
}

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
				if f.Inactive() || f.Type == "journal" || f.Type == "project" || (len(want) > 0 && !want[f.Type]) {
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

const missLog = "misses.log"

// recordMiss appends a lookup that found nothing to the space's .pg/misses.log
// (local, git-ignored). pg misses turns it into evidence of what memory
// lacks, or of where word search falls short.
func recordMiss(s *store, kind, query string) {
	if s == nil || strings.TrimSpace(query) == "" {
		return
	}
	p := s.v.Meta(missLog)
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
			f, err := os.Open(s.v.Meta(missLog))
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

package cli

import (
	"fmt"
	"regexp"
	"sort"
	"strings"

	"github.com/Utku4836/Paragma/internal/addr"
	"github.com/Utku4836/Paragma/internal/index"
	"github.com/Utku4836/Paragma/internal/md"
	"github.com/Utku4836/Paragma/internal/space"
	"github.com/Utku4836/Paragma/internal/textutil"
	"github.com/spf13/cobra"
)

// The context pack is built so that the agent does not need to ask again:
// each note's parts come together, small notes come whole, the project's
// rules come along (once per conversation), and a last line says what else
// memory holds on the topic and that there is nothing more.

// wholeNoteTokens: notes up to this size are printed whole.
const wholeNoteTokens = 300

// allRulesTokens: a project whose rules fit in this many tokens gets all of
// them printed; a bigger one gets the matching rules and an index of the rest.
const allRulesTokens = 1200

type ctxItem struct {
	f     *index.File
	addr  string
	from  int // line range to print
	to    int
	score float64
	why   string
}

// keyRange is the part of a note that carries its point: an error's fix, a
// decision's decision, a project's goal, else the note's opening text.
func (a *app) keyRange(s *store, f *index.File) (string, int, int) {
	key := map[string]string{"error": "fix", "decision": "decision", "project": "goal"}[f.Type]
	if key != "" {
		if sec := findSection(s, f, key); sec != nil {
			return f.Addr() + "#" + sec.Path, sec.Line, sec.BodyEnd
		}
	}
	secs, _ := s.res.Sections(f)
	for _, sec := range secs {
		if sec.Level == 0 || sec.IsTitle {
			if sec.BodyEnd > sec.Line || sec.Level == 0 {
				return f.Addr(), sec.Line, sec.BodyEnd
			}
		}
	}
	if len(secs) > 1 {
		return f.Addr() + "#" + secs[1].Path, secs[1].Line, secs[1].BodyEnd
	}
	return f.Addr(), f.BodyLine, f.NLines
}

func (a *app) contextCmd() *cobra.Command {
	var budget int
	c := &cobra.Command{
		Use:   "context <topic|addr|code path>",
		Short: "One call for everything memory knows about a topic: best sections plus linked fixes, decisions and rules",
		Long: `Collect what memory knows about a topic in one output, within a token
budget: the best search hits, the notes they link to (fixes, decisions,
superseded and promoted notes), and for a code path the rules and errors
about that file. Use it instead of several search + read calls. Items that
did not fit are listed at the end with their cost.`,
		Example: `pg context "refund window"
pg context payments/refund.go --budget 400
pg context sqlite-database-locked`,
		Args: cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			q := strings.Join(args, " ")
			s, err := a.main()
			if err != nil {
				return err
			}
			if budget <= 0 {
				budget = a.budgetOr(1000)
			}
			items := a.gatherContext(s, q)
			if len(items) == 0 {
				recordMiss(s, "context", q)
				a.println("no results")
				a.searchHint(s, q)
				return errNoResults
			}
			return a.printContext(s, q, items, budget)
		},
	}
	c.Flags().IntVar(&budget, "budget", 0, "token budget for the topic part of the pack (default 1000)")
	return c
}

// gatherContext ranks the sections worth reading for q.
func (a *app) gatherContext(s *store, q string) []ctxItem {
	cand := map[string]*ctxItem{}
	add := func(it ctxItem) {
		if cur, ok := cand[it.addr]; ok {
			if it.score > cur.score {
				cur.score, cur.why = it.score, it.why
			} else {
				cur.score += it.score * 0.2 // reached twice: a little more relevant
			}
			return
		}
		cand[it.addr] = &it
	}
	seedFiles := map[int64]float64{}

	// 1. an exact address
	if p, err := addr.Parse(q); err == nil && !strings.Contains(q, " ") {
		if t, err := s.res.Resolve(p); err == nil {
			ad, from, to := t.Addr(), t.From, t.To
			if t.Kind == addr.KindFile {
				ad, from, to = a.keyRange(s, t.File)
			}
			add(ctxItem{f: t.File, addr: ad, from: from, to: to, score: 10, why: "asked"})
			seedFiles[t.File.ID] = 10
		}
	}
	// 2. a code path: notes about that file
	if len(md.CodeRefs(" "+q+" ")) > 0 && !strings.Contains(strings.TrimSpace(q), " ") {
		for _, h := range a.relatedNotes(s, []string{a.codePath(q)}, 1.5, false) {
			from, to := h.line, h.line
			if t, err := s.res.Resolve(mustParse(h.addr)); err == nil {
				from, to = t.From, t.To
			}
			add(ctxItem{f: h.f, addr: h.addr, from: from, to: to, score: 3 + h.score, why: h.why})
			seedFiles[h.f.ID] = max(seedFiles[h.f.ID], 3)
		}
	}
	// 3. full-text hits
	hits, _ := s.db.Search(index.SearchOpts{Query: q, Max: 400})
	if len(hits) > 0 {
		// one note per fix: the same error recorded in many packages must
		// not crowd out everything else
		seenFix := map[string]bool{}
		kept := hits[:0]
		for _, h := range hits {
			if k := a.fixKey(s, h.File); k != "" {
				if seenFix[k] {
					continue
				}
				seenFix[k] = true
			}
			kept = append(kept, h)
		}
		hits = kept
		// what binds a change ranks above what merely mentions the topic: in
		// a big memory dozens of error notes say "refund" in passing, the
		// one rule that governs refunds must not drown among them
		for i := range hits {
			w := contextTypeWeight[hits[i].File.Type]
			if w == 0 {
				w = 1
			}
			hits[i].Score *= w
		}
		sort.SliceStable(hits, func(i, j int) bool { return hits[i].Score > hits[j].Score })
		hits = hits[:min(len(hits), 10)]
		s.db.Hydrate(hits)
		top := hits[0].Score
		for _, h := range hits {
			if h.Sec == nil {
				continue
			}
			sc := 5 * h.Score / top
			seedFiles[h.File.ID] = max(seedFiles[h.File.ID], sc)
			// a section of rules: take the individual rules that match,
			// not the whole list
			if strings.Contains(" "+h.Sec.Kinds+" ", " rule ") {
				if rules := a.matchingBlocks(s, h.File, h.Sec.Line, h.Sec.BodyEnd, q); len(rules) > 0 {
					for i, r := range rules {
						add(ctxItem{f: h.File, addr: r.addr, from: r.from, to: r.to, score: sc * (1 - 0.05*float64(i)), why: "rule matches"})
					}
					continue
				}
			}
			ad := hitAddr(h)
			from, to := h.Sec.Line, h.Sec.BodyEnd
			if h.Sec.IsTitle || h.Sec.Path == "" {
				ad, from, to = a.keyRange(s, h.File)
			}
			add(ctxItem{f: h.File, addr: ad, from: from, to: to, score: sc, why: "matches"})
		}
	}
	// 4. one hop along links from the strongest seeds
	type seed struct {
		id int64
		sc float64
	}
	var seeds []seed
	for id, sc := range seedFiles {
		seeds = append(seeds, seed{id, sc})
	}
	sort.Slice(seeds, func(i, j int) bool { return seeds[i].sc > seeds[j].sc })
	for _, sd := range seeds[:min(len(seeds), 4)] {
		f, err := s.db.FileByID(sd.id)
		if err != nil {
			continue
		}
		for _, e := range a.edges(s, f) {
			if e.f.ID == f.ID || e.f.Type == "journal" {
				continue
			}
			w := 0.35
			if e.kind != "wiki" && e.kind != "md" && e.kind != "embed" {
				w = 0.55 // typed links (fixes, supersedes, promoted, from) carry more
			}
			ad, from, to := e.addr, e.from, e.to
			if e.whole {
				ad, from, to = a.keyRange(s, e.f)
			}
			add(ctxItem{f: e.f, addr: ad, from: from, to: to, score: sd.sc * w, why: e.kind + " " + e.dir + " " + f.Addr()})
		}
	}
	var out []ctxItem
	for _, it := range cand {
		if it.f.Inactive() {
			it.score *= 0.3
		}
		// a whole section of rules, however it was reached: keep only the
		// rules that match the topic
		if it.f.Type == "rule" && strings.Contains(it.addr, "#") {
			if rules := a.matchingBlocks(s, it.f, it.from, it.to, q); len(rules) > 0 {
				for i, r := range rules {
					if _, dup := cand[r.addr]; !dup {
						out = append(out, ctxItem{f: it.f, addr: r.addr, from: r.from, to: r.to, score: it.score * (1 - 0.05*float64(i)), why: it.why})
					}
				}
				continue
			}
		}
		out = append(out, *it)
	}
	sort.SliceStable(out, func(i, j int) bool {
		if out[i].score != out[j].score {
			return out[i].score > out[j].score
		}
		return out[i].addr < out[j].addr
	})
	return out
}

type blockRange struct {
	addr     string
	from, to int
}

// matchingBlocks returns the ^id blocks in [from,to] of f whose text shares
// a word with q, best overlap first.
func (a *app) matchingBlocks(s *store, f *index.File, from, to int, q string) []blockRange {
	rows, err := s.db.Query(`SELECT block_id, line, line_end FROM blocks WHERE file_id=? AND line>=? AND line<=? ORDER BY line`, f.ID, from, to)
	if err != nil {
		return nil
	}
	type cand struct {
		blockRange
		hits int
	}
	var cs []cand
	var bs []blockRange
	for rows.Next() {
		var b blockRange
		var id string
		rows.Scan(&id, &b.from, &b.to)
		b.addr = f.Addr() + "^" + id
		bs = append(bs, b)
	}
	rows.Close()
	lines, err := fileLines(s, f)
	if err != nil {
		return nil
	}
	words := strings.Fields(textutil.Fold(q))
	for _, b := range bs {
		text := textutil.Fold(strings.Join(sliceLines(lines, b.from, b.to), " "))
		n := 0
		for _, w := range words {
			if len(w) >= 3 && strings.Contains(text, w) {
				n++
			}
		}
		if n > 0 {
			cs = append(cs, cand{b, n})
		}
	}
	sort.SliceStable(cs, func(i, j int) bool { return cs[i].hits > cs[j].hits })
	var out []blockRange
	for _, c := range cs[:min(len(cs), 4)] {
		out = append(out, c.blockRange)
	}
	return out
}

func mustParse(s string) addr.Addr {
	p, _ := addr.Parse(s)
	return p
}

// cleanLine strips callout markers, quote marks and trailing block ids.
func cleanLine(l string) string {
	l = strings.TrimSpace(strings.TrimPrefix(strings.TrimSpace(l), ">"))
	for _, k := range []string{"[!rule]", "[!decision]", "[!warning]", "[!pin]", "[!note]", "[!question]", "[!stale]"} {
		l = strings.TrimSpace(strings.TrimPrefix(l, k))
	}
	return strings.TrimSpace(reTrailingID.ReplaceAllString(l, ""))
}

var reTrailingID = regexp.MustCompile(`\s\^[A-Za-z0-9][\w-]*\s*$`)

// sectionText renders lines [from,to] of a note: the heading as a "Name:"
// prefix, blank lines dropped.
func sectionText(lines []string, from, to int, withHeading bool) []string {
	body := sliceLines(lines, from, to)
	head := ""
	if len(body) > 0 && isHeadingLine(body[0]) {
		head = strings.TrimSpace(strings.TrimLeft(body[0], "#"))
		body = body[1:]
	}
	var out []string
	for _, l := range body {
		if isBlank(l) || strings.HasPrefix(strings.TrimSpace(l), "```") {
			continue
		}
		if isHeadingLine(l) {
			out = append(out, strings.TrimSpace(strings.TrimLeft(l, "#"))+":")
			continue
		}
		if c := cleanLine(l); c != "" {
			out = append(out, c)
		}
	}
	if withHeading && head != "" && len(out) > 0 {
		out[0] = head + ": " + strings.TrimPrefix(out[0], "- ")
	}
	return out
}

// wholeNote renders a note's body (title line and front matter left out).
func (a *app) wholeNote(s *store, f *index.File, lines []string) []string {
	secs, err := s.res.Sections(f)
	if err != nil || len(secs) == 0 {
		return sectionText(lines, f.BodyLine, f.NLines, false)
	}
	var out []string
	for _, sec := range secs {
		if sec.IsTitle {
			out = append(out, sectionText(lines, sec.Line, sec.BodyEnd, false)...)
			continue
		}
		out = append(out, sectionText(lines, sec.Line, sec.BodyEnd, true)...)
	}
	return out
}

type ctxGroup struct {
	f     *index.File
	items []ctxItem
	score float64
}

func (a *app) printContext(s *store, q string, items []ctxItem, budget int) error {
	if a.jsonOut {
		var out []map[string]any
		for _, it := range items {
			lines, _ := fileLines(s, it.f)
			out = append(out, map[string]any{"addr": it.addr, "type": it.f.Type, "score": it.score, "why": it.why, "text": strings.Join(sliceLines(lines, it.from, it.to), "\n")})
		}
		return a.emitJSON(out)
	}
	// group by note, best note first
	var groups []*ctxGroup
	byFile := map[int64]*ctxGroup{}
	for _, it := range items {
		g := byFile[it.f.ID]
		if g == nil {
			g = &ctxGroup{f: it.f, score: it.score}
			byFile[it.f.ID] = g
			groups = append(groups, g)
		}
		g.items = append(g.items, it)
	}
	// decisions and rules bind more than notes that merely mention the topic
	rank := func(g *ctxGroup) float64 {
		w := contextTypeWeight[g.f.Type]
		if w == 0 {
			w = 1
		}
		if g.f.Inactive() {
			w *= 0.4
		}
		if s.project != "" && space.ProjectOf(g.f.Path) == s.project {
			w *= 1.1
		}
		return g.score * w
	}
	sort.SliceStable(groups, func(i, j int) bool { return rank(groups[i]) > rank(groups[j]) })

	proj := s.project
	if proj == "" && len(groups) > 0 {
		proj = space.ProjectOf(groups[0].f.Path)
	}
	head := fmt.Sprintf("context: %q", q)
	if proj != "" {
		head += "  (project " + proj + ")"
	}
	a.println(head)

	// notes that repeat another one's fix (the same error in several
	// packages) are folded into the first: one note and a list
	sameAs := map[int64][]string{} // first note -> addresses with the same fix
	dup := map[int64]bool{}
	firstByFix := map[string]int64{}
	for _, g := range groups {
		if k := a.fixKey(s, g.f); k != "" {
			if first, ok := firstByFix[k]; ok {
				dup[g.f.ID] = true
				sameAs[first] = append(sameAs[first], g.f.Addr())
			} else {
				firstByFix[k] = g.f.ID
			}
		}
	}

	used := 0
	shownRules := map[string]bool{} // rule addresses already printed
	var notShown []string
	counts := map[string]int{}
	before := loadShown(s)
	earlier := 0
	for _, g := range groups {
		if dup[g.f.ID] {
			continue
		}
		if key := "ctx:" + g.f.Addr(); !a.again && before.recent(key, shownWindow) {
			a.printf("## %s  (shown earlier in this conversation; pg read %s)\n", g.f.Addr(), g.f.Addr())
			earlier++
			continue
		} else {
			before.mark(key)
		}
		lines, err := fileLines(s, g.f)
		if err != nil {
			continue
		}
		var text []string
		label := g.f.Addr()
		if g.f.Type == "rule" {
			// the matching rules of a rules note, one per line
			sort.SliceStable(g.items, func(i, j int) bool { return g.items[i].from < g.items[j].from })
			for _, it := range g.items {
				if i := strings.Index(it.addr, "^"); i >= 0 && !shownRules[it.addr] {
					t := strings.Join(sectionText(lines, it.from, it.to, false), " ")
					text = append(text, "- "+it.addr+" "+t)
					shownRules[it.addr] = true
				} else if i < 0 {
					text = append(text, sectionText(lines, it.from, it.to, true)...)
				}
			}
			label += "  [rules matching]"
		} else if g.f.Tokens <= wholeNoteTokens {
			text = a.wholeNote(s, g.f, lines)
			label += "  (whole note)"
		} else {
			sort.SliceStable(g.items, func(i, j int) bool { return g.items[i].from < g.items[j].from })
			last := 0
			for _, it := range g.items {
				if it.from <= last { // inside a range already printed
					continue
				}
				text = append(text, sectionText(lines, it.from, it.to, true)...)
				last = it.to
			}
			label += fmt.Sprintf("  (parts; whole note %s: pg read %s)", textutil.FmtTokens(g.f.Tokens), g.f.Addr())
		}
		if len(text) == 0 {
			continue
		}
		cost := textutil.Tokens(strings.Join(text, "\n")) + 12
		if used+cost > budget && used > 0 {
			notShown = append(notShown, fmt.Sprintf("%s %s", g.f.Addr(), textutil.FmtTokens(cost)))
			continue
		}
		used += cost
		typ := g.f.Type
		if typ == "note" {
			typ = ""
		}
		seen := ""
		if g.f.Seen > 0 {
			seen = fmt.Sprintf("seen:%d", g.f.Seen)
		}
		date := ""
		if g.f.Type == "decision" || g.f.Type == "error" {
			date = g.f.Date()
		}
		if g.f.Type == "rule" {
			typ = ""
			counts["rules"] += len(text)
		} else {
			counts[typ+"s"]++
		}
		a.println("## " + join(label, meta(typ, statusWord(g.f), seen, date)))
		for _, l := range text {
			a.println(l)
		}
		if same := sameAs[g.f.ID]; len(same) > 0 {
			a.printf("(the same fix is recorded in %d more notes: %s)\n", len(same), strings.Join(same[:min(len(same), 5)], ", ")+map[bool]string{true: ", ...", false: ""}[len(same) > 5])
		}
	}

	// the project's rules, which apply to every change
	printed := map[int64]bool{a.ctxSkip: true} // a note the caller printed itself
	for _, g := range groups {
		printed[g.f.ID] = true
	}
	a.saveShown(s, before) // before printProjectRules, which keeps its own marks
	if id := a.printProjectRules(s, proj, shownRules); id != 0 {
		printed[id] = true
	}

	// what else mentions the topic: each with its point, so that listing a
	// note does not send the agent to read it
	var other []string
	more, folded := 0, 0
	capped := false
	if hits, err := s.db.Search(index.SearchOpts{Query: q, Max: 400}); err == nil {
		capped = len(hits) >= 400
		var pick []index.Hit
		seenF := map[int64]bool{}
		for _, h := range hits {
			if printed[h.File.ID] || seenF[h.File.ID] || h.File.Type == "journal" || dup[h.File.ID] {
				continue
			}
			seenF[h.File.ID] = true
			if k := a.fixKey(s, h.File); k != "" {
				if first, ok := firstByFix[k]; ok {
					if printed[first] {
						folded++ // the same fix as a note shown above
					}
					continue
				}
				firstByFix[k] = h.File.ID
			}
			if len(pick) < 6 {
				pick = append(pick, h)
			} else {
				more++
			}
		}
		s.db.Hydrate(pick)
		for _, h := range pick {
			lines, err := fileLines(s, h.File)
			if err != nil {
				continue
			}
			from, to, ad, max := h.File.BodyLine, h.File.NLines, h.File.Addr(), 400
			if h.Sec != nil && !h.Sec.IsTitle && h.File.Tokens > 120 {
				from, to, ad, max = h.Sec.Line, h.Sec.BodyEnd, h.File.Addr()+"#"+h.Sec.Path, 400
			}
			text := textutil.OneLine(strings.Join(sectionText(lines, from, to, false), " "), max)
			if strings.HasPrefix(text, h.File.Title) {
				text = strings.TrimSpace(strings.TrimPrefix(text, h.File.Title))
			}
			other = append(other, fmt.Sprintf("- %s %s: %s", ad, textutil.FmtTokens(h.File.Tokens), text))
		}
	}
	if len(other) > 0 {
		a.printf("## also mentions %q, not shown above (their gist; read one only if it matters)\n", q)
		for _, o := range other {
			a.println(o)
		}
	}
	var parts []string
	for _, k := range []string{"rules", "decisions", "errors", "concepts", "projects", "s"} {
		if counts[k] > 0 {
			name := k
			if k == "s" {
				name = "notes"
			}
			parts = append(parts, fmt.Sprintf("%d %s", counts[k], name))
		}
	}
	line := "covered: " + strings.Join(parts, ", ")
	if earlier > 0 {
		if len(parts) > 0 {
			line += "; "
		}
		line += fmt.Sprintf("%d notes shown earlier in this conversation (--again prints them)", earlier)
	}
	if len(notShown) > 0 {
		line += "; over budget (pg read <addr>): " + strings.Join(notShown[:min(len(notShown), 6)], ", ")
	}
	if more > 0 {
		line += fmt.Sprintf("; %d more notes mention it in passing (pg search %s)", more, shellQuote(q))
	}
	if folded > 0 {
		line += fmt.Sprintf("; %d more notes repeat a fix shown above", folded)
	}
	switch {
	case capped:
		line += fmt.Sprintf(". Over 400 notes match %q: a narrower topic gives a sharper pack.", q)
	case len(notShown) == 0 && more == 0:
		line += ". This is everything memory has on it."
	default:
		line += ". Nothing else matches."
	}
	a.println(line)
	return nil
}

// printProjectRules prints the rules of project p that were not printed
// above: all of them when they are few, else an index of rule groups. Once
// per conversation (see shown); later packs print one line instead.
func (a *app) printProjectRules(s *store, p string, already map[string]bool) int64 {
	if p == "" {
		return 0
	}
	f, err := s.res.ResolveFile(p + "/rules")
	if err != nil || f.Path != p+"/rules.md" {
		return 0
	}
	rows, err := s.db.Query(`SELECT COALESCE(m.block_id,''), m.line, COALESCE(x.heading_path,'') FROM marks m LEFT JOIN sections x ON x.id = m.section_id WHERE m.file_id = ? AND m.kind = 'rule' ORDER BY m.line`, f.ID)
	if err != nil {
		return 0
	}
	type rule struct {
		addr, sec, text string
	}
	var rs []rule
	lines, _ := fileLines(s, f)
	for rows.Next() {
		var b, sec string
		var l int
		rows.Scan(&b, &l, &sec)
		ad := fmt.Sprintf("%s:%d", f.Addr(), l)
		if b != "" {
			ad = f.Addr() + "^" + b
		}
		if l < 1 || l > len(lines) || already[ad] {
			continue
		}
		if i := strings.LastIndex(sec, "/"); i >= 0 {
			sec = sec[i+1:]
		}
		rs = append(rs, rule{ad, sec, cleanLine(lines[l-1])})
	}
	rows.Close()
	if len(rs) == 0 {
		return 0
	}
	sh := loadShown(s)
	key := "rules:" + p
	if sh.recent(key, shownWindow) {
		a.printf("## %s: %d more project rules, printed earlier in this conversation (pg rule list)\n", f.Addr(), len(rs))
		return 0
	}
	total := 0
	for _, r := range rs {
		total += textutil.Tokens(r.text) + 4
	}
	if total <= allRulesTokens {
		a.printf("## %s  [all other project rules: they apply to every change]\n", f.Addr())
		cur := ""
		for _, r := range rs {
			if r.sec != cur {
				cur = r.sec
				a.println(cur + ":")
			}
			a.printf("- %s %s\n", r.addr[strings.LastIndex(r.addr, "^")+1:], r.text)
		}
	} else {
		// too many to print: say what exists, so nothing is a surprise
		type grp struct {
			n, tok int
		}
		gs := map[string]*grp{}
		var order []string
		for _, r := range rs {
			if gs[r.sec] == nil {
				gs[r.sec] = &grp{}
				order = append(order, r.sec)
			}
			gs[r.sec].n++
			gs[r.sec].tok += textutil.Tokens(r.text) + 4
		}
		var parts []string
		for _, sec := range order {
			parts = append(parts, fmt.Sprintf("%s (%d, %s)", sec, gs[sec].n, textutil.FmtTokens(gs[sec].tok)))
		}
		a.printf("## %s  [%d other project rules; read the groups that concern your change: pg read %s#<group>]\n", f.Addr(), len(rs), f.Addr())
		a.println(strings.Join(parts, ", "))
	}
	sh.mark(key)
	a.saveShown(s, sh)
	return f.ID
}

// contextTypeWeight scales search scores in a context pack by note type.
var contextTypeWeight = map[string]float64{
	"rule": 2.0, "decision": 1.7, "concept": 1.4, "note": 1.2, "project": 1.1,
	"error": 1.0, "journal": 0.5,
}

// fixKey identifies an error note by its fix (words only), so notes that
// record the same fix in different places can be folded together.
func (a *app) fixKey(s *store, f *index.File) string {
	if f.Type != "error" {
		return ""
	}
	fix := strings.Join(a.sectionBody(s, f, "fix"), " ")
	if len(fix) < 20 {
		return ""
	}
	return strings.Join(strings.Fields(textutil.Fold(fix)), " ")
}

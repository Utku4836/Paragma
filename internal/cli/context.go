package cli

import (
	"encoding/json"
	"fmt"
	"os"
	"sort"
	"strings"
	"time"

	"github.com/Utku-Private/Paragma/internal/index"
	"github.com/Utku-Private/Paragma/internal/space"
	"github.com/Utku-Private/Paragma/internal/textutil"
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

// ---- what was already shown in this conversation ----

// shown remembers what pg printed recently (rules in a context pack, rule
// reminders after pg run) so it is not repeated in the same conversation.
// pg prime, run at the start of a conversation, forgets everything.
type shown map[string]int64

const shownWindow = 30 * time.Minute

func loadShown(s *store) shown {
	m := shown{}
	if b, err := os.ReadFile(s.v.Meta("shown.json")); err == nil {
		json.Unmarshal(b, &m)
	}
	return m
}

func (m shown) recent(key string, window time.Duration) bool {
	t, ok := m[key]
	return ok && time.Since(time.Unix(t, 0)) < window
}

func (m shown) mark(key string) { m[key] = time.Now().Unix() }

func (a *app) saveShown(s *store, m shown) {
	if a.dryRun {
		return
	}
	cut := time.Now().Add(-24 * time.Hour).Unix()
	for k, t := range m {
		if t < cut {
			delete(m, k)
		}
	}
	if b, err := json.Marshal(m); err == nil {
		space.WriteFileAtomic(s.v.Meta("shown.json"), b)
	}
}

func (a *app) forgetShown(s *store) {
	if !a.dryRun {
		os.Remove(s.v.Meta("shown.json"))
	}
}

// ---- rendering ----

// cleanLine strips callout markers, quote marks and trailing block ids.
func cleanLine(l string) string {
	l = strings.TrimSpace(strings.TrimPrefix(strings.TrimSpace(l), ">"))
	for _, k := range []string{"[!rule]", "[!decision]", "[!warning]", "[!pin]", "[!note]", "[!question]", "[!stale]"} {
		l = strings.TrimSpace(strings.TrimPrefix(l, k))
	}
	return strings.TrimSpace(reTrailingID.ReplaceAllString(l, ""))
}

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
		w := map[string]float64{"rule": 1.3, "decision": 1.25, "error": 1.15, "concept": 1.1, "journal": 0.6}[g.f.Type]
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

	used := 0
	shownRules := map[string]bool{} // rule addresses already printed
	var notShown []string
	counts := map[string]int{}
	for _, g := range groups {
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
	}

	// the project's rules, which apply to every change
	printed := map[int64]bool{}
	for _, g := range groups {
		printed[g.f.ID] = true
	}
	if id := a.printProjectRules(s, proj, shownRules); id != 0 {
		printed[id] = true
	}

	// what else mentions the topic: each with its point, so that listing a
	// note does not send the agent to read it
	var other []string
	more := 0
	if hits, err := s.db.Search(index.SearchOpts{Query: q, Max: 60}); err == nil {
		var pick []index.Hit
		seenF := map[int64]bool{}
		for _, h := range hits {
			if printed[h.File.ID] || seenF[h.File.ID] || h.File.Type == "journal" {
				continue
			}
			seenF[h.File.ID] = true
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
	if len(notShown) > 0 {
		line += "; over budget (pg read <addr>): " + strings.Join(notShown[:min(len(notShown), 6)], ", ")
	}
	if more > 0 {
		line += fmt.Sprintf("; %d more notes mention it in passing (pg search %s)", more, shellQuote(q))
	}
	if len(notShown) == 0 && more == 0 {
		line += ". This is everything memory has on it."
	} else {
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

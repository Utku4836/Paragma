// Package md parses Paragma Markdown files into the structures the index
// stores: frontmatter, heading sections with line ranges, block IDs, links,
// tasks, callouts and tags.
//
// It is a line-oriented parser on purpose. Every structure Paragma cares about
// is line-anchored (headings, list items, callouts, ^block ids), and exact
// 1-based line ranges are what `pg read` and later the write commands need.
// Fenced code and $$ math blocks are skipped so that `#` or `[[` inside code
// never produce headings or links.
package md

import (
	"regexp"
	"strings"
	"time"

	"gopkg.in/yaml.v3"
)

// Doc is a parsed Markdown file. All line numbers are 1-based and refer to
// the original file, frontmatter included.
type Doc struct {
	Lines    []string
	Front    map[string]any
	FrontErr error
	BodyLine int // first line after the frontmatter

	Title    string // first H1 if the file has exactly one, else ""
	Sections []Section
	Blocks   []Block
	Links    []Link
	Tasks    []Task
	Callouts []Callout
	Marks    []Mark
	Tags     []string // inline #tags in the whole body, deduplicated
}

// Section is a heading and the lines below it. Index 0 may be a preamble
// (Path "" and Level 0) holding text before the first heading.
type Section struct {
	Path    string // "Katmanlar/Depolama"; the single H1 title is not a prefix
	Text    string // heading text only
	Level   int
	Line    int  // heading line (or first body line for the preamble)
	BodyEnd int  // last line of the section's own text, before any child heading
	End     int  // last line of the whole subtree
	Title   bool // the single H1 that names the whole note
	Kinds   []string
	Tags    []string
}

type Block struct {
	ID      string
	Line    int
	End     int
	Section int
}

type Link struct {
	Target  string // raw target without alias: "note#Heading" or "note^blk"
	Kind    string // wiki | embed | md | <typed key> e.g. fixes
	Line    int
	Section int
	Context string
}

type Task struct {
	ID       string
	Parent   string // block id of the parent task, if any
	Line     int
	End      int
	Indent   int
	State    byte
	Text     string // text with metadata tokens removed
	Priority string
	Due      string
	Done     string
	Blocked  string
	By       string
	Meta     [][2]string // every @key(value) in order, the ones above included
	Tags     []string
	Section  int
}

type Callout struct {
	Kind    string
	Line    int
	End     int
	Section int
	BlockID string
}

// Mark is something highlighted for attention: a callout or a mark tag
// such as #pin / #stale / #verify on a line.
type Mark struct {
	Kind    string
	Line    int
	Section int
	BlockID string
	Note    string // from @note(...) on the same line
}

// MarkTags are inline tags that double as marks.
var MarkTags = map[string]bool{"pin": true, "stale": true, "verify": true, "important": true, "question": true, "warning": true}

var (
	reHeading   = regexp.MustCompile(`^ {0,3}(#{1,6})(?:[ \t]+(.*?))?[ \t]*$`)
	reFence     = regexp.MustCompile("^ {0,3}(`{3,}|~{3,})")
	reBlockID   = regexp.MustCompile(`(?:^|\s)\^([A-Za-z0-9][A-Za-z0-9_-]*)(?:\s|$)`)
	reListItem  = regexp.MustCompile(`^(\s*)(?:[-*+]|\d+[.)])(?:\s|$)`)
	reTask      = regexp.MustCompile(`^(\s*)[-*+] \[(.)\](?:\s+(.*))?$`)
	reWiki      = regexp.MustCompile(`(!?)\[\[([^\[\]\n]+?)\]\]`)
	reTypedPre  = regexp.MustCompile(`([A-Za-z][\w-]*)::\s*(?:!?\[\[[^\]]*\]\]\s*,?\s*)*$`)
	reMdLink    = regexp.MustCompile(`\[[^\]]*\]\(([^)\s]+)\)`)
	reTag       = regexp.MustCompile(`(?:^|\s)#([\p{L}\p{N}_][\p{L}\p{N}_/-]*)`)
	reCallout   = regexp.MustCompile(`^\s*>\s*\[!([A-Za-z][\w-]*)\]`)
	reQuote     = regexp.MustCompile(`^\s*>`)
	reCodeSpan  = regexp.MustCompile("`[^`\n]*`")
	rePriority  = regexp.MustCompile(`(?:^|\s)!(p[0-9])(?:\s|$)`)
	reMeta      = regexp.MustCompile(`(?:^|\s)@([A-Za-z][\w-]*)\(([^)]*)\)`)
	reNote      = regexp.MustCompile(`(?:^|\s)@note\(([^)]*)\)`)
	reTrailMark = regexp.MustCompile(`\s+(?:#[\p{L}\p{N}_][\p{L}\p{N}_/-]*|@note\([^)]*\))\s*$`)
	reOnlyDigit = regexp.MustCompile(`^[0-9]+$`)
)

// Parse parses src. It never fails; malformed frontmatter is recorded in
// FrontErr and the rest of the file is still parsed.
func Parse(src []byte) *Doc {
	d := &Doc{Lines: SplitLines(src), BodyLine: 1}
	d.parseFront()

	code := d.codeMask()
	d.parseSections(code)
	d.parseLines(code)
	d.parseFrontLinks()
	return d
}

// SplitLines splits a file into lines the way Parse does: CRLF and a UTF-8
// BOM are normalised away and a trailing newline does not add a line.
func SplitLines(src []byte) []string {
	text := strings.ReplaceAll(string(src), "\r\n", "\n")
	text = strings.TrimPrefix(text, string(rune(0xFEFF)))
	lines := strings.Split(text, "\n")
	if len(lines) > 0 && lines[len(lines)-1] == "" {
		lines = lines[:len(lines)-1]
	}
	return lines
}

func (d *Doc) parseFront() {
	if len(d.Lines) == 0 || strings.TrimSpace(d.Lines[0]) != "---" {
		return
	}
	for i := 1; i < len(d.Lines); i++ {
		t := strings.TrimSpace(d.Lines[i])
		if t == "---" || t == "..." {
			raw := strings.Join(d.Lines[1:i], "\n")
			m := map[string]any{}
			if err := yaml.Unmarshal([]byte(raw), &m); err != nil {
				d.FrontErr = err
			} else {
				d.Front = m
			}
			d.BodyLine = i + 2
			return
		}
	}
}

// codeMask marks lines (0-based) that are inside fenced code or $$ blocks,
// fence lines included.
func (d *Doc) codeMask() []bool {
	mask := make([]bool, len(d.Lines))
	var fence string
	math := false
	for i := d.BodyLine - 1; i < len(d.Lines); i++ {
		l := d.Lines[i]
		switch {
		case fence != "":
			mask[i] = true
			t := strings.TrimSpace(l)
			if strings.HasPrefix(t, fence) && strings.Trim(t, fence[:1]) == "" {
				fence = ""
			}
		case math:
			mask[i] = true
			if strings.TrimSpace(l) == "$$" || strings.HasSuffix(strings.TrimSpace(l), "$$") {
				math = false
			}
		default:
			if m := reFence.FindStringSubmatch(l); m != nil {
				fence = m[1]
				mask[i] = true
			} else if t := strings.TrimSpace(l); strings.HasPrefix(t, "$$") && (t == "$$" || !strings.HasSuffix(t[2:], "$$")) {
				math = true
				mask[i] = true
			}
		}
	}
	return mask
}

func (d *Doc) parseSections(code []bool) {
	type hd struct {
		level int
		text  string
		line  int
	}
	var hs []hd
	for i := d.BodyLine - 1; i < len(d.Lines); i++ {
		if code[i] {
			continue
		}
		if m := reHeading.FindStringSubmatch(d.Lines[i]); m != nil {
			t := HeadingText(m[2])
			if t == "" {
				continue
			}
			hs = append(hs, hd{len(m[1]), t, i + 1})
		}
	}
	h1 := 0
	for _, h := range hs {
		if h.level == 1 {
			h1++
		}
	}
	titleMode := h1 == 1 && len(hs) > 0 && hs[0].level == 1
	if titleMode {
		d.Title = hs[0].text
	}
	last := len(d.Lines)

	// preamble
	firstHeading := last + 1
	if len(hs) > 0 {
		firstHeading = hs[0].line
	}
	if d.BodyLine < firstHeading {
		start, end := d.BodyLine, trimEnd(d.Lines, d.BodyLine, firstHeading-1)
		if end >= start {
			d.Sections = append(d.Sections, Section{Line: start, BodyEnd: end, End: end})
		}
	}

	type st struct {
		level int
		text  string
	}
	var stack []st
	for i, h := range hs {
		for len(stack) > 0 && stack[len(stack)-1].level >= h.level {
			stack = stack[:len(stack)-1]
		}
		var parts []string
		for _, s := range stack {
			if titleMode && s.level == 1 {
				continue
			}
			parts = append(parts, s.text)
		}
		parts = append(parts, h.text)
		stack = append(stack, st{h.level, h.text})

		bodyEnd := last
		if i+1 < len(hs) {
			bodyEnd = hs[i+1].line - 1
		}
		end := last
		for j := i + 1; j < len(hs); j++ {
			if hs[j].level <= h.level {
				end = hs[j].line - 1
				break
			}
		}
		d.Sections = append(d.Sections, Section{
			Path:    strings.Join(parts, "/"),
			Text:    h.text,
			Level:   h.level,
			Line:    h.line,
			BodyEnd: max(h.line, trimEnd(d.Lines, h.line, bodyEnd)),
			End:     max(h.line, trimEnd(d.Lines, h.line, end)),
			Title:   titleMode && i == 0,
		})
	}
}

// trimEnd moves end back over trailing blank lines, never before start.
func trimEnd(lines []string, start, end int) int {
	for end >= start && end >= 1 && strings.TrimSpace(lines[end-1]) == "" {
		end--
	}
	return end
}

// SectionAt returns the index of the section whose own body contains line,
// or -1.
func (d *Doc) SectionAt(line int) int {
	for i := len(d.Sections) - 1; i >= 0; i-- {
		s := d.Sections[i]
		if line >= s.Line && line <= max(s.BodyEnd, s.Line) {
			return i
		}
	}
	// blank lines between sections belong to the previous section
	for i := len(d.Sections) - 1; i >= 0; i-- {
		if line >= d.Sections[i].Line {
			return i
		}
	}
	return -1
}

func (d *Doc) parseLines(code []bool) {
	tagSeen := map[string]bool{}
	type open struct {
		indent int
		id     string
	}
	var taskStack []open
	secTags := make([]map[string]bool, len(d.Sections))
	secKinds := make([]map[string]bool, len(d.Sections))

	for i := d.BodyLine - 1; i < len(d.Lines); i++ {
		if code[i] {
			continue
		}
		raw := d.Lines[i]
		ln := i + 1
		if m := reHeading.FindStringSubmatch(raw); m != nil && HeadingText(m[2]) != "" {
			// trailing #tags / @note() on a heading mark the whole section
			sec := d.SectionAt(ln)
			trail := strings.TrimPrefix(strings.TrimSpace(strings.TrimRight(m[2], "#")), HeadingText(m[2]))
			note := ""
			if nm := reNote.FindStringSubmatch(trail); nm != nil {
				note = nm[1]
			}
			for _, tm := range reTag.FindAllStringSubmatch(trail, -1) {
				t := strings.ToLower(tm[1])
				if !tagSeen[t] {
					tagSeen[t] = true
					d.Tags = append(d.Tags, t)
				}
				if MarkTags[t] {
					d.Marks = append(d.Marks, Mark{Kind: t, Line: ln, Section: sec, Note: note})
				}
			}
			continue
		}
		clean := reCodeSpan.ReplaceAllStringFunc(raw, func(s string) string { return strings.Repeat(" ", len(s)) })
		sec := d.SectionAt(ln)

		// block ids
		for _, m := range reBlockID.FindAllStringSubmatchIndex(clean, -1) {
			id := clean[m[2]:m[3]]
			start, end := d.blockExtent(i, code)
			// a line holding only "^id" labels the block above it
			if strings.TrimSpace(clean) == "^"+id && i > 0 {
				start, end = d.blockExtent(i-1, code)
				end = ln
			}
			d.Blocks = append(d.Blocks, Block{ID: id, Line: start, End: end, Section: sec})
		}

		// links
		for _, m := range reWiki.FindAllStringSubmatchIndex(clean, -1) {
			inner := clean[m[4]:m[5]]
			target := strings.TrimSpace(strings.SplitN(inner, "|", 2)[0])
			if target == "" {
				continue
			}
			kind := "wiki"
			if m[3] > m[2] {
				kind = "embed"
			}
			if tm := reTypedPre.FindStringSubmatch(clean[:m[0]]); tm != nil {
				kind = strings.ToLower(tm[1])
			}
			d.Links = append(d.Links, Link{Target: target, Kind: kind, Line: ln, Section: sec, Context: strings.TrimSpace(raw)})
		}
		for _, m := range reMdLink.FindAllStringSubmatch(clean, -1) {
			t := m[1]
			if strings.Contains(t, "://") || strings.HasPrefix(t, "mailto:") || strings.HasPrefix(t, "#") {
				continue
			}
			p, frag, _ := strings.Cut(t, "#")
			if !strings.HasSuffix(strings.ToLower(p), ".md") {
				continue
			}
			p = strings.TrimSuffix(strings.TrimSuffix(p, ".md"), ".MD")
			p = strings.ReplaceAll(p, "%20", " ")
			if frag != "" {
				p += "#" + strings.ReplaceAll(frag, "%20", " ")
			}
			d.Links = append(d.Links, Link{Target: p, Kind: "md", Line: ln, Section: sec, Context: strings.TrimSpace(raw)})
		}

		// tags
		var lineTags []string
		for _, m := range reTag.FindAllStringSubmatch(clean, -1) {
			t := strings.ToLower(strings.TrimRight(m[1], "/-"))
			if t == "" || reOnlyDigit.MatchString(t) {
				continue
			}
			lineTags = append(lineTags, t)
			if !tagSeen[t] {
				tagSeen[t] = true
				d.Tags = append(d.Tags, t)
			}
			if sec >= 0 {
				if secTags[sec] == nil {
					secTags[sec] = map[string]bool{}
				}
				secTags[sec][t] = true
			}
		}
		lineBlock := ""
		if m := reBlockID.FindStringSubmatch(clean); m != nil {
			lineBlock = m[1]
		}
		note := ""
		if nm := reNote.FindStringSubmatch(clean); nm != nil {
			note = nm[1]
		}
		for _, t := range lineTags {
			if MarkTags[t] {
				d.Marks = append(d.Marks, Mark{Kind: t, Line: ln, Section: sec, BlockID: lineBlock, Note: note})
			}
		}

		// callouts
		if m := reCallout.FindStringSubmatch(clean); m != nil {
			kind := strings.ToLower(m[1])
			start, end := d.blockExtent(i, code)
			c := Callout{Kind: kind, Line: start, End: end, Section: sec}
			for j := start; j <= end; j++ {
				if bm := reBlockID.FindStringSubmatch(d.Lines[j-1]); bm != nil {
					c.BlockID = bm[1]
					break
				}
			}
			d.Callouts = append(d.Callouts, c)
			d.Marks = append(d.Marks, Mark{Kind: kind, Line: start, Section: sec, BlockID: c.BlockID})
			if sec >= 0 {
				if secKinds[sec] == nil {
					secKinds[sec] = map[string]bool{}
				}
				secKinds[sec][kind] = true
			}
		}

		// tasks
		if m := reTask.FindStringSubmatch(raw); m != nil {
			indent := indentWidth(m[1])
			for len(taskStack) > 0 && taskStack[len(taskStack)-1].indent >= indent {
				taskStack = taskStack[:len(taskStack)-1]
			}
			t := parseTask(m[3])
			t.Line = ln
			_, t.End = d.blockExtent(i, code)
			t.Indent = indent
			t.State = m[2][0]
			t.Section = sec
			if len(taskStack) > 0 {
				t.Parent = taskStack[len(taskStack)-1].id
			}
			taskStack = append(taskStack, open{indent, t.ID})
			d.Tasks = append(d.Tasks, t)
		} else if strings.TrimSpace(raw) != "" && !reListItem.MatchString(raw) && indentWidth(leadingWS(raw)) == 0 {
			taskStack = nil
		}
	}

	for i := range d.Sections {
		d.Sections[i].Tags = sortedKeys(secTags[i])
		d.Sections[i].Kinds = sortedKeys(secKinds[i])
	}
}

func parseTask(s string) Task {
	var t Task
	if m := reBlockID.FindStringSubmatch(s); m != nil {
		t.ID = m[1]
	}
	if m := rePriority.FindStringSubmatch(s); m != nil {
		t.Priority = m[1]
	}
	for _, m := range reMeta.FindAllStringSubmatch(s, -1) {
		t.Meta = append(t.Meta, [2]string{m[1], m[2]})
		switch m[1] {
		case "due":
			t.Due = m[2]
		case "done":
			t.Done = m[2]
		case "blocked":
			t.Blocked = m[2]
		case "by":
			t.By = m[2]
		}
	}
	for _, m := range reTag.FindAllStringSubmatch(s, -1) {
		if !reOnlyDigit.MatchString(m[1]) {
			t.Tags = append(t.Tags, strings.ToLower(m[1]))
		}
	}
	text := reBlockID.ReplaceAllString(s, " ")
	text = rePriority.ReplaceAllString(text, " ")
	text = reMeta.ReplaceAllString(text, " ")
	text = reTag.ReplaceAllString(text, " ")
	t.Text = strings.Join(strings.Fields(text), " ")
	return t
}

// HeadingText cleans raw heading text: closing #s and trailing mark tags or
// @note(...) are not part of the heading's name (or its address).
func HeadingText(raw string) string {
	t := strings.TrimSpace(strings.TrimRight(strings.TrimSpace(raw), "#"))
	for {
		u := reTrailMark.ReplaceAllString(t, "")
		if u == t || u == "" {
			return strings.TrimSpace(t)
		}
		t = strings.TrimSpace(u)
	}
}

// TaskLine is a parsed task line that can be changed and written back.
type TaskLine struct {
	Indent string // leading whitespace
	Marker string // "-", "*" or "+"
	State  byte
	Task
}

var reTaskLine = regexp.MustCompile(`^(\s*)([-*+]) \[(.)\](?:\s+(.*))?$`)

// ParseTaskLine parses one task line; ok is false for any other line.
func ParseTaskLine(line string) (TaskLine, bool) {
	m := reTaskLine.FindStringSubmatch(line)
	if m == nil {
		return TaskLine{}, false
	}
	return TaskLine{Indent: m[1], Marker: m[2], State: m[3][0], Task: parseTask(m[4])}, true
}

// Get returns a metadata value ("" when absent).
func (t *TaskLine) Get(key string) string {
	for _, kv := range t.Meta {
		if kv[0] == key {
			return kv[1]
		}
	}
	return ""
}

// Set sets (or with value "" removes) a metadata value.
func (t *TaskLine) Set(key, value string) {
	out := t.Meta[:0:0]
	done := false
	for _, kv := range t.Meta {
		if kv[0] == key {
			if value != "" && !done {
				out = append(out, [2]string{key, value})
				done = true
			}
			continue
		}
		out = append(out, kv)
	}
	if value != "" && !done {
		out = append(out, [2]string{key, value})
	}
	t.Meta = out
}

// String renders the task line: text, ^id, !priority, @meta, #tags.
func (t *TaskLine) String() string {
	var b strings.Builder
	b.WriteString(t.Indent + t.Marker + " [" + string(t.State) + "] " + t.Text)
	if t.ID != "" {
		b.WriteString(" ^" + t.ID)
	}
	if t.Priority != "" {
		b.WriteString(" !" + t.Priority)
	}
	for _, kv := range t.Meta {
		b.WriteString(" @" + kv[0] + "(" + kv[1] + ")")
	}
	for _, tag := range t.Tags {
		b.WriteString(" #" + tag)
	}
	return b.String()
}

// blockExtent returns the 1-based [start,end] of the Markdown block holding
// 0-based line i: a list item with its nested lines, a blockquote, or a
// paragraph.
func (d *Doc) blockExtent(i int, code []bool) (int, int) {
	L := d.Lines
	blank := func(j int) bool { return strings.TrimSpace(L[j]) == "" }
	if m := reListItem.FindStringSubmatch(L[i]); m != nil {
		ind := indentWidth(m[1])
		end := i
		for j := i + 1; j < len(L); j++ {
			if blank(j) {
				break
			}
			if indentWidth(leadingWS(L[j])) <= ind {
				break
			}
			end = j
		}
		return i + 1, end + 1
	}
	if reQuote.MatchString(L[i]) {
		s, e := i, i
		for s > 0 && reQuote.MatchString(L[s-1]) {
			s--
		}
		for e+1 < len(L) && reQuote.MatchString(L[e+1]) {
			e++
		}
		return s + 1, e + 1
	}
	s, e := i, i
	for s > d.BodyLine-1 && !blank(s-1) && !reHeading.MatchString(L[s-1]) && !reListItem.MatchString(L[s-1]) {
		s--
	}
	for e+1 < len(L) && !blank(e+1) && !reHeading.MatchString(L[e+1]) && !reListItem.MatchString(L[e+1]) {
		e++
	}
	return s + 1, e + 1
}

// parseFrontLinks turns [[links]] inside frontmatter values into typed links
// named after their key, e.g. `supersedes: ["[[0003-journal-mode]]"]`.
func (d *Doc) parseFrontLinks() {
	if d.Front == nil {
		return
	}
	for k, v := range d.Front {
		var walk func(any)
		walk = func(v any) {
			switch x := v.(type) {
			case string:
				for _, m := range reWiki.FindAllStringSubmatch(x, -1) {
					t := strings.TrimSpace(strings.SplitN(m[2], "|", 2)[0])
					if t != "" {
						d.Links = append(d.Links, Link{Target: t, Kind: strings.ToLower(k), Line: 1, Section: -1, Context: k + ": " + x})
					}
				}
			case []any:
				for _, e := range x {
					walk(e)
				}
			}
		}
		// unquoted `supersedes: [[x]]` is valid YAML: a list holding a list
		if l, ok := v.([]any); ok {
			for _, e := range l {
				if inner, ok := e.([]any); ok && len(inner) == 1 {
					if s, ok := inner[0].(string); ok {
						d.Links = append(d.Links, Link{Target: s, Kind: strings.ToLower(k), Line: 1, Section: -1, Context: k})
						continue
					}
				}
				walk(e)
			}
			continue
		}
		walk(v)
	}
}

func leadingWS(s string) string {
	return s[:len(s)-len(strings.TrimLeft(s, " \t"))]
}

func indentWidth(ws string) int {
	n := 0
	for _, r := range ws {
		if r == '\t' {
			n += 4
		} else {
			n++
		}
	}
	return n
}

func sortedKeys(m map[string]bool) []string {
	if len(m) == 0 {
		return nil
	}
	out := make([]string, 0, len(m))
	for k := range m {
		out = append(out, k)
	}
	for i := 1; i < len(out); i++ {
		for j := i; j > 0 && out[j] < out[j-1]; j-- {
			out[j], out[j-1] = out[j-1], out[j]
		}
	}
	return out
}

// FrontString returns a frontmatter value as a string ("" if missing).
func (d *Doc) FrontString(key string) string {
	if d.Front == nil {
		return ""
	}
	switch v := d.Front[key].(type) {
	case nil:
		return ""
	case string:
		return v
	default:
		return strings.TrimSpace(yamlScalar(v))
	}
}

// FrontList returns a frontmatter value as a list of strings.
func (d *Doc) FrontList(key string) []string {
	if d.Front == nil {
		return nil
	}
	switch v := d.Front[key].(type) {
	case string:
		var out []string
		for _, p := range strings.Split(v, ",") {
			if p = strings.TrimSpace(p); p != "" {
				out = append(out, p)
			}
		}
		return out
	case []any:
		var out []string
		for _, e := range v {
			if s := strings.TrimSpace(yamlScalar(e)); s != "" {
				out = append(out, s)
			}
		}
		return out
	}
	return nil
}

func yamlScalar(v any) string {
	switch x := v.(type) {
	case string:
		return x
	case nil:
		return ""
	case time.Time:
		if x.Hour() == 0 && x.Minute() == 0 && x.Second() == 0 {
			return x.Format("2006-01-02")
		}
		return x.Format("2006-01-02 15:04")
	}
	b, err := yaml.Marshal(v)
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(b))
}

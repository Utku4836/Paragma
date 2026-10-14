package md

import (
	"strings"
	"testing"
)

const sample = `---
id: 01TEST
type: session
tags: [auth, jwt]
supersedes: [[old-note]]
---
# Auth refactor

Intro line #pin

## Plan
- [x] Done thing ^t001 @done(2026-10-13)
- [/] Refresh endpoint ^t002 !p1 @due(2026-10-20) #auth
    - [ ] Tests ^t003
- [!] Migration ^t004 @blocked(db access)

## Notes

` + "```go" + `
// # not a heading, [[not-a-link]]
` + "```" + `

> [!warning] Careful ^w1
> second line fixes:: [[jwt-exp]]

See [[architecture#Middleware|mw]] and ![[diagram]] and [x](docs/a.md#Top).

### Deep
text ` + "`[[code-link]]`" + ` #123 #real-tag
`

func TestParse(t *testing.T) {
	d := Parse([]byte(sample))
	if d.FrontErr != nil {
		t.Fatal(d.FrontErr)
	}
	if d.BodyLine != 7 {
		t.Errorf("BodyLine = %d", d.BodyLine)
	}
	if d.Title != "Auth refactor" {
		t.Errorf("Title = %q", d.Title)
	}
	var paths []string
	for _, s := range d.Sections {
		paths = append(paths, s.Path)
	}
	if got := strings.Join(paths, "|"); got != "Auth refactor|Plan|Notes|Notes/Deep" {
		t.Errorf("paths = %s", got)
	}
	plan := d.Sections[1]
	if plan.Line != 11 || plan.BodyEnd != 15 || plan.End != 15 {
		t.Errorf("Plan lines = %d %d %d", plan.Line, plan.BodyEnd, plan.End)
	}
	notes := d.Sections[2]
	if notes.End <= notes.BodyEnd {
		t.Errorf("Notes subtree should extend past its own body: %+v", notes)
	}
	if strings.Join(notes.Kinds, ",") != "warning" {
		t.Errorf("Notes kinds = %v", notes.Kinds)
	}

	if len(d.Tasks) != 4 {
		t.Fatalf("tasks = %d", len(d.Tasks))
	}
	t2 := d.Tasks[1]
	if t2.ID != "t002" || t2.State != '/' || t2.Priority != "p1" || t2.Due != "2026-10-20" || t2.Text != "Refresh endpoint" || t2.End != 14 {
		t.Errorf("task2 = %+v", t2)
	}
	if d.Tasks[2].Parent != "t002" || d.Tasks[3].Parent != "" || d.Tasks[3].Blocked != "db access" {
		t.Errorf("parents/blocked wrong: %+v %+v", d.Tasks[2], d.Tasks[3])
	}

	kinds := map[string]string{}
	for _, l := range d.Links {
		kinds[l.Target] = l.Kind
	}
	want := map[string]string{"old-note": "supersedes", "jwt-exp": "fixes", "architecture#Middleware": "wiki", "diagram": "embed", "docs/a#Top": "md"}
	for k, v := range want {
		if kinds[k] != v {
			t.Errorf("link %s kind = %q, want %q (all: %v)", k, kinds[k], v, kinds)
		}
	}
	if _, ok := kinds["not-a-link"]; ok {
		t.Error("link inside code fence was parsed")
	}
	if _, ok := kinds["code-link"]; ok {
		t.Error("link inside code span was parsed")
	}

	blocks := map[string]Block{}
	for _, b := range d.Blocks {
		blocks[b.ID] = b
	}
	if b := blocks["w1"]; b.Line != 23 || b.End != 24 {
		t.Errorf("callout block = %+v", b)
	}
	if b := blocks["t002"]; b.Line != 13 || b.End != 14 {
		t.Errorf("task block = %+v", b)
	}

	tags := strings.Join(d.Tags, ",")
	if !strings.Contains(tags, "real-tag") || strings.Contains(tags, "123") || !strings.Contains(tags, "pin") {
		t.Errorf("tags = %s", tags)
	}
	foundPin := false
	for _, m := range d.Marks {
		if m.Kind == "pin" {
			foundPin = true
		}
	}
	if !foundPin {
		t.Error("#pin mark missing")
	}
}

func TestNoTitleMode(t *testing.T) {
	d := Parse([]byte("# A\n## B\n# C\ntext\n"))
	if d.Title != "" {
		t.Errorf("two H1s must not set a title, got %q", d.Title)
	}
	if d.Sections[1].Path != "A/B" || d.Sections[2].Path != "C" {
		t.Errorf("paths: %q %q", d.Sections[1].Path, d.Sections[2].Path)
	}
}

func TestPreambleAndCRLF(t *testing.T) {
	d := Parse([]byte("intro\r\nmore\r\n\r\n## H\r\nbody\r\n"))
	if len(d.Sections) != 2 || d.Sections[0].Path != "" || d.Sections[0].End != 2 {
		t.Fatalf("sections = %+v", d.Sections)
	}
	if d.Lines[0] != "intro" {
		t.Errorf("CR not stripped: %q", d.Lines[0])
	}
}

func TestBadFrontmatter(t *testing.T) {
	d := Parse([]byte("---\n: : bad\n  - x\n---\n# T\n"))
	if d.FrontErr == nil {
		t.Error("expected frontmatter error")
	}
	if d.BodyLine != 5 || len(d.Sections) != 1 {
		t.Errorf("body should still parse: %d %d", d.BodyLine, len(d.Sections))
	}
}
